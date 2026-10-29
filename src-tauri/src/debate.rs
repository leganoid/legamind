//! Дебаты моделей: протокол из раздела 6 TASK.md.
//!
//! Все вызовы последовательные (не параллелим), участники анонимизированы как
//! «Участник A/B/...». Судья — отдельная модель либо «синтез всех» через голосование.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::db::{DebateRow, DebateTurnRow};
use crate::logging::log_line;
use crate::models::{local_label, ChatMessage, ChatOutcome, ChatParams, Router};
use crate::AppState;

#[derive(Debug, Clone, Deserialize)]
pub struct DebateRequest {
    pub task: String,
    pub participants: Vec<String>,
    pub rounds: Option<i64>,
    pub judge: Option<String>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DebateStart {
    pub debate_id: String,
}

#[derive(Debug, Clone, Serialize)]
struct TurnPayload {
    debate_id: String,
    turn: DebateTurnRow,
}

#[derive(Debug, Clone, Serialize)]
struct DeltaPayload {
    debate_id: String,
    model_id: String,
    round: i64,
    delta: String,
}

#[derive(Debug, Clone, Serialize)]
struct DonePayload {
    debate_id: String,
    verdict: String,
    final_answer: String,
    scores: Vec<ScoreEntry>,
}

#[derive(Debug, Clone, Serialize)]
struct ErrorPayload {
    debate_id: String,
    error: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScoreEntry {
    pub label: String,
    pub score: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DebateDetail {
    pub debate: DebateRow,
    pub turns: Vec<DebateTurnRow>,
}

fn letter(index: usize) -> char {
    char::from(b'A' + (index as u8 % 26))
}

fn anon(index: usize) -> String {
    format!("Участник {}", letter(index))
}

fn model_label(id: &str) -> String {
    local_label(id).map(|s| s.to_string()).unwrap_or_else(|| id.to_string())
}

fn proposals_block(participants: &[String], replies: &[String], exclude: Option<usize>) -> String {
    participants
        .iter()
        .enumerate()
        .filter(|(i, _)| Some(*i) != exclude)
        .map(|(i, _)| format!("{}:\n{}", anon(i), replies[i].trim()))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn proposal_prompt(label: &str, task: &str, max_tokens: u32) -> String {
    format!(
        "Ты — {label}, участник обсуждения. Задача пользователя:\n---\n{task}\n---\n\
Предложи решение. Будь конкретен: шаги, структура, ключевые решения.\n\
Не обращайся к другим участникам. Не пересказывай задачу. Отвечай по-русски.\n\
Максимум {max_tokens} токенов."
    )
}

fn critique_prompt(own: &str, others: &str) -> String {
    format!(
        "Твоё предложение:\n---\n{own}\n---\nПредложения других участников:\n{others}\n\n\
Критично оцени все предложения, включая своё. Найди слабые места и риски.\n\
Затем предложи улучшенный план. Не повторяй чужие формулировки. По-русски."
    )
}

fn judge_prompt(task: &str, final_proposals: &str) -> String {
    format!(
        "Задача:\n---\n{task}\n---\nПредложения после обсуждения:\n{final_proposals}\n\n\
Ты — арбитр. Выбери лучшую стратегию и объясни выбор в 3–5 предложениях.\n\
Затем выдай итоговое решение: конкретный план/текст/код, который пользователь может применить.\n\
Не упоминай «участник A/B» — говори по существу. По-русски."
    )
}

fn vote_prompt(task: &str, proposals: &str) -> String {
    format!(
        "Задача:\n---\n{task}\n---\nПредложения участников:\n{proposals}\n\n\
Оцени каждое предложение (кроме своего) по шкале 1–10.\n\
Ответь строго JSON без пояснений: {{\"scores\":{{\"Участник A\":7,\"Участник B\":5}}}}"
    )
}

/// Терпимый разбор оценок: сначала JSON, затем поиск чисел после имён.
pub fn parse_scores(text: &str) -> Vec<(String, i64)> {
    let cleaned = text
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    if let Ok(value) = serde_json::from_str::<Value>(cleaned) {
        let table = value
            .get("scores")
            .and_then(|s| s.as_object())
            .cloned()
            .or_else(|| value.as_object().cloned());
        if let Some(table) = table {
            let mut out = Vec::new();
            for (name, raw) in table {
                let score = raw
                    .as_i64()
                    .or_else(|| raw.as_f64().map(|f| f.round() as i64))
                    .or_else(|| raw.as_str().and_then(|s| first_number(s)));
                if let Some(score) = score {
                    out.push((name, score.clamp(1, 10)));
                }
            }
            if !out.is_empty() {
                return out;
            }
        }
    }

    // Битый JSON: ищем «Участник X» и ближайшее число после него.
    let mut out = Vec::new();
    for index in 0..4 {
        let label = anon(index);
        if let Some(pos) = cleaned.find(&label) {
            let after = &cleaned[pos + label.len()..];
            if let Some(score) = first_number(&after.chars().take(30).collect::<String>()) {
                out.push((label, score.clamp(1, 10)));
            }
        }
    }
    out
}

fn first_number(text: &str) -> Option<i64> {
    let mut digits = String::new();
    let mut started = false;
    for ch in text.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
            started = true;
        } else if started {
            break;
        }
    }
    digits.parse::<i64>().ok().filter(|n| (1..=10).contains(n))
}

/// Делит ответ судьи на «вердикт» и «итоговое решение» по маркеру в тексте.
pub fn split_verdict(text: &str) -> (String, String) {
    let lower = text.to_lowercase();
    for marker in ["итоговое решение", "итоговый план", "итог:"] {
        if let Some(pos) = lower.find(marker) {
            let line_start = text[..pos].rfind('\n').map(|p| p + 1).unwrap_or(pos);
            let verdict = text[..line_start].trim();
            let answer = text[pos..].trim();
            if !verdict.is_empty() && !answer.is_empty() {
                return (verdict.to_string(), answer.to_string());
            }
        }
    }
    (text.trim().to_string(), text.trim().to_string())
}

#[tauri::command]
pub async fn debate_start(
    app: AppHandle,
    state: State<'_, AppState>,
    req: DebateRequest,
) -> Result<DebateStart, String> {
    let task = req.task.trim().to_string();
    if task.is_empty() {
        return Err("Опишите задачу для дебатов".into());
    }
    if !(2..=4).contains(&req.participants.len()) {
        return Err("Нужно от 2 до 4 участников".into());
    }
    let mut unique = req.participants.clone();
    unique.sort();
    unique.dedup();
    if unique.len() != req.participants.len() {
        return Err("Участники не должны повторяться".into());
    }
    let rounds = req.rounds.unwrap_or(2).clamp(1, 3);

    let participants_json = serde_json::to_string(&req.participants).map_err(|e| e.to_string())?;
    let debate = state
        .db
        .debate_create(&task, &participants_json, rounds, req.judge.as_deref())
        .map_err(|e| e.to_string())?;

    let cancel = Arc::new(AtomicBool::new(false));
    let debate_id = debate.id.clone();
    state
        .cancel
        .lock()
        .unwrap()
        .insert(debate_id.clone(), cancel.clone());

    let app_task = app.clone();
    let request = DebateRequest { task, rounds: Some(rounds), ..req };
    let id_task = debate_id.clone();
    tauri::async_runtime::spawn(async move {
        run_debate(app_task, id_task, request, cancel).await;
    });

    Ok(DebateStart { debate_id })
}

#[tauri::command]
pub async fn debate_stop(state: State<'_, AppState>, debate_id: String) -> Result<(), String> {
    if let Some(flag) = state.cancel.lock().unwrap().get(&debate_id) {
        flag.store(true, Ordering::Relaxed);
    }
    Ok(())
}

#[tauri::command]
pub async fn debates_list(state: State<'_, AppState>, limit: Option<i64>) -> Result<Vec<DebateRow>, String> {
    state.db.debates_list(limit.unwrap_or(50)).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn debate_get(state: State<'_, AppState>, id: String) -> Result<DebateDetail, String> {
    let debate = state
        .db
        .debate_get(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Дебаты не найдены".to_string())?;
    let turns = state.db.debate_turns(&id).map_err(|e| e.to_string())?;
    Ok(DebateDetail { debate, turns })
}

#[tauri::command]
pub async fn debate_export_md(state: State<'_, AppState>, id: String) -> Result<String, String> {
    let debate = state
        .db
        .debate_get(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Дебаты не найдены".to_string())?;
    let turns = state.db.debate_turns(&id).map_err(|e| e.to_string())?;
    let detail = DebateDetail { debate, turns };
    let markdown = render_markdown(&detail);
    let dir = state.data_dir.join("exports");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("debate-{id}.md"));
    std::fs::write(&path, markdown).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().to_string())
}

fn render_markdown(detail: &DebateDetail) -> String {
    let participants: Vec<String> = serde_json::from_str(&detail.debate.participants).unwrap_or_default();
    let mut out = String::new();
    out.push_str("# Дебаты Legamind\n\n");
    out.push_str(&format!("**Дата:** {}\n\n", detail.debate.created_at));
    out.push_str(&format!(
        "**Участники:** {}\n\n",
        participants.iter().map(|id| model_label(id)).collect::<Vec<_>>().join(", ")
    ));
    out.push_str(&format!("**Раундов:** {}\n\n", detail.debate.rounds));
    out.push_str(&format!("**Судья:** {}\n\n", detail.debate.judge.clone().unwrap_or_else(|| "синтез всех".into())));
    out.push_str("## Задача\n\n");
    out.push_str(detail.debate.task.trim());
    out.push_str("\n\n");

    let mut current_round = -1;
    for turn in &detail.turns {
        if turn.round != current_round {
            current_round = turn.round;
            out.push_str(&format!("## Раунд {}\n\n", turn.round));
        }
        out.push_str(&format!("### {} ({})\n\n", model_label(&turn.model_id), turn.role));
        out.push_str(turn.content.trim());
        out.push_str("\n\n");
    }

    if let Some(verdict) = detail.debate.verdict.as_deref().filter(|v| !v.is_empty()) {
        out.push_str("## Вердикт\n\n");
        out.push_str(verdict.trim());
        out.push_str("\n\n");
    }
    if let Some(answer) = detail.debate.final_answer.as_deref().filter(|v| !v.is_empty()) {
        out.push_str("## Итоговое решение\n\n");
        out.push_str(answer.trim());
        out.push_str("\n");
    }
    out
}

async fn call_model(
    app: &AppHandle,
    state: &AppState,
    debate_id: &str,
    model_id: &str,
    prompt: &str,
    round: i64,
    max_tokens: u32,
    temperature: f64,
    cancel: &AtomicBool,
) -> Result<ChatOutcome> {
    if model_id.starts_with("local/") {
        crate::sidecar::ensure_ready(app).await?;
    }
    let messages = vec![
        ChatMessage::new("system", "Ты — участник технического обсуждения. Отвечай по-русски, конкретно."),
        ChatMessage::new("user", prompt),
    ];
    let params = ChatParams { max_tokens, temperature, think: false, reasoning: None };
    let router = Router { state };
    let mut on_delta = |text: &str| {
        let _ = app.emit(
            "debate:delta",
            DeltaPayload {
                debate_id: debate_id.to_string(),
                model_id: model_id.to_string(),
                round,
                delta: text.to_string(),
            },
        );
    };
    router
        .stream(
            model_id,
            &messages,
            &params,
            cancel,
            &mut on_delta,
            &mut |_: &crate::models::ChatStatus| {},
        )
        .await
}

async fn run_debate(app: AppHandle, debate_id: String, req: DebateRequest, cancel: Arc<AtomicBool>) {
    let state = app.state::<AppState>();
    let result = run_protocol(&app, &state, &debate_id, &req, &cancel).await;

    match result {
        Ok((verdict, final_answer, scores)) => {
            let _ = state.db.debate_finish(&debate_id, &verdict, &final_answer);
            let _ = crate::memory::save_episode(
                &state,
                &format!("Дебаты «{}»: итог — {}", truncate(&req.task, 120), truncate(&final_answer, 240)),
                None,
            );
            let _ = app.emit(
                "debate:done",
                DonePayload { debate_id: debate_id.clone(), verdict, final_answer, scores },
            );
        }
        Err(error) => {
            log_line("app.log", &format!("debate {debate_id} error: {error:#}"));
            let _ = app.emit(
                "debate:error",
                ErrorPayload { debate_id: debate_id.clone(), error: error.to_string() },
            );
        }
    }
    state.cancel.lock().unwrap().remove(&debate_id);
}

fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        text.to_string()
    } else {
        text.chars().take(limit).collect::<String>() + "…"
    }
}

async fn run_protocol(
    app: &AppHandle,
    state: &AppState,
    debate_id: &str,
    req: &DebateRequest,
    cancel: &AtomicBool,
) -> Result<(String, String, Vec<ScoreEntry>)> {
    let rounds = req.rounds.unwrap_or(2).clamp(1, 3);
    let max_tokens = req.max_tokens.unwrap_or(768).clamp(256, 2048);
    let temperature = req.temperature.unwrap_or(0.7);
    let participants = &req.participants;
    let mut replies: Vec<String> = vec![String::new(); participants.len()];

    for round in 1..=rounds {
        for (index, model_id) in participants.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                bail!("Дебаты остановлены пользователем");
            }
            let prompt = if round == 1 {
                proposal_prompt(&model_label(model_id), &req.task, max_tokens)
            } else {
                critique_prompt(&replies[index], &proposals_block(participants, &replies, Some(index)))
            };
            let outcome = call_model(
                app,
                state,
                debate_id,
                model_id,
                &prompt,
                round,
                max_tokens,
                temperature,
                cancel,
            )
            .await?;
            let role = if round == 1 { "proposal" } else { "critique" };
            let turn = state.db.debate_turn_add(
                debate_id,
                round,
                role,
                model_id,
                &outcome.text,
                Some(outcome.tokens as i64),
                Some(outcome.seconds),
            )?;
            let _ = app.emit(
                "debate:turn",
                TurnPayload { debate_id: debate_id.to_string(), turn: turn.clone() },
            );
            log_line(
                "prompts.log",
                &format!("=== debate {debate_id} round {round} model {model_id}\n{prompt}"),
            );
            replies[index] = outcome.text;
        }
    }

    if cancel.load(Ordering::Relaxed) {
        bail!("Дебаты остановлены пользователем");
    }

    let final_block = proposals_block(participants, &replies, None);
    let mut scores: Vec<ScoreEntry> = Vec::new();

    // «Синтез всех»: голосование участников, затем итог от победителя.
    let winner_index = match req.judge.as_deref() {
        Some(judge_id) if !judge_id.is_empty() && judge_id != "synthesis" => None,
        _ => {
            let mut totals = vec![0i64; participants.len()];
            for (index, model_id) in participants.iter().enumerate() {
                if cancel.load(Ordering::Relaxed) {
                    bail!("Дебаты остановлены пользователем");
                }
                let prompt = vote_prompt(&req.task, &final_block);
                let outcome = call_model(
                    app, state, debate_id, model_id, &prompt, rounds + 1, 256, 0.2, cancel,
                )
                .await?;
                let _ = state.db.debate_turn_add(
                    debate_id,
                    rounds + 1,
                    "vote",
                    model_id,
                    &outcome.text,
                    Some(outcome.tokens as i64),
                    Some(outcome.seconds),
                );
                for (label, score) in parse_scores(&outcome.text) {
                    if let Some(winner) = (0..participants.len()).find(|i| anon(*i) == label) {
                        if winner != index {
                            totals[winner] += score;
                        }
                    }
                }
            }
            scores = (0..participants.len())
                .map(|i| ScoreEntry { label: format!("{} ({})", anon(i), model_label(&participants[i])), score: totals[i] })
                .collect();
            let best = totals
                .iter()
                .enumerate()
                .max_by_key(|(_, &score)| score)
                .map(|(i, _)| i)
                .unwrap_or(0);
            Some(best)
        }
    };

    let (judge_model, judge_label) = match (winner_index, req.judge.as_deref()) {
        (Some(index), _) => (participants[index].clone(), format!("синтез всех — победитель {}", anon(index))),
        (None, Some(judge_id)) => (judge_id.to_string(), model_label(judge_id)),
        (None, None) => (participants[0].clone(), "синтез всех".to_string()),
    };

    let prompt = judge_prompt(&req.task, &final_block);
    let verdict_outcome = call_model(
        app,
        state,
        debate_id,
        &judge_model,
        &prompt,
        rounds + 1,
        max_tokens,
        temperature,
        cancel,
    )
    .await;
    let outcome = match verdict_outcome {
        Ok(outcome) => outcome,
        Err(err) if judge_model != participants[0] => {
            log_line("app.log", &format!("judge {judge_model} failed: {err:#}; fallback"));
            call_model(
                app,
                state,
                debate_id,
                &participants[0],
                &prompt,
                rounds + 1,
                max_tokens,
                temperature,
                cancel,
            )
            .await?
        }
        Err(err) => return Err(err),
    };

    let turn = state.db.debate_turn_add(
        debate_id,
        rounds + 1,
        "judge",
        &judge_model,
        &outcome.text,
        Some(outcome.tokens as i64),
        Some(outcome.seconds),
    )?;
    let _ = app.emit(
        "debate:turn",
        TurnPayload { debate_id: debate_id.to_string(), turn },
    );
    log_line(
        "prompts.log",
        &format!("=== debate {debate_id} judge {judge_label} ({judge_model})\n{prompt}"),
    );

    let (verdict, final_answer) = split_verdict(&outcome.text);
    Ok((verdict, final_answer, scores))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scores_from_json() {
        let text = r#"{"scores":{"Участник A":7,"Участник B":5}}"#;
        let scores = parse_scores(text);
        assert_eq!(scores.len(), 2);
        assert!(scores.iter().any(|(name, score)| name == "Участник A" && *score == 7));
    }

    #[test]
    fn scores_from_broken_json() {
        let text = "Вот оценки: Участник A — 8, Участник B: 6. Надеюсь, подходит.";
        let scores = parse_scores(text);
        assert!(scores.iter().any(|(name, score)| name == "Участник A" && *score == 8));
        assert!(scores.iter().any(|(name, score)| name == "Участник B" && *score == 6));
    }

    #[test]
    fn scores_clamped() {
        let scores = parse_scores(r#"{"scores":{"Участник A": 99}}"#);
        assert_eq!(scores[0].1, 10);
    }

    #[test]
    fn verdict_split() {
        let (verdict, answer) = split_verdict("Лучше второй вариант.\n\nИтоговое решение:\n1. Сделать так.");
        assert!(verdict.contains("второй вариант"));
        assert!(answer.contains("Сделать так"));
    }

    #[test]
    fn verdict_without_marker() {
        let (verdict, answer) = split_verdict("Просто ответ");
        assert_eq!(verdict, "Просто ответ");
        assert_eq!(answer, "Просто ответ");
    }

    #[test]
    fn anonymized_block_hides_names() {
        let participants = vec!["local/a".to_string(), "local/b".to_string()];
        let replies = vec!["предложение 1".to_string(), "предложение 2".to_string()];
        let block = proposals_block(&participants, &replies, Some(0));
        assert!(block.contains("Участник B"));
        assert!(!block.contains("Участник A"));
        assert!(!block.contains("local/"));
    }
}
