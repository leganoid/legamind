//! Вложения: выбор текстового/кодового файла и чтение в контекст запроса.

use std::path::PathBuf;

use serde::Serialize;

const MAX_BYTES: u64 = 512 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct PickedAttachment {
    pub name: String,
    pub path: String,
    pub content: String,
    pub size: u64,
}

#[tauri::command]
pub async fn pick_text_attachment() -> Result<Option<PickedAttachment>, String> {
    let file = rfd::FileDialog::new()
        .set_title("Приложить текстовый или кодовый файл")
        .add_filter(
            "Текст и код",
            &[
                "txt", "md", "rs", "ts", "tsx", "js", "jsx", "json", "toml", "py", "css",
                "html", "yml", "yaml", "sql", "ps1", "c", "h", "cpp", "hpp", "cs", "go",
                "java", "sh", "bat", "ini", "cfg", "conf", "log",
            ],
        )
        .add_filter("Все файлы", &["*"])
        .pick_file();
    let Some(path) = file else {
        return Ok(None);
    };
    let metadata = std::fs::metadata(&path).map_err(|e| e.to_string())?;
    if metadata.len() > MAX_BYTES {
        return Err(format!(
            "Файл больше {} КБ — приложите фрагмент поменьше",
            MAX_BYTES / 1024
        ));
    }
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    if bytes.iter().take(4096).any(|byte| *byte == 0) {
        return Err("Похоже на бинарный файл — поддерживаются текстовые и кодовые файлы".into());
    }
    let content = String::from_utf8_lossy(&bytes).to_string();
    Ok(Some(PickedAttachment {
        name: path
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_else(|| "file".to_string()),
        path: path.to_string_lossy().to_string(),
        content,
        size: metadata.len(),
    }))
}

/// Проверка, что размер вложения допустим (используется тестом).
pub fn attachment_size_ok(size: u64) -> bool {
    size <= MAX_BYTES
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_limit() {
        assert!(attachment_size_ok(1024));
        assert!(!attachment_size_ok(MAX_BYTES + 1));
    }
}
