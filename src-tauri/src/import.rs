use std::path::Path;

use anyhow::Result;
use serde::Serialize;
use tauri::State;

use crate::AppState;

#[derive(Debug, Serialize)]
pub struct ImportResult {
    pub session_id: String,
    pub title: String,
    pub messages: usize,
}

fn normalize_role(role: &str) -> Option<&'static str> {
    match role.trim().to_lowercase().as_str() {
        "user" | "human" | "you" | "пользователь" => Some("user"),
        "assistant" | "ai" | "bot" | "chatgpt" | "claude" | "ассистент" => Some("assistant"),
        "system" | "system prompt" | "система" => Some("system"),
        _ => None,
    }
}

fn content_text(value: &serde_json::Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        return Some(text.to_string());
    }
    if let Some(parts) = value.as_array() {
        let mut out = Vec::new();
        for part in parts {
            if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                out.push(text.to_string());
            } else if let Some(text) = part.get("content").and_then(|v| v.as_str()) {
                out.push(text.to_string());
            }
        }
        if !out.is_empty() {
            return Some(out.join("\n"));
        }
    }
    value
        .get("text")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn parse_json(text: &str) -> Option<Vec<(String, String)>> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let items = value
        .as_array()
        .or_else(|| value.get("messages").and_then(|v| v.as_array()))
        .or_else(|| value.get("conversation").and_then(|v| v.as_array()))?;
    let mut out = Vec::new();
    for item in items {
        let role = item.get("role").and_then(|v| v.as_str())?;
        let Some(role) = normalize_role(role) else {
            continue;
        };
        let content = item.get("content").and_then(content_text).unwrap_or_default();
        let content = content.trim();
        if !content.is_empty() {
            out.push((role.to_string(), content.to_string()));
        }
    }
    Some(out)
}

fn role_prefix(line: &str) -> Option<(String, String)> {
    let stripped = line
        .trim()
        .trim_start_matches('#')
        .trim()
        .trim_matches('*')
        .trim();
    let lower = stripped.to_lowercase();
    for (label, role) in [
        ("system", "system"),
        ("система", "system"),
        ("user", "user"),
        ("human", "user"),
        ("you", "user"),
        ("пользователь", "user"),
        ("assistant", "assistant"),
        ("chatgpt", "assistant"),
        ("claude", "assistant"),
        ("ai", "assistant"),
        ("bot", "assistant"),
        ("ассистент", "assistant"),
    ] {
        if lower == label {
            return Some((role.to_string(), String::new()));
        }
        if let Some(rest) = lower.strip_prefix(label) {
            if rest.starts_with(':') {
                let inline = stripped[label.len()..].trim_start_matches(':').trim();
                return Some((role.to_string(), inline.to_string()));
            }
        }
    }
    None
}

fn parse_markdown(text: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut current: Option<String> = None;
    let mut buffer = String::new();

    let flush = |out: &mut Vec<(String, String)>, role: &Option<String>, buffer: &mut String| {
        if let Some(role) = role {
            let content = buffer.trim();
            if !content.is_empty() {
                out.push((role.clone(), content.to_string()));
            }
        }
        buffer.clear();
    };

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some((role, inline)) = role_prefix(trimmed) {
            flush(&mut out, &current, &mut buffer);
            current = Some(role);
            buffer = inline;
            continue;
        }
        if current.is_some() {
            if !buffer.is_empty() {
                buffer.push('\n');
            }
            buffer.push_str(trimmed);
        }
    }
    flush(&mut out, &current, &mut buffer);
    out
}

pub fn parse_transcript(text: &str) -> Option<Vec<(String, String)>> {
    let trimmed = text.trim_start();
    let messages = if trimmed.starts_with('{') || trimmed.starts_with('[') {
        parse_json(trimmed)?
    } else {
        parse_markdown(trimmed)
    };
    if messages.is_empty() {
        return None;
    }
    Some(messages)
}

#[tauri::command]
pub async fn import_chat(
    state: State<'_, AppState>,
    path: String,
) -> Result<ImportResult, String> {
    let target = Path::new(&path);
    let text = std::fs::read_to_string(target).map_err(|e| format!("Не удалось прочитать файл: {e}"))?;
    if text.len() > 2_000_000 {
        return Err("Файл больше 2 МБ — импорт отменён".into());
    }
    let messages = parse_transcript(&text).ok_or_else(|| {
        "Не найден формат чата: нужен JSON с role/content или Markdown с «User:»/«Assistant:»"
            .to_string()
    })?;
    if messages.len() > 500 {
        return Err("В файле больше 500 сообщений — импорт отменён".into());
    }
    let stem = target
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("chat");
    let title = format!("Импорт: {stem}");
    let session = state
        .db
        .session_create(&title, "")
        .map_err(|e| e.to_string())?;
    let mut imported = 0usize;
    for (role, content) in &messages {
        state
            .db
            .message_add(&session.id, role, content, None, None)
            .map_err(|e| e.to_string())?;
        imported += 1;
    }
    let _ = crate::logging::log_line(
        "app.log",
        &format!("chat imported: {path} -> {} ({imported} messages)", session.id),
    );
    Ok(ImportResult {
        session_id: session.id,
        title,
        messages: imported,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_json_role_content() {
        let text = r#"[{"role":"user","content":"привет"},{"role":"assistant","content":[{"type":"text","text":"ответ"}]}]"#;
        let messages = parse_transcript(text).unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].1, "привет");
        assert_eq!(messages[1].1, "ответ");
    }

    #[test]
    fn parses_markdown_headers() {
        let text = "## User:\nПривет\n\n## Assistant:\nЗдравствуйте\n\nЕщё строка";
        let messages = parse_transcript(text).unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].0, "user");
        assert_eq!(messages[0].1, "Привет");
        assert!(messages[1].1.contains("Ещё строка"));
    }

    #[test]
    fn rejects_plain_text() {
        assert!(parse_transcript("просто текст без ролей").is_none());
    }
}
