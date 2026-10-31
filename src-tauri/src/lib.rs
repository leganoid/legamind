//! Legamind: Tauri-приложение с локальными (OpenVINO) и облачными моделями.

pub mod chat;
pub mod db;
pub mod debate;
pub mod logging;
pub mod learning;
pub mod memory;
pub mod models;
pub mod providers;
pub mod rag;
pub mod secrets;
pub mod sidecar;
pub mod skills;
pub mod tools;
pub mod web;
pub mod workspace;
pub mod backup;
pub mod watcher;
pub mod local_models;
pub mod logs;
pub mod attachments;
pub mod export;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Result};
use serde::Serialize;
use tauri::{Manager, State};

use db::{ChatMessageRow, Db, Session};

pub struct AppState {
    pub db: Db,
    pub db_path: PathBuf,
    pub data_dir: PathBuf,
    pub client: reqwest::Client,
    pub sidecar: tokio::sync::Mutex<Option<Arc<sidecar::Sidecar>>>,
    pub cancel: Mutex<HashMap<String, Arc<AtomicBool>>>,
    pub engine_error: Mutex<Option<String>>,
    /// Ожидающие подтверждения записи файлов (approval_id → канал ответа).
    pub approvals: Mutex<HashMap<String, crate::tools::PendingApproval>>,
    pub watcher: Mutex<Option<notify::RecommendedWatcher>>,
    pub watch_root: Mutex<Option<PathBuf>>,
    /// Текст вложений текущего запроса (request_id → блок для промпта).
    pub chat_attachments: Mutex<HashMap<String, String>>,
    /// Изображения текущего запроса (request_id → data URL).
    pub chat_images: Mutex<HashMap<String, Vec<String>>>,
}

