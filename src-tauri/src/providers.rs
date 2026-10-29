//! Облачные провайдеры: CRUD, проверка соединения, список моделей.
//!
//! Ключ хранится только в Credential Manager / DPAPI; в БД — ссылка `provider:<id>`.

use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::State;

use crate::db::{now, ProviderRow};
use crate::models::{mask_key, ModelInfo};
use crate::secrets;
use crate::AppState;

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderInput {
    pub id: Option<String>,
    pub kind: String,
    pub label: String,
    pub base_url: String,
    pub api_key: Option<String>,
    pub timeout_s: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderView {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub base_url: String,
    pub enabled: bool,
    pub timeout_s: i64,
    pub has_key: bool,
    pub key_mask: Option<String>,
    pub models_count: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckResult {
    pub ok: bool,
    pub message: String,
    pub models: Vec<String>,
}

fn slugify(text: &str) -> String {
    let slug: String = text
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() { "provider".to_string() } else { slug }
}

fn provider_view(state: &AppState, row: &ProviderRow) -> Result<ProviderView> {
    let key = secrets::get_key(state, &row.id)?;
    let models_count = state.db.provider_models(&row.id)?.len() as i64;
    Ok(ProviderView {
        id: row.id.clone(),
        kind: row.kind.clone(),
        label: row.label.clone(),
        base_url: row.base_url.clone(),
        enabled: row.enabled,
        timeout_s: row.timeout_s,
        has_key: key.is_some(),
        key_mask: key.as_deref().map(mask_key),
        models_count,
        created_at: row.created_at.clone(),
    })
}

/// GET {base_url}/models (OpenAI) или /v1/models (Anthropic).
pub async fn fetch_models(
    client: &reqwest::Client,
    kind: &str,
    base_url: &str,
    key: &str,
    timeout_s: i64,
) -> Result<Vec<(String, Option<String>, Option<i64>)>> {
    let base = base_url.trim_end_matches('/');
    let (url, request) = if kind == "anthropic" {
        (
            format!("{base}/v1/models"),
            client
                .get(format!("{base}/v1/models"))
                .header("x-api-key", key)
                .header("anthropic-version", "2023-06-01"),
        )
    } else {
        (
            format!("{base}/models"),
            client.get(format!("{base}/models")).header("Authorization", format!("Bearer {key}")),
        )
    };
    let _ = url;

    let resp = request
        .timeout(Duration::from_secs(timeout_s.max(10) as u64))
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                anyhow!("Сервер не ответил вовремя")
            } else {
                anyhow!("Сервер недоступен: {e}")
            }
        })?;

    match resp.status().as_u16() {
        401 | 403 => bail!("Ключ не принят"),
        s if s >= 500 => bail!("Ошибка сервера провайдера (HTTP {s})"),
        s if !(200..300).contains(&s) => bail!("Сервер вернул HTTP {s}"),
        _ => {}
    }

    let value: Value = resp.json().await.map_err(|e| anyhow!("Некорректный ответ сервера: {e}"))?;
    let items = value
        .get("data")
        .and_then(|d| d.as_array())
        .cloned()
        .or_else(|| value.as_array().cloned())
        .unwrap_or_default();

    let mut models = Vec::new();
    for item in items {
        let id = item
            .get("id")
            .or_else(|| item.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if id.is_empty() {
            continue;
        }
        let label = item
            .get("display_name")
            .or_else(|| item.get("name"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let context = item
            .get("context_length")
            .or_else(|| item.get("context_window"))
            .or_else(|| item.get("max_context_length"))
            .and_then(|v| v.as_i64());
        models.push((id, label, context));
    }
    if models.is_empty() {
        bail!("Сервер ответил, но список моделей пуст");
    }
    Ok(models)
}

#[tauri::command]
pub async fn providers_list(state: State<'_, AppState>) -> Result<Vec<ProviderView>, String> {
    let rows = state.db.providers_list().map_err(|e| e.to_string())?;
    rows.iter()
        .map(|row| provider_view(&state, row).map_err(|e| e.to_string()))
        .collect()
}

#[tauri::command]
pub async fn provider_save(state: State<'_, AppState>, input: ProviderInput) -> Result<ProviderView, String> {
    let base_url = input.base_url.trim().trim_end_matches('/').to_string();
    if !base_url.starts_with("http://") && !base_url.starts_with("https://") {
        return Err("Адрес должен начинаться с http:// или https://".into());
    }
    if input.label.trim().is_empty() {
        return Err("Укажите название провайдера".into());
    }
    let kind = match input.kind.as_str() {
        "anthropic" => "anthropic".to_string(),
        _ => "openai".to_string(),
    };
    let id = input
        .id
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| slugify(&input.label));

    let existing = state.db.provider_get(&id).map_err(|e| e.to_string())?;
    let row = ProviderRow {
        id: id.clone(),
        kind,
        label: input.label.trim().to_string(),
        base_url,
        key_ref: format!("provider:{id}"),
        enabled: true,
        timeout_s: input.timeout_s.unwrap_or(120),
        created_at: existing.map(|p| p.created_at).unwrap_or_else(now),
    };
    state.db.provider_upsert(&row).map_err(|e| e.to_string())?;

    if let Some(key) = input.api_key.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        secrets::set_key(&state, &id, key).map_err(|e| e.to_string())?;
    }

    // Сразу подтягиваем список моделей, чтобы он появился в выборе без отдельных действий.
    if let Ok(Some(key)) = secrets::get_key(&state, &id) {
        if let Ok(models) = fetch_models(
            &state.client,
            &row.kind,
            &row.base_url,
            &key,
            row.timeout_s.min(20),
        )
        .await
        {
            state
                .db
                .provider_models_replace(&id, &models)
                .map_err(|e| e.to_string())?;
        }
    }

    provider_view(&state, &row).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn provider_check(state: State<'_, AppState>, input: ProviderInput) -> Result<CheckResult, String> {
    let key = match input.api_key.as_deref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        Some(key) => key.to_string(),
        None => {
            let id = input.id.clone().unwrap_or_else(|| slugify(&input.label));
            secrets::get_key(&state, &id)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "Сначала введите API-ключ".to_string())?
        }
    };

    match fetch_models(
        &state.client,
        &input.kind,
        &input.base_url,
        &key,
        input.timeout_s.unwrap_or(120),
    )
    .await
    {
        Ok(models) => {
            // TASK.md 5.4.4: успешная проверка сразу сохраняет список моделей,
            // чтобы они появились в общем списке выбора (если провайдер уже в БД).
            let provider_id = input.id.clone().unwrap_or_else(|| slugify(&input.label));
            if state
                .db
                .provider_get(&provider_id)
                .map_err(|e| e.to_string())?
                .is_some()
            {
                state
                    .db
                    .provider_models_replace(&provider_id, &models)
                    .map_err(|e| e.to_string())?;
            }
            Ok(CheckResult {
                ok: true,
                message: format!("Соединение установлено, моделей: {}", models.len()),
                models: models.into_iter().map(|(id, _, _)| id).collect(),
            })
        }
        Err(e) => Ok(CheckResult { ok: false, message: e.to_string(), models: vec![] }),
    }
}

