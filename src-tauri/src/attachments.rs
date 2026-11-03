//! Вложения: текст/код, PDF и изображения.
//!
//! Произвольные бинарники не анализируются: это приложение для работы с ИИ-моделями,
//! а не hex-анализатор прошивок. Для изображений сообщаем формат и размеры.

use serde::Serialize;

const MAX_BYTES: u64 = 8 * 1024 * 1024;
const MAX_TEXT_CHARS: usize = 100_000;

#[derive(Debug, Clone, Serialize)]
pub struct PickedAttachment {
    pub name: String,
    pub path: String,
    /// text | pdf | image
    pub kind: String,
    pub content: String,
    /// data:image/...;base64 для изображения (для vision-моделей).
    pub data_url: Option<String>,
    pub size: u64,
}

fn image_data_url(format: &str, bytes: &[u8]) -> Option<String> {
    use base64::Engine;
    let mime = match format {
        "PNG" => "image/png",
        "JPEG" => "image/jpeg",
        "GIF" => "image/gif",
        "WebP" => "image/webp",
        _ => return None,
    };
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    Some(format!("data:{mime};base64,{encoded}"))
}

fn image_info(bytes: &[u8]) -> Option<(&'static str, u32, u32)> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") && bytes.len() >= 24 {
        let width = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
        let height = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
        return Some(("PNG", width, height));
    }
    if bytes.starts_with(b"GIF8") && bytes.len() >= 10 {
        let width = u16::from_le_bytes(bytes[6..8].try_into().ok()?) as u32;
        let height = u16::from_le_bytes(bytes[8..10].try_into().ok()?) as u32;
        return Some(("GIF", width, height));
    }
    if bytes.starts_with(&[0xFF, 0xD8]) {
        let mut index = 2usize;
        while index + 9 < bytes.len() {
            if bytes[index] != 0xFF {
                index += 1;
                continue;
            }
            let marker = bytes[index + 1];
            if (0xC0..=0xC3).contains(&marker) {
                let height = u16::from_be_bytes([bytes[index + 5], bytes[index + 6]]) as u32;
                let width = u16::from_be_bytes([bytes[index + 7], bytes[index + 8]]) as u32;
                return Some(("JPEG", width, height));
            }
            let length = u16::from_be_bytes([bytes[index + 2], bytes[index + 3]]) as usize;
            if length < 2 {
                break;
            }
            index += length + 2;
        }
        return Some(("JPEG", 0, 0));
    }
    if bytes.len() > 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some(("WebP", 0, 0));
    }
    None
}

fn looks_text(bytes: &[u8]) -> bool {
    if bytes.iter().take(8192).any(|byte| *byte == 0) {
        return false;
    }
    let sample: Vec<u8> = bytes.iter().take(8192).copied().collect();
    let control = sample
        .iter()
        .filter(|byte| **byte < 0x09 || (**byte > 0x0D && **byte < 0x20))
        .count();
    control * 10 < sample.len().max(1)
}

fn image_description(name: &str, bytes: &[u8]) -> String {
    let mut out = format!("Изображение `{name}` ({} байт).", bytes.len());
    if let Some((format, width, height)) = image_info(bytes) {
        out.push_str(&format!(" Формат: {format}"));
        if width > 0 && height > 0 {
            out.push_str(&format!(", {width}×{height}"));
        }
        out.push('.');
    }
    out.push_str(
        "\nТекстовая модель не анализирует пиксели. Для описания изображения нужна \
         vision-модель (поддержка появится отдельно).",
    );
    out
}

fn extract_pdf(bytes: &[u8]) -> Option<String> {
    match pdf_extract::extract_text_from_mem(bytes) {
        Ok(text) => {
            let text = text.trim().to_string();
            if text.is_empty() {
                None
            } else {
                Some(text.chars().take(MAX_TEXT_CHARS).collect())
            }
        }
        Err(_) => None,
    }
}

#[tauri::command]
pub async fn pick_text_attachment() -> Result<Option<PickedAttachment>, String> {
    #[cfg(target_os = "android")]
    let file: Option<std::path::PathBuf> = None;
    #[cfg(not(target_os = "android"))]
    let file = rfd::FileDialog::new()
        .set_title("Приложить файл: текст, PDF или изображение")
        .add_filter(
            "Документы и код",
            &[
                "txt", "md", "rs", "ts", "tsx", "js", "jsx", "json", "toml", "py", "css",
                "html", "yml", "yaml", "sql", "ps1", "c", "h", "cpp", "hpp", "cs", "go",
                "java", "sh", "bat", "ini", "cfg", "conf", "log", "pdf",
            ],
        )
        .add_filter("Изображения", &["png", "jpg", "jpeg", "webp", "gif", "bmp"])
        .add_filter("Все файлы", &["*"])
        .pick_file();
    let Some(path) = file else {
        return Ok(None);
    };
    let metadata = std::fs::metadata(&path).map_err(|e| e.to_string())?;
    if metadata.len() > MAX_BYTES {
        return Err(format!(
            "Файл больше {} МБ — приложите фрагмент поменьше",
            MAX_BYTES / (1024 * 1024)
        ));
    }
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    let name = path
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".to_string());
    let is_pdf = bytes.starts_with(b"%PDF");
    let mut data_url = None;
    let (kind, content) = if is_pdf {
        match extract_pdf(&bytes) {
            Some(text) => ("pdf".to_string(), format!("Текст PDF `{name}`:\n{text}")),
            None => {
                return Err(
                    "В PDF не найден текстовый слой (возможно, это скан). Приложите PDF с текстом."
                        .into(),
                )
            }
        }
    } else if looks_text(&bytes) {
        let text: String = String::from_utf8_lossy(&bytes)
            .chars()
            .take(MAX_TEXT_CHARS)
            .collect();
        ("text".to_string(), text)
    } else if let Some((format, _, _)) = image_info(&bytes) {
        data_url = image_data_url(format, &bytes);
        ("image".to_string(), image_description(&name, &bytes))
    } else {
        return Err(format!(
            "`{name}` — бинарный файл. Поддерживаются текст/код, PDF и изображения; \
             hex-анализ не входит в задачи приложения."
        ));
    };

    Ok(Some(PickedAttachment {
        name,
        path: path.to_string_lossy().to_string(),
        kind,
        content,
        data_url,
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

    #[test]
    fn detects_png_size() {
        let mut png = vec![0u8; 33];
        png[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        png[16..20].copy_from_slice(&640u32.to_be_bytes());
        png[20..24].copy_from_slice(&480u32.to_be_bytes());
        let info = image_info(&png).expect("png");
        assert_eq!(info.0, "PNG");
        assert_eq!((info.1, info.2), (640, 480));
    }
}
