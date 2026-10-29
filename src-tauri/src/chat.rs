//! Чат: команды, стриминг событий `chat:*`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::Result;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::db::{new_id, ChatMessageRow};
use crate::logging::log_line;
use crate::models::{ChatMessage, ChatParams, Router};
use crate::AppState;

/// Сколько последних сообщений отправляем модели.
const HISTORY_LIMIT: usize = 20;

#[derive(Debug, Clone, Serialize)]
pub struct DeltaPayload {
    pub request_id: String,
    pub session_id: String,
    pub delta: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DonePayload {
    pub request_id: String,
    pub session_id: String,
    pub message: ChatMessageRow,
    pub tokens: Option<i64>,
    pub seconds: f64,
    pub stopped: bool,
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

    let request_id = new_id();
    let cancel = Arc::new(AtomicBool::new(false));
    state
        .cancel
        .lock()
        .unwrap()
        .insert(request_id.clone(), cancel.clone());

    let request_id_task = request_id.clone();
    let app_task = app.clone();
    tauri::async_runtime::spawn(async move {
        run_chat(app_task, request_id_task, session_id, model_id, cancel).await;
    });

    Ok(ChatStart { request_id, message })
}

#[tauri::command]
pub async fn chat_stop(state: State<'_, AppState>, request_id: String) -> Result<(), String> {
    if let Some(flag) = state.cancel.lock().unwrap().get(&request_id) {
        flag.store(true, Ordering::Relaxed);
    }
    Ok(())
}

fn build_messages(history: &[ChatMessageRow], memory_block: Option<String>) -> Vec<ChatMessage> {
    let mut system = String::from(
        "Ты — Legamind, локальный ассистент для разработки и инженерии. Отвечай по-русски, по делу.",
    );
    if let Some(block) = memory_block {
        system.push_str("\n\n");
        system.push_str(&block);
    }
    let mut messages = vec![ChatMessage::new("system", system)];
    let start = history.len().saturating_sub(HISTORY_LIMIT);
    messages.extend(
        history[start..]
            .iter()
            .filter(|m| m.role == "user" || m.role == "assistant")
            .map(|m| ChatMessage::new(&m.role, m.content.clone())),
    );
    messages
}

async fn run_chat(
    app: AppHandle,
    request_id: String,
    session_id: String,
    model_id: String,
    cancel: Arc<AtomicBool>,
) {
    let state = app.state::<AppState>();

    let result: Result<(crate::models::ChatOutcome, Vec<ChatMessage>)> = async {
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
        let messages = build_messages(&history, memory_block);

        // Логируем собранный промпт без ключей (требование приёмки M4).
        let prompt_dump = messages
            .iter()
            .map(|m| format!("[{}] {}", m.role, m.content))
            .collect::<Vec<_>>()
            .join("\n");
        log_line("prompts.log", &format!("=== chat {request_id} model={model_id}\n{prompt_dump}"));

        let params = ChatParams::default();
        let router = Router { state: &state };
        let mut on_delta = |text: &str| {
            let payload = DeltaPayload {
                request_id: request_id.clone(),
                session_id: session_id.clone(),
                delta: text.to_string(),
            };
            let _ = app.emit("chat:delta", payload);
        };
        let outcome = router
            .stream(&model_id, &messages, &params, &cancel, &mut on_delta)
            .await?;
        Ok((outcome, messages))
    }
    .await;

    match result {
        Ok((outcome, messages)) => {
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