/// Обновление кэша моделей у сохранённого провайдера.
#[tauri::command]
pub async fn provider_refresh(state: State<'_, AppState>, id: String) -> Result<CheckResult, String> {
    let row = state
        .db
        .provider_get(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Провайдер не найден".to_string())?;
    let key = secrets::get_key(&state, &id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Ключ не найден. Введите его заново.".to_string())?;

    let models = match fetch_models(&state.client, &row.kind, &row.base_url, &key, row.timeout_s).await {
        Ok(models) => models,
        Err(e) => return Ok(CheckResult { ok: false, message: e.to_string(), models: vec![] }),
    };
    let count = models.len();
    state
        .db
        .provider_models_replace(&id, &models)
        .map_err(|e| e.to_string())?;
    Ok(CheckResult {
        ok: true,
        message: format!("Модели обновлены: {count}"),
        models: models.into_iter().map(|(id, _, _)| id).collect(),
    })
}

#[tauri::command]
pub async fn provider_set_enabled(state: State<'_, AppState>, id: String, enabled: bool) -> Result<(), String> {
    state.db.provider_set_enabled(&id, enabled).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn provider_delete(state: State<'_, AppState>, id: String) -> Result<(), String> {
    secrets::delete_key(&state, &id).map_err(|e| e.to_string())?;
    state.db.provider_delete(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn provider_models(state: State<'_, AppState>, id: String) -> Result<Vec<crate::db::ProviderModelRow>, String> {
    state.db.provider_models(&id).map_err(|e| e.to_string())
}

/// Полный список моделей: локальные + модели включённых провайдеров.
#[tauri::command]
pub async fn models_list(state: State<'_, AppState>) -> Result<Vec<ModelInfo>, String> {
    let mut models = crate::models::local_models();

    // Статус «загружена» берём у сайдкара, если он уже работает.
    if let Some(port) = crate::sidecar::port_if_ready(&state).await {
        let url = format!("http://127.0.0.1:{port}/models");
        if let Ok(resp) = state.client.get(url).timeout(Duration::from_secs(2)).send().await {
            if let Ok(list) = resp.json::<Vec<Value>>().await {
                for item in list {
                    let id = item.get("id").and_then(|v| v.as_str()).unwrap_or("");
                    let loaded = item.get("loaded").and_then(|v| v.as_bool()).unwrap_or(false);
                    if let Some(model) = models.iter_mut().find(|m| m.id == format!("local/{id}")) {
                        model.loaded = loaded;
                    }
                }
            }
        }
    }

    let providers = state.db.providers_list().map_err(|e| e.to_string())?;
    let provider_models = state.db.provider_models_all().map_err(|e| e.to_string())?;
    for provider in providers.iter().filter(|p| p.enabled) {
        for item in provider_models.iter().filter(|m| m.provider_id == provider.id) {
            models.push(ModelInfo {
                id: format!("{}/{}", provider.id, item.model_id),
                label: item.label.clone().unwrap_or_else(|| item.model_id.clone()),
                provider: provider.id.clone(),
                kind: "cloud".to_string(),
                device: None,
                size_gb: None,
                loaded: false,
                thinks: false,
                context: item.context.map(|c| c as u32),
            });
        }
    }
    Ok(models)
}
