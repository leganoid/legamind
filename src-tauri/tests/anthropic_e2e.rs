//! Проверка разбора Anthropic SSE на локальном мок-сервере.
//!
//! Запуск: cargo test --test anthropic_e2e -- --nocapture

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use legamind_lib::db::Db;
use legamind_lib::models::{ChatMessage, ChatParams, Router};
use legamind_lib::secrets;
use legamind_lib::sidecar::Sidecar;
use legamind_lib::AppState;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const BODY: &str = concat!(
    "event: message_start\n",
    "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\"}}\n\n",
    "event: content_block_delta\n",
    "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Привет\"}}\n\n",
    "event: content_block_delta\n",
    "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\", мир\"}}\n\n",
    "event: message_delta\n",
    "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":7}}\n\n",
    "event: message_stop\n",
    "data: {\"type\":\"message_stop\"}\n\n"
);

async fn spawn_mock() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        if let Ok((mut socket, _)) = listener.accept().await {
            let mut buf = [0u8; 8192];
            let _ = socket.read(&mut buf).await;
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                BODY.len()
            );
            let _ = socket.write_all(head.as_bytes()).await;
            // Отдаём события кусками, как настоящий провайдер.
            for chunk in BODY.as_bytes().chunks(40) {
                let _ = socket.write_all(chunk).await;
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            let _ = socket.flush().await;
        }
    });
    port
}

fn make_state(provider_id: &str, base_url: &str) -> AppState {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test-data");
    std::fs::create_dir_all(&dir).expect("test-data");
    let db_path = dir.join(format!("anthropic-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&db_path);
    let db = Db::open(&db_path).expect("БД");
    db.provider_upsert(&legamind_lib::db::ProviderRow {
        id: provider_id.to_string(),
        kind: "anthropic".to_string(),
        label: "Anthropic mock".to_string(),
        base_url: base_url.to_string(),
        key_ref: format!("provider:{provider_id}"),
        enabled: true,
        timeout_s: 30,
        created_at: "2026-01-01T00:00:00Z".to_string(),
    })
    .expect("провайдер");

    AppState {
        db,
        db_path,
        data_dir: dir,
        client: reqwest::Client::new(),
        sidecar: tokio::sync::Mutex::new(Some(Arc::new(Sidecar::for_test(1)))),
        cancel: Mutex::new(HashMap::new()),
        engine_error: Mutex::new(None),
        approvals: Mutex::new(HashMap::new()),
        watcher: Mutex::new(None),
        watch_root: Mutex::new(None),
        chat_attachments: Mutex::new(HashMap::new()),
        chat_images: Mutex::new(HashMap::new()),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn anthropic_stream_is_parsed() {
    let port = spawn_mock().await;
    let provider_id = format!("anthropic-test-{}", std::process::id());
    let base = format!("http://127.0.0.1:{port}");
    let state = make_state(&provider_id, &base);
    secrets::set_key(&state, &provider_id, "sk-ant-test").expect("ключ");

    let router = Router { state: &state };
    let messages = vec![ChatMessage::new("user", "привет")];
    let params = ChatParams { max_tokens: 32, temperature: 0.3, think: false, reasoning: None };
    let cancel = AtomicBool::new(false);
    let mut text = String::new();
    let mut deltas = 0;
    let outcome = router
        .stream(
            &format!("{provider_id}/claude-test"),
            &messages,
            &params,
            &cancel,
            &mut |d| {
                text.push_str(d);
                deltas += 1;
            },
            &mut |_: &legamind_lib::models::ChatStatus| {},
        )
        .await
        .expect("anthropic stream");

    println!("deltas={deltas} text={text} tokens={}", outcome.tokens);
    assert_eq!(text, "Привет, мир");
    assert_eq!(deltas, 2);
    assert_eq!(outcome.tokens, 7);

    let _ = secrets::delete_key(&state, &provider_id);
}
