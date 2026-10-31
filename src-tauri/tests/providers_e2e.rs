//! Интеграционные тесты M2: ключи в Credential Manager и список моделей у провайдера.
//!
//! Запуск: cargo test --test providers_e2e -- --ignored --nocapture

use std::collections::{HashMap, HashSet};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use legamind_lib::db::Db;
use legamind_lib::providers;
use legamind_lib::secrets;
use legamind_lib::sidecar::Sidecar;
use legamind_lib::AppState;

const PROVIDER: &str = "mocktest";

fn make_state() -> AppState {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test-data");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let db_path = dir.join(format!("providers-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&db_path);
    let db = Db::open(&db_path).expect("БД");
    AppState {
        db,
        db_path,
        data_dir: dir,
        client: reqwest::Client::new(),
        sidecar: tokio::sync::Mutex::new(Some(Arc::new(Sidecar::for_test(1)))),
        cancel: Mutex::new(HashMap::new()),
        engine_error: Mutex::new(None),
        approvals: Mutex::new(HashMap::new()),
        allowed_approvals: Mutex::new(HashSet::new()),
        watcher: Mutex::new(None),
        watch_root: Mutex::new(None),
    }
}

fn cmdkey_has(needle: &str) -> bool {
    let output = Command::new("cmdkey").arg("/list").output().expect("cmdkey");
    String::from_utf8_lossy(&output.stdout).contains(needle)
}

#[test]
fn key_lives_in_credential_manager_and_is_deleted() {
    let state = make_state();
    // Windows хранит запись как LegacyGeneric:target=<account>.<service>
    let target = format!("provider:{PROVIDER}.legamind");

    secrets::set_key(&state, PROVIDER, "sk-test-1234567890").expect("сохранение ключа");
    assert!(cmdkey_has(&target), "cmdkey должен показывать {target}");
    assert_eq!(
        secrets::get_key(&state, PROVIDER).unwrap().as_deref(),
        Some("sk-test-1234567890")
    );

    // В базе не должно быть открытого ключа: таблица провайдеров пуста.
    let providers = state.db.providers_list().unwrap();
    assert!(providers.is_empty());

    secrets::delete_key(&state, PROVIDER).expect("удаление ключа");
    assert!(!cmdkey_has(&target), "запись {target} должна исчезнуть");
    assert!(secrets::get_key(&state, PROVIDER).unwrap().is_none());
}

fn start_mock() -> std::process::Child {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf();
    let python = std::env::var("LEGAMIND_PYTHON").unwrap_or_else(|_| {
        format!(r"{}\venvs\ai\Scripts\python.exe", std::env::var("LOCALAPPDATA").unwrap_or_default())
    });
    let child = Command::new(python)
        .args(["scripts/mock_openai.py", "--port", "8111"])
        .current_dir(&root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("мок-сервер");
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if std::net::TcpStream::connect(("127.0.0.1", 8111)).is_ok() {
            return child;
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    panic!("мок не поднялся");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "запускает локальный мок-сервер; запускать вручную"]
async fn provider_models_fetched_from_mock() {
    let mut mock = start_mock();
    let state = make_state();
    let result = providers::fetch_models(
        &state.client,
        "openai",
        "http://127.0.0.1:8111/v1",
        "sk-anything",
        10,
    )
    .await
    .expect("fetch /models");
    println!("модели мока: {:?}", result.iter().map(|(id, _, _, _)| id.clone()).collect::<Vec<_>>());
    assert!(result.iter().any(|(id, _, _, _)| id == "mock-fast"));

    let _ = mock.kill();
    let _ = mock.wait();
}
