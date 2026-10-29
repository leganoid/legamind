//! Рабочая папка сессии: выбор каталога проекта и его инициализация.
//!
//! В папке создаётся `.legamind/` со служебными файлами, `exports/`, `datasets/`
//! и `notes.md`. Экспорт переписки и датасеты из сессии пишутся туда.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use serde::Serialize;
use tauri::State;

use crate::db::Session;
use crate::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct WorkdirInfo {
    pub path: String,
    pub created: bool,
    pub initialized: bool,
    pub note: Option<String>,
}

/// Нативный диалог выбора папки (Windows). Возвращает None, если отменили.
#[tauri::command]
pub async fn pick_folder() -> Result<Option<String>, String> {
    let picked = tauri::async_runtime::spawn_blocking(|| {
        rfd::FileDialog::new()
            .set_title("Рабочая папка проекта")
            .pick_folder()
    })
    .await
    .map_err(|e| e.to_string())?;
    Ok(picked.map(|path| path.to_string_lossy().to_string()))
}

/// Убирает Windows-префикс `\\?\` из канонического пути (он ломает отображение и explorer).
fn strip_verbatim(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = text.strip_prefix(r"\\?\") {
        return PathBuf::from(rest);
    }
    path.to_path_buf()
}

/// Проверяет путь и (при необходимости) создаёт каталог.
pub fn prepare_workdir(path: &str, create: bool) -> Result<PathBuf> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        bail!("Пустой путь");
    }
    let candidate = PathBuf::from(trimmed);
    if !candidate.is_absolute() {
        bail!("Нужен абсолютный путь, например C:\\Projects\\my-project");
    }
    let existed = candidate.exists();
    if !existed {
        if !create {
            bail!("Папка не существует");
        }
        std::fs::create_dir_all(&candidate)
            .with_context(|| format!("Не удалось создать папку {}", candidate.display()))?;
    }
    if !candidate.is_dir() {
        bail!("Это файл, а не папка");
    }
    Ok(strip_verbatim(&candidate.canonicalize().unwrap_or(candidate)))
}

fn write_if_missing(path: &Path, content: &str) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    std::fs::write(path, content).with_context(|| format!("Не удалось записать {}", path.display()))
}

/// Создаёт в рабочей папке структуру проекта Legamind.
pub fn init_project(dir: &Path, session: &Session) -> Result<WorkdirInfo> {
    let meta = dir.join(".legamind");
    std::fs::create_dir_all(&meta)?;
    std::fs::create_dir_all(dir.join("exports"))?;
    std::fs::create_dir_all(dir.join("datasets"))?;

    let session_json = serde_json::json!({
        "session_id": session.id,
        "title": session.title,
        "model": session.model_id,
        "created_at": session.created_at,
        "workdir": dir.to_string_lossy(),
        "app": "Legamind",
    });
    write_if_missing(
        &meta.join("session.json"),
        &serde_json::to_string_pretty(&session_json)?,
    )?;
    write_if_missing(
        &meta.join("README.md"),
        "# Проект Legamind\n\n\
         Эта папка — рабочее пространство сессии Legamind.\n\n\
         - `exports/` — экспорт переписки (`/export`);\n\
         - `datasets/` — JSONL-датасеты, собранные из этой сессии;\n\
         - `notes.md` — заметки проекта (можно вести вручную);\n\
         - `.legamind/session.json` — связь папки с сессией.\n\n\
         Модель не пишет сюда файлы автоматически: доступ к файлам появится с инструментами (M6).\n",
    )?;
    write_if_missing(
        &dir.join("notes.md"),
        &format!("# {}\n\nЗаметки проекта.\n", session.title),
    )?;

    Ok(WorkdirInfo {
        path: dir.to_string_lossy().to_string(),
        created: false,
        initialized: true,
        note: None,
    })
}

#[tauri::command]
pub async fn session_set_workdir(
    state: State<'_, AppState>,
    session_id: String,
    path: Option<String>,
    create: Option<bool>,
) -> Result<Session, String> {
    let session = state
        .db
        .session_get(&session_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Сессия не найдена".to_string())?;

    match path {
        None => {
            state
                .db
                .session_set_workdir(&session_id, None)
                .map_err(|e| e.to_string())?;
        }
        Some(raw) => {
            let create = create.unwrap_or(true);
            let dir = prepare_workdir(&raw, create).map_err(|e| e.to_string())?;
            init_project(&dir, &session).map_err(|e| e.to_string())?;
            state
                .db
                .session_set_workdir(&session_id, Some(&dir.to_string_lossy()))
                .map_err(|e| e.to_string())?;
            let _ = crate::logging::log_line(
                "app.log",
                &format!("workdir: {} -> {}", session_id, dir.display()),
            );
        }
    }
    state
        .db
        .session_get(&session_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Сессия не найдена".to_string())
}

/// Открыть папку или файл в проводнике.
#[tauri::command]
pub async fn open_path(path: String) -> Result<(), String> {
    let target = PathBuf::from(&path);
    if !target.exists() {
        return Err(anyhow!("Путь не существует: {path}").to_string());
    }
    #[cfg(windows)]
    {
        std::process::Command::new("explorer")
            .arg(target.as_os_str())
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir(name: &str) -> PathBuf {
        let base = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-data")
            .join("workdir")
            .join(name);
        let _ = std::fs::remove_dir_all(&base);
        base
    }

    #[test]
    fn rejects_relative_and_creates_absolute() {
        assert!(prepare_workdir("relative/path", true).is_err());
        let dir = test_dir("create");
        let prepared = prepare_workdir(&dir.to_string_lossy(), true).expect("создание");
        assert!(prepared.is_dir());
    }

    #[test]
    fn init_creates_project_files() {
        let dir = test_dir("project");
        std::fs::create_dir_all(&dir).unwrap();
        let session = Session {
            id: "s1".into(),
            title: "Демо".into(),
            model_id: "local/qwen2.5-1.5b".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
            workdir: None,
        };
        init_project(&dir, &session).expect("init");
        assert!(dir.join(".legamind").join("session.json").exists());
        assert!(dir.join("exports").is_dir());
        assert!(dir.join("datasets").is_dir());
        assert!(dir.join("notes.md").exists());
        let json = std::fs::read_to_string(dir.join(".legamind").join("session.json")).unwrap();
        assert!(json.contains("s1"));
    }
}
