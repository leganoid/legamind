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
pub struct PatchPart {
    pub old: String,
    pub new: String,
}

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
    /// Несколько фрагментов за один apply_patch.
    #[serde(default)]
    pub patches: Vec<PatchPart>,
    /// Новый путь для rename_file.
    #[serde(default, alias = "to")]
    pub new_path: Option<String>,
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
        if name == ".git" || name == "node_modules" || name == "target" || name == ".legamind" {
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

pub fn prepare_patch_many(
    workdir: &Path,
    trust: &str,
    raw: &str,
    parts: &[PatchPart],
) -> Result<(WritePreview, PathBuf, String)> {
    if trust == "read_only" {
        bail!("Политика доверия «только чтение» запрещает запись");
    }
    if parts.is_empty() {
        bail!("Пустой список patches");
    }
    let path = resolve_path(workdir, trust, raw)?;
    if !path.is_file() {
        bail!("Файл не найден: {}", path.display());
    }
    let original = read_text(&path)?;
    let mut content = original.clone();
    for (index, part) in parts.iter().enumerate() {
        if part.old.is_empty() {
            bail!("Часть {}: пустой old", index + 1);
        }
        let count = content.matches(&part.old).count();
        if count == 0 {
            bail!("Часть {}: фрагмент не найден", index + 1);
        }
        if count > 1 {
            bail!("Часть {}: фрагмент встречается {count} раз", index + 1);
        }
        content = content.replacen(&part.old, &part.new, 1);
    }
    if content.len() > MAX_WRITE_BYTES {
        bail!("Итоговый файл больше {} КБ", MAX_WRITE_BYTES / 1024);
    }
    let relative = path
        .strip_prefix(workdir)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| path.to_string_lossy().to_string());
    let preview = WritePreview {
        path: path.to_string_lossy().to_string(),
        relative,
        diff: render_diff(&original, &content),
        is_new: false,
        bytes: content.len(),
    };
    Ok((preview, path, content))
}

#[derive(Debug, Clone, Serialize)]
pub struct DeletePreview {
    pub path: String,
    pub relative: String,
    pub bytes: usize,
    pub diff: String,
}

pub fn prepare_delete(
    workdir: &Path,
    trust: &str,
    raw: &str,
) -> Result<(DeletePreview, PathBuf)> {
    if trust == "read_only" {
        bail!("Политика доверия «только чтение» запрещает удаление");
    }
    let path = resolve_path(workdir, trust, raw)?;
    if !path.is_file() {
        bail!("Файл не найден: {}", path.display());
    }
    let text = read_text(&path).unwrap_or_default();
    let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0) as usize;
    let relative = path
        .strip_prefix(workdir)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| path.to_string_lossy().to_string());
    let preview = DeletePreview {
        path: path.to_string_lossy().to_string(),
        relative,
        bytes,
        diff: render_diff(&text, ""),
    };
    Ok((preview, path))
}

#[derive(Debug, Clone, Serialize)]
pub struct MovePreview {
    pub from: String,
    pub to: String,
    pub relative_from: String,
    pub relative_to: String,
}

pub fn prepare_move(
    workdir: &Path,
    trust: &str,
    from_raw: &str,
    to_raw: &str,
) -> Result<(MovePreview, PathBuf, PathBuf)> {
    if trust == "read_only" {
        bail!("Политика доверия «только чтение» запрещает перемещение");
    }
    let from = resolve_path(workdir, trust, from_raw)?;
    let to = resolve_path(workdir, trust, to_raw)?;
    if !from.is_file() {
        bail!("Исходный файл не найден: {}", from.display());
    }
    if to.exists() {
        bail!("Цель уже существует: {}", to.display());
    }
    let relative = |path: &Path| {
        path.strip_prefix(workdir)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| path.to_string_lossy().to_string())
    };
    let preview = MovePreview {
        from: from.to_string_lossy().to_string(),
        to: to.to_string_lossy().to_string(),
        relative_from: relative(&from),
        relative_to: relative(&to),
    };
    Ok((preview, from, to))
}

pub fn apply_move(from: &Path, to: &Path) -> Result<()> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::rename(from, to)?;
    Ok(())
}

