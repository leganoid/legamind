//! Просмотр хвоста логов из интерфейса.

use std::path::PathBuf;

use serde::Serialize;
use tauri::State;

use crate::AppState;

const ALLOWED: &[&str] = &[
    "app.log",
    "audit.log",
    "prompts.log",
    "engine_stdout.log",
    "engine_stderr.log",
];

const MAX_BYTES: usize = 512 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct LogTail {
    pub name: String,
    pub path: String,
    pub text: String,
    pub size: u64,
}

#[tauri::command]
pub async fn logs_tail(
    state: State<'_, AppState>,
    name: String,
    lines: Option<usize>,
) -> Result<LogTail, String> {
    let name = name.trim();
    if !ALLOWED.contains(&name) {
        return Err("Неизвестный лог".into());
    }
    let path: PathBuf = state.data_dir.join("logs").join(name);
    let bytes = std::fs::read(&path).map_err(|e| format!("Не удалось прочитать лог: {e}"))?;
    let size = bytes.len() as u64;
    let start = bytes.len().saturating_sub(MAX_BYTES);
    let text = String::from_utf8_lossy(&bytes[start..]).to_string();
    let limit = lines.unwrap_or(400).clamp(10, 2000);
    let all: Vec<&str> = text.lines().collect();
    let tail = if all.len() > limit {
        all[all.len() - limit..].join("\n")
    } else {
        all.join("\n")
    };
    Ok(LogTail {
        name: name.to_string(),
        path: path.to_string_lossy().to_string(),
        text: tail,
        size,
    })
}
