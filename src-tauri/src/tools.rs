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
    /// Старый фрагмент для apply_patch.
    #[serde(default)]
    pub old: Option<String>,
    /// Новый фрагмент для apply_patch.
    #[serde(default, alias = "replace")]
    pub new: Option<String>,
    /// Команда для run_command.
    #[serde(default, alias = "cmd")]
    pub command: Option<String>,
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

/// Правка фрагмента: old должен встречаться ровно один раз.
pub fn prepare_patch(
    workdir: &Path,
    trust: &str,
    raw: &str,
    old: &str,
    new: &str,
) -> Result<(WritePreview, PathBuf, String)> {
    if trust == "read_only" {
        bail!("Политика доверия «только чтение» запрещает запись");
    }
    if new.len() > MAX_WRITE_BYTES {
        bail!("Новый фрагмент больше {} КБ", MAX_WRITE_BYTES / 1024);
    }
    let path = resolve_path(workdir, trust, raw)?;
    if path.is_dir() {
        bail!("Это папка, а не файл");
    }
    if old.is_empty() {
        bail!("Поле old пустое: для новой правки укажите точный старый фрагмент");
    }
    if !path.is_file() {
        bail!("Файл не найден: {}", path.display());
    }
    let original = read_text(&path)?;
    let matches = original.matches(old).count();
    if matches == 0 {
        bail!("Фрагмент не найден в файле — прочитайте файл и уточните old");
    }
    if matches > 1 {
        bail!("Фрагмент встречается {matches} раз — нужен более точный old");
    }
    let updated = original.replacen(old, new, 1);
    if updated.len() > MAX_WRITE_BYTES {
        bail!("Итоговый файл больше {} КБ", MAX_WRITE_BYTES / 1024);
    }
    let relative = path
        .strip_prefix(workdir)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| path.to_string_lossy().to_string());
    let preview = WritePreview {
        path: path.to_string_lossy().to_string(),
        relative,
        diff: render_diff(&original, &updated),
        is_new: false,
        bytes: updated.len(),
    };
    Ok((preview, path, updated))
}