pub async fn execute_command(workdir: &Path, command: &str) -> Result<ToolExecution> {    let command = command.trim();
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

/// Копирует файл в `.legamind/backups` перед изменением. Для новых файлов копии нет.
pub fn make_backup(
    workdir: &Path,
    file: &Path,
    created: bool,
) -> Result<Option<String>> {
    if created || !file.is_file() {
        return Ok(None);
    }
    let root = workdir.join(".legamind").join("backups");
    std::fs::create_dir_all(&root)?;
    let name = file
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".to_string());
    let dest = root.join(format!("{}-{name}", crate::db::new_id()));
    std::fs::copy(file, &dest)?;
    prune_backups(&root, 20);
    Ok(Some(dest.to_string_lossy().to_string()))
}

/// Оставляет в папке бэкапов только `keep` самых свежих файлов.
pub fn prune_backups(dir: &Path, keep: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(std::time::SystemTime, std::path::PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_file() {
                return None;
            }
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, path))
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

/// Результат проверки команды перед подтверждением.
#[derive(Debug, Clone)]
pub struct GuardResult {
    pub allowed: bool,
    pub warning: Option<String>,
    pub reason: Option<String>,
}

const DENIED_PATTERNS: &[(&str, &str)] = &[
    ("format ", "форматирование диска"),
    ("diskpart", "управление разделами диска"),
    ("bcdedit", "правка загрузчика"),
    ("vssadmin", "удаление теневых копий"),
    ("wbadmin", "удаление резервных копий Windows"),
    ("cipher /w", "затирание свободного места"),
    ("reg delete", "удаление веток реестра"),
    ("reg add hklm", "правка системного реестра"),
    ("reg add hkcu\\software\\microsoft\\windows\\currentversion\\run", "автозапуск через реестр"),
    ("shutdown", "выключение компьютера"),
    ("restart-computer", "перезагрузка компьютера"),
    ("stop-computer", "выключение компьютера"),
    ("sc delete", "удаление службы"),
    ("schtasks /delete", "удаление задачи планировщика"),
    ("taskkill /f", "принудительное завершение процессов"),
    ("net user", "управление пользователями"),
    ("net localgroup", "управление группами"),
    ("takeown", "смена владельца"),
    ("icacls", "смена прав доступа"),
    ("rm -rf /", "удаление корня файловой системы"),
    ("rm -rf ~", "удаление домашней папки"),
    ("del /f /s /q c:\\", "удаление с корня диска"),
    ("rd /s /q c:\\", "удаление каталогов с корня диска"),
    ("rmdir /s /q c:\\", "удаление каталогов с корня диска"),
    ("powershell -enc", "скрытый закодированный скрипт"),
    ("powershell -encodedcommand", "скрытый закодированный скрипт"),
    ("curl | cmd", "запуск скачанного кода"),
    ("iwr | iex", "запуск скачанного кода"),
];

const WARN_PATTERNS: &[(&str, &str)] = &[
    ("del ", "удаление файлов без корзины"),
    ("rm ", "удаление файлов"),
    ("rd ", "удаление каталогов"),
    ("rmdir ", "удаление каталогов"),
    ("git reset --hard", "потеря незакоммиченных изменений"),
    ("git clean", "удаление неотслеживаемых файлов"),
    ("git push --force", "перезапись удалённой истории"),
    ("npm publish", "публикация пакета"),
    ("cargo publish", "публикация пакета"),
    ("pip install", "изменение окружения Python"),
    ("npm install", "изменение зависимостей"),
];

/// Проверяет команду: опасные — блокируются, потенциально разрушительные — с предупреждением.
pub fn guard_command(command: &str, _trust: &str) -> GuardResult {
    let normalized = command.to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ");
    for (pattern, reason) in DENIED_PATTERNS {
        if normalized.contains(pattern) {
            return GuardResult {
                allowed: false,
                warning: None,
                reason: Some(format!("Запрещённая команда ({reason})")),
            };
        }
    }
    let warnings: Vec<&str> = WARN_PATTERNS
        .iter()
        .filter(|(pattern, _)| normalized.contains(pattern))
        .map(|(_, reason)| *reason)
        .collect();
    GuardResult {
        allowed: true,
        warning: if warnings.is_empty() {
            None
        } else {
            Some(format!("Внимание: {}", warnings.join("; ")))
        },
        reason: None,
    }
}

/// Отменяет последнее изменение файла в сессии.
pub fn undo_last(db: &crate::db::Db, session_id: &str) -> Result<String> {
    let row = db
        .tool_backup_last(session_id)?
        .ok_or_else(|| anyhow::anyhow!("Нет изменений для отмены"))?;
    let path = std::path::PathBuf::from(&row.path);
    let backup = row.backup_path.as_deref().map(std::path::PathBuf::from);
    match row.action.as_str() {
        "write" | "patch" => {
            if row.created {
                if path.exists() {
                    std::fs::remove_file(&path)?;
                }
            } else {
                let backup = backup.ok_or_else(|| anyhow::anyhow!("Нет резервной копии"))?;
                std::fs::copy(&backup, &path)?;
            }
        }
        "delete" => {
            let backup = backup.ok_or_else(|| anyhow::anyhow!("Нет резервной копии"))?;
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&backup, &path)?;
        }
        "move" => {
            let target = row
                .target
                .as_deref()
                .map(std::path::PathBuf::from)
                .ok_or_else(|| anyhow::anyhow!("Нет целевого пути"))?;
            if !target.exists() {
                bail!("Файл уже перемещён или удалён — откат невозможен");
            }
            if path.exists() {
                bail!("Исходный путь занят — откат невозможен");
            }
            std::fs::rename(&target, &path)?;
        }
        other => bail!("Неизвестное действие для отмены: {other}"),
    }
    db.tool_backup_mark_undone(&row.id)?;
    audit("undo", session_id, &format!("{} {}", row.action, row.path));
    Ok(format!("Отменено: {} {}", row.action, row.path))
}

