//! Чат: команды, стриминг событий `chat:*`, бюджет контекста.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::Result;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::db::{new_id, ChatMessageRow};
use crate::logging::log_line;
use crate::models::{
    count_local_tokens, estimate_tokens, ChatMessage, ChatParams, ChatStatus, Router,
};
use crate::AppState;

/// Страховочный предел числа сообщений поверх бюджета токенов.
const HISTORY_LIMIT: usize = 200;
/// Минимум токенов, оставляемых под ответ модели.
const MIN_RESERVE: u32 = 512;
/// Контекст по умолчанию, если провайдер не сообщил свой.
const DEFAULT_CONTEXT: u32 = 8192;

#[derive(Debug, Clone, Serialize)]
pub struct ContextUsage {
    pub used: u32,
    pub limit: u32,
    pub reserve: u32,
    pub dropped: u32,
    pub included: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeltaPayload {
    pub request_id: String,
    pub session_id: String,
    pub delta: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatusPayload {
    pub request_id: String,
    pub session_id: String,
    pub phase: String,
    pub label: Option<String>,
    pub device: Option<String>,
    pub seconds: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DonePayload {
    pub request_id: String,
    pub session_id: String,
    pub message: ChatMessageRow,
    pub tokens: Option<i64>,
    pub seconds: f64,
    pub stopped: bool,
    pub context: Option<ContextUsage>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorPayload {
    pub request_id: String,
    pub session_id: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatStart {
    pub request_id: String,
    pub message: ChatMessageRow,
}

#[derive(Debug, Clone, Serialize)]
pub struct RegenStart {
    pub request_id: String,
    pub removed_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EditStart {
    pub request_id: String,
    pub removed_ids: Vec<String>,
    pub message: ChatMessageRow,
}

/// «запомни: …» — команда пользователя сохранить факт.
fn extract_memory_command(text: &str) -> Option<String> {
    let trimmed = text.trim();
    let lower = trimmed.to_lowercase();
    for prefix in ["запомни:", "запомни ", "запомните:"] {
        if lower.starts_with(prefix) {
            let rest = trimmed[prefix.len()..].trim();
            if !rest.is_empty() {
                return Some(rest.to_string());
            }
        }
    }
    None
}

fn system_prompt(memory_block: Option<String>) -> String {
    let mut system =
        String::from("Ты — Legamind, локальный ассистент для разработки и инженерии. Отвечай по-русски, по делу.");
    if let Some(block) = memory_block {
        system.push_str("\n\n");
        system.push_str(&block);
    }
    system
}

/// Предел контекста выбранной модели в токенах.
fn context_limit(state: &AppState, model_id: &str) -> u32 {
    if model_id.starts_with("local/") {
        return crate::models::local_context_tokens(model_id).unwrap_or(DEFAULT_CONTEXT);
    }
    if let Some((provider, model)) = model_id.split_once('/') {
        if let Ok(Some(row)) = state.db.provider_model_get(provider, model) {
            if let Some(context) = row.context.filter(|c| *c > 0) {
                return context as u32;
            }
        }
    }
    DEFAULT_CONTEXT
}

/// Сколько токенов в каждом тексте: точно для локальных моделей, оценкой для облачных.
async fn count_texts(state: &AppState, model_id: &str, texts: &[String]) -> Vec<u32> {
    if model_id.starts_with("local/") {
        if let Ok(counts) = count_local_tokens(state, model_id, texts).await {
            return counts;
        }
    }
    texts.iter().map(|text| estimate_tokens(text)).collect()
}

/// Сколько самых старых сообщений отбросить, чтобы влезть в контекст.
/// Возвращает (сколько отброшено от начала, сколько токенов осталось).
fn fit_window(system_tokens: u32, message_counts: &[u32], limit: u32, reserve: u32) -> (usize, u32) {
    let mut total: u32 = system_tokens + message_counts.iter().sum::<u32>();
    let mut start = 0usize;
    // Последнее сообщение оставляем всегда, даже если оно само больше лимита.
    while total.saturating_add(reserve) > limit && start + 1 < message_counts.len() {
        total = total.saturating_sub(message_counts[start]);
        start += 1;
    }
    (start, total)
}

async fn build_budgeted_messages(
    state: &AppState,
    model_id: &str,
    history: &[ChatMessageRow],
    memory_block: Option<String>,
    reserve: u32,
) -> Result<(Vec<ChatMessage>, ContextUsage)> {
    let limit = context_limit(state, model_id);
    let system = system_prompt(memory_block);

    let dialog: Vec<&ChatMessageRow> = history
        .iter()
        .filter(|m| m.role == "user" || m.role == "assistant")
        .collect();
    let cap_start = dialog.len().saturating_sub(HISTORY_LIMIT);
    let dialog = &dialog[cap_start..];

    let mut texts = Vec::with_capacity(dialog.len() + 1);
    texts.push(system.clone());
    texts.extend(dialog.iter().map(|m| m.content.clone()));
    let counts = count_texts(state, model_id, &texts).await;
    let (system_tokens, message_counts) = match counts.split_first() {
        Some((first, rest)) => (*first, rest),
        None => (0, &[][..]),
    };

    let (dropped_in_window, used) = fit_window(system_tokens, message_counts, limit, reserve);

    let mut messages = vec![ChatMessage::new("system", system)];
    messages.extend(
        dialog[dropped_in_window..]
            .iter()
            .map(|m| ChatMessage::new(&m.role, m.content.clone())),
    );

    Ok((
        messages,
        ContextUsage {
            used,
            limit,
            reserve,
            dropped: (cap_start + dropped_in_window) as u32,
            included: (dialog.len() - dropped_in_window) as u32,
        },
    ))
}

/// Сборка промпта без бюджета — используется тестами и как простое представление.
pub(crate) fn build_messages(
    history: &[ChatMessageRow],
    memory_block: Option<String>,
) -> Vec<ChatMessage> {
    let mut messages = vec![ChatMessage::new("system", system_prompt(memory_block))];
    let start = history.len().saturating_sub(HISTORY_LIMIT);
    messages.extend(
        history[start..]
            .iter()
            .filter(|m| m.role == "user" || m.role == "assistant")
            .map(|m| ChatMessage::new(&m.role, m.content.clone())),
    );
    messages
}

fn spawn_run(app: &AppHandle, state: &AppState, session_id: String, model_id: String) -> String {
    let request_id = new_id();
    let cancel = Arc::new(AtomicBool::new(false));
    state
        .cancel
        .lock()
        .unwrap()
        .insert(request_id.clone(), cancel.clone());

    let app_task = app.clone();
    let request_id_task = request_id.clone();
    tauri::async_runtime::spawn(async move {
        run_chat(app_task, request_id_task, session_id, model_id, cancel).await;
    });
    request_id
}

#[tauri::command]
pub async fn chat_send(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    content: String,
    model_id: String,
) -> Result<ChatStart, String> {
    let content = content.trim().to_string();
    if content.is_empty() {
        return Err("Пустое сообщение".into());
    }
    state
        .db
        .session_get(&session_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Сессия не найдена".to_string())?;

    if let Some(fact) = extract_memory_command(&content) {
        state
            .db
            .memory_add("fact", &fact, Some(&session_id))
            .map_err(|e| e.to_string())?;
    }

    state
        .db
        .session_set_model(&session_id, &model_id)
        .map_err(|e| e.to_string())?;
    let message = state
        .db
        .message_add(&session_id, "user", &content, Some(&model_id), None)
        .map_err(|e| e.to_string())?;

    let request_id = spawn_run(&app, &state, session_id, model_id);
    Ok(ChatStart { request_id, message })
}

#[tauri::command]
pub async fn chat_stop(state: State<'_, AppState>, request_id: String) -> Result<(), String> {
    if let Some(flag) = state.cancel.lock().unwrap().get(&request_id) {
        flag.store(true, Ordering::Relaxed);
    }
    Ok(())
}

/// Перегенерировать последний ответ: удаляем ответ ассистента и повторяем запрос.
#[tauri::command]
pub async fn chat_regenerate(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    model_id: String,
) -> Result<RegenStart, String> {
    state
        .db
        .session_get(&session_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Сессия не найдена".to_string())?;
    state
        .db
        .session_set_model(&session_id, &model_id)
        .map_err(|e| e.to_string())?;

    let removed_ids: Vec<String> = state
        .db
        .message_delete_last_assistant(&session_id)
        .map_err(|e| e.to_string())?
        .into_iter()
        .collect();

    let request_id = spawn_run(&app, &state, session_id, model_id);
    Ok(RegenStart { request_id, removed_ids })
}

/// Изменить сообщение пользователя и переотправить с этого места.
#[tauri::command]
pub async fn chat_edit_and_resend(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    message_id: String,
    content: String,
    model_id: String,
) -> Result<EditStart, String> {
    let content = content.trim().to_string();
    if content.is_empty() {
        return Err("Пустое сообщение".into());
    }
    state
        .db
        .session_get(&session_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Сессия не найдена".to_string())?;

    let removed_ids = state
        .db
        .messages_delete_from(&session_id, &message_id)
        .map_err(|e| e.to_string())?;

    if let Some(fact) = extract_memory_command(&content) {
        state
            .db
            .memory_add("fact", &fact, Some(&session_id))
            .map_err(|e| e.to_string())?;
    }

    state
        .db
        .session_set_model(&session_id, &model_id)
        .map_err(|e| e.to_string())?;
    let message = state
        .db
        .message_add(&session_id, "user", &content, Some(&model_id), None)
        .map_err(|e| e.to_string())?;

    let request_id = spawn_run(&app, &state, session_id, model_id);
    Ok(EditStart { request_id, removed_ids, message })
}

/// Сколько контекста займёт следующий запрос при текущей истории.
#[tauri::command]
pub async fn context_usage(
    state: State<'_, AppState>,
    session_id: String,
    model_id: String,
) -> Result<ContextUsage, String> {
    if model_id.trim().is_empty() {
        return Err("Модель не выбрана".into());
    }
    let history = state.db.messages_list(&session_id).map_err(|e| e.to_string())?;
    let last_user = history
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .map(|m| m.content.clone())
        .unwrap_or_default();
    let memory_block = crate::memory::build_context(&state, &last_user).map_err(|e| e.to_string())?;
    let params = ChatParams::default();
    let (_messages, usage) = build_budgeted_messages(
        &state,
        &model_id,
        &history,
        memory_block,
        params.max_tokens.max(MIN_RESERVE),
    )
    .await
    .map_err(|e| e.to_string())?;
    Ok(usage)
}

async fn run_chat(
    app: AppHandle,
    request_id: String,
    session_id: String,
    model_id: String,
    cancel: Arc<AtomicBool>,
) {
    let state = app.state::<AppState>();

    let result: Result<(crate::models::ChatOutcome, Vec<ChatMessage>, ContextUsage)> = async {
        if model_id.starts_with("local/") {
            crate::sidecar::ensure_ready(&app).await?;
        }
        let history = state.db.messages_list(&session_id)?;
        let last_user = history
            .iter()
            .rev()
            .find(|m| m.role == "user")
            .map(|m| m.content.clone())
            .unwrap_or_default();
        let memory_block = crate::memory::build_context(&state, &last_user)?;

        let params = ChatParams::default();
        let reserve = params.max_tokens.max(MIN_RESERVE);
        let (messages, usage) =
            build_budgeted_messages(&state, &model_id, &history, memory_block, reserve).await?;

        // Логируем собранный промпт без ключей (требование приёмки M4).
        let prompt_dump = messages
            .iter()
            .map(|m| format!("[{}] {}", m.role, m.content))
            .collect::<Vec<_>>()
            .join("\n");
        log_line(
            "prompts.log",
            &format!(
                "=== chat {request_id} model={model_id} context={}/{} dropped={}\n{prompt_dump}",
                usage.used, usage.limit, usage.dropped
            ),
        );

        let router = Router { state: &state };
        let mut on_delta = |text: &str| {
            let payload = DeltaPayload {
                request_id: request_id.clone(),
                session_id: session_id.clone(),
                delta: text.to_string(),
            };
            let _ = app.emit("chat:delta", payload);
        };
        let mut on_status = |status: &ChatStatus| {
            let payload = StatusPayload {
                request_id: request_id.clone(),
                session_id: session_id.clone(),
                phase: status.phase.clone(),
                label: status.label.clone(),
                device: status.device.clone(),
                seconds: status.seconds,
            };
            let _ = app.emit("chat:status", payload);
        };
        let outcome = router
            .stream(&model_id, &messages, &params, &cancel, &mut on_delta, &mut on_status)
            .await?;
        Ok((outcome, messages, usage))
    }
    .await;

    match result {
        Ok((outcome, messages, usage)) => {
            if let Ok(message) = state.db.message_add(
                &session_id,
                "assistant",
                &outcome.text,
                Some(&model_id),
                Some(outcome.tokens as i64),
            ) {
                let _ = state.db.session_touch(&session_id);
                let _ = app.emit(
                    "chat:done",
                    DonePayload {
                        request_id: request_id.clone(),
                        session_id: session_id.clone(),
                        message,
                        tokens: Some(outcome.tokens as i64),
                        seconds: outcome.seconds,
                        stopped: outcome.stopped,
                        context: Some(usage),
                    },
                );
                maybe_save_episode(&state, &session_id, &messages, &outcome.text);
            }
        }
        Err(error) => {
            log_line("app.log", &format!("chat error: {error:#}"));
            let _ = app.emit(
                "chat:error",
                ErrorPayload {
                    request_id: request_id.clone(),
                    session_id: session_id.clone(),
                    error: error.to_string(),
                },
            );
        }
    }

    state.cancel.lock().unwrap().remove(&request_id);
}

/// Длинный чат (>= 6 сообщений, кратно 6) сохраняется как эпизод памяти.
fn maybe_save_episode(
    state: &AppState,
    session_id: &str,
    messages: &[ChatMessage],
    answer: &str,
) {
    let count = messages.len();
    if count < 6 || count % 6 != 0 {
        return;
    }
    let title = state
        .db
        .session_get(session_id)
        .ok()
        .flatten()
        .map(|s| s.title)
        .unwrap_or_else(|| "сессия".to_string());
    let question = messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .map(|m| m.content.chars().take(120).collect::<String>())
        .unwrap_or_default();
    let answer_short: String = answer.chars().take(240).collect();
    let text = format!("Чат «{title}»: вопрос «{question}» — итог: {answer_short}");
    let _ = crate::memory::save_episode(state, &text, Some(session_id));
}

#[cfg(test)]
mod tests {
    use super::{build_messages, extract_memory_command, fit_window};
    use crate::db::ChatMessageRow;

    #[test]
    fn memory_command_is_recognized() {
        assert_eq!(
            extract_memory_command("запомни: в binres используется JFFS2").as_deref(),
            Some("в binres используется JFFS2")
        );
        assert_eq!(
            extract_memory_command("Запомни: отвечай кратко").as_deref(),
            Some("отвечай кратко")
        );
        assert_eq!(extract_memory_command("обычное сообщение"), None);
        assert_eq!(extract_memory_command("запомни:"), None);
    }

    #[test]
    fn memory_block_lands_in_system_prompt() {
        let history = vec![ChatMessageRow {
            id: "m1".into(),
            session_id: "s1".into(),
            role: "user".into(),
            content: "какой формат используется в binres?".into(),
            model_id: None,
            tokens: None,
            created_at: "2026-01-01T00:00:00Z".into(),
        }];
        let block = Some(
            "Известное о проекте (используй, если релевантно):\n- в binres используется JFFS2"
                .to_string(),
        );
        let messages = build_messages(&history, block);
        assert_eq!(messages[0].role, "system");
        assert!(messages[0].content.contains("Известное о проекте"), "{}", messages[0].content);
        assert!(messages[0].content.contains("JFFS2"), "{}", messages[0].content);
        assert_eq!(messages.last().unwrap().content, history[0].content);
    }

    #[test]
    fn context_window_keeps_everything_when_it_fits() {
        let (start, used) = fit_window(100, &[100, 100], 1000, 200);
        assert_eq!(start, 0);
        assert_eq!(used, 300);
    }

    #[test]
    fn context_window_drops_oldest_first() {
        // system 100 + [200, 200, 200] = 700; reserve 200, limit 700 → нужно 900, тесно.
        let (start, used) = fit_window(100, &[200, 200, 200], 700, 200);
        assert_eq!(start, 1, "должно быть отброшено одно самое старое сообщение");
        assert_eq!(used, 500);
    }

    #[test]
    fn context_window_keeps_last_message_even_if_too_big() {
        let (start, used) = fit_window(5000, &[100, 9000], 4096, 512);
        assert_eq!(start, 1, "последнее сообщение не выбрасываем");
        assert_eq!(used, 14000, "в промпт уйдёт system + последнее сообщение");
    }
}
