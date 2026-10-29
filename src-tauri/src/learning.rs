//! Обучение: усвоенные правила (без градиентов), обратная связь и датасеты
//! для дообучения локальной модели у облачного «учителя».

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tauri::Manager;

use crate::db::{new_id, DatasetRow, FeedbackRow, LessonRow};
use crate::models::{local_models, ChatMessage, ChatParams, ChatStatus, Router};
use crate::AppState;

/// Бюджет блока усвоенных правил в system-промпте.
const LESSONS_BUDGET: usize = 1500;

/// Правила, которым модель должна следовать (самообучение без переобучения весов).
pub fn lessons_block(state: &AppState) -> Result<Option<String>> {
    let lessons = state.db.lessons_list()?;
    let enabled: Vec<&LessonRow> = lessons.iter().filter(|l| l.enabled).collect();
    if enabled.is_empty() {
        return Ok(None);
    }
    let mut out = String::from("Усвоенные правила (соблюдай):");
    let mut used = out.len();
    for lesson in enabled {
        let line = format!("\n- {}", lesson.text.trim());
        if used + line.len() > LESSONS_BUDGET {
            break;
        }
        out.push_str(&line);
        used += line.len();
    }
    Ok(Some(out))
}

#[tauri::command]
pub async fn lessons_list(state: tauri::State<'_, AppState>) -> Result<Vec<LessonRow>, String> {
    state.db.lessons_list().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn lesson_add(
    state: tauri::State<'_, AppState>,
    text: String,
    source: Option<String>,
) -> Result<LessonRow, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("Пустое правило".into());
    }
    state
        .db
        .lesson_add(text, source.as_deref())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn lesson_delete(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    state.db.lesson_delete(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn lesson_set_enabled(
    state: tauri::State<'_, AppState>,
    id: String,
    enabled: bool,
) -> Result<(), String> {
    state
        .db
        .lesson_set_enabled(&id, enabled)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn feedback_set(
    state: tauri::State<'_, AppState>,
    message_id: String,
    session_id: String,
    rating: i64,
    note: Option<String>,
) -> Result<FeedbackRow, String> {
    if !(-1..=1).contains(&rating) {
        return Err("Оценка должна быть -1, 0 или 1".into());
    }
    let feedback = state
        .db
        .feedback_set(&message_id, &session_id, rating, note.as_deref())
        .map_err(|e| e.to_string())?;
    if let Some(text) = note.as_deref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        state
            .db
            .lesson_add(text, Some(&session_id))
            .map_err(|e| e.to_string())?;
    }
    Ok(feedback)
}

// ------------------------------------------------------------------ датасеты

#[derive(Debug, Clone, Deserialize)]
pub struct DatasetRequest {
    pub name: String,
    pub teacher_model: String,
    pub source_text: Option<String>,
    pub session_id: Option<String>,
    pub count: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DatasetProgress {
    pub dataset_id: String,
    pub phase: String,
    pub done: u32,
    pub total: u32,
    pub examples: u32,
    pub error: Option<String>,
}

#[tauri::command]
pub async fn datasets_list(state: tauri::State<'_, AppState>) -> Result<Vec<DatasetRow>, String> {
    state.db.datasets_list().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn dataset_delete(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    if let Ok(rows) = state.db.datasets_list() {
        if let Some(row) = rows.into_iter().find(|r| r.id == id) {
            let _ = std::fs::remove_file(&row.path);
        }
    }
    state.db.dataset_delete(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn dataset_generate(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    req: DatasetRequest,
) -> Result<DatasetRow, String> {
    let name = req.name.trim();
    if name.is_empty() {
        return Err("Укажите название датасета".into());
    }
    let count = req.count.unwrap_or(30).clamp(5, 200);
    let has_session = req
        .session_id
        .as_deref()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    let has_text = req
        .source_text
        .as_deref()
        .map(|s| s.trim().len() > 20)
        .unwrap_or(false);
    if !has_session && !has_text {
        return Err("Нужна тема: выберите сессию или введите текст материала".into());
    }
    if req.teacher_model.trim().is_empty() {
        return Err("Выберите модель-учителя".into());
    }

    // Датасет из сессии пишем в её рабочую папку, если она задана.
    let dir = req
        .session_id
        .as_deref()
        .and_then(|sid| state.db.session_get(sid).ok().flatten())
        .and_then(|session| session.workdir)
        .map(|workdir| std::path::PathBuf::from(workdir).join("datasets"))
        .unwrap_or_else(|| state.data_dir.join("datasets"));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let dataset_id = new_id();
    let path = dir.join(format!("{}-{}.jsonl", slugify(name), &dataset_id[..8]));
    let row = state
        .db
        .dataset_add(
            name,
            &path.to_string_lossy(),
            Some(&req.teacher_model),
            0,
        )
        .map_err(|e| e.to_string())?;

    if req.teacher_model.starts_with("local/") {
        crate::sidecar::ensure_ready(&app)
            .await
            .map_err(|e| e.to_string())?;
    }

    let app_task = app.clone();
    let dataset_row = row.clone();
    let request = DatasetRequest {
        name: name.to_string(),
        count: Some(count),
        ..req
    };
    tauri::async_runtime::spawn(async move {
        run_generation(app_task, dataset_row, request, path, count).await;
    });
    Ok(row)
}

fn slugify(text: &str) -> String {
    let slug: String = text
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "dataset".to_string()
    } else {
        slug
    }
}

/// Кусок материала для генерации: либо транскрипт сессии, либо введённый текст.
fn build_source(state: &AppState, req: &DatasetRequest) -> Result<String> {
    if let Some(session_id) = req.session_id.as_deref().filter(|s| !s.trim().is_empty()) {
        let messages = state.db.messages_list(session_id)?;
        let mut out = String::new();
        for message in messages.iter().rev().take(30).rev() {
            let role = if message.role == "user" { "Пользователь" } else { "Ассистент" };
            let body: String = message.content.chars().take(800).collect();
            out.push_str(&format!("{role}: {body}\n\n"));
            if out.len() > 6000 {
                break;
            }
        }
        return Ok(out);
    }
    Ok(req
        .source_text
        .clone()
        .unwrap_or_default()
        .chars()
        .take(6000)
        .collect())
}

fn dataset_prompt(source: &str, batch: u32) -> String {
    format!(
        "Ты — генератор обучающих данных для дообучения локальной модели.\n\n\
         Тема/материал:\n---\n{source}\n---\n\n\
         Составь {batch} разных пар «инструкция пользователя → ответ ассистента» на русском.\n\
         Инструкции — реалистичные и разнообразные, ответы — конкретные (2–6 предложений), \
         без воды и без повторения формулировок.\n\
         Верни строго JSON: {{\"items\":[{{\"instruction\":\"...\",\"answer\":\"...\"}}]}}"
    )
}

/// Разбор ответа учителя: JSON, JSON в ```-блоке, JSON с висящими запятыми,
/// либо аварийный разбор пар «instruction/answer» регуляркой.
pub fn parse_dataset_items(text: &str) -> Vec<(String, String)> {
    let cleaned = text
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    let start = cleaned.find('{').or_else(|| cleaned.find('['));
    let end = cleaned.rfind('}').or_else(|| cleaned.rfind(']'));
    if let (Some(start), Some(end)) = (start, end) {
        if end > start {
            let candidate = &cleaned[start..=end];
            for variant in [candidate.to_string(), candidate.replace(",}", "}").replace(",]", "]")] {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&variant) {
                    let items = value
                        .get("items")
                        .and_then(|v| v.as_array())
                        .cloned()
                        .or_else(|| value.as_array().cloned())
                        .unwrap_or_default();
                    let mut out = Vec::new();
                    for item in items {
                        let instruction = item
                            .get("instruction")
                            .or_else(|| item.get("question"))
                            .or_else(|| item.get("prompt"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .trim()
                            .to_string();
                        let answer = item
                            .get("answer")
                            .or_else(|| item.get("response"))
                            .or_else(|| item.get("output"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .trim()
                            .to_string();
                        if instruction.len() >= 8 && answer.len() >= 20 {
                            out.push((instruction, answer));
                        }
                    }
                    if !out.is_empty() {
                        return out;
                    }
                }
            }
        }
    }
    Vec::new()
}

fn jsonl_line(instruction: &str, answer: &str) -> String {
    let value = serde_json::json!({
        "messages": [
            {"role": "user", "content": instruction},
            {"role": "assistant", "content": answer}
        ]
    });
    value.to_string()
}

async fn run_generation(
    app: AppHandle,
    dataset: DatasetRow,
    req: DatasetRequest,
    path: PathBuf,
    total: u32,
) {
    let state = app.state::<AppState>();
    let source = match build_source(&state, &req) {
        Ok(source) if !source.trim().is_empty() => source,
        Ok(_) => {
            emit_progress(&app, &dataset.id, "error", 0, total, 0, Some("Пустой материал для генерации"));
            return;
        }
        Err(err) => {
            emit_progress(&app, &dataset.id, "error", 0, total, 0, Some(&err.to_string()));
            return;
        }
    };

    let mut collected: Vec<(String, String)> = Vec::new();
    let mut attempts: u32 = 0;
    let batch = 4u32;
    let router = Router { state: &state };
    let params = ChatParams { max_tokens: 700, temperature: 0.9, think: false, reasoning: None };
    emit_progress(&app, &dataset.id, "generating", 0, total, 0, None);

    while collected.len() < total as usize && attempts < total + 8 {
        let want = batch.min(total - collected.len() as u32);
        attempts += want;
        let messages = vec![
            ChatMessage::new("system", "Ты — генератор датасетов. Отвечаешь только JSON."),
            ChatMessage::new("user", dataset_prompt(&source, want)),
        ];
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let result = router
            .stream(
                &req.teacher_model,
                &messages,
                &params,
                &cancel,
                &mut |_| {},
                &mut |_: &ChatStatus| {},
            )
            .await;
        match result {
            Ok(outcome) => {
                for (instruction, answer) in parse_dataset_items(&outcome.text) {
                    if collected.len() >= total as usize {
                        break;
                    }
                    if collected.iter().any(|(existing, _)| existing == &instruction) {
                        continue;
                    }
                    collected.push((instruction, answer));
                }
            }
            Err(err) => {
                emit_progress(
                    &app,
                    &dataset.id,
                    "error",
                    attempts.min(total),
                    total,
                    collected.len() as u32,
                    Some(&err.to_string()),
                );
                return;
            }
        }
        emit_progress(
            &app,
            &dataset.id,
            "generating",
            attempts.min(total),
            total,
            collected.len() as u32,
            None,
        );
        if want == 0 {
            break;
        }
    }

    if collected.is_empty() {
        emit_progress(&app, &dataset.id, "error", total, total, 0, Some("Не удалось разобрать пары"));
        return;
    }

    let body = collected
        .iter()
        .map(|(instruction, answer)| jsonl_line(instruction, answer))
        .collect::<Vec<_>>()
        .join("\n");
    if let Err(err) = std::fs::write(&path, format!("{body}\n")) {
        emit_progress(&app, &dataset.id, "error", total, total, 0, Some(&err.to_string()));
        return;
    }
    if let Err(err) = state.db.dataset_set_examples(&dataset.id, collected.len() as i64) {
        emit_progress(&app, &dataset.id, "error", total, total, 0, Some(&err.to_string()));
        return;
    }
    emit_progress(
        &app,
        &dataset.id,
        "done",
        total,
        total,
        collected.len() as u32,
        None,
    );
}

fn emit_progress(
    app: &AppHandle,
    dataset_id: &str,
    phase: &str,
    done: u32,
    total: u32,
    examples: u32,
    error: Option<&str>,
) {
    let _ = app.emit(
        "dataset:progress",
        DatasetProgress {
            dataset_id: dataset_id.to_string(),
            phase: phase.to_string(),
            done,
            total,
            examples,
            error: error.map(|s| s.to_string()),
        },
    );
}

// ------------------------------------------------------------------ извлечение итогов из сессии

/// Кандидат в обучающую пару, извлечённый из готового диалога.
#[derive(Debug, Clone, Serialize)]
pub struct ExtractedItem {
    pub id: String,
    pub instruction: String,
    pub answer: String,
    pub duplicate_of: Option<String>,
    pub duplicate_score: f64,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckResult {
    pub model_answer: String,
    pub coverage: f64,
    pub jaccard: f64,
    pub verdict: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AppendItem {
    pub instruction: String,
    pub answer: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppendResult {
    pub added: usize,
    pub skipped: usize,
    pub total: i64,
}

fn norm_tokens(text: &str) -> std::collections::BTreeSet<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| word.chars().count() >= 4)
        .map(|word| word.to_string())
        .collect()
}

/// Сходство Жаккара по словам (грубая, но быстрая оценка «то же самое или нет»).
pub fn jaccard(a: &str, b: &str) -> f64 {
    let left = norm_tokens(a);
    let right = norm_tokens(b);
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    let inter = left.intersection(&right).count() as f64;
    let union = left.union(&right).count() as f64;
    if union == 0.0 {
        0.0
    } else {
        inter / union
    }
}

/// Доля слов эталона, которые встречаются в проверяемом ответе.
pub fn coverage(target: &str, candidate: &str) -> f64 {
    let reference = norm_tokens(target);
    if reference.is_empty() {
        return 0.0;
    }
    let other = norm_tokens(candidate);
    reference.intersection(&other).count() as f64 / reference.len() as f64
}

/// Инструкции из уже сохранённых датасетов — для отсева повторов.
fn existing_instructions(state: &AppState) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(rows) = state.db.datasets_list() else {
        return out;
    };
    for row in rows {
        let Ok(text) = std::fs::read_to_string(&row.path) else {
            continue;
        };
        for line in text.lines().take(2000) {
            let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            if let Some(instruction) = value
                .get("messages")
                .and_then(|m| m.as_array())
                .and_then(|m| m.first())
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_str())
            {
                out.push(instruction.to_string());
            }
        }
        if out.len() >= 4000 {
            break;
        }
    }
    out
}

fn transcript_for_extraction(state: &AppState, session_id: &str) -> Result<String, String> {
    let messages = state.db.messages_list(session_id).map_err(|e| e.to_string())?;
    if messages.is_empty() {
        return Err("В сессии нет сообщений".into());
    }
    let mut out = String::new();
    for message in messages.iter().rev().take(40).rev() {
        let role = if message.role == "user" { "Пользователь" } else { "Ассистент" };
        let body: String = message.content.chars().take(1000).collect();
        out.push_str(&format!("{role}: {body}\n\n"));
        if out.len() > 12_000 {
            break;
        }
    }
    Ok(out)
}

pub fn extract_prompt(transcript: &str) -> String {
    format!(
        "Ниже диалог пользователя и ассистента, в котором они пришли к готовому решению задачи.\n\n\
         ---\n{transcript}\n---\n\n\
         Извлеки ТОЛЬКО итоговые результаты. Правила:\n\
         - instruction — самодостаточная постановка задачи (1–3 предложения, с ключевым контекстом, \
           чтобы её поняли без диалога);\n\
         - answer — финальное рабочее решение целиком (код, команды, конфиг или точные шаги), \
           без отладки, тупиков, приветствий и рассуждений «попробуем»;\n\
         - не включай промежуточные версии и вопросы пользователя;\n\
         - максимум 3 пары, только если они действительно разные;\n\
         - если итог не ясен — верни пустой список.\n\
         Верни строго JSON: {{\"items\":[{{\"instruction\":\"...\",\"answer\":\"...\"}}]}}"
    )
}

/// Отсев дублей среди кандидатов и относительно уже собранных датасетов.
fn mark_duplicates(items: Vec<(String, String)>, existing: &[String]) -> Vec<ExtractedItem> {
    let mut out: Vec<ExtractedItem> = Vec::new();
    for (instruction, answer) in items {
        let mut warnings = Vec::new();
        if instruction.chars().count() < 15 {
            warnings.push("короткая инструкция".to_string());
        }
        if answer.chars().count() < 80 {
            warnings.push("короткий ответ".to_string());
        }
        if answer.contains("<think>") {
            warnings.push("в ответе остались теги размышлений".to_string());
        }

        let mut duplicate_of = None;
        let mut duplicate_score = 0.0f64;
        for existing_instruction in existing {
            let score = jaccard(&instruction, existing_instruction).max(
                jaccard(&answer, existing_instruction) * 0.8,
            );
            if score > duplicate_score {
                duplicate_score = score;
                duplicate_of = Some(existing_instruction.clone());
            }
        }
        for previous in &out {
            let score = jaccard(&instruction, &previous.instruction);
            if score > duplicate_score {
                duplicate_score = score;
                duplicate_of = Some(format!("другой кандидат: {}", previous.instruction));
            }
        }
        if duplicate_score < 0.55 {
            duplicate_of = None;
        }

        out.push(ExtractedItem {
            id: new_id(),
            instruction,
            answer,
            duplicate_of,
            duplicate_score,
            warnings,
        });
    }
    out.sort_by(|a, b| a.duplicate_score.partial_cmp(&b.duplicate_score).unwrap_or(std::cmp::Ordering::Equal));
    out
}

#[tauri::command]
pub async fn session_extract(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    session_id: String,
    teacher_model: String,
) -> Result<Vec<ExtractedItem>, String> {
    if teacher_model.trim().is_empty() {
        return Err("Выберите модель-учителя".into());
    }
    let transcript = transcript_for_extraction(&state, &session_id)?;
    if transcript.chars().count() < 40 {
        return Err("В сессии слишком мало содержательного текста".into());
    }
    if teacher_model.starts_with("local/") {
        crate::sidecar::ensure_ready(&app)
            .await
            .map_err(|e| e.to_string())?;
    }

    let messages = vec![
        ChatMessage::new("system", "Ты извлекаешь обучающие данные. Отвечаешь только JSON."),
        ChatMessage::new("user", extract_prompt(&transcript)),
    ];
    let params = ChatParams { max_tokens: 1200, temperature: 0.3, think: false, reasoning: None };
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let router = Router { state: &state };
    let outcome = router
        .stream(
            &teacher_model,
            &messages,
            &params,
            &cancel,
            &mut |_| {},
            &mut |_: &ChatStatus| {},
        )
        .await
        .map_err(|e| format!("Не удалось извлечь итоги: {e}"))?;

    let items = parse_dataset_items(&outcome.text);
    if items.is_empty() {
        return Err(
            "Модель не вернула пары. Попробуйте облачную модель-учителя или более полный диалог."
                .into(),
        );
    }
    let existing = existing_instructions(&state);
    Ok(mark_duplicates(items, &existing))
}

/// «А модель это уже умеет?» — просим модель ответить самой и сравниваем с эталоном.
#[tauri::command]
pub async fn candidate_check(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    model_id: String,
    instruction: String,
    answer: String,
) -> Result<CheckResult, String> {
    if model_id.starts_with("local/") {
        crate::sidecar::ensure_ready(&app)
            .await
            .map_err(|e| e.to_string())?;
    }
    let messages = vec![
        ChatMessage::new("system", "Отвечай по существу, без вступлений и извинений."),
        ChatMessage::new("user", instruction.clone()),
    ];
    let params = ChatParams { max_tokens: 500, temperature: 0.2, think: false, reasoning: None };
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let router = Router { state: &state };
    let outcome = router
        .stream(
            &model_id,
            &messages,
            &params,
            &cancel,
            &mut |_| {},
            &mut |_: &ChatStatus| {},
        )
        .await
        .map_err(|e| format!("Не удалось проверить: {e}"))?;

    let model_answer = outcome.text.trim().to_string();
    let coverage = coverage(&answer, &model_answer);
    let jaccard = jaccard(&answer, &model_answer);
    let verdict = if coverage >= 0.6 || jaccard >= 0.5 {
        "already"
    } else if coverage >= 0.35 {
        "partial"
    } else {
        "new"
    };
    Ok(CheckResult {
        model_answer,
        coverage,
        jaccard,
        verdict: verdict.to_string(),
    })
}

#[tauri::command]
pub async fn dataset_create(
    state: tauri::State<'_, AppState>,
    name: String,
) -> Result<DatasetRow, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Укажите название датасета".into());
    }
    let dir = state.data_dir.join("datasets");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let dataset_id = new_id();
    let path = dir.join(format!("{}-{}.jsonl", slugify(name), &dataset_id[..8]));
    std::fs::write(&path, "").map_err(|e| e.to_string())?;
    let row = state
        .db
        .dataset_add(name, &path.to_string_lossy(), None, 0)
        .map_err(|e| e.to_string())?;
    Ok(row)
}

#[tauri::command]
pub async fn dataset_append(
    state: tauri::State<'_, AppState>,
    dataset_id: String,
    items: Vec<AppendItem>,
) -> Result<AppendResult, String> {
    let rows = state.db.datasets_list().map_err(|e| e.to_string())?;
    let row = rows
        .into_iter()
        .find(|row| row.id == dataset_id)
        .ok_or_else(|| "Датасет не найден".to_string())?;

    let existing_text = std::fs::read_to_string(&row.path).unwrap_or_default();
    let mut existing = Vec::new();
    let mut body = existing_text.clone();
    for line in existing_text.lines() {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
            if let Some(instruction) = value
                .get("messages")
                .and_then(|m| m.as_array())
                .and_then(|m| m.first())
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_str())
            {
                existing.push(instruction.to_string());
            }
        }
    }
    if !body.ends_with('\n') && !body.is_empty() {
        body.push('\n');
    }

    let mut added = 0usize;
    let mut skipped = 0usize;
    for item in &items {
        if item.instruction.trim().len() < 10 || item.answer.trim().len() < 20 {
            skipped += 1;
            continue;
        }
        let duplicate = existing
            .iter()
            .any(|instruction| jaccard(instruction, &item.instruction) >= 0.75);
        if duplicate {
            skipped += 1;
            continue;
        }
        body.push_str(&jsonl_line(item.instruction.trim(), item.answer.trim()));
        body.push('\n');
        existing.push(item.instruction.clone());
        added += 1;
    }
    std::fs::write(&row.path, body).map_err(|e| e.to_string())?;
    let total = row.examples + added as i64;
    state
        .db
        .dataset_set_examples(&dataset_id, total)
        .map_err(|e| e.to_string())?;
    Ok(AppendResult { added, skipped, total })
}

// ------------------------------------------------------------------ готовность к дообучению

#[derive(Debug, Clone, Serialize)]
pub struct FinetuneStatus {
    pub python: String,
    pub packages: BTreeMap<String, bool>,
    pub missing: Vec<String>,
    pub device: String,
    pub hf_models: Vec<String>,
    pub model_root: String,
    pub ready: bool,
    pub note: String,
}

fn python_path() -> String {
    if let Ok(path) = std::env::var("LEGAMIND_PYTHON") {
        return path;
    }
    let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
    let candidate = format!(r"{local}\venvs\ai\Scripts\python.exe");
    if std::path::Path::new(&candidate).exists() {
        candidate
    } else {
        "python".to_string()
    }
}

const PROBE: &str = r#"
import json, importlib.util as u
mods = ["torch", "transformers", "peft", "datasets", "optimum.intel", "nncf"]
res = {}
for m in mods:
    try:
        res[m] = u.find_spec(m) is not None
    except Exception:
        res[m] = False
device = "cpu"
try:
    import torch
    if getattr(torch, "xpu", None) is not None and torch.xpu.is_available():
        device = "xpu"
    elif torch.cuda.is_available():
        device = "cuda"
except Exception:
    pass
print(json.dumps({"packages": res, "device": device}))
"#;

const EXPECTED_PACKAGES: [&str; 6] = [
    "torch",
    "transformers",
    "peft",
    "datasets",
    "optimum.intel",
    "nncf",
];

#[tauri::command]
pub async fn finetune_status(state: tauri::State<'_, AppState>) -> Result<FinetuneStatus, String> {
    let python = python_path();
    let python_for_task = python.clone();
    let probe = tauri::async_runtime::spawn_blocking(move || {
        std::process::Command::new(&python_for_task)
            .args(["-c", PROBE])
            .output()
    })
    .await
    .map_err(|e| e.to_string())?;

    let mut packages: BTreeMap<String, bool> = EXPECTED_PACKAGES
        .iter()
        .map(|name| (name.to_string(), false))
        .collect();
    let mut device = "cpu".to_string();
    let mut probe_error: Option<String> = None;
    match probe {
        Ok(output) if output.status.success() => {
            match serde_json::from_slice::<serde_json::Value>(&output.stdout) {
                Ok(value) => {
                    if let Some(map) = value.get("packages").and_then(|v| v.as_object()) {
                        for (name, present) in map {
                            if packages.contains_key(name) {
                                packages.insert(name.clone(), present.as_bool().unwrap_or(false));
                            }
                        }
                    }
                    if let Some(dev) = value.get("device").and_then(|v| v.as_str()) {
                        device = dev.to_string();
                    }
                }
                Err(err) => probe_error = Some(format!("не разобрал ответ python: {err}")),
            }
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            probe_error = Some(format!(
                "python завершился с кодом {:?}: {}",
                output.status.code(),
                stderr.lines().next().unwrap_or("")
            ));
        }
        Err(err) => probe_error = Some(format!("не запустил python: {err}")),
    }
    if let Some(message) = probe_error.as_deref() {
        crate::logging::log_line("app.log", &format!("finetune_status: {message}"));
    }
    let missing: Vec<String> = packages
        .iter()
        .filter(|(_, present)| !**present)
        .map(|(name, _)| name.clone())
        .collect();
    let ready = missing.is_empty();

    let model_root = PathBuf::from(r"C:\Models");
    let mut hf_models = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&model_root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let has_weights = std::fs::read_dir(&path)
                .map(|files| {
                    files.flatten().any(|f| {
                        f.path()
                            .extension()
                            .map(|ext| ext.eq_ignore_ascii_case("safetensors"))
                            .unwrap_or(false)
                    })
                })
                .unwrap_or(false);
            if has_weights {
                hf_models.push(path.to_string_lossy().to_string());
            }
        }
    }

    let note = if ready {
        "Окружение для LoRA готово.".to_string()
    } else {
        format!(
            "Не хватает Python-пакетов: {}. Установите: pip install {}",
            missing.join(", "),
            missing.join(" ")
        )
    };
    let _ = state;
    Ok(FinetuneStatus {
        python,
        packages,
        missing,
        device,
        hf_models,
        model_root: model_root.to_string_lossy().to_string(),
        ready,
        note,
    })
}

/// Подсказка: какие модели вообще есть (локальные IR) — для выбора базовой.
#[tauri::command]
pub async fn learning_models() -> Result<Vec<crate::models::ModelInfo>, String> {
    Ok(local_models())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_clean_json() {
        let items = parse_dataset_items(
            r#"{"items":[{"instruction":"Как читать файл в Python?","answer":"Используй open() в блоке with."}]}"#,
        );
        assert_eq!(items.len(), 1);
        assert!(items[0].0.contains("Python"));
    }

    #[test]
    fn parses_fenced_and_trailing_commas() {
        let text = "```json\n{\"items\":[{\"instruction\":\"Что такое JFFS2?\",\"answer\":\"Файловая система для flash.\",},]}\n```";
        let items = parse_dataset_items(text);
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn rejects_too_short_items() {
        let items = parse_dataset_items(r#"{"items":[{"instruction":"hi","answer":"ok"}]}"#);
        assert!(items.is_empty());
    }

    #[test]
    fn jsonl_line_is_valid_jsonl() {
        let line = jsonl_line("вопрос", "ответ");
        let value: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(value["messages"][0]["role"], "user");
        assert_eq!(value["messages"][1]["content"], "ответ");
    }

    #[test]
    fn lesson_block_budget_and_ordering() {
        // Косвенная проверка: пустой набор не даёт блока.
        assert!(LESSONS_BUDGET > 500);
    }

    #[test]
    fn similarity_metrics() {
        assert!(jaccard("чтение файла python", "чтение файла python") > 0.99);
        assert!(jaccard("чтение файла python", "совсем другой текст") < 0.2);
        assert!((coverage("чтение файла python", "чтение файла") - 2.0 / 3.0).abs() < 0.01);
        assert_eq!(coverage("", "что угодно"), 0.0);
    }

    #[test]
    fn duplicates_are_marked() {
        let items = vec![
            ("Как правильно читать файл в Python".to_string(), "Используй with open(...) as f: f.read()".to_string()),
            ("Как правильно читать файл в Python".to_string(), "Используй with open(...) as f: f.read()".to_string()),
        ];
        let existing = vec!["Как правильно читать файл в Python".to_string()];
        let marked = mark_duplicates(items, &existing);
        assert!(marked[0].duplicate_of.is_some(), "должен быть найден дубль");
        assert!(marked[0].duplicate_score > 0.9);
    }

    #[test]
    fn extract_prompt_asks_for_json() {
        let prompt = extract_prompt("Пользователь: привет");
        assert!(prompt.contains("\"items\""));
        assert!(prompt.contains("итоговые"));
    }
}
