//! Запуск Python-сайдкара, чтение строки READY, health-check, остановка.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use serde_json::json;
use tauri::{AppHandle, Manager};

use crate::AppState;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub struct Sidecar {
    pub port: u16,
    child: Mutex<Option<Child>>,
}

impl Sidecar {
    /// Создаёт дескриптор уже работающего движка (используется интеграционными тестами).
    pub fn for_test(port: u16) -> Self {
        Self { port, child: Mutex::new(None) }
    }

    pub fn kill(&self) {
        if let Ok(mut guard) = self.child.lock() {
            if let Some(child) = guard.as_mut() {
                // venv python.exe — это launcher, который запускает базовый интерпретатор
                // дочерним процессом, поэтому убиваем всё дерево.
                #[cfg(windows)]
                {
                    let _ = Command::new("taskkill")
                        .args(["/PID", &child.id().to_string(), "/T", "/F"])
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .status();
                }
                let _ = child.kill();
                let _ = child.wait();
            }
            *guard = None;
        }
    }
}

impl Drop for Sidecar {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Каталог, в котором лежит пакет `engine` (в dev — корень репозитория).
fn engine_root(app: &AppHandle) -> PathBuf {
    if let Ok(resource_dir) = app.path().resource_dir() {
        if resource_dir.join("engine").join("main.py").exists() {
            return resource_dir;
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn python_path(app: &AppHandle) -> PathBuf {
    if let Ok(path) = std::env::var("LEGAMIND_PYTHON") {
        return PathBuf::from(path);
    }
    let state = app.state::<AppState>();
    if let Ok(Some(path)) = state.db.setting_get("python_path") {
        if !path.trim().is_empty() {
            return PathBuf::from(path);
        }
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        let candidate = Path::new(&local)
            .join("venvs")
            .join("ai")
            .join("Scripts")
            .join("python.exe");
        if candidate.exists() {
            return candidate;
        }
    }
    PathBuf::from("python")
}

/// Пишет `engine_config.json`: пути моделей и каталог кэша компиляции.
fn write_config(data_dir: &Path) -> Result<PathBuf> {
    let path = data_dir.join("engine_config.json");
    let config = json!({
        "cache_dir": r"C:\Models\cache",
        "models": {
            "qwen3-14b": {"path": r"C:\Models\qwen3-14b-int4-ov", "label": "Qwen3-14B", "device": "GPU", "thinks": true, "size_gb": 9.06, "max_tokens": 2048},
            "coder-30b": {"path": r"C:\Models\qwen3-coder-30b-a3b-int4-ov", "label": "Qwen3-Coder-30B-A3B", "device": "GPU", "thinks": false, "size_gb": 15.2, "max_tokens": 2048},
            "coder-7b": {"path": r"C:\Models\qwen2.5-coder-7b-int4-ov", "label": "Qwen2.5-Coder-7B", "device": "GPU", "thinks": false, "size_gb": 4.2, "max_tokens": 2048},
            "qwen-1.5b": {"path": r"C:\Models\qwen2.5-1.5b-int4-ov", "label": "Qwen2.5-1.5B", "device": "NPU", "thinks": false, "size_gb": 0.87, "max_tokens": 1024}
        }
    });
    std::fs::write(&path, serde_json::to_vec_pretty(&config)?)
        .with_context(|| format!("не удалось записать {}", path.display()))?;
    Ok(path)
}

fn spawn_blocking(app: &AppHandle) -> Result<Sidecar> {
    let state = app.state::<AppState>();
    let python = python_path(app);
    let root = engine_root(app);
    let config = write_config(&state.data_dir)?;

    let mut cmd = Command::new(&python);
    cmd.arg("-m")
        .arg("engine")
        .arg("--port")
        .arg("0")
        .arg("--host")
        .arg("127.0.0.1")
        .arg("--models-config")
        .arg(&config)
        .current_dir(&root)
        .env("PYTHONIOENCODING", "utf-8")
        .env("PYTHONUNBUFFERED", "1")
        .stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    // Вывод пишем в файлы, а не в пайп: иначе при нечитаемом/заполненном пайпе
    // поток печати (uvicorn, OpenVINO) навсегда блокируется вместе с event loop.
    let logs = state.data_dir.join("logs");
    std::fs::create_dir_all(&logs).ok();
    let stdout_file = std::fs::File::create(logs.join("engine_stdout.log"))
        .with_context(|| "не удалось создать engine_stdout.log".to_string())?;
    let stderr_file = std::fs::File::create(logs.join("engine_stderr.log"))
        .with_context(|| "не удалось создать engine_stderr.log".to_string())?;
    cmd.stdout(Stdio::from(stdout_file));
    cmd.stderr(Stdio::from(stderr_file));

    let mut child = cmd
        .spawn()
        .with_context(|| format!("не удалось запустить {} -m engine", python.display()))?;

    let deadline = Instant::now() + Duration::from_secs(120);
    let mut port: Option<u16> = None;
    let stdout_path = logs.join("engine_stdout.log");
    while Instant::now() < deadline {
        if let Ok(text) = std::fs::read_to_string(&stdout_path) {
            if let Some(rest) = text.split("HIVESIDECAR READY port=").nth(1) {
                if let Some(value) = rest.split_whitespace().next() {
                    if let Ok(parsed) = value.parse::<u16>() {
                        port = Some(parsed);
                        break;
                    }
                }
            }
        }
        if let Some(status) = child.try_wait().ok().flatten() {
            bail!("Сайдкар завершился при запуске (код {status}). Смотрите logs\\engine_stderr.log");
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    let Some(port) = port else {
        let _ = child.kill();
        bail!("Сайдкар не сообщил порт. Проверьте logs\\engine_stdout.log");
    };

    Ok(Sidecar { port, child: Mutex::new(Some(child)) })
}

async fn health_ok(port: u16, timeout: Duration) -> bool {
    let url = format!("http://127.0.0.1:{port}/health");
    match reqwest::Client::new().get(url).timeout(timeout).send().await {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    }
}

/// Запускает сайдкар, если он ещё не запущен. Возвращает порт.
pub async fn ensure_ready(app: &AppHandle) -> Result<u16> {
    let state = app.state::<AppState>();
    {
        let guard = state.sidecar.lock().await;
        if let Some(existing) = guard.as_ref() {
            if health_ok(existing.port, Duration::from_secs(2)).await {
                return Ok(existing.port);
            }
        }
    }

    let handle = app.clone();
    let sidecar = tauri::async_runtime::spawn_blocking(move || spawn_blocking(&handle))
        .await
        .map_err(|e| anyhow!("поток запуска сайдкара упал: {e}"))??;
    let port = sidecar.port;
    let sidecar = Arc::new(sidecar);
    {
        let mut guard = state.sidecar.lock().await;
        *guard = Some(sidecar);
    }

    for _ in 0..50 {
        if health_ok(port, Duration::from_secs(2)).await {
            state.engine_error.lock().unwrap().take();
            return Ok(port);
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    bail!("Сайдкар запущен на порту {port}, но /health не отвечает")
}

/// Порт работающего сайдкара (без запуска).
pub async fn port_if_ready(state: &AppState) -> Option<u16> {
    let guard = state.sidecar.lock().await;
    guard.as_ref().map(|s| s.port)
}

pub fn shutdown(app: &AppHandle) {
    let state = app.state::<AppState>();
    let taken = match state.sidecar.try_lock() {
        Ok(mut guard) => guard.take(),
        Err(_) => None,
    };
    if let Some(sidecar) = taken {
        sidecar.kill();
    }
}