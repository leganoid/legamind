//! Opt-in телеметрия: только версия, ОС и случайный install_id.
//! Никакого содержимого чатов, путей и ключей.

use anyhow::{bail, Result};
use serde::Serialize;
use tauri::State;

use crate::AppState;

const ENABLED_KEY: &str = "telemetry_enabled";
const URL_KEY: &str = "telemetry_url";
const INSTALL_KEY: &str = "telemetry_install_id";

#[derive(Debug, Clone, Serialize)]
pub struct TelemetryConfig {
    pub enabled: bool,
    pub url: String,
    pub install_id: String,
}

fn install_id(state: &AppState) -> Result<String> {
    if let Some(value) = state.db.setting_get(INSTALL_KEY)? {
        if !value.trim().is_empty() {
            return Ok(value);
        }
    }
    let value = crate::db::new_id();
    state.db.setting_set(INSTALL_KEY, &value)?;
    Ok(value)
}

fn config(state: &AppState) -> Result<TelemetryConfig> {
    Ok(TelemetryConfig {
        enabled: state
            .db
            .setting_get(ENABLED_KEY)?
            .map(|value| value == "1")
            .unwrap_or(false),
        url: state.db.setting_get(URL_KEY)?.unwrap_or_default(),
        install_id: install_id(state)?,
    })
}

#[tauri::command]
pub fn telemetry_get(state: State<'_, AppState>) -> Result<TelemetryConfig, String> {
    config(&state).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn telemetry_set(
    state: State<'_, AppState>,
    enabled: bool,
    url: String,
) -> Result<TelemetryConfig, String> {
    let url = url.trim().to_string();
    if !url.is_empty() && !url.starts_with("https://") && !url.starts_with("http://") {
        return Err("URL должен начинаться с http:// или https://".into());
    }
    state
        .db
        .setting_set(ENABLED_KEY, if enabled { "1" } else { "0" })
        .map_err(|e| e.to_string())?;
    state
        .db
        .setting_set(URL_KEY, &url)
        .map_err(|e| e.to_string())?;
    config(&state).map_err(|e| e.to_string())
}

async fn post_event(state: &AppState, event: &str) -> Result<()> {
    let cfg = config(state)?;
    if !cfg.enabled || cfg.url.trim().is_empty() {
        bail!("Телеметрия выключена или URL не задан");
    }
    let payload = serde_json::json!({
        "event": event,
        "app": "legamind",
        "version": env!("CARGO_PKG_VERSION"),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "install_id": cfg.install_id,
        "timestamp": chrono::Utc::now().to_rfc3339(),
    });
    let resp = state
        .client
        .post(&cfg.url)
        .json(&payload)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await?;
    if !resp.status().is_success() {
        bail!("Endpoint вернул {}", resp.status());
    }
    Ok(())
}

#[tauri::command]
pub async fn telemetry_test(state: State<'_, AppState>) -> Result<(), String> {
    post_event(&state, "test").await.map_err(|e| e.to_string())
}

pub async fn startup(app: tauri::AppHandle) {
    use tauri::Manager;
    let state = app.state::<AppState>();
    if let Err(err) = post_event(&state, "app_start").await {
        let _ = crate::logging::log_line("app.log", &format!("telemetry skipped: {err}"));
    }
}
