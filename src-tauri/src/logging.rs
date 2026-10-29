//! Простое логирование в `%APPDATA%\com.legamind.app\logs\`.
//! Ключи и заголовки Authorization сюда не попадают по построению.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

pub fn log_dir() -> PathBuf {
    let base = std::env::var("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    base.join("com.legamind.app").join("logs")
}

pub fn log_line(file: &str, line: &str) {
    let dir = log_dir();
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    if let Ok(mut handle) = OpenOptions::new().create(true).append(true).open(dir.join(file)) {
        let _ = writeln!(handle, "[{stamp}] {line}");
    }
}
