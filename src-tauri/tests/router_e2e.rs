//! Интеграционный тест: настоящий Python-сайдкар + ModelRouter из приложения.
//!
//! Запускается вручную (нужны модели OpenVINO и venv):
//!   cargo test --test router_e2e -- --ignored --nocapture

use std::collections::HashMap;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use legamind_lib::models::{ChatMessage, ChatParams, ChatStatus, Router};
use legamind_lib::sidecar::Sidecar;
use legamind_lib::{db::Db, AppState};

struct Engine {
    child: Child,
    port: u16,
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn python_path() -> String {
    if let Ok(path) = std::env::var("LEGAMIND_PYTHON") {
        return path;
    }
    let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
    format!(r"{local}\venvs\ai\Scripts\python.exe")
}

fn start_engine() -> Engine {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("корень репозитория")
        .to_path_buf();
    // Вывод — в файлы: тест не должен закрывать пайп, иначе print в движке
    // получит BrokenPipe и вернёт 500.
    let logs = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test-data");
    std::fs::create_dir_all(&logs).expect("test-data");
    let stdout_file = std::fs::File::create(logs.join(format!("engine-{}.out", std::process::id())))
        .expect("stdout файл");
    let stderr_file = std::fs::File::create(logs.join(format!("engine-{}.err", std::process::id())))
        .expect("stderr файл");
    let mut child = Command::new(python_path())
        .args(["-m", "engine", "--port", "0", "--host", "127.0.0.1"])
        .current_dir(&root)
        .env("PYTHONIOENCODING", "utf-8")
        .env("PYTHONUNBUFFERED", "1")
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(stderr_file))
        .spawn()
        .expect("не удалось запустить сайдкар");

