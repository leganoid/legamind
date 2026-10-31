//! Экспорт всех данных (сессии + сообщения) в один JSON-файл.

use std::path::Path;

use anyhow::Result;
use serde::Serialize;
use tauri::State;

use crate::AppState;

#[derive(Debug, Serialize)]
struct ExportBundle {
    version: u32,
    exported_at: String,
    sessions: Vec<ExportSession>,
}

#[derive(Debug, Serialize)]
struct ExportSession {
    #[serde(flatten)]
    session: crate::db::Session,
    messages: Vec<crate::db::ChatMessageRow>,
}

pub fn write_export(db: &crate::db::Db, exports_dir: &Path) -> Result<String> {
    std::fs::create_dir_all(exports_dir)?;
    let mut sessions = Vec::new();
    for session in db.sessions_list()? {
        let messages = db.messages_list(&session.id)?;
        sessions.push(ExportSession { session, messages });
    }
    let bundle = ExportBundle {
        version: 1,
        exported_at: chrono::Utc::now().to_rfc3339(),
        sessions,
    };
    let file = exports_dir.join(format!(
        "all-{}.json",
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    ));
    std::fs::write(&file, serde_json::to_vec_pretty(&bundle)?)?;
    Ok(file.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn export_all(state: State<'_, AppState>) -> Result<String, String> {
    let file = write_export(&state.db, &state.data_dir.join("exports"))
        .map_err(|e| e.to_string())?;
    let _ = crate::logging::log_line("app.log", &format!("export all → {file}"));
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exports_sessions_and_messages() {
        let db = crate::db::Db::open_memory().unwrap();
        let session = db.session_create("Экспорт", "local/qwen-1.5b").unwrap();
        db.message_add(&session.id, "user", "привет", None, None)
            .unwrap();
        db.message_add(&session.id, "assistant", "здравствуйте", None, None)
            .unwrap();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-data")
            .join("export");
        let _ = std::fs::remove_dir_all(&dir);
        let file = write_export(&db, &dir).unwrap();
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(value["sessions"].as_array().unwrap().len(), 1);
        assert_eq!(
            value["sessions"][0]["messages"].as_array().unwrap().len(),
            2
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
