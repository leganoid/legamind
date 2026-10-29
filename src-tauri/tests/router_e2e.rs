//! РРЅС‚РµРіСЂР°С†РёРѕРЅРЅС‹Р№ С‚РµСЃС‚: РЅР°СЃС‚РѕСЏС‰РёР№ Python-СЃР°Р№РґРєР°СЂ + ModelRouter РёР· РїСЂРёР»РѕР¶РµРЅРёСЏ.
//!
//! Р—Р°РїСѓСЃРєР°РµС‚СЃСЏ РІСЂСѓС‡РЅСѓСЋ (РЅСѓР¶РЅС‹ РјРѕРґРµР»Рё OpenVINO Рё venv):
//!   cargo test --test router_e2e -- --ignored --nocapture

use std::collections::HashMap;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use legamind_lib::models::{ChatMessage, ChatParams, Router};
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
        .expect("РєРѕСЂРµРЅСЊ СЂРµРїРѕР·РёС‚РѕСЂРёСЏ")
        .to_path_buf();
    // Р’С‹РІРѕРґ вЂ” РІ С„Р°Р№Р»С‹: С‚РµСЃС‚ РЅРµ РґРѕР»Р¶РµРЅ Р·Р°РєСЂС‹РІР°С‚СЊ РїР°Р№Рї, РёРЅР°С‡Рµ print РІ РґРІРёР¶РєРµ
    // РїРѕР»СѓС‡РёС‚ BrokenPipe Рё РІРµСЂРЅС‘С‚ 500.
    let logs = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("test-data");
    std::fs::create_dir_all(&logs).expect("test-data");
    let stdout_file = std::fs::File::create(logs.join(format!("engine-{}.out", std::process::id())))
        .expect("stdout С„Р°Р№Р»");
    let stderr_file = std::fs::File::create(logs.join(format!("engine-{}.err", std::process::id())))
        .expect("stderr С„Р°Р№Р»");
    let mut child = Command::new(python_path())
        .args(["-m", "engine", "--port", "0", "--host", "127.0.0.1"])
        .current_dir(&root)
        .env("PYTHONIOENCODING", "utf-8")
        .env("PYTHONUNBUFFERED", "1")
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(stderr_file))
        .spawn()
        .expect("РЅРµ СѓРґР°Р»РѕСЃСЊ Р·Р°РїСѓСЃС‚РёС‚СЊ СЃР°Р№РґРєР°СЂ");

    let stdout_path = logs.join(format!("engine-{}.out", std::process::id()));
    let deadline = Instant::now() + Duration::from_secs(120);
    let mut port = None;
    while Instant::now() < deadline {
        if let Ok(text) = std::fs::read_to_string(&stdout_path) {
            if let Some(rest) = text.split("HIVESIDECAR READY port=").nth(1) {
                port = rest.split_whitespace().next().and_then(|v| v.parse::<u16>().ok());
                if port.is_some() {
                    break;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let port = port.expect("СЃР°Р№РґРєР°СЂ РЅРµ СЃРѕРѕР±С‰РёР» РїРѕСЂС‚");
    Engine { child, port }
}

fn make_state(port: u16) -> (AppState, std::path::PathBuf) {
    // РџРёС€РµРј РІРЅСѓС‚СЂСЊ target/: С‚РµСЃС‚РѕРІС‹Р№ Р±РёРЅР°СЂРЅРёРє Р·Р°РїСѓСЃРєР°РµС‚СЃСЏ РёР· workspace.
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test-data");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let db_path = dir.join(format!("test-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&db_path);
    let db = Db::open(&db_path).expect("Р‘Р”");
    let state = AppState {
        db,
        db_path: db_path.clone(),
        data_dir: dir,
        client: reqwest::Client::new(),
        sidecar: tokio::sync::Mutex::new(Some(Arc::new(Sidecar::for_test(port)))),
        cancel: Mutex::new(HashMap::new()),
        engine_error: Mutex::new(None),
    };
    (state, db_path)
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "С‚СЂРµР±СѓРµС‚ OpenVINO-РјРѕРґРµР»Рё Рё venv; Р·Р°РїСѓСЃРєР°С‚СЊ РІСЂСѓС‡РЅСѓСЋ"]
async fn raw_reqwest_stream_probe() {
    use futures_util::StreamExt;
    let engine = start_engine();
    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{}/chat", engine.port);
    let body = serde_json::json!({
        "model": "qwen-1.5b",
        "messages": [{"role": "user", "content": "РЎРєР°Р¶Рё РѕРґРЅРѕ СЃР»РѕРІРѕ."}],
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
        .expect("Р·Р°РїСЂРѕСЃ");
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
    assert!(chunks > 0, "РїРѕС‚РѕРє РїСѓСЃС‚");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "С‚СЂРµР±СѓРµС‚ OpenVINO-РјРѕРґРµР»Рё Рё venv; Р·Р°РїСѓСЃРєР°С‚СЊ РІСЂСѓС‡РЅСѓСЋ"]
async fn local_model_streams_through_router() {
    let engine = start_engine();
    let (state, _db_path) = make_state(engine.port);

    let messages = vec![
        ChatMessage::new("system", "РћС‚РІРµС‡Р°Р№ РїРѕ-СЂСѓСЃСЃРєРё, РєСЂР°С‚РєРѕ."),
        ChatMessage::new("user", "РќР°Р·РѕРІРё РѕРґРЅСѓ РїСЂРёС‡РёРЅСѓ РёСЃРїРѕР»СЊР·РѕРІР°С‚СЊ OpenVINO. РћРґРЅРѕ РїСЂРµРґР»РѕР¶РµРЅРёРµ."),
    ];
    let params = ChatParams { max_tokens: 64, temperature: 0.2, think: false };
    let cancel = AtomicBool::new(false);

    let mut text = String::new();
    let mut delta_count = 0;
    let router = Router { state: &state };
    let outcome = router
        .stream("local/qwen-1.5b", &messages, &params, &cancel, &mut |delta| {
            text.push_str(delta);
            delta_count += 1;
        })
        .await
        .expect("СЃС‚СЂРёРјРёРЅРі С‡РµСЂРµР· СЂРѕСѓС‚РµСЂ");

    println!("deltas={delta_count} text={text}");
    assert!(delta_count > 1, "РѕР¶РёРґР°Р»РёСЃСЊ РЅРµСЃРєРѕР»СЊРєРѕ РґРµР»СЊС‚, РїРѕР»СѓС‡РµРЅРѕ {delta_count}");
    assert!(!text.trim().is_empty(), "РїСѓСЃС‚РѕР№ РѕС‚РІРµС‚ РјРѕРґРµР»Рё");
    assert!(outcome.tokens > 0);
    assert!(!outcome.stopped);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "С‚СЂРµР±СѓРµС‚ OpenVINO-РјРѕРґРµР»Рё Рё venv; Р·Р°РїСѓСЃРєР°С‚СЊ РІСЂСѓС‡РЅСѓСЋ"]
async fn stop_flag_aborts_generation() {
    let engine = start_engine();
    let (state, _db_path) = make_state(engine.port);

    let messages = vec![ChatMessage::new(
        "user",
        "РќР°РїРёС€Рё РґР»РёРЅРЅС‹Р№ СЃРїРёСЃРѕРє РёР· РґРІР°РґС†Р°С‚Рё РїСѓРЅРєС‚РѕРІ РїСЂРѕ OpenVINO.",
    )];
    let params = ChatParams { max_tokens: 512, temperature: 0.7, think: false };
    let cancel = AtomicBool::new(false);

    let router = Router { state: &state };
    let mut deltas = 0;
    let outcome = router
        .stream("local/qwen-1.5b", &messages, &params, &cancel, &mut |_| {
            deltas += 1;
            if deltas == 2 {
                // РџРѕР»СЊР·РѕРІР°С‚РµР»СЊ РЅР°Р¶Р°Р» В«РЎС‚РѕРїВ» РїРѕСЃР»Рµ РїРµСЂРІС‹С… С‚РѕРєРµРЅРѕРІ.
                cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        })
        .await
        .expect("СЃС‚СЂРёРјРёРЅРі");

    assert!(outcome.stopped, "РіРµРЅРµСЂР°С†РёСЏ РґРѕР»Р¶РЅР° Р±С‹Р»Р° РѕСЃС‚Р°РЅРѕРІРёС‚СЊСЃСЏ");
    assert!(deltas >= 2);
}