    let stdout_path = logs.join(format!("engine-{}.out", std::process::id()));
    let deadline = Instant::now() + Duration::from_secs(120);
    let mut port = None;
    while Instant::now() < deadline {
        if let Ok(text) = std::fs::read_to_string(&stdout_path) {
            if let Some(rest) = text.split("LEGAMINDSIDECAR READY port=").nth(1) {
                port = rest.split_whitespace().next().and_then(|v| v.parse::<u16>().ok());
                if port.is_some() {
                    break;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let port = port.expect("сайдкар не сообщил порт");
    Engine { child, port }
}

fn make_state(port: u16) -> (AppState, std::path::PathBuf) {
    // Пишем внутрь target/: тестовый бинарник запускается из workspace.
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test-data");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let db_path = dir.join(format!("test-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&db_path);
    let db = Db::open(&db_path).expect("БД");
    let state = AppState {
        db,
        db_path: db_path.clone(),
        data_dir: dir,
        client: reqwest::Client::new(),
        sidecar: tokio::sync::Mutex::new(Some(Arc::new(Sidecar::for_test(port)))),
        cancel: Mutex::new(HashMap::new()),
        engine_error: Mutex::new(None),
        approvals: Mutex::new(HashMap::new()),
        watcher: Mutex::new(None),
        watch_root: Mutex::new(None),
        chat_attachments: Mutex::new(HashMap::new()),
    };
    (state, db_path)
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "требует OpenVINO-модели и venv; запускать вручную"]
async fn raw_reqwest_stream_probe() {
    use futures_util::StreamExt;
    let engine = start_engine();
    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{}/chat", engine.port);
    let body = serde_json::json!({
        "model": "qwen-1.5b",
        "messages": [{"role": "user", "content": "Скажи одно слово."}],
        "max_tokens": 20,
        "temperature": 0.2,
        "think": false
    });
    let resp = client
        .post(&url)
        .json(&body)
        .timeout(Duration::from_secs(120))
        .send()
        .await
        .expect("запрос");
    println!("status={} headers={:?}", resp.status(), resp.headers());
    let mut stream = resp.bytes_stream();
    let mut total = 0;
    let mut chunks = 0;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.expect("chunk");
        chunks += 1;
        total += chunk.len();
        println!("chunk {} bytes: {:?}", chunk.len(), String::from_utf8_lossy(&chunk));
    }
    println!("chunks={chunks} total={total}");
    assert!(chunks > 0, "поток пуст");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "требует OpenVINO-модели и venv; запускать вручную"]
async fn local_model_streams_through_router() {
    let engine = start_engine();
    let (state, _db_path) = make_state(engine.port);

    let messages = vec![
        ChatMessage::new("system", "Отвечай по-русски, кратко."),
        ChatMessage::new(
            "user",
            "Назови одну причину использовать OpenVINO. Одно предложение.",
        ),
    ];
    let params = ChatParams { max_tokens: 64, temperature: 0.2, think: false, reasoning: None };
    let cancel = AtomicBool::new(false);

    let mut text = String::new();
    let mut delta_count = 0;
    let mut phases: Vec<String> = Vec::new();
    let router = Router { state: &state };
    let outcome = router
        .stream(
            "local/qwen-1.5b",
            &messages,
            &params,
            &cancel,
            &mut |delta| {
                text.push_str(delta);
                delta_count += 1;
            },
            &mut |status: &ChatStatus| phases.push(status.phase.clone()),
        )
        .await
        .expect("стриминг через роутер");

    println!("phases={phases:?} deltas={delta_count} text={text}");
    assert!(delta_count > 1, "ожидались несколько дельт, получено {delta_count}");
    assert!(!text.trim().is_empty(), "пустой ответ модели");
    assert!(outcome.tokens > 0);
    assert!(!outcome.stopped);
    assert!(phases.iter().any(|p| p == "loading"), "не пришёл статус загрузки: {phases:?}");
    assert!(phases.iter().any(|p| p == "ready"), "не пришёл статус готовности: {phases:?}");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "требует OpenVINO-модели и venv; запускать вручную"]
async fn compact_replaces_history_with_summary() {
    let engine = start_engine();
    let (state, _db_path) = make_state(engine.port);
    let session = state
        .db
        .session_create("Тест сжатия", "local/qwen-1.5b")
        .expect("сессия");
    let dialog = [
        ("user", "Мы проектируем формат хранения дампов прошивки."),
        ("assistant", "Предлагаю заголовок: magic, version, размер, CRC32."),
        ("user", "Нужно ли сжатие?"),
        ("assistant", "Да, zstd быстрее и лучше жмёт, чем gzip."),
        ("user", "Решили: little-endian и zstd."),
        ("assistant", "Записал: little-endian, zstd, CRC32 в конце."),
    ];
    for (role, content) in dialog {
        state
            .db
            .message_add(&session.id, role, content, Some("local/qwen-1.5b"), None)
            .expect("сообщение");
    }

    let result = legamind_lib::chat::compact_session(&state, &session.id, "local/qwen-1.5b")
        .await
        .expect("сжатие истории");

    println!("compact: kind={} text={} removed={}", result.kind, result.text, result.removed_ids.len());
    let messages = state.db.messages_list(&session.id).expect("сообщения");
    assert_eq!(result.kind, "compact");
    assert_eq!(messages.len(), 1, "должна остаться одна сводка");
    assert!(messages[0].content.contains("Сводка"), "{}", messages[0].content);
    assert!(result.removed_ids.len() == dialog.len());
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "требует OpenVINO-модели и venv; запускать вручную"]
async fn stop_flag_aborts_generation() {
    let engine = start_engine();
    let (state, _db_path) = make_state(engine.port);

    let messages = vec![ChatMessage::new(
        "user",
        "Напиши длинный список из двадцати пунктов про OpenVINO.",
    )];
    let params = ChatParams { max_tokens: 512, temperature: 0.7, think: false, reasoning: None };
    let cancel = AtomicBool::new(false);

    let router = Router { state: &state };
    let mut deltas = 0;
    let outcome = router
        .stream(
            "local/qwen-1.5b",
            &messages,
            &params,
            &cancel,
            &mut |_| {
                deltas += 1;
                if deltas == 2 {
                    // Пользователь нажал «Стоп» после первых токенов.
                    cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            },
            &mut |_: &ChatStatus| {},
        )
        .await
        .expect("стриминг");

    assert!(outcome.stopped, "генерация должна была остановиться");
    assert!(deltas >= 2);
}