pub async fn execute_command(workdir: &Path, command: &str) -> Result<ToolExecution> {
    let command = command.trim();
    if command.is_empty() {
        bail!("Пустая команда");
    }
    let started = std::time::Instant::now();
    let child = tokio::process::Command::new("cmd")
        .arg("/C")
        .arg(command)
        .current_dir(workdir)
        .kill_on_drop(true)
        .output();
    let output = tokio::time::timeout(Duration::from_secs(60), child)
        .await
        .map_err(|_| anyhow::anyhow!("Команда не завершилась за 60 секунд"))??;
    let code = output.status.code().unwrap_or(-1);
    let mut text = String::new();
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.trim().is_empty() {
        text.push_str("\n[stderr]\n");
        text.push_str(&stderr);
    }
    let truncated: String = text.chars().take(MAX_PROMPT_CHARS).collect();
    let suffix = if text.chars().count() > MAX_PROMPT_CHARS { "\n… (вывод обрезан)" } else { "" };
    let output_text = format!(
        "Команда: `{command}`\nПапка: {}\nКод возврата: {code}\n{truncated}{suffix}",
        workdir.display()
    );
    Ok(ToolExecution {
        summary: format!(
            "run_command `{command}` → exit {code} за {:.1} с",
            started.elapsed().as_secs_f64()
        ),
        output: output_text,
    })
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
    /// "write" | "command"
    pub kind: String,
    pub path: Option<String>,
    pub relative: Option<String>,
    pub diff: Option<String>,
    pub is_new: bool,
    pub bytes: usize,
    pub command: Option<String>,
    pub cwd: Option<String>,
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
pub async fn wait_approval(
    app: &AppHandle,
    state: &AppState,
    mut payload: ApprovalPayload,
) -> bool {
    let approval_id = crate::db::new_id();
    payload.approval_id = approval_id.clone();
    let (tx, rx) = tokio::sync::oneshot::channel::<bool>();
    state
        .approvals
        .lock()
        .unwrap()
        .insert(approval_id.clone(), tx);
    let _ = app.emit("tool:approval", payload);
    let approved = matches!(
        tokio::time::timeout(Duration::from_secs(APPROVAL_TIMEOUT_SECS), rx).await,
        Ok(Ok(true))
    );
    state.approvals.lock().unwrap().remove(&approval_id);
    approved
}

pub async fn request_approval(
    app: &AppHandle,
    state: &AppState,
    request_id: &str,
    session_id: &str,
    preview: &WritePreview,
) -> bool {
    wait_approval(
        app,
        state,
        ApprovalPayload {
            approval_id: String::new(),
            request_id: request_id.to_string(),
            session_id: session_id.to_string(),
            kind: "write".to_string(),
            path: Some(preview.path.clone()),
            relative: Some(preview.relative.clone()),
            diff: Some(preview.diff.clone()),
            is_new: preview.is_new,
            bytes: preview.bytes,
            command: None,
            cwd: None,
        },
    )
    .await
}

pub async fn request_command_approval(
    app: &AppHandle,
    state: &AppState,
    request_id: &str,
    session_id: &str,
    command: &str,
    cwd: &Path,
) -> bool {
    wait_approval(
        app,
        state,
        ApprovalPayload {
            approval_id: String::new(),
            request_id: request_id.to_string(),
            session_id: session_id.to_string(),
            kind: "command".to_string(),
            path: None,
            relative: None,
            diff: None,
            is_new: false,
            bytes: 0,
            command: Some(command.to_string()),
            cwd: Some(cwd.to_string_lossy().to_string()),
        },
    )
    .await
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
        "read_only" => "Запись и команды запрещены политикой «только чтение».",
        _ => "Перед записью и командой пользователь увидит diff/команду и подтвердит.",
    };
    format!(
        "Рабочая папка проекта: `{workdir}`. У тебя есть инструменты:\n\
         - {{\"tool\":\"list_dir\",\"path\":\".\"}} — список файлов;\n\
         - {{\"tool\":\"read_file\",\"path\":\"src/main.rs\"}} — прочитать файл;\n\
         - {{\"tool\":\"write_file\",\"path\":\"notes.md\",\"content\":\"...\"}} — создать/перезаписать файл;\n\
         - {{\"tool\":\"apply_patch\",\"path\":\"src/main.rs\",\"old\":\"точный старый фрагмент\",\"new\":\"новый фрагмент\"}} — точечная правка;\n\
         - {{\"tool\":\"run_command\",\"command\":\"cargo test\"}} — команда в рабочей папке.\n\
         Чтобы вызвать инструмент, ответь ТОЛЬКО JSON-объектом в блоке ```tool … ``` без другого текста.\n\
         После результата инструмента продолжи работу. Когда всё готово — дай обычный ответ без блоков tool.\n\
         Пути — относительно рабочей папки (абсолютные запрещены, кроме доверия «полный доступ»).\n\
         Предпочитай apply_patch перезаписи файла; run_command — для проверок и сборки.\n\
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

    let result: Result<ToolExecution> = async {
        match tool.as_str() {
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
        "apply_patch" | "patch" | "edit_file" => {
            let old = call.old.clone().unwrap_or_default();
            let new = call.new.clone().unwrap_or_default();
            let (preview, target, updated) =
                prepare_patch(workdir, trust, &path, &old, &new)?;
            let approval_session = app.state::<AppState>();
            if !request_approval(app, &approval_session, request_id, session_id, &preview).await {
                audit("patch_rejected", session_id, &preview.path);
                bail!("Пользователь отклонил правку файла");
            }
            apply_write(&target, &updated)?;
            audit(
                "patch",
                session_id,
                &format!("{} ({} B)", preview.path, preview.bytes),
            );
            Ok(ToolExecution {
                summary: format!(
                    "apply_patch {} → {} B (подтверждено)",
                    preview.relative, preview.bytes
                ),
                output: format!(
                    "Файл изменён: {} ({} байт).\nDiff:\n{}",
                    preview.path, preview.bytes, preview.diff
                ),
            })
        }
        "run_command" | "run" | "shell" | "exec" => {
            if trust == "read_only" {
                bail!("Политика доверия «только чтение» запрещает команды");
            }
            let command = call
                .command
                .clone()
                .ok_or_else(|| anyhow::anyhow!("Не указана команда"))?;
            let approval_session = app.state::<AppState>();
            if !request_command_approval(
                app,
                &approval_session,
                request_id,
                session_id,
                &command,
                workdir,
            )
            .await
            {
                audit("command_rejected", session_id, &command);
                bail!("Пользователь отклонил команду");
            }
            let execution = execute_command(workdir, &command).await?;
            audit(
                "command",
                session_id,
                &format!("{command} → {}", execution.summary),
            );
            Ok(execution)
        }
        other => bail!("Неизвестный инструмент: {other}"),
        }
    }
    .await;

    let (ok, summary, output) = match &result {
        Ok(execution) => (true, execution.summary.clone(), execution.output.clone()),
        Err(err) => (false, format!("ошибка: {err}"), err.to_string()),
    };
    let _ = state.db.tool_run_add(
        session_id,
        request_id,
        &tool,
        Some(&path),
        ok,
        &summary,
        &output,
    );

    let _ = app.emit(
        "tool:done",
        ToolEventPayload {
            request_id: request_id.to_string(),
            session_id: session_id.to_string(),
            tool: tool.clone(),
            path: Some(path),
            summary: Some(summary),
            ok,
        },
    );
    result
}

/// Блок «что уже делали» для system-промпта (последние вызовы инструментов).
pub fn history_prompt(runs: &[crate::db::ToolRunRow]) -> Option<String> {
    if runs.is_empty() {
        return None;
    }
    let mut out = String::from("Что уже делали с файлами в этой сессии (последние вызовы):\n");
    for run in runs.iter().take(5) {
        let mark = if run.ok { "✓" } else { "✗" };
        let path = run.path.clone().unwrap_or_default();
        out.push_str(&format!(
            "{mark} {} {} — {}\n",
            run.tool,
            path,
            run.summary.clone().unwrap_or_default()
        ));
        if let Some(output) = run.output.as_deref().filter(|text| !text.trim().is_empty()) {
            let snippet: String = output.chars().take(400).collect();
            out.push_str("  ");
            out.push_str(&snippet.replace('\n', "\n  "));
            out.push('\n');
        }
        if out.chars().count() > 3500 {
            break;
        }
    }
    Some(out.trim_end().to_string())
}

#[tauri::command]
pub async fn tool_runs_list(
    state: tauri::State<'_, AppState>,
    session_id: String,
    limit: Option<usize>,
) -> Result<Vec<crate::db::ToolRunRow>, String> {
    state
        .db
        .tool_runs_recent(&session_id, limit.unwrap_or(20).clamp(1, 100))
        .map_err(|e| e.to_string())
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

    #[test]
    fn patch_requires_unique_fragment() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-data")
            .join("tools-patch");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.txt");
        std::fs::write(&file, "hello world\n").unwrap();

        let (preview, _, updated) =
            prepare_patch(&dir, "workspace", "a.txt", "world", "there").expect("patch");
        assert!(updated.contains("hello there"));
        assert!(preview.diff.contains("-hello world"));
        assert!(preview.diff.contains("+hello there"));

        assert!(prepare_patch(&dir, "workspace", "a.txt", "missing", "x").is_err());
        assert!(prepare_patch(&dir, "read_only", "a.txt", "hello", "x").is_err());

        std::fs::write(&file, "x x\n").unwrap();
        assert!(prepare_patch(&dir, "workspace", "a.txt", "x", "y").is_err());
    }

    #[tokio::test]
    async fn runs_simple_command() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-data")
            .join("tools-command");
        std::fs::create_dir_all(&dir).unwrap();
        let result = execute_command(&dir, "echo hello").await.expect("command");
        assert!(result.output.contains("hello"), "{}", result.output);
    }

    #[test]
    fn history_prompt_contains_runs() {
        let runs = vec![crate::db::ToolRunRow {
            id: "1".into(),
            session_id: "s".into(),
            request_id: Some("r".into()),
            tool: "read_file".into(),
            path: Some("notes.md".into()),
            ok: true,
            summary: Some("read_file notes.md → 42 символов".into()),
            output: Some("Файл notes.md:\nпривет".into()),
            created_at: "2026-01-01T00:00:00Z".into(),
        }];
        let block = history_prompt(&runs).expect("history");
        assert!(block.contains("read_file"));
        assert!(block.contains("notes.md"));
        assert!(block.contains("привет"));
    }
}
