//! Резервное копирование и восстановление базы данных.

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::Serialize;
use tauri::{AppHandle, State};

use crate::AppState;

const AUTO_KEEP: usize = 5;
const AUTO_INTERVAL_HOURS: i64 = 24;

fn timestamp() -> String {
    chrono::Local::now().format("%Y%m%d-%H%M%S").to_string()
}

fn backups_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("backups")
}

/// Оставляет только `keep` самых свежих автокопий.
pub fn prune_auto(dir: &Path, keep: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with("auto-") || !name.ends_with(".db") {
                return None;
            }
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, entry.path()))
        })
        .collect();
    if files.len() <= keep {
        return;
    }
    files.sort_by(|left, right| right.0.cmp(&left.0));
    for (_, path) in files.into_iter().skip(keep) {
        let _ = std::fs::remove_file(path);
    }
}

/// Делает автокопию, если с прошлой прошло больше суток. Возвращает путь новой копии.
pub fn auto_backup(db: &crate::db::Db, data_dir: &Path) -> Result<Option<String>> {
    let now = chrono::Utc::now();
    if let Some(last) = db.setting_get("last_auto_backup")? {
        if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(&last) {
            if now.signed_duration_since(parsed).num_hours() < AUTO_INTERVAL_HOURS {
                return Ok(None);
            }
        }
    }
    let dir = backups_dir(data_dir);
    std::fs::create_dir_all(&dir)?;
    let dest = dir.join(format!("auto-{}.db", timestamp()));
    db.backup_to(&dest)?;
    db.setting_set("last_auto_backup", &now.to_rfc3339())?;
    prune_auto(&dir, AUTO_KEEP);
    let _ = crate::logging::log_line(
        "app.log",
        &format!("auto backup → {}", dest.display()),
    );
    Ok(Some(dest.to_string_lossy().to_string()))
}

#[derive(Debug, Clone, Serialize)]
pub struct BackupInfo {
    pub last_auto: Option<String>,
    pub dir: String,
    pub auto_count: usize,
}

#[tauri::command]
pub fn db_backup_info(state: State<'_, AppState>) -> Result<BackupInfo, String> {
    let dir = backups_dir(&state.data_dir);
    let auto_count = std::fs::read_dir(&dir)
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| {
                    let name = entry.file_name().to_string_lossy().to_string();
                    name.starts_with("auto-") && name.ends_with(".db")
                })
                .count()
        })
        .unwrap_or(0);
    Ok(BackupInfo {
        last_auto: state.db.setting_get("last_auto_backup").map_err(|e| e.to_string())?,
        dir: dir.to_string_lossy().to_string(),
        auto_count,
    })
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
    state
        .db
        .setting_set("last_auto_backup", &chrono::Utc::now().to_rfc3339())
        .ok();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_backup_respects_interval_and_prunes() {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-data")
            .join(format!("backup-auto-{}", crate::db::new_id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();

        let db = crate::db::Db::open_memory().unwrap();
        let first = auto_backup(&db, &base).expect("first backup").expect("path");
        assert!(std::path::Path::new(&first).is_file());
        assert!(auto_backup(&db, &base).expect("second").is_none());

        let dir = base.join("backups");
        for index in 0..7 {
            std::fs::write(dir.join(format!("auto-fake{index}.db")), b"x").unwrap();
        }
        prune_auto(&dir, 5);
        let count = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|entry| {
                let name = entry.file_name().to_string_lossy().to_string();
                name.starts_with("auto-") && name.ends_with(".db")
            })
            .count();
        assert_eq!(count, 5);

        let _ = std::fs::remove_dir_all(&base);
    }
}