impl AppState {
    pub async fn engine_base(&self) -> Result<String> {
        let guard = self.sidecar.lock().await;
        match guard.as_ref() {
            Some(sidecar) => Ok(format!("http://127.0.0.1:{}", sidecar.port)),
            None => Err(anyhow!(
                "Локальный движок не запущен. Проверьте logs\\engine.log и путь к Python в настройках."
            )),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AppInfo {
    pub version: String,
    pub db_path: String,
    pub data_dir: String,
    pub sidecar_port: Option<u16>,
    pub engine_status: String,
    pub python_path: String,
}

#[tauri::command]
async fn app_info(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<AppInfo, String> {
    let port = sidecar::port_if_ready(&state).await;
    let engine_status = if port.is_some() { "работает".to_string() } else { "не запущен".to_string() };
    let python = std::env::var("LEGAMIND_PYTHON").unwrap_or_else(|_| {
        std::env::var("LOCALAPPDATA")
            .map(|local| format!("{local}\\venvs\\ai\\Scripts\\python.exe"))
            .unwrap_or_else(|_| "python".to_string())
    });
    let _ = app;
    Ok(AppInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        db_path: state.db_path.to_string_lossy().to_string(),
        data_dir: state.data_dir.to_string_lossy().to_string(),
        sidecar_port: port,
        engine_status,
        python_path: python,
    })
}

#[tauri::command]
async fn open_data_dir(state: State<'_, AppState>) -> Result<(), String> {
    #[cfg(windows)]
    {
        std::process::Command::new("explorer")
            .arg(state.data_dir.as_os_str())
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ---------------------------------------------------------------------- сессии
#[tauri::command]
async fn sessions_list(state: State<'_, AppState>) -> Result<Vec<Session>, String> {
    state.db.sessions_list().map_err(|e| e.to_string())
}

#[tauri::command]
async fn sessions_create(state: State<'_, AppState>, title: Option<String>, model_id: String) -> Result<Session, String> {
    let title = title.unwrap_or_else(|| "Новая сессия".to_string());
    state.db.session_create(&title, &model_id).map_err(|e| e.to_string())
}

#[tauri::command]
async fn sessions_rename(state: State<'_, AppState>, id: String, title: String) -> Result<(), String> {
    state.db.session_rename(&id, title.trim()).map_err(|e| e.to_string())
}

#[tauri::command]
async fn sessions_delete(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.db.session_delete(&id).map_err(|e| e.to_string())
}

#[tauri::command]
async fn messages_list(state: State<'_, AppState>, session_id: String) -> Result<Vec<ChatMessageRow>, String> {
    state.db.messages_list(&session_id).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------- модели
#[derive(Debug, Clone, Serialize)]
pub struct LoadResult {
    pub loaded: bool,
    pub load_seconds: Option<f64>,
}

#[tauri::command]
async fn models_load(app: tauri::AppHandle, state: State<'_, AppState>, id: String) -> Result<LoadResult, String> {
    sidecar::ensure_ready(&app).await.map_err(|e| e.to_string())?;
    let base = state.engine_base().await.map_err(|e| e.to_string())?;
    let resp = state
        .client
        .post(format!("{base}/models/load"))
        .json(&serde_json::json!({ "id": id }))
        .timeout(std::time::Duration::from_secs(180))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(resp.text().await.unwrap_or_else(|_| "ошибка загрузки".into()));
    }
    let value: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    Ok(LoadResult {
        loaded: value.get("loaded").and_then(|v| v.as_bool()).unwrap_or(true),
        load_seconds: value.get("load_seconds").and_then(|v| v.as_f64()),
    })
}

#[tauri::command]
async fn models_unload(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let base = state.engine_base().await.map_err(|e| e.to_string())?;
    state
        .client
        .post(format!("{base}/models/unload"))
        .json(&serde_json::json!({ "id": id }))
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn engine_restart(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<u16, String> {
    {
        let mut guard = state.sidecar.lock().await;
        if let Some(existing) = guard.take() {
            existing.kill();
        }
    }
    let result = sidecar::ensure_ready(&app).await;
    match result {
        Ok(port) => Ok(port),
        Err(err) => {
            *state.engine_error.lock().unwrap() = Some(err.to_string());
            Err(err.to_string())
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
fn copy_dir_all(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let target = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Переносит данные из старого имени `com.legamind.app` в `com.legamind.app`.
fn migrate_legacy_data(new_dir: &std::path::Path) {
    let Some(local) = std::env::var("APPDATA").ok() else {
        return;
    };
    let legacy_dir = std::path::PathBuf::from(local).join("com.legamind.app");
    if !new_dir.exists() && legacy_dir.exists() {
        match copy_dir_all(&legacy_dir, new_dir) {
            Ok(()) => logging::log_line("app.log", "migrated data from com.legamind.app"),
            Err(err) => logging::log_line("app.log", &format!("data migration failed: {err}")),
        }
    }
    let _ = std::fs::create_dir_all(new_dir);
    let legacy_db = new_dir.join("legamind.db");
    let target_db = new_dir.join("legamind.db");
    if legacy_db.exists() && !target_db.exists() {
        let _ = std::fs::rename(&legacy_db, &target_db);
        for suffix in ["-wal", "-shm"] {
            let from = new_dir.join(format!("legamind.db{suffix}"));
            let to = new_dir.join(format!("legamind.db{suffix}"));
            if from.exists() {
                let _ = std::fs::rename(from, to);
            }
        }
    }
}

pub fn run() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .try_init()
        .ok();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            migrate_legacy_data(&data_dir);
            std::fs::create_dir_all(&data_dir)?;
            let db_path = data_dir.join("legamind.db");
            let db = Db::open(&db_path)?;
            let client = reqwest::Client::builder()
                .user_agent("legamind/0.1")
                .build()?;
            secrets::migrate_legacy_keys(&db);
            let _ = backup::auto_backup(&db, &data_dir);

            // На HiDPI-экранах окно из конфига (логические пиксели) может не влезть
            // в экран. После появления окна подгоняем размер под монитор.
            if let Some(window) = app.get_webview_window("main") {
                let window = window.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(350)).await;
                    let monitor = window.current_monitor().ok().flatten().or_else(|| {
                        window.primary_monitor().ok().flatten()
                    });
                    if let Some(monitor) = monitor {
                        let scale = if monitor.scale_factor() > 0.0 {
                            monitor.scale_factor()
                        } else {
                            1.0
                        };
                        let size = monitor.size();
                        let logical_w = size.width as f64 / scale;
                        let logical_h = size.height as f64 / scale;
                        let max_width = (logical_w - 32.0).max(640.0);
                        let max_height = (logical_h - 72.0).max(480.0);

                        // Не раздуваем окно: уменьшаем только если оно больше монитора.
                        if let Ok(current) = window.inner_size() {
                            let logical = current.to_logical::<f64>(scale);
                            if logical.width > max_width || logical.height > max_height {
                                let _ = window.set_size(tauri::LogicalSize::new(
                                    logical.width.min(max_width),
                                    logical.height.min(max_height),
                                ));
                            }
                        }

                        // И не даём окну уехать за пределы монитора.
                        if let (Ok(position), Ok(outer)) =
                            (window.outer_position(), window.outer_size())
                        {
                            let monitor_position = monitor.position();
                            let max_x = monitor_position.x + size.width as i32 - outer.width as i32;
                            let max_y = monitor_position.y + size.height as i32 - outer.height as i32;
                            let x = position
                                .x
                                .clamp(monitor_position.x, max_x.max(monitor_position.x));
                            let y = position
                                .y
                                .clamp(monitor_position.y, max_y.max(monitor_position.y));
                            if x != position.x || y != position.y {
                                let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
                            }
                        }
                    }
                });
            }

            app.manage(AppState {
                db,
                db_path,
                data_dir,
                client,
                sidecar: tokio::sync::Mutex::new(None),
                cancel: Mutex::new(HashMap::new()),
                engine_error: Mutex::new(None),
                approvals: Mutex::new(HashMap::new()),
                watcher: Mutex::new(None),
                watch_root: Mutex::new(None),
                chat_attachments: Mutex::new(HashMap::new()),
                chat_images: Mutex::new(HashMap::new()),
            });

            // Автокопия: проверка раз в час, фактически — не чаще раза в 24 часа.
            let backup_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
                    let state = backup_handle.state::<AppState>();
                    let _ = backup::auto_backup(&state.db, &state.data_dir);
                }
            });

            // Сайдкар стартует вместе с приложением, но не блокирует окно.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Err(err) = sidecar::ensure_ready(&handle).await {
                    logging::log_line("app.log", &format!("sidecar start failed: {err:#}"));
                    let state = handle.state::<AppState>();
                    *state.engine_error.lock().unwrap() = Some(err.to_string());
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            open_data_dir,
            sessions_list,
            sessions_create,
            sessions_rename,
            sessions_delete,
            messages_list,
            chat::chat_send,
            chat::chat_stop,
            chat::session_pin,
            chat::chat_regenerate,
            chat::chat_edit_and_resend,
            chat::context_usage,
            chat::chat_command,
            chat::sources_list,
            chat::messages_search,
            chat::message_tools,
            workspace::pick_folder,
            workspace::session_set_workdir,
            workspace::session_set_policy,
            workspace::open_path,
            tools::tool_approve,
            tools::tool_runs_list,
            tools::tool_undo_last,
            tools::session_permissions_clear,
            rag::rag_reindex,
            rag::rag_status,
            rag::rag_get_enabled,
            rag::rag_set_enabled,
            backup::db_backup,
            backup::db_restore,
            backup::db_backup_info,
            watcher::watch_workdir,
            watcher::unwatch_workdir,
            local_models::local_models_list,
            local_models::local_model_add,
            local_models::local_model_remove,
            logs::logs_tail,
            attachments::pick_text_attachment,
            export::export_all,
            skills::skills_list,
            skills::skill_save,
            skills::skill_delete,
            skills::skill_set_enabled,
            learning::lessons_list,
            learning::lesson_add,
            learning::lesson_delete,
            learning::lesson_set_enabled,
            learning::feedback_set,
            learning::datasets_list,
            learning::dataset_generate,
            learning::dataset_delete,
            learning::dataset_create,
            learning::dataset_append,
            learning::session_extract,
            learning::candidate_check,
            web::web_config_get,
            web::web_config_set,
            web::web_search,
            web::web_test,
            web::web_fetch_page,
            learning::finetune_status,
            learning::learning_models,
            models_load,
            models_unload,
            engine_restart,
            providers::models_list,
            providers::engine_stats,
            providers::providers_list,
            providers::provider_save,
            providers::provider_check,
            providers::provider_refresh,
            providers::provider_set_enabled,
            providers::provider_delete,
            providers::provider_models,
            memory::memory_add,
            memory::memories_list,
            memory::memory_search,
            memory::memory_delete,
            memory::memory_context_preview,
            debate::debate_start,
            debate::debate_stop,
            debate::debates_list,
            debate::debate_get,
            debate::debate_export_md,
        ])
        .build(tauri::generate_context!())
        .expect("ошибка сборки Tauri")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                sidecar::shutdown(app);
            }
        });
}

/// Команда-заглушка не нужна: реестр локальных моделей отдаётся через `models_list`.
#[allow(dead_code)]
pub fn cancel_all(state: &AppState) {
    for (_, flag) in state.cancel.lock().unwrap().iter() {
        flag.store(true, Ordering::Relaxed);
    }
}
