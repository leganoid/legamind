//! Инструменты M6: чтение/запись файлов в рабочей папке сессии.
//!
//! Модель вызывает инструменты JSON-блоком:
//! ```tool
//! {"tool":"read_file","path":"src/main.rs"}
//! ```
//! Запись всегда требует подтверждения пользователя (модалка с diff).

use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use similar::{ChangeTag, TextDiff};
use tauri::{AppHandle, Emitter, Manager};

use crate::AppState;

pub const MAX_READ_BYTES: usize = 256 * 1024;
pub const MAX_WRITE_BYTES: usize = 256 * 1024;
pub const MAX_PROMPT_CHARS: usize = 8000;
pub const MAX_DIFF_CHARS: usize = 12000;
pub const APPROVAL_TIMEOUT_SECS: u64 = 180;

#[derive(Debug, Clone, Deserialize)]
pub struct ToolCall {
    #[serde(alias = "name", alias = "function")]
    pub tool: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
}

/// Ищет вызов инструмента в ответе модели: ```tool … ```, ```json … ``` или голый JSON.
pub fn parse_tool_call(text: &str) -> Option<ToolCall> {
    let mut rest = text;
    while let Some(start) = rest.find("```") {
        let after = &rest[start + 3..];
        let (body, tail) = match after.find("```") {
            Some(end) => (&after[..end], &after[end + 3..]),
            None => break,
        };
        let candidate = body
            .strip_prefix("tool")
            .or_else(|| body.strip_prefix("json"))
            .unwrap_or(body)
            .trim();
        if let Ok(call) = serde_json::from_str::<ToolCall>(candidate) {
            if !call.tool.trim().is_empty() {
                return Some(call);
            }
        }
        rest = tail;
    }
    let trimmed = text.trim();
    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        if let Ok(call) = serde_json::from_str::<ToolCall>(trimmed) {
            if !call.tool.trim().is_empty() {
                return Some(call);
            }
        }
    }
    None
}

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Проверяет путь с учётом политики доверия и рабочей папки.
pub fn resolve_path(workdir: &Path, trust: &str, raw: &str) -> Result<PathBuf> {
    let raw = raw.trim();
    if raw.is_empty() {
        bail!("Пустой путь");
    }
    let raw_path = Path::new(raw);
    let joined = if raw_path.is_absolute() {
        if trust != "full" {
            bail!("Абсолютные пути разрешены только при доверии «полный доступ»");
        }
        raw_path.to_path_buf()
    } else {
        workdir.join(raw_path)
    };
    let normalized = normalize(&joined);

    if trust == "full" {
        return Ok(normalized);
    }

    let base_lex = normalize(workdir);
    if !normalized.starts_with(&base_lex) {
        bail!("Путь выходит за пределы рабочей папки");
    }

    // Для существующих путей дополнительно сверяем канонические адреса (симлинки).
    if normalized.exists() {
        let base = workdir.canonicalize().unwrap_or_else(|_| base_lex.clone());
        let canonical = normalized.canonicalize()?;
        if !canonical.starts_with(&base) {
            bail!("Путь выходит за пределы рабочей папки");
        }
        return Ok(canonical);
    }
    Ok(normalized)
}

fn read_text(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)?;
    if bytes.iter().take(4096).any(|byte| *byte == 0) {
        bail!("Похоже на бинарный файл — не читаю");
    }
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

