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

/// Подсказка режима работы, добавляемая к system-промпту.
fn mode_prompt(mode: &str) -> Option<&'static str> {
    match mode {
        "plan" => Some(
            "Режим «Планирование». Сначала уточни цель и ограничения, затем предложи план шагами \
             (что и в каком порядке делать). Не пиши финальный код и не делай вид, что изменения уже \
             внесены; в конце спроси подтверждение плана.",
        ),
        "build" => Some(
            "Режим «Создание». Сразу выдавай рабочий результат: готовый код, команды, структуру файлов \
             и точные шаги. Минимум рассуждений и вступлений; если чего-то не хватает — сделай разумное \
             допущение и явно отметь его.",
        ),
        "review" => Some(
            "Режим «Ревью». Проверяй решение: ищи ошибки, риски, уязвимости и краевые случаи, \
             предлагай конкретные исправления с примерами. Не переписывай всё без нужды.",
        ),
        _ => None,
    }
}

fn system_prompt(
    memory_block: Option<String>,
    skills_block: Option<String>,
    lessons_block: Option<String>,
    web_block: Option<String>,
    mode: &str,
) -> String {
    let mut system =
        String::from("Ты — Legamind, локальный ассистент для разработки и инженерии. Отвечай по-русски, по делу.");
    if let Some(mode_text) = mode_prompt(mode) {
        system.push_str("\n\n");
        system.push_str(mode_text);
    }
    if let Some(skills) = skills_block {
        system.push_str("\n\n");
        system.push_str(&skills);
    }
    if let Some(lessons) = lessons_block {
        system.push_str("\n\n");
        system.push_str(&lessons);
    }
    if let Some(web) = web_block {
        system.push_str("\n\n");
        system.push_str(&web);
    }
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
    skills_block: Option<String>,
    lessons_block: Option<String>,
    web_block: Option<String>,
    mode: &str,
    reserve: u32,
) -> Result<(Vec<ChatMessage>, ContextUsage)> {
    let limit = context_limit(state, model_id);
    let system = system_prompt(memory_block, skills_block, lessons_block, web_block, mode);

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
    skills_block: Option<String>,
    lessons_block: Option<String>,
    web_block: Option<String>,
    mode: &str,
) -> Vec<ChatMessage> {
    let mut messages = vec![ChatMessage::new(
        "system",
        system_prompt(memory_block, skills_block, lessons_block, web_block, mode),
    )];
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
    let skills_block = crate::skills::active_block(&state, &last_user).map_err(|e| e.to_string())?;
    let lessons_block = crate::learning::lessons_block(&state).map_err(|e| e.to_string())?;
    let session = state
        .db
        .session_get(&session_id)
        .map_err(|e| e.to_string())?;
    let mode = session.as_ref().map(|s| s.mode.as_str()).unwrap_or("chat");
    let params = ChatParams::default();
    let (_messages, usage) = build_budgeted_messages(
        &state,
        &model_id,
        &history,
        memory_block,
        skills_block,
        lessons_block,
        None,
        mode,
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

    let result: Result<(crate::models::ChatOutcome, Vec<ChatMessage>, ContextUsage, Vec<crate::web::WebResult>)> = async {
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
        let skills_block = crate::skills::active_block(&state, &last_user)?;
        let lessons_block = crate::learning::lessons_block(&state)?;

        // Интернет: ищем до обращения к модели, чтобы вставить свежие данные в контекст.
        if crate::web::enabled(&state) && crate::web::should_search(&last_user) {
            let _ = app.emit(
                "chat:status",
                StatusPayload {
                    request_id: request_id.clone(),
                    session_id: session_id.clone(),
                    phase: "web".to_string(),
                    label: Some("Ищу в интернете…".to_string()),
                    device: None,
                    seconds: None,
                },
            );
        }
        let (web_block, sources) = crate::web::context_for(&state, &last_user).await;

        let session = state.db.session_get(&session_id)?;
        let mode = session.as_ref().map(|s| s.mode.as_str()).unwrap_or("chat");
        let mut params = ChatParams::default();
        if let Some(policy) = session.as_ref().and_then(|s| s.reasoning.as_deref()) {
            match policy {
                "on" => params.think = true,
                "off" => params.think = false,
                other => params.reasoning = Some(other.to_string()),
            }
        }
        let reserve = params.max_tokens.max(MIN_RESERVE);
        let (messages, usage) = build_budgeted_messages(
            &state,
            &model_id,
            &history,
            memory_block,
            skills_block,
            lessons_block,
            web_block,
            mode,
            reserve,
        )
        .await?;

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

        // M6: если задана рабочая папка — модель может читать/писать файлы.
        if let Some(session_ref) = session.as_ref() {
            if let Some(workdir) = session_ref.workdir.clone() {
                let trust = session_ref.trust.clone();
                let workdir_path = std::path::PathBuf::from(&workdir);
                let mut conversation = messages.clone();
                let mut history_runs = 0usize;
                let mut rag_used = false;
                if let Some(system) = conversation.first_mut() {
                    system.content.push_str("\n\n");
                    system.content.push_str(&crate::tools::tools_prompt(&workdir, &trust));
                    if let Ok(runs) = state.db.tool_runs_recent(&session_id, 5) {
                        history_runs = runs.len();
                        if let Some(history) = crate::tools::history_prompt(&runs) {
                            system.content.push_str("\n\n");
                            system.content.push_str(&history);
                        }
                    }
                    if let Some(rag) = crate::rag::context_block(&state.db, &last_user, 5) {
                        rag_used = true;
                        system.content.push_str("\n\n");
                        system.content.push_str(&rag);
                    }
                }
                params.max_tokens = params.max_tokens.max(1536);
                log_line(
                    "prompts.log",
                    &format!(
                        "=== tools {request_id} session={session_id} workdir={workdir} history_runs={history_runs} rag={rag_used}"
                    ),
                );

                for _ in 0..4 {
                    let outcome = router
                        .stream(
                            &model_id,
                            &conversation,
                            &params,
                            &cancel,
                            &mut |_| {},
                            &mut on_status,
                        )
                        .await?;
                    if let Some(call) = crate::tools::parse_tool_call(&outcome.text) {
                        let execution = crate::tools::execute_call(
                            &app,
                            &request_id,
                            &session_id,
                            &workdir_path,
                            &trust,
                            &call,
                        )
                        .await
                        .unwrap_or_else(|err| {
                            crate::tools::audit("tool_error", &session_id, &err.to_string());
                            crate::tools::ToolExecution {
                                summary: format!("ошибка {err}"),
                                output: format!("Ошибка инструмента: {err}"),
                            }
                        });
                        conversation.push(ChatMessage::new(
                            "user",
                            format!(
                                "[Результат инструмента {}]\n{}\n(продолжай; если готов — ответь обычным текстом без блока tool)",
                                call.tool, execution.output
                            ),
                        ));
                        continue;
                    }
                    let _ = app.emit(
                        "chat:delta",
                        DeltaPayload {
                            request_id: request_id.clone(),
                            session_id: session_id.clone(),
                            delta: outcome.text.clone(),
                        },
                    );
                    return Ok((outcome, conversation, usage, sources));
                }

                // Лимит раундов: просим финальный ответ без инструментов.
                conversation.push(ChatMessage::new(
                    "user",
                    "Лимит инструментов исчерпан. Дай финальный ответ без вызова инструментов.",
                ));
                let outcome = router
                    .stream(
                        &model_id,
                        &conversation,
                        &params,
                        &cancel,
                        &mut |_| {},
                        &mut on_status,
                    )
                    .await?;
                let _ = app.emit(
                    "chat:delta",
                    DeltaPayload {
                        request_id: request_id.clone(),
                        session_id: session_id.clone(),
                        delta: outcome.text.clone(),
                    },
                );
                return Ok((outcome, conversation, usage, sources));
            }
        }

        let outcome = router
            .stream(&model_id, &messages, &params, &cancel, &mut on_delta, &mut on_status)
            .await?;
        Ok((outcome, messages, usage, sources))
    }
    .await;

    match result {
        Ok((outcome, messages, usage, sources)) => {
            if let Ok(message) = state.db.message_add(
                &session_id,
                "assistant",
                &outcome.text,
                Some(&model_id),
                Some(outcome.tokens as i64),
            ) {
                for source in &sources {
                    let _ = state.db.source_add(
                        &session_id,
                        &message.id,
                        &source.title,
                        &source.url,
                        &source.snippet,
                    );
                }
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

// ------------------------------------------------------------------ слэш-команды

#[derive(Debug, Clone, Serialize)]
pub struct CommandResult {
    pub kind: String,
    pub text: String,
    pub removed_ids: Vec<String>,
    pub message: Option<ChatMessageRow>,
}

fn parse_command(input: &str) -> Option<(String, String)> {
    let trimmed = input.trim();
    let rest = trimmed.strip_prefix('/')?;
    if rest.is_empty() {
        return None;
    }
    let mut parts = rest.splitn(2, char::is_whitespace);
    let cmd = parts.next().unwrap_or("").to_lowercase();
    let args = parts.next().unwrap_or("").trim().to_string();
    Some((cmd, args))
}

fn notice(text: impl Into<String>) -> Result<CommandResult, String> {
    Ok(CommandResult {
        kind: "notice".to_string(),
        text: text.into(),
        removed_ids: vec![],
        message: None,
    })
}

pub fn help_text() -> String {
    [
        "Команды:",
        "/help — этот список",
        "/compact — сжать историю в сводку (освобождает контекст)",
        "/clear — очистить историю сессии",
        "/remember <текст> — сохранить факт в память",
        "/skills — список навыков",
        "/skill <название> — включить/выключить навык",
        "/export — выгрузить переписку в .md",
        "/pwd — рабочая папка проекта",
        "/workdir <путь> — задать рабочую папку (создаст файлы проекта)",
        "/web <запрос> — поиск в интернете, /web on|off — вкл/выкл",
        "",
        "Без слэша: «запомни: …» тоже сохраняет факт.",
    ]
    .join("\n")
}

/// Последние сообщения для сводки: не больше 40 реплик и ~12k символов.
fn compact_transcript(history: &[ChatMessageRow]) -> String {
    const TOTAL: usize = 12_000;
    let mut lines: Vec<String> = Vec::new();
    let mut size = 0usize;
    for message in history.iter().rev().take(40) {
        let role = if message.role == "user" { "Пользователь" } else { "Ассистент" };
        let body: String = message.content.chars().take(1200).collect();
        let line = format!("{role}: {body}");
        if size + line.len() > TOTAL {
            break;
        }
        size += line.len() + 2;
        lines.push(line);
    }
    lines.reverse();
    lines.join("\n\n")
}

/// Сжатие истории: модель пересказывает диалог, старые сообщения заменяются сводкой.
/// Сайдкар должен быть уже поднят (это делает `chat_command`).
pub async fn compact_session(
    state: &AppState,
    session_id: &str,
    model_id: &str,
) -> Result<CommandResult, String> {
    if model_id.trim().is_empty() {
        return Err("Модель не выбрана".into());
    }
    let history = state.db.messages_list(session_id).map_err(|e| e.to_string())?;
    if history.len() < 2 {
        return Err("История слишком короткая для сжатия".into());
    }
    let transcript = compact_transcript(&history);

    let messages = vec![
        ChatMessage::new(
            "system",
            "Ты — редактор, который сжимает диалоги в подробную сводку. Пиши по-русски, только по делу.",
        ),
        ChatMessage::new(
            "user",
            format!(
                "Сожми диалог ниже в подробную сводку: сохрани цель, принятые решения, факты, \
                 незавершённые задачи и важные детали. Не выдумывай. Верни только текст сводки.\n\n\
                 ---\n{transcript}\n---\n\nСводка:"
            ),
        ),
    ];
    let params = ChatParams { max_tokens: 900, temperature: 0.3, think: false, reasoning: None };
    let cancel = Arc::new(AtomicBool::new(false));
    let router = Router { state };
    let outcome = router
        .stream(
            model_id,
            &messages,
            &params,
            &cancel,
            &mut |_| {},
            &mut |_: &ChatStatus| {},
        )
        .await
        .map_err(|e| format!("Не удалось сжать историю: {e}"))?;

    let summary = outcome.text.trim().to_string();
    if summary.is_empty() {
        return Err("Модель не вернула сводку".into());
    }

    let removed_ids = state
        .db
        .messages_delete_all(session_id)
        .map_err(|e| e.to_string())?;
    let content = format!("📝 **Сводка прошлого диалога** (история сжата)\n\n{summary}");
    let message = state
        .db
        .message_add(
            session_id,
            "assistant",
            &content,
            Some(model_id),
            Some(outcome.tokens as i64),
        )
        .map_err(|e| e.to_string())?;
    let _ = state.db.session_touch(session_id);
    let _ = crate::memory::save_episode(
        state,
        &format!("Сжатие диалога: {}", summary.chars().take(300).collect::<String>()),
        Some(session_id),
    );
    log_line(
        "prompts.log",
        &format!(
            "=== compact {session_id} model={model_id} tokens={} compressed={}\n{transcript}",
            outcome.tokens,
            removed_ids.len()
        ),
    );

    Ok(CommandResult {
        kind: "compact".to_string(),
        text: format!("История сжата: удалено {} → осталась одна сводка", removed_ids.len()),
        removed_ids,
        message: Some(message),
    })
}

fn export_session(
    db: &crate::db::Db,
    data_dir: &std::path::Path,
    session_id: &str,
) -> Result<CommandResult, String> {
    let session = db
        .session_get(session_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Сессия не найдена".to_string())?;
    if session.trust == "read_only" {
        return Err("Политика доверия «только чтение» запрещает запись файлов".into());
    }
    let history = db.messages_list(session_id).map_err(|e| e.to_string())?;
    if history.is_empty() {
        return Err("В сессии нет сообщений".into());
    }
    let mut md = format!("# Чат «{}»\n\n_{}_\n\n", session.title, session.created_at);
    if let Some(workdir) = session.workdir.as_deref() {
        md.push_str(&format!("_Рабочая папка: `{workdir}`_\n\n"));
    }
    for message in &history {
        let who = if message.role == "user" {
            "Вы".to_string()
        } else {
            message
                .model_id
                .as_deref()
                .and_then(crate::models::local_label)
                .map(|s| s.to_string())
                .unwrap_or_else(|| "Ассистент".to_string())
        };
        md.push_str(&format!("## {who}\n\n{}\n\n", message.content));
    }
    let dir = match session.workdir.as_deref() {
        Some(workdir) => std::path::PathBuf::from(workdir).join("exports"),
        None => data_dir.join("exports"),
    };
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("chat-{session_id}.md"));
    std::fs::write(&path, md).map_err(|e| e.to_string())?;
    notice(format!("Экспортировано: {}", path.display()))
}

/// `/pwd` и `/workdir <path>` — рабочая папка проекта сессии.
fn workdir_command(
    db: &crate::db::Db,
    session_id: &str,
    action: &str,
    args: &str,
) -> Result<CommandResult, String> {
    let session = db
        .session_get(session_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Сессия не найдена".to_string())?;

    if action == "pwd" || args.trim().is_empty() {
        return match session.workdir.as_deref() {
            Some(path) => notice(format!("Рабочая папка: {path}")),
            None => notice("Рабочая папка не задана. Укажите: /workdir C:\\Projects\\my-project"),
        };
    }

    if session.trust == "read_only" {
        return Err(
            "Политика доверия «только чтение» запрещает создавать папки и файлы".into(),
        );
    }

    let dir = crate::workspace::prepare_workdir(args, true).map_err(|e| e.to_string())?;
    crate::workspace::init_project(&dir, &session).map_err(|e| e.to_string())?;
    db.session_set_workdir(session_id, Some(&dir.to_string_lossy()))
        .map_err(|e| e.to_string())?;
    notice(format!(
        "Рабочая папка: {}\nСозданы exports/, datasets/, notes.md и .legamind/",
        dir.display()
    ))
}

/// Команды, которым не нужна модель: help/clear/remember/skills/skill/export.
/// Возвращает `None`, если команда требует генерации (`/compact`) или неизвестна.
pub fn local_command(
    db: &crate::db::Db,
    data_dir: &std::path::Path,
    session_id: &str,
    cmd: &str,
    args: &str,
) -> Result<Option<CommandResult>, String> {
    match cmd {
        "help" => Ok(Some(notice(help_text())?)),
        "clear" => {
            let removed_ids = db
                .messages_delete_all(session_id)
                .map_err(|e| e.to_string())?;
            let _ = db.tool_runs_delete(session_id);
            Ok(Some(CommandResult {
                kind: "clear".to_string(),
                text: format!("История очищена: удалено {}", removed_ids.len()),
                removed_ids,
                message: None,
            }))
        }
        "remember" => {
            if args.is_empty() {
                return Err("Использование: /remember <текст>".into());
            }
            db.memory_add("fact", args, Some(session_id))
                .map_err(|e| e.to_string())?;
            Ok(Some(notice(format!("Запомнил: {args}"))?))
        }
        "skills" => {
            let skills = db.skills_list().map_err(|e| e.to_string())?;
            if skills.is_empty() {
                return Ok(Some(notice("Навыков нет. Создайте их на экране «Скиллы».")?));
            }
            let mut lines = vec!["Навыки:".to_string()];
            for skill in &skills {
                let mark = if skill.enabled { "✅" } else { "⬜" };
                let description = skill.description.clone().unwrap_or_default();
                let suffix = if description.is_empty() {
                    String::new()
                } else {
                    format!(" — {description}")
                };
                lines.push(format!("{mark} {}{suffix}", skill.name));
            }
            lines.push(String::new());
            lines.push("Включить/выключить: /skill <название>".to_string());
            Ok(Some(notice(lines.join("\n"))?))
        }
        "skill" => {
            if args.is_empty() {
                return Err("Использование: /skill <название>".into());
            }
            let skill = crate::skills::toggle_by_name(db, args)?;
            Ok(Some(notice(format!(
                "Навык «{}» {}",
                skill.name,
                if skill.enabled { "включён" } else { "выключен" }
            ))?))
        }
        "export" => Ok(Some(export_session(db, data_dir, session_id)?)),
        "pwd" => Ok(Some(workdir_command(db, session_id, "pwd", "")?)),
        "workdir" => Ok(Some(workdir_command(db, session_id, "workdir", args)?)),
        _ => Ok(None),
    }
}

/// Источники, на которые опирались ответы сессии.
#[tauri::command]
pub async fn sources_list(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Vec<crate::db::SourceRow>, String> {
    state.db.sources_list(&session_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn chat_command(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    model_id: String,
    input: String,
) -> Result<CommandResult, String> {
    state
        .db
        .session_get(&session_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Сессия не найдена".to_string())?;

    let (cmd, args) = parse_command(&input)
        .ok_or_else(|| "Команда должна начинаться с «/». Список: /help".to_string())?;

    if cmd == "web" {
        if args.is_empty() {
            let enabled = crate::web::enabled(&state);
            return notice(format!(
                "Интернет: {} · провайдер: {}",
                if enabled { "включён" } else { "выключен" },
                crate::web::provider(&state)
            ));
        }
        if args == "on" || args == "off" {
            state
                .db
                .setting_set("web_enabled", if args == "on" { "1" } else { "0" })
                .map_err(|e| e.to_string())?;
            return notice(format!(
                "Интернет {}",
                if args == "on" { "включён" } else { "выключен" }
            ));
        }
        let results = crate::web::search(&state, &args, 5)
            .await
            .map_err(|e| e.to_string())?;
        if results.is_empty() {
            return notice("Ничего не найдено");
        }
        let text = results
            .iter()
            .enumerate()
            .map(|(index, result)| {
                format!(
                    "[{}] {}\n{}\n{}",
                    index + 1,
                    result.title,
                    result.url,
                    result.snippet
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        return notice(text);
    }

    if let Some(result) = local_command(&state.db, &state.data_dir, &session_id, &cmd, &args)? {
        return Ok(result);
    }

    match cmd.as_str() {
        "compact" => {
            if model_id.starts_with("local/") {
                crate::sidecar::ensure_ready(&app)
                    .await
                    .map_err(|e| e.to_string())?;
            }
            compact_session(&state, &session_id, &model_id).await
        }
        other => Err(format!("Неизвестная команда «/{other}». /help — список команд")),
    }
}

#[cfg(test)]
mod tests {
    use super::{build_messages, compact_transcript, extract_memory_command, fit_window, parse_command};
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
        let messages = build_messages(&history, block, None, None, None, "chat");
        assert_eq!(messages[0].role, "system");
        assert!(messages[0].content.contains("Известное о проекте"), "{}", messages[0].content);
        assert!(messages[0].content.contains("JFFS2"), "{}", messages[0].content);
        assert_eq!(messages.last().unwrap().content, history[0].content);
    }

    #[test]
    fn skills_block_lands_in_system_prompt() {
        let history = vec![ChatMessageRow {
            id: "m1".into(),
            session_id: "s1".into(),
            role: "user".into(),
            content: "привет".into(),
            model_id: None,
            tokens: None,
            created_at: "2026-01-01T00:00:00Z".into(),
        }];
        let skills = Some("Активные навыки (следуй им):\n### Кратко\nОтвечай сжато.".to_string());
        let messages = build_messages(&history, None, skills, None, None, "chat");
        assert!(messages[0].content.contains("Активные навыки"), "{}", messages[0].content);
        assert!(messages[0].content.contains("Отвечай сжато"), "{}", messages[0].content);
    }

    #[test]
    fn lessons_block_lands_in_system_prompt() {
        let history = vec![ChatMessageRow {
            id: "m1".into(),
            session_id: "s1".into(),
            role: "user".into(),
            content: "привет".into(),
            model_id: None,
            tokens: None,
            created_at: "2026-01-01T00:00:00Z".into(),
        }];
        let lessons = Some("Усвоенные правила (соблюдай):\n- отвечай без вступлений".to_string());
        let messages = build_messages(&history, None, None, lessons, None, "chat");
        assert!(messages[0].content.contains("Усвоенные правила"), "{}", messages[0].content);
        assert!(messages[0].content.contains("без вступлений"), "{}", messages[0].content);
    }

    #[test]
    fn mode_prompt_lands_in_system_prompt() {
        let history = vec![ChatMessageRow {
            id: "m1".into(),
            session_id: "s1".into(),
            role: "user".into(),
            content: "сделай".into(),
            model_id: None,
            tokens: None,
            created_at: "2026-01-01T00:00:00Z".into(),
        }];
        let plan = build_messages(&history, None, None, None, None, "plan");
        assert!(plan[0].content.contains("Планирование"), "{}", plan[0].content);
        let build = build_messages(&history, None, None, None, None, "build");
        assert!(build[0].content.contains("Создание"), "{}", build[0].content);
        let chat = build_messages(&history, None, None, None, None, "chat");
        assert!(!chat[0].content.contains("Режим «"), "{}", chat[0].content);
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

    #[test]
    fn slash_commands_are_parsed() {
        assert_eq!(parse_command("/compact"), Some(("compact".into(), "".into())));
        assert_eq!(
            parse_command("/skill  Кратко "),
            Some(("skill".into(), "Кратко".into()))
        );
        assert_eq!(parse_command("обычный текст"), None);
        assert_eq!(parse_command("/"), None);
    }

    #[test]
    fn local_commands_work_on_db() {
        use crate::chat::local_command;

        let db = crate::db::Db::open_memory().unwrap();
        let session = db.session_create("s", "local/qwen-1.5b").unwrap();
        db.message_add(&session.id, "user", "привет", None, None).unwrap();
        db.message_add(&session.id, "assistant", "здравствуйте", None, None).unwrap();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-data")
            .join("commands");
        std::fs::create_dir_all(&dir).unwrap();

        let help = local_command(&db, &dir, &session.id, "help", "").unwrap().unwrap();
        assert!(help.text.contains("/compact"), "{}", help.text);

        let remember = local_command(&db, &dir, &session.id, "remember", "в binres JFFS2")
            .unwrap()
            .unwrap();
        assert!(remember.text.contains("Запомнил"));
        assert!(db.memories_list(10).unwrap().iter().any(|m| m.text.contains("JFFS2")));

        let skills = local_command(&db, &dir, &session.id, "skills", "").unwrap().unwrap();
        assert!(skills.text.contains("Навыки"), "{}", skills.text);

        let toggled = local_command(&db, &dir, &session.id, "skill", "Кратко").unwrap().unwrap();
        assert!(toggled.text.contains("включён"), "{}", toggled.text);

        let export = local_command(&db, &dir, &session.id, "export", "").unwrap().unwrap();
        assert!(export.text.contains("Экспортировано"), "{}", export.text);

        let cleared = local_command(&db, &dir, &session.id, "clear", "").unwrap().unwrap();
        assert_eq!(cleared.removed_ids.len(), 2);
        assert!(db.messages_list(&session.id).unwrap().is_empty());

        // /compact требует модель — обрабатывается отдельно в chat_command.
        assert!(local_command(&db, &dir, &session.id, "compact", "").unwrap().is_none());
        // Неизвестная команда тоже уходит в общий обработчик.
        assert!(local_command(&db, &dir, &session.id, "nope", "").unwrap().is_none());
    }

    #[test]
    fn compact_transcript_keeps_tail() {        let mut history = Vec::new();
        for i in 0..50 {
            history.push(ChatMessageRow {
                id: format!("m{i}"),
                session_id: "s".into(),
                role: if i % 2 == 0 { "user".into() } else { "assistant".into() },
                content: format!("сообщение {i}"),
                model_id: None,
                tokens: None,
                created_at: "2026-01-01T00:00:00Z".into(),
            });
        }
        let transcript = compact_transcript(&history);
        assert!(transcript.contains("сообщение 49"), "хвост должен остаться");
        let entries = transcript
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count();
        assert!(entries <= 40, "реплик слишком много: {entries}");
        assert!(transcript.len() <= 12_400);
    }
}
