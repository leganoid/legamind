//! Расширенная самопроверка: БД, движок, модели, RAG, провайдеры, интернет, копии.

use serde::Serialize;
use tauri::State;

use crate::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct CheckItem {
    pub id: String,
    pub label: String,
    /// ok | warn | fail
    pub status: String,
    pub detail: String,
}

fn item(id: &str, label: &str, status: &str, detail: impl Into<String>) -> CheckItem {
    CheckItem {
        id: id.to_string(),
        label: label.to_string(),
        status: status.to_string(),
        detail: detail.into(),
    }
}

#[tauri::command]
pub async fn self_check(state: State<'_, AppState>) -> Result<Vec<CheckItem>, String> {
    let mut checks = Vec::new();

    // 1. База данных.
    match state.db.schema_version() {
        Ok(version) => checks.push(item(
            "db",
            "База данных",
            "ok",
            format!("схема v{version}, доступна"),
        )),
        Err(err) => checks.push(item(
            "db",
            "База данных",
            "fail",
            format!("недоступна: {err}"),
        )),
    }

    // 2. Движок.
    let port = crate::sidecar::port_if_ready(&state).await;
    match port {
        Some(port) => {
            let url = format!("http://127.0.0.1:{port}/health");
            match state
                .client
                .get(url)
                .timeout(std::time::Duration::from_secs(3))
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => checks.push(item(
                    "engine",
                    "Движок OpenVINO",
                    "ok",
                    format!("отвечает на порту {port}"),
                )),
                Ok(resp) => checks.push(item(
                    "engine",
                    "Движок OpenVINO",
                    "warn",
                    format!("ответил статусом {}", resp.status()),
                )),
                Err(err) => checks.push(item(
                    "engine",
                    "Движок OpenVINO",
                    "fail",
                    format!("не отвечает: {err}"),
                )),
            }
        }
        None => checks.push(item(
            "engine",
            "Движок OpenVINO",
            "warn",
            "сайдкар ещё не готов (локальные модели недоступны)",
        )),
    }

    // 3. Модели.
    match state.db.provider_models_all() {
        Ok(models) => {
            let local = crate::models::local_models().len();
            let cloud = models.len();
            checks.push(item(
                "models",
                "Модели",
                if local + cloud > 0 { "ok" } else { "warn" },
                format!("локальных: {local}, облачных: {cloud}"),
            ));
        }
        Err(err) => checks.push(item(
            "models",
            "Модели",
            "fail",
            format!("не удалось прочитать список: {err}"),
        )),
    }

    // 4. RAG-индексы.
    match state.db.rag_overview() {
        Ok(overview) if overview.is_empty() => checks.push(item(
            "rag",
            "RAG-индекс",
            "warn",
            "индекс не построен — откройте проект и проиндексируйте",
        )),
        Ok(overview) => {
            let missing = overview
                .iter()
                .filter(|(root, _, _)| !std::path::Path::new(root).is_dir())
                .count();
            let total: i64 = overview.iter().map(|(_, chunks, _)| chunks).sum();
            checks.push(item(
                "rag",
                "RAG-индекс",
                if missing == 0 { "ok" } else { "warn" },
                format!(
                    "проектов: {}, чанков: {}{}",
                    overview.len(),
                    total,
                    if missing > 0 {
                        format!(", недоступно корней: {missing}")
                    } else {
                        String::new()
                    }
                ),
            ));
        }
        Err(err) => checks.push(item(
            "rag",
            "RAG-индекс",
            "fail",
            format!("ошибка чтения индекса: {err}"),
        )),
    }

    // 5. Провайдеры и ключи.
    match state.db.providers_list() {
        Ok(providers) => {
            let enabled: Vec<_> = providers.iter().filter(|p| p.enabled).collect();
            if enabled.is_empty() {
                checks.push(item(
                    "providers",
                    "Провайдеры",
                    "warn",
                    "нет включённых облачных провайдеров — доступны только локальные модели",
                ));
            } else {
                let missing: Vec<&str> = enabled
                    .iter()
                    .filter(|p| !crate::secrets::has_key(&state, &p.id))
                    .map(|p| p.label.as_str())
                    .collect();
                checks.push(item(
                    "providers",
                    "Провайдеры",
                    if missing.is_empty() { "ok" } else { "fail" },
                    if missing.is_empty() {
                        format!("включено: {}, ключи на месте", enabled.len())
                    } else {
                        format!("нет ключа: {}", missing.join(", "))
                    },
                ));
            }
        }
        Err(err) => checks.push(item(
            "providers",
            "Провайдеры",
            "fail",
            format!("не удалось прочитать список: {err}"),
        )),
    }

    // 6. Интернет-поиск.
    let web_enabled = crate::web::enabled(&state);
    let web_provider = crate::web::provider(&state);
    checks.push(item(
        "web",
        "Интернет-поиск",
        if web_enabled { "ok" } else { "warn" },
        if web_enabled {
            format!("включён, провайдер: {web_provider}")
        } else {
            "выключен в настройках".to_string()
        },
    ));

    // 7. Резервные копии.
    let info = crate::backup::backup_info(&state);
    checks.push(item(
        "backups",
        "Резервные копии",
        if info.auto_count > 0 { "ok" } else { "warn" },
        if info.auto_count > 0 {
            format!(
                "автокопий: {}; последняя: {}",
                info.auto_count,
                info.last_auto.clone().unwrap_or_else(|| "—".to_string())
            )
        } else {
            format!("автокопий пока нет; папка: {}", info.dir)
        },
    ));

    // 8. Логи.
    let logs_dir = state.data_dir.join("logs");
    checks.push(item(
        "logs",
        "Логи",
        if logs_dir.is_dir() { "ok" } else { "warn" },
        if logs_dir.is_dir() {
            logs_dir.to_string_lossy().to_string()
        } else {
            "папка логов ещё не создана".to_string()
        },
    ));

    Ok(checks)
}
