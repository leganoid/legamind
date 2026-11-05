//! «Совет»: одна задача — ответы нескольких моделей.

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

#[tauri::command]
pub async fn council_ask(
    app: AppHandle,
    state: State<'_, AppState>,
    prompt: String,
    models: Vec<String>,
) -> Result<Vec<CouncilAnswer>, String> {
    let prompt = prompt.trim().to_string();
    if prompt.is_empty() {
        return Err("Пустая задача".into());
    }
    if models.is_empty() {
        return Err("Выберите хотя бы одну модель".into());
    }
    let mut answers = Vec::new();
    for model in models {
        let _ = app.emit("council:start", serde_json::json!({ "model": model }));
        let messages = vec![
            ChatMessage::new(
                "system",
                "Ты участник совета Legamind. Ответь на задачу пользователя по существу, кратко и по-русски. Не упоминай других участников.",
            ),
            ChatMessage::new("user", prompt.clone()),
        ];
        let params = ChatParams {
            max_tokens: 1536,
            ..Default::default()
        };
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let mut text = String::new();
        let outcome = {
            let mut on_delta = |chunk: &str| {
                text.push_str(chunk);
                let _ = app.emit(
                    "council:delta",
                    serde_json::json!({ "model": model, "delta": chunk }),
                );
            };
            let mut on_status = |_: &ChatStatus| {};
            Router { state: &state }
                .stream(&model, &messages, &params, &cancel, &mut on_delta, &mut on_status)
                .await
        };
        let (final_text, tokens, seconds, error) = match outcome {
            Ok(outcome) => {
                let answer = if outcome.text.is_empty() {
                    text
                } else {
                    outcome.text
                };
                (answer, outcome.tokens, outcome.seconds, None)
            }
            Err(err) => (text, 0, 0.0, Some(err.to_string())),
        };
        let answer = CouncilAnswer {
            model: model.clone(),
            text: final_text,
            tokens,
            seconds,
            error,
        };
        let _ = app.emit("council:done", &answer);
        answers.push(answer);
    }
    Ok(answers)
}
