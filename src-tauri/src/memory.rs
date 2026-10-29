//! Память: факты, эпизоды, предпочтения. Поиск — FTS5 (BM25), подмешивание в system-промпт.

use anyhow::Result;
use serde::Serialize;
use tauri::State;

use crate::db::MemoryRow;
use crate::AppState;

/// Бюджет блока памяти: не более ~800 токенов (≈3000 символов).
const MEMORY_BUDGET_CHARS: usize = 3000;
const MEMORY_TOP_K: i64 = 5;

pub fn build_context(state: &AppState, query: &str) -> Result<Option<String>> {
    let found = state.db.memory_search(query, MEMORY_TOP_K)?;
    if found.is_empty() {
        return Ok(None);
    }
    let header = "Известное о проекте (используй, если релевантно):\n";
    let mut out = String::from(header);
    for memory in found {
        let line = format!("- {}\n", memory.text.trim());
        if out.len() + line.len() > MEMORY_BUDGET_CHARS {
            break;
        }
        out.push_str(&line);
    }
    if out.trim() == header.trim() {
        return Ok(None);
    }
    Ok(Some(out.trim_end().to_string()))
}

/// Автосохранение эпизода после длинного чата или дебатов.
pub fn save_episode(state: &AppState, text: &str, session_id: Option<&str>) -> Result<()> {
    state.db.memory_add("episode", text, session_id)?;
    Ok(())
}

#[derive(Serialize)]
pub struct MemoryStats {
    pub total: i64,
}

#[tauri::command]
pub async fn memory_add(
    state: State<'_, AppState>,
    kind: String,
    text: String,
    session_id: Option<String>,
) -> Result<MemoryRow, String> {
    let kind = match kind.as_str() {
        "fact" | "episode" | "preference" => kind,
        _ => "fact".to_string(),
    };
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("Пустой текст памяти".into());
    }
    state
        .db
        .memory_add(&kind, &text, session_id.as_deref())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn memories_list(state: State<'_, AppState>, limit: Option<i64>) -> Result<Vec<MemoryRow>, String> {
    state
        .db
        .memories_list(limit.unwrap_or(200))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn memory_search(state: State<'_, AppState>, query: String, limit: Option<i64>) -> Result<Vec<MemoryRow>, String> {
    state
        .db
        .memory_search(&query, limit.unwrap_or(MEMORY_TOP_K))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn memory_delete(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.db.memory_delete(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn memory_context_preview(state: State<'_, AppState>, query: String) -> Result<Option<String>, String> {
    build_context(&state, &query).map_err(|e| e.to_string())
}
