//! Резервное копирование и восстановление базы данных.

use std::path::PathBuf;

use tauri::{AppHandle, State};

use crate::AppState;

fn timestamp() -> String {
    chrono::Local::now().format("%Y%m%d-%H%M%S").to_string()
}

#[tauri::command]
pub async fn db_backup(
    state: State<'_, AppState>,
    path: Option<String>,
) -> Result<String, String> {
    let dest = match path {
        Some(value) => PathBuf::from(value),
        None => {
            let file = rfd::FileDialog::new()
                .set_title("Сохранить копию базы Legamind")
                .add_filter("SQLite", &["db"])
                .set_file_name(&format!("legamind-backup-{}.db", timestamp()))
                .save_file();
            file.ok_or_else(|| "Сохранение отменено".to_string())?
        }
    };
    state
        .db
        .backup_to(&dest)
        .map_err(|e| e.to_string())?;
    let _ = crate::logging::log_line(
        "app.log",
        &format!("db backup → {}", dest.display()),
    );
    Ok(dest.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn db_restore(
    app: AppHandle,
    state: State<'_, AppState>,
    path: Option<String>,
) -> Result<String, String> {
    let source = match path {
        Some(value) => PathBuf::from(value),
        None => {
            let file = rfd::FileDialog::new()
                .set_title("Выберите копию базы Legamind")
                .add_filter("SQLite", &["db"])
                .pick_file();
            file.ok_or_else(|| "Выбор отменён".to_string())?
        }
    };

    let safety = state
        .data_dir
        .join("backups")
        .join(format!("before-restore-{}.db", timestamp()));
    state
        .db
        .backup_to(&safety)
        .map_err(|e| format!("Не удалось сохранить текущую базу: {e}"))?;
    state
        .db
        .restore_from(&source)
        .map_err(|e| format!("Не удалось восстановить: {e}"))?;
    let _ = crate::logging::log_line(
        "app.log",
        &format!(
            "db restored from {} (safety copy {})",
            source.display(),
            safety.display()
        ),
    );

    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        handle.restart();
    });

    Ok(format!(
        "База восстановлена. Копия текущей базы: {}",
        safety.display()
    ))
}