#[tauri::command]
pub async fn tool_undo_last(
    state: tauri::State<'_, AppState>,
    session_id: String,
) -> Result<String, String> {
    undo_last(&state.db, &session_id).map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Serialize)]
pub struct ApprovalPayload {
    pub approval_id: String,
    pub request_id: String,
    pub session_id: String,
    /// "write" | "command" | "delete" | "move"
    pub kind: String,
    pub path: Option<String>,
    pub relative: Option<String>,
    pub diff: Option<String>,
    pub is_new: bool,
    pub bytes: usize,
    pub command: Option<String>,
    pub cwd: Option<String>,
    pub target: Option<String>,
    /// Предупреждение о потенциально опасной команде.
    pub warning: Option<String>,
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
            target: None,
            warning: None,
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
    warning: Option<&str>,
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
            target: None,
            warning: warning.map(|value| value.to_string()),
        },
    )
    .await
}

pub async fn request_delete_approval(
    app: &AppHandle,
    state: &AppState,
    request_id: &str,
    session_id: &str,
    preview: &DeletePreview,
) -> bool {
    wait_approval(
        app,
        state,
        ApprovalPayload {
            approval_id: String::new(),
            request_id: request_id.to_string(),
            session_id: session_id.to_string(),
            kind: "delete".to_string(),
            path: Some(preview.path.clone()),
            relative: Some(preview.relative.clone()),
            diff: Some(preview.diff.clone()),
            is_new: false,
            bytes: preview.bytes,
            command: None,
            cwd: None,
            target: None,
            warning: None,
        },
    )
    .await
}