pub fn render_diff(old: &str, new: &str) -> String {
    let diff = TextDiff::from_lines(old, new);
    let mut out = String::new();
    for change in diff.iter_all_changes() {
        let sign = match change.tag() {
            ChangeTag::Delete => "-",
            ChangeTag::Insert => "+",
            ChangeTag::Equal => " ",
        };
        out.push_str(sign);
        out.push_str(change.value());
        if !change.value().ends_with('\n') {
            out.push('\n');
        }
        if out.chars().count() > MAX_DIFF_CHARS {
            out.push_str("… (diff обрезан)\n");
            break;
        }
    }
    out
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolExecution {
    pub summary: String,
    pub output: String,
}

pub fn list_dir(workdir: &Path, trust: &str, raw: &str) -> Result<ToolExecution> {
    let path = resolve_path(workdir, trust, raw)?;
    if !path.is_dir() {
        bail!("Это не папка");
    }
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    for entry in std::fs::read_dir(&path)?.flatten().take(400) {
        let name = entry.file_name().to_string_lossy().to_string();
        if name == ".git" || name == "node_modules" || name == "target" {
            continue;
        }
        let meta = entry.metadata().ok();
        if meta.as_ref().map(|m| m.is_dir()).unwrap_or(false) {
            dirs.push(format!("{name}/"));
        } else {
            let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            files.push(format!("{name} ({size} B)"));
        }
    }
    dirs.sort();
    files.sort();
    let mut listing = dirs.into_iter().chain(files).collect::<Vec<_>>();
    let total = listing.len();
    listing.truncate(200);
    let output = format!(
        "Содержимое {} ({}):\n{}",
        path.display(),
        total,
        listing.join("\n")
    );
    Ok(ToolExecution {
        summary: format!("list_dir {} → {} записей", raw, total),
        output,
    })
}

pub fn read_file(workdir: &Path, trust: &str, raw: &str) -> Result<ToolExecution> {
    let path = resolve_path(workdir, trust, raw)?;
    if !path.is_file() {
        bail!("Файл не найден: {}", path.display());
    }
    let text = read_text(&path)?;
    let total = text.chars().count();
    let shown: String = text.chars().take(MAX_PROMPT_CHARS).collect();
    let suffix = if total > MAX_PROMPT_CHARS { "\n… (файл обрезан)" } else { "" };
    let output = format!(
        "Файл {} ({total} символов):\n{shown}{suffix}",
        path.display()
    );
    Ok(ToolExecution {
        summary: format!("read_file {raw} → {total} символов"),
        output,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct WritePreview {
    pub path: String,
    pub relative: String,
    pub diff: String,
    pub is_new: bool,
    pub bytes: usize,
}

pub fn prepare_write(
    workdir: &Path,
    trust: &str,
    raw: &str,
    content: &str,
) -> Result<(WritePreview, PathBuf)> {
    if trust == "read_only" {
        bail!("Политика доверия «только чтение» запрещает запись");
    }
    if content.len() > MAX_WRITE_BYTES {
        bail!("Файл больше {} КБ", MAX_WRITE_BYTES / 1024);
    }
    let path = resolve_path(workdir, trust, raw)?;
    if path.is_dir() {
        bail!("Это папка, а не файл");
    }
    let old = if path.exists() {
        read_text(&path).unwrap_or_default()
    } else {
        String::new()
    };
    let is_new = !path.exists();
    let relative = path
        .strip_prefix(workdir)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| path.to_string_lossy().to_string());
    let preview = WritePreview {
        path: path.to_string_lossy().to_string(),
        relative,
        diff: render_diff(&old, content),
        is_new,
        bytes: content.len(),
    };
    Ok((preview, path))
}

pub fn apply_write(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, content)?;
    Ok(())
}

pub fn audit(action: &str, session_id: &str, detail: &str) {
    crate::logging::log_line(
        "audit.log",
        &format!("session={session_id} {action} {detail}"),
    );
}

#[derive(Debug, Clone, Serialize)]
pub struct ApprovalPayload {
    pub approval_id: String,
    pub request_id: String,
    pub session_id: String,
    pub path: String,
    pub relative: String,
    pub diff: String,
    pub is_new: bool,
    pub bytes: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolEventPayload {
    pub request_id: String,
    pub session_id: String,
    pub tool: String,
    pub path: Option<String>,
    pub summary: Option<String>,
    pub ok: bool,
}

/// Ждёт подтверждения пользователя через модалку (oneshot + таймаут).
pub async fn request_approval(
    app: &AppHandle,
    state: &AppState,
    request_id: &str,
    session_id: &str,
    preview: &WritePreview,
) -> bool {
    let approval_id = crate::db::new_id();
    let (tx, rx) = tokio::sync::oneshot::channel::<bool>();
    state
        .approvals
        .lock()
        .unwrap()
        .insert(approval_id.clone(), tx);
    let _ = app.emit(
        "tool:approval",
        ApprovalPayload {
            approval_id: approval_id.clone(),
            request_id: request_id.to_string(),
            session_id: session_id.to_string(),
            path: preview.path.clone(),
            relative: preview.relative.clone(),
            diff: preview.diff.clone(),
            is_new: preview.is_new,
            bytes: preview.bytes,
        },
    );
    let approved = matches!(
        tokio::time::timeout(Duration::from_secs(APPROVAL_TIMEOUT_SECS), rx).await,
        Ok(Ok(true))
    );
    state.approvals.lock().unwrap().remove(&approval_id);
    approved
}

#[tauri::command]
pub async fn tool_approve(
    state: tauri::State<'_, AppState>,
    approval_id: String,
    approve: bool,
) -> Result<(), String> {
    let sender = state.approvals.lock().unwrap().remove(&approval_id);
    if let Some(sender) = sender {
        let _ = sender.send(approve);
    }
    Ok(())
}

/// Описание протокола инструментов для system-промпта.
pub fn tools_prompt(workdir: &str, trust: &str) -> String {
    let write_rule = match trust {
        "read_only" => "Запись запрещена политикой «только чтение» — не вызывай write_file.",
        _ => "Перед записью пользователь увидит diff и подтвердит её.",
    };
    format!(
        "Рабочая папка проекта: `{workdir}`. У тебя есть инструменты:\n\
         - {{\"tool\":\"list_dir\",\"path\":\".\"}} — список файлов;\n\
         - {{\"tool\":\"read_file\",\"path\":\"src/main.rs\"}} — прочитать файл;\n\
         - {{\"tool\":\"write_file\",\"path\":\"notes.md\",\"content\":\"...\"}} — создать/перезаписать файл.\n\
         Чтобы вызвать инструмент, ответь ТОЛЬКО JSON-объектом в блоке ```tool … ``` без другого текста.\n\
         После результата инструмента продолжи работу. Когда всё готово — дай обычный ответ без блоков tool.\n\
         Пути — относительно рабочей папки (абсолютные запрещены, кроме доверия «полный доступ»).\n\
         {write_rule}\n\
         Не выдумывай содержимое файлов: сначала read_file, потом правь."
    )
}

/// Выполняет вызов инструмента (кроме подтверждения записи — оно снаружи).
pub async fn execute_call(
    app: &AppHandle,
    request_id: &str,
    session_id: &str,
    workdir: &Path,
    trust: &str,
    call: &ToolCall,
) -> Result<ToolExecution> {
    let state = app.state::<AppState>();
    let tool = call.tool.trim().to_lowercase();
    let tool = tool.strip_prefix("tools.").unwrap_or(&tool).to_string();
    let path = call.path.clone().unwrap_or_else(|| ".".to_string());
    let _ = app.emit(
        "tool:call",
        ToolEventPayload {
            request_id: request_id.to_string(),
            session_id: session_id.to_string(),
            tool: tool.clone(),
            path: Some(path.clone()),
            summary: None,
            ok: true,
        },
    );

    let execution = match tool.as_str() {
        "list_dir" | "ls" => list_dir(workdir, trust, &path),
        "read_file" | "read" => read_file(workdir, trust, &path),
        "write_file" | "write" => {
            let content = call.content.clone().unwrap_or_default();
            let (preview, target) = prepare_write(workdir, trust, &path, &content)?;
            let approval_session = app.state::<AppState>();
            if !request_approval(app, &approval_session, request_id, session_id, &preview).await {
                audit("write_rejected", session_id, &preview.path);
                bail!("Пользователь отклонил запись файла");
            }
            apply_write(&target, &content)?;
            audit("write", session_id, &format!("{} ({} B)", preview.path, preview.bytes));
            Ok(ToolExecution {
                summary: format!(
                    "write_file {} → {} B (подтверждено)",
                    preview.relative, preview.bytes
                ),
                output: format!(
                    "Файл записан: {} ({} байт).\nDiff:\n{}",
                    preview.path, preview.bytes, preview.diff
                ),
            })
        }
        other => bail!("Неизвестный инструмент: {other}"),
    };

    let execution = execution?;
    let _ = app.emit(
        "tool:done",
        ToolEventPayload {
            request_id: request_id.to_string(),
            session_id: session_id.to_string(),
            tool: tool.clone(),
            path: Some(path),
            summary: Some(execution.summary.clone()),
            ok: true,
        },
    );
    let _ = state;
    Ok(execution)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fenced_and_bare_calls() {
        let fenced = "Думаю.\n```tool\n{\"tool\":\"read_file\",\"path\":\"a.txt\"}\n```";
        let call = parse_tool_call(fenced).expect("fenced");
        assert_eq!(call.tool, "read_file");
        assert_eq!(call.path.as_deref(), Some("a.txt"));

        let bare = "{\"name\":\"list_dir\",\"path\":\".\"}";
        let call = parse_tool_call(bare).expect("bare");
        assert_eq!(call.tool, "list_dir");

        let write = "```json\n{\"tool\":\"write_file\",\"path\":\"x.md\",\"content\":\"hi\"}\n```";
        let call = parse_tool_call(write).expect("write");
        assert_eq!(call.content.as_deref(), Some("hi"));
        assert!(parse_tool_call("обычный ответ").is_none());
    }

    #[test]
    fn workspace_sandbox_blocks_escape() {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-data")
            .join("tools");
        std::fs::create_dir_all(&base).unwrap();
        assert!(resolve_path(&base, "workspace", "../secret.txt").is_err());
        assert!(resolve_path(&base, "workspace", "C:\\Windows\\win.ini").is_err());
        assert!(resolve_path(&base, "workspace", "sub/file.txt").is_ok());
        assert!(resolve_path(&base, "full", "C:\\Windows\\win.ini").is_ok());
    }

    #[test]
    fn diff_marks_added_lines() {
        let diff = render_diff("old\n", "old\nnew\n");
        assert!(diff.contains("+new"), "{diff}");
        assert!(diff.contains(" old"), "{diff}");
    }
}
