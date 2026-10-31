//! Наблюдение за рабочей папкой: автоматическая переиндексация RAG после изменений.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use anyhow::Result;
use notify::{RecursiveMode, Watcher};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::AppState;

const DEBOUNCE_MS: u64 = 1500;

#[derive(Debug, Clone, Serialize)]
pub struct RagIndexedPayload {
    pub chunks: usize,
    pub files: usize,
}

/// Игнорируем служебные папки, чтобы не переиндексировать сами себя.
pub fn is_relevant(path: &Path) -> bool {
    let text = path.to_string_lossy().to_lowercase();
    for marker in [
        "\\.legamind\\",
        "/.legamind/",
        "\\node_modules\\",
        "/node_modules/",
        "\\target\\",
        "/target/",
        "\\.git\\",
        "/.git/",
        "\\dist\\",
        "/dist/",
    ] {
        if text.contains(marker) {
            return false;
        }
    }
    true
}

fn spawn_debounce(app: AppHandle, rx: Receiver<Vec<PathBuf>>, root: PathBuf) {
    std::thread::spawn(move || {
        while let Ok(first) = rx.recv() {
            let mut changed: HashSet<PathBuf> = first.into_iter().collect();
            let deadline = Instant::now() + Duration::from_millis(DEBOUNCE_MS);
            loop {
                let now = Instant::now();
                if now >= deadline {
                    break;
                }
                match rx.recv_timeout(deadline - now) {
                    Ok(paths) => {
                        changed.extend(paths);
                    }
                    Err(RecvTimeoutError::Timeout) => break,
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }

            let state = app.state::<AppState>();
            let root_key = root.to_string_lossy().to_string();
            let has_index = state
                .db
                .rag_stats(&root_key)
                .map(|stats| stats.chunks > 0)
                .unwrap_or(false);
            if !has_index {
                continue;
            }

            // Точечно переиндексируем изменённые файлы; папки и удаления — полный проход.
            let mut full = false;
            for path in &changed {
                if path.is_dir() {
                    full = true;
                    break;
                }
                if path.exists() && !crate::rag::is_indexable(path) {
                    continue;
                }
                if !path.exists() && path.extension().is_none() {
                    full = true;
                    break;
                }
                if let Err(err) = crate::rag::reindex_file(&state.db, &root, path) {
                    let _ = crate::logging::log_line(
                        "app.log",
                        &format!("watcher reindex file failed: {err}"),
                    );
                    full = true;
                    break;
                }
            }

            let stats = if full {
                crate::rag::index_workdir(&state.db, &root)
                    .map(|stats| (stats.chunks as usize, stats.files as usize))
            } else {
                state
                    .db
                    .rag_stats(&root_key)
                    .map(|stats| (stats.chunks as usize, stats.files as usize))
            };
            match stats {
                Ok((chunks, files)) => {
                    let _ = app.emit("rag:indexed", RagIndexedPayload { chunks, files });
                }
                Err(err) => {
                    let _ = crate::logging::log_line(
                        "app.log",
                        &format!("watcher reindex failed: {err}"),
                    );
                }
            }
        }
    });
}

#[tauri::command]
pub async fn watch_workdir(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<String, String> {
    let workdir = state
        .db
        .session_get(&session_id)
        .map_err(|e| e.to_string())?
        .and_then(|session| session.workdir)
        .ok_or_else(|| "У сессии нет рабочей папки".to_string())?;
    let root = PathBuf::from(&workdir);
    if !root.is_dir() {
        return Err("Рабочая папка недоступна".to_string());
    }

    // Останавливаем предыдущий watcher (его поток завершится сам).
    *state.watcher.lock().unwrap() = None;

    let (tx, rx) = std::sync::mpsc::channel::<Vec<PathBuf>>();
    let mut watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
        if let Ok(event) = result {
            let paths: Vec<PathBuf> = event
                .paths
                .into_iter()
                .filter(|path| is_relevant(path))
                .collect();
            if !paths.is_empty() {
                let _ = tx.send(paths);
            }
        }
    })
    .map_err(|e| format!("Не удалось создать watcher: {e}"))?;
    watcher
        .watch(&root, RecursiveMode::Recursive)
        .map_err(|e| format!("Не удалось наблюдать за папкой: {e}"))?;

    *state.watch_root.lock().unwrap() = Some(root.clone());
    *state.watcher.lock().unwrap() = Some(watcher);
    spawn_debounce(app, rx, root.clone());
    Ok(root.to_string_lossy().to_string())
}

#[tauri::command]
pub fn unwatch_workdir(state: State<'_, AppState>) {
    *state.watcher.lock().unwrap() = None;
    *state.watch_root.lock().unwrap() = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignores_service_directories() {
        assert!(!is_relevant(Path::new(
            r"C:\project\.legamind\backups\a.db"
        )));
        assert!(!is_relevant(Path::new(r"C:\project\target\debug\x.exe")));
        assert!(!is_relevant(Path::new(r"C:\project\node_modules\a.js")));
        assert!(is_relevant(Path::new(r"C:\project\src\main.rs")));
    }
}
