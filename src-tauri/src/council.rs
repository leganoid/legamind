//! «Совет»: одна задача — ответы нескольких моделей и сводка арбитра.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::models::{ChatMessage, ChatParams, ChatStatus, Router};
use crate::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct CouncilAnswer {
    pub model: String,
    pub text: String,
    pub tokens: u32,
    pub seconds: f64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CouncilOutcome {
    pub answers: Vec<CouncilAnswer>,
    pub summary: Option<CouncilAnswer>,
}

async fn run_model(
    app: &AppHandle,
    state: &AppState,
    model: &str,
    messages: &[ChatMessage],
    cancel: &AtomicBool,
    prefix: &str,
) -> CouncilAnswer {
    let params = ChatParams {
        max_tokens: 1536,
        ..Default::default()
    };
    let mut text = String::new();
    let outcome = {
        let mut on_delta = |chunk: &str| {
            text.push_str(chunk);
            let _ = app.emit(
                format!("{prefix}:delta").as_str(),
                serde_json::json!({ "model": model, "delta": chunk }),
            );
        };
        let mut on_status = |_: &ChatStatus| {};
        Router { state }
            .stream(model, messages, &params, cancel, &mut on_delta, &mut on_status)
            .await
    };
    match outcome {
        Ok(outcome) => CouncilAnswer {
            model: model.to_string(),
            text: if outcome.text.is_empty() { text } else { outcome.text },
            tokens: outcome.tokens,
            seconds: outcome.seconds,
            error: None,
        },
        Err(err) => CouncilAnswer {
            model: model.to_string(),
            text,
            tokens: 0,
            seconds: 0.0,
            error: Some(err.to_string()),
        },
    }
}

#[tauri::command]
pub async fn council_ask(
    app: AppHandle,
    state: State<'_, AppState>,
    prompt: String,
    models: Vec<String>,
    arbiter: Option<String>,
) -> Result<CouncilOutcome, String> {
    let prompt = prompt.trim().to_string();
    if prompt.is_empty() {
        return Err("Пустая задача".into());
    }
    if models.is_empty() {
        return Err("Выберите хотя бы одну модель".into());
    }
    let cancel = Arc::new(AtomicBool::new(false));
    state
        .cancel
        .lock()
        .unwrap()
        .insert("council".into(), cancel.clone());

    let participant_messages = vec![
        ChatMessage::new(
            "system",
            "Ты участник совета Legamind. Ответь на задачу пользователя по существу, кратко и по-русски. Не упоминай других участников.",
        ),
        ChatMessage::new("user", prompt.clone()),
    ];

    let mut answers = Vec::new();
    for model in &models {
        if cancel.load(Ordering::SeqCst) {
            break;
        }
        let _ = app.emit("council:start", serde_json::json!({ "model": model }));
        let answer = run_model(
            &app,
            &state,
            model,
            &participant_messages,
            &cancel,
            "council",
        )
        .await;
        let _ = app.emit("council:done", &answer);
        answers.push(answer);
    }

    let mut summary = None;
    if !cancel.load(Ordering::SeqCst) {
        if let Some(arbiter) = arbiter.filter(|value| !value.trim().is_empty()) {
            let mut digest = String::new();
            for answer in &answers {
                if answer.error.is_none() && !answer.text.trim().is_empty() {
                    let text: String = answer.text.chars().take(3000).collect();
                    digest.push_str(&format!("\n\n### {}\n{}", answer.model, text));
                }
            }
            if !digest.trim().is_empty() {
                let messages = vec![
                    ChatMessage::new(
                        "system",
                        "Ты арбитр совета Legamind. Сравни ответы участников: что общего, в чём расхождения, какой вывод правильный. Пиши по-русски, кратко и структурно.",
                    ),
                    ChatMessage::new(
                        "user",
                        format!(
                            "Задача:\n{prompt}\n\nОтветы участников:{digest}\n\nСделай сводку: общее, различия, итоговый вывод."
                        ),
                    ),
                ];
                let _ = app.emit(
                    "council:summary_start",
                    serde_json::json!({ "model": arbiter }),
                );
                let answer = run_model(
                    &app,
                    &state,
                    &arbiter,
                    &messages,
                    &cancel,
                    "council:summary",
                )
                .await;
                let _ = app.emit("council:summary_done", &answer);
                summary = Some(answer);
            }
        }
    }

    state.cancel.lock().unwrap().remove("council");
    Ok(CouncilOutcome { answers, summary })
}

#[tauri::command]
pub fn council_cancel(state: State<'_, AppState>) {
    if let Some(flag) = state.cancel.lock().unwrap().get("council") {
        flag.store(true, Ordering::SeqCst);
    }
}
