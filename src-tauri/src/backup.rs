//! Резервное копирование и восстановление базы данных.

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::Serialize;
use tauri::{AppHandle, State};

use crate::AppState;

const AUTO_KEEP: usize = 5;
const AUTO_INTERVAL_HOURS: i64 = 24;
const ENC_MAGIC: &[u8] = b"LMENC1\n";

fn timestamp() -> String {
    chrono::Local::now().format("%Y%m%d-%H%M%S").to_string()
}

/// Зашифрован ли файл копии (DPAPI).
pub fn is_encrypted(path: &Path) -> Result<bool> {
    let bytes = std::fs::read(path)?;
    Ok(bytes.starts_with(ENC_MAGIC))
}

/// Делает копию базы и шифрует её через DPAPI; открытый файл удаляется.
pub fn write_encrypted_backup(db: &crate::db::Db, dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = dest.with_extension("tmp-plain");
    let result = (|| -> Result<()> {
        db.backup_to(&tmp)?;
        let plain = std::fs::read(&tmp)?;
        let blob = crate::secrets::dpapi::protect(&plain)?;
        let mut data = Vec::with_capacity(ENC_MAGIC.len() + blob.len());
        data.extend_from_slice(ENC_MAGIC);
        data.extend_from_slice(&blob);
        std::fs::write(dest, data)?;
        Ok(())
    })();
    let _ = std::fs::remove_file(&tmp);
    result
}

/// Восстанавливает копию: зашифрованную расшифровывает во временный файл, обычную использует как есть.
pub fn restore_backup(db: &crate::db::Db, source: &Path) -> Result<()> {
    if !source.is_file() {
        anyhow::bail!("Файл копии не найден");
    }
    if !is_encrypted(source)? {
        return db.restore_from(source);
    }
    let data = std::fs::read(source)?;
    let plain = crate::secrets::dpapi::unprotect(&data[ENC_MAGIC.len()..])?;
    let tmp = source.with_extension("restore-tmp.db");
    std::fs::write(&tmp, &plain)?;
    let result = db.restore_from(&tmp);
    let _ = std::fs::remove_file(&tmp);
    result
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
    write_encrypted_backup(db, &dest)?;
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
    write_encrypted_backup(&state.db, &dest).map_err(|e| e.to_string())?;
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
    write_encrypted_backup(&state.db, &safety)
        .map_err(|e| format!("Не удалось сохранить текущую базу: {e}"))?;
    restore_backup(&state.db, &source)
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
    fn encrypted_backup_roundtrip() {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-data")
            .join(format!("backup-enc-{}", crate::db::new_id()));
        std::fs::create_dir_all(&base).unwrap();
        let db = crate::db::Db::open_memory().unwrap();
        let first = db.session_create("Секретная", "local/qwen-1.5b").unwrap();
        let dest = base.join("enc.db");
        write_encrypted_backup(&db, &dest).expect("encrypted backup");

        let bytes = std::fs::read(&dest).unwrap();
        assert!(bytes.starts_with(b"LMENC1\n"));
        assert!(!bytes.starts_with(b"SQLite format 3\0"));

        db.session_create("Лишняя", "local/qwen-1.5b").unwrap();
        assert_eq!(db.sessions_list().unwrap().len(), 2);
        restore_backup(&db, &dest).expect("restore encrypted");
        let sessions = db.sessions_list().unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, first.id);

        let _ = std::fs::remove_dir_all(&base);
    }

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
