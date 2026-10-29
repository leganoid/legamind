//! Навыки: наборы инструкций, которые подмешиваются в system-промпт.
//!
//! Конструктор хранит части навыка отдельно (роль, инструкции, формат, примеры,
//! ограничения, триггеры), а в промпт собирается единый блок.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::db::{new_id, now, SkillRow};
use crate::AppState;

/// Сколько символов суммарно отводим под навыки в промпте.
const SKILLS_BUDGET: usize = 4000;

#[derive(Debug, Clone, Deserialize)]
pub struct SkillInput {
    pub id: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub role: Option<String>,
    pub guidelines: Option<String>,
    pub output_format: Option<String>,
    pub examples: Option<String>,
    pub constraints: Option<String>,
    pub triggers: Option<String>,
    pub enabled: Option<bool>,
}

fn clean(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Собирает текст навыка для system-промпта.
pub fn compose(skill: &SkillRow) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(description) = clean(&skill.description) {
        parts.push(description);
    }
    if let Some(role) = clean(&skill.role) {
        parts.push(format!("Роль: {role}"));
    }
    if let Some(guidelines) = clean(&skill.guidelines) {
        parts.push(guidelines);
    }
    if let Some(output_format) = clean(&skill.output_format) {
        parts.push(format!("Формат ответа: {output_format}"));
    }
    if let Some(examples) = clean(&skill.examples) {
        parts.push(format!("Примеры:\n{examples}"));
    }
    if let Some(constraints) = clean(&skill.constraints) {
        parts.push(format!("Ограничения: {constraints}"));
    }
    if parts.is_empty() {
        return String::new();
    }
    format!("### {}\n{}", skill.name, parts.join("\n\n"))
}

/// Подходит ли навык к сообщению пользователя (по ключевым словам).
fn triggers_match(skill: &SkillRow, text: &str) -> bool {
    let Some(triggers) = clean(&skill.triggers) else {
        return true; // без триггеров навык активен всегда
    };
    let words: Vec<String> = triggers
        .to_lowercase()
        .split(|c: char| c == ',' || c == ';' || c == '\n')
        .map(|w| w.trim().to_string())
        .filter(|w| !w.is_empty())
        .collect();
    if words.is_empty() {
        return true;
    }
    let haystack = text.to_lowercase();
    words.iter().any(|word| haystack.contains(word))
}

/// Блок активных навыков для промпта (с бюджетом и учётом триггеров).
pub fn active_block(state: &AppState, user_text: &str) -> Result<Option<String>> {
    let skills = state.db.skills_list()?;
    let mut out = String::from("Активные навыки (следуй им):\n");
    let mut used = out.len();
    let mut included = 0usize;
    for skill in skills.iter().filter(|s| s.enabled) {
        if !triggers_match(skill, user_text) {
            continue;
        }
        let body = compose(skill);
        if body.is_empty() {
            continue;
        }
        if used + body.len() + 2 > SKILLS_BUDGET {
            break;
        }
        out.push('\n');
        out.push_str(&body);
        out.push('\n');
        used += body.len() + 2;
        included += 1;
    }
    if included == 0 {
        return Ok(None);
    }
    Ok(Some(out.trim_end().to_string()))
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillView {
    #[serde(flatten)]
    pub row: SkillRow,
    pub preview: String,
}

#[tauri::command]
pub async fn skills_list(state: State<'_, AppState>) -> Result<Vec<SkillView>, String> {
    let rows = state.db.skills_list().map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .map(|row| SkillView { preview: compose(&row), row })
        .collect())
}

#[tauri::command]
pub async fn skill_save(state: State<'_, AppState>, input: SkillInput) -> Result<SkillRow, String> {
    let name = input.name.trim().to_string();
    if name.is_empty() {
        return Err("Укажите название навыка".into());
    }
    let existing = state
        .db
        .skill_get_by_name(&name)
        .map_err(|e| e.to_string())?;
    if let Some(existing) = existing {
        if Some(&existing.id) != input.id.as_ref() {
            return Err(format!("Навык с названием «{name}» уже есть"));
        }
    }

    let previous = match input.id.as_deref() {
        Some(id) => state.db.skill_get(id).map_err(|e| e.to_string())?,
        None => None,
    };

    let row = SkillRow {
        id: previous
            .as_ref()
            .map(|p| p.id.clone())
            .unwrap_or_else(new_id),
        name,
        description: clean(&input.description),
        role: clean(&input.role),
        guidelines: clean(&input.guidelines),
        output_format: clean(&input.output_format),
        examples: clean(&input.examples),
        constraints: clean(&input.constraints),
        triggers: clean(&input.triggers),
        enabled: input.enabled.unwrap_or_else(|| previous.as_ref().map(|p| p.enabled).unwrap_or(false)),
        builtin: previous.as_ref().map(|p| p.builtin).unwrap_or(false),
        created_at: previous.map(|p| p.created_at).unwrap_or_else(now),
    };
    if compose(&row).is_empty() {
        return Err("Навык пустой: заполните хотя бы инструкции или роль".into());
    }
    state.db.skill_save(&row).map_err(|e| e.to_string())?;
    Ok(row)
}

#[tauri::command]
pub async fn skill_delete(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.db.skill_delete(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn skill_set_enabled(
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> Result<(), String> {
    state
        .db
        .skill_set_enabled(&id, enabled)
        .map_err(|e| e.to_string())
}

/// Переключить навык по имени (для команды `/skill <name>`).
pub fn toggle_by_name(db: &crate::db::Db, name: &str) -> Result<SkillRow, String> {
    let skill = db
        .skill_get_by_name(name)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Навык «{name}» не найден"))?;
    let enabled = !skill.enabled;
    db.skill_set_enabled(&skill.id, enabled)
        .map_err(|e| e.to_string())?;
    Ok(SkillRow { enabled, ..skill })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill(name: &str) -> SkillRow {
        SkillRow {
            id: "id".into(),
            name: name.into(),
            description: Some("описание".into()),
            role: Some("роль".into()),
            guidelines: Some("шаги".into()),
            output_format: Some("формат".into()),
            examples: Some("пример".into()),
            constraints: Some("нельзя".into()),
            triggers: None,
            enabled: true,
            builtin: false,
            created_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    #[test]
    fn compose_contains_all_parts() {
        let text = compose(&skill("Ревью"));
        assert!(text.starts_with("### Ревью"));
        for part in ["описание", "Роль: роль", "шаги", "Формат ответа: формат", "Примеры", "Ограничения: нельзя"] {
            assert!(text.contains(part), "нет «{part}» в {text}");
        }
    }

    #[test]
    fn compose_empty_when_no_content() {
        let mut empty = skill("Пусто");
        empty.description = None;
        empty.role = None;
        empty.guidelines = None;
        empty.output_format = None;
        empty.examples = None;
        empty.constraints = None;
        assert!(compose(&empty).is_empty());
    }

    #[test]
    fn triggers_are_matched() {
        let mut s = skill("Код");
        s.triggers = Some("код, python, рефакторинг".into());
        assert!(triggers_match(&s, "Помоги с Python-скриптом"));
        assert!(!triggers_match(&s, "Расскажи про историю Рима"));
        s.triggers = None;
        assert!(triggers_match(&s, "что угодно"));
    }
}