pub async fn request_move_approval(
    app: &AppHandle,
    state: &AppState,
    request_id: &str,
    session_id: &str,
    preview: &MovePreview,
) -> bool {
    wait_approval(
        app,
        state,
        ApprovalPayload {
            approval_id: String::new(),
            request_id: request_id.to_string(),
            session_id: session_id.to_string(),
            kind: "move".to_string(),
            path: Some(preview.from.clone()),
            relative: Some(preview.relative_from.clone()),
            diff: None,
            is_new: false,
            bytes: 0,
            command: None,
            cwd: None,
            target: Some(preview.relative_to.clone()),
            warning: None,
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
         - {{\"tool\":\"apply_patch\",\"path\":\"a.rs\",\"patches\":[{{\"old\":\"...\",\"new\":\"...\"}},{{\"old\":\"...\",\"new\":\"...\"}}]}} — несколько правок за раз;\n\
         - {{\"tool\":\"delete_file\",\"path\":\"old.rs\"}} — удалить файл;\n\
         - {{\"tool\":\"rename_file\",\"path\":\"a.rs\",\"new_path\":\"b.rs\"}} — переместить/переименовать;\n\
         - {{\"tool\":\"run_command\",\"command\":\"cargo test\"}} — команда в рабочей папке.\n\
         Чтобы вызвать инструмент, ответь ТОЛЬКО JSON-объектом в блоке ```tool … ``` без другого текста.\n\
         После результата инструмента продолжи работу. Когда всё готово — дай обычный ответ без блоков tool.\n\
         Пути — относительно рабочей папки (абсолютные запрещены, кроме доверия «полный доступ»).\n\
         Предпочитай apply_patch перезаписи файла; run_command — для проверок и сборки.\n\
         Удаление и перемещение необратимы — делай их только по прямой просьбе.\n\
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
            let created = !target.exists();
            let backup = make_backup(workdir, &target, created)?;
            let _ = state.db.tool_backup_add(
                session_id,
                request_id,
                "write_file",
                "write",
                &preview.path,
                None,
                backup.as_deref(),
                created,
            );
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
            let (preview, target, updated) = if !call.patches.is_empty() {
                prepare_patch_many(workdir, trust, &path, &call.patches)?
            } else {
                let old = call.old.clone().unwrap_or_default();
                let new = call.new.clone().unwrap_or_default();
                prepare_patch(workdir, trust, &path, &old, &new)?
            };
            let approval_session = app.state::<AppState>();
            if !request_approval(app, &approval_session, request_id, session_id, &preview).await {
                audit("patch_rejected", session_id, &preview.path);
                bail!("Пользователь отклонил правку файла");
            }
            let backup = make_backup(workdir, &target, false)?;
            let _ = state.db.tool_backup_add(
                session_id,
                request_id,
                "apply_patch",
                "patch",
                &preview.path,
                None,
                backup.as_deref(),
                false,
            );
            apply_write(&target, &updated)?;
            let parts = call.patches.len().max(1);
            audit(
                "patch",
                session_id,
                &format!("{} ({} B, {parts} частей)", preview.path, preview.bytes),
            );
            Ok(ToolExecution {
                summary: format!(
                    "apply_patch {} → {} B ({parts} частей, подтверждено)",
                    preview.relative, preview.bytes
                ),
                output: format!(
                    "Файл изменён: {} ({} байт).\nDiff:\n{}",
                    preview.path, preview.bytes, preview.diff
                ),
            })
        }
        "delete_file" | "delete" | "remove_file" => {
            let (preview, target) = prepare_delete(workdir, trust, &path)?;
            let approval_session = app.state::<AppState>();
            if !request_delete_approval(
                app,
                &approval_session,
                request_id,
                session_id,
                &preview,
            )
            .await
            {
                audit("delete_rejected", session_id, &preview.path);
                bail!("Пользователь отклонил удаление файла");
            }
            let backup = make_backup(workdir, &target, false)?;
            let _ = state.db.tool_backup_add(
                session_id,
                request_id,
                "delete_file",
                "delete",
                &preview.path,
                None,
                backup.as_deref(),
                false,
            );
            std::fs::remove_file(&target)?;
            audit(
                "delete",
                session_id,
                &format!("{} ({} B)", preview.path, preview.bytes),
            );
            Ok(ToolExecution {
                summary: format!(
                    "delete_file {} ({} B, подтверждено)",
                    preview.relative, preview.bytes
                ),
                output: format!("Файл удалён: {}", preview.path),
            })
        }
        "rename_file" | "move_file" | "move" | "rename" => {
            let to_raw = call
                .new_path
                .clone()
                .ok_or_else(|| anyhow::anyhow!("Не указан новый путь (new_path)"))?;
            let (preview, from, to) = prepare_move(workdir, trust, &path, &to_raw)?;
            let approval_session = app.state::<AppState>();
            if !request_move_approval(app, &approval_session, request_id, session_id, &preview)
                .await
            {
                audit("move_rejected", session_id, &preview.from);
                bail!("Пользователь отклонил перемещение файла");
            }
            let backup = make_backup(workdir, &from, false)?;
            let _ = state.db.tool_backup_add(
                session_id,
                request_id,
                "rename_file",
                "move",
                &preview.from,
                Some(&preview.to),
                backup.as_deref(),
                false,
            );
            apply_move(&from, &to)?;
            audit(
                "move",
                session_id,
                &format!("{} → {}", preview.from, preview.to),
            );
            Ok(ToolExecution {
                summary: format!(
                    "rename_file {} → {} (подтверждено)",
                    preview.relative_from, preview.relative_to
                ),
                output: format!("Файл перемещён: {} → {}", preview.from, preview.to),
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
            let guard = guard_command(&command, trust);
            if !guard.allowed {
                audit(
                    "command_blocked",
                    session_id,
                    &format!("{command} ({})", guard.reason.clone().unwrap_or_default()),
                );
                bail!("{}: {command}", guard.reason.unwrap_or_else(|| "Команда запрещена".into()));
            }
            let approval_session = app.state::<AppState>();
            if !request_command_approval(
                app,
                &approval_session,
                request_id,
                session_id,
                &command,
                workdir,
                guard.warning.as_deref(),
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
    fn patch_many_delete_and_move() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-data")
            .join("tools-ops");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "alpha\nbeta\n").unwrap();

        let parts = vec![
            PatchPart { old: "alpha".into(), new: "ALPHA".into() },
            PatchPart { old: "beta".into(), new: "BETA".into() },
        ];
        let (preview, _, updated) =
            prepare_patch_many(&dir, "workspace", "a.txt", &parts).expect("many");
        assert_eq!(updated, "ALPHA\nBETA\n");
        assert!(preview.diff.contains("+ALPHA"));

        let (deleted, _) = prepare_delete(&dir, "workspace", "a.txt").expect("delete");
        assert!(deleted.bytes > 0);
        assert!(prepare_delete(&dir, "read_only", "a.txt").is_err());

        std::fs::write(dir.join("x.txt"), "x").unwrap();
        let (moved, _, target) =
            prepare_move(&dir, "workspace", "x.txt", "sub/y.txt").expect("move");
        assert!(moved.relative_to.ends_with("y.txt"));
        assert!(!target.exists());
        assert!(prepare_move(&dir, "workspace", "x.txt", "a.txt").is_err());
    }

    #[test]
    fn changes_are_undoable() {
        let db = crate::db::Db::open_memory().unwrap();
        let session = db.session_create("t", "local/qwen-1.5b").unwrap();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-data")
            .join("tools-undo");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.txt");
        std::fs::write(&file, "one").unwrap();

        // Изменение существующего файла.
        let backup = make_backup(&dir, &file, false).unwrap();
        db.tool_backup_add(
            &session.id,
            "r1",
            "write_file",
            "write",
            &file.to_string_lossy(),
            None,
            backup.as_deref(),
            false,
        )
        .unwrap();
        std::fs::write(&file, "two").unwrap();
        undo_last(&db, &session.id).expect("undo write");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "one");

        // Новый файл — откат удаляет его.
        let created = dir.join("b.txt");
        let backup = make_backup(&dir, &created, true).unwrap();
        db.tool_backup_add(
            &session.id,
            "r2",
            "write_file",
            "write",
            &created.to_string_lossy(),
            None,
            backup.as_deref(),
            true,
        )
        .unwrap();
        std::fs::write(&created, "x").unwrap();
        undo_last(&db, &session.id).expect("undo create");
        assert!(!created.exists());

        // Удаление — откат восстанавливает.
        let backup = make_backup(&dir, &file, false).unwrap();
        db.tool_backup_add(
            &session.id,
            "r3",
            "delete_file",
            "delete",
            &file.to_string_lossy(),
            None,
            backup.as_deref(),
            false,
        )
        .unwrap();
        std::fs::remove_file(&file).unwrap();
        undo_last(&db, &session.id).expect("undo delete");
        assert!(file.exists());

        // Перемещение — откат возвращает на место.
        let target = dir.join("c.txt");
        let backup = make_backup(&dir, &file, false).unwrap();
        db.tool_backup_add(
            &session.id,
            "r4",
            "rename_file",
            "move",
            &file.to_string_lossy(),
            Some(&target.to_string_lossy()),
            backup.as_deref(),
            false,
        )
        .unwrap();
        std::fs::rename(&file, &target).unwrap();
        undo_last(&db, &session.id).expect("undo move");
        assert!(file.exists());
        assert!(!target.exists());
        assert!(undo_last(&db, &session.id).is_err());
    }

    #[test]
    fn command_guard_blocks_and_warns() {
        let blocked = guard_command("format C: /q", "workspace");
        assert!(!blocked.allowed);
        assert!(blocked.reason.unwrap().contains("формат"));

        let blocked = guard_command("rm -rf /", "full");
        assert!(!blocked.allowed);

        let ok = guard_command("cargo test", "workspace");
        assert!(ok.allowed);
        assert!(ok.warning.is_none());

        let warn = guard_command("del notes.txt", "workspace");
        assert!(warn.allowed);
        assert!(warn.warning.is_some());
    }

    #[test]
    fn prune_keeps_newest_backups() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-data")
            .join("tools-prune");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for index in 0..5 {
            std::fs::write(dir.join(format!("b{index}.bak")), "x").unwrap();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        prune_backups(&dir, 2);
        let count = std::fs::read_dir(&dir).unwrap().count();
        assert_eq!(count, 2);
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
