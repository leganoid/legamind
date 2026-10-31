//! Тесты памяти: FTS5-поиск и блок «Известное о проекте» для промпта.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use legamind_lib::db::Db;
use legamind_lib::memory;
use legamind_lib::sidecar::Sidecar;
use legamind_lib::AppState;

fn make_state(name: &str) -> AppState {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test-data");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let db_path = dir.join(format!("{name}-{}.db", std::process::id()));
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
        watcher: Mutex::new(None),
        watch_root: Mutex::new(None),
    }
}

#[test]
fn fact_is_injected_into_prompt_block() {
    let state = make_state("memory-fact");
    state
        .db
        .memory_add("fact", "в binres используется JFFS2", None)
        .expect("факт");
    state
        .db
        .memory_add("preference", "отвечай кратко", None)
        .expect("предпочтение");

    let block = memory::build_context(&state, "какой формат используется в binres")
        .expect("контекст")
        .expect("блок памяти должен быть непустым");

    assert!(block.starts_with("Известное о проекте"), "заголовок блока: {block}");
    assert!(block.contains("JFFS2"), "факт должен попасть в блок: {block}");
}

#[test]
fn memory_block_respects_budget() {
    let state = make_state("memory-budget");
    for i in 0..40 {
        state
            .db
            .memory_add("fact", &format!("binres JFFS2 деталь номер {i} {}", "очень длинный текст ".repeat(12)), None)
            .expect("факт");
    }
    let block = memory::build_context(&state, "binres JFFS2 формат")
        .expect("контекст")
        .expect("блок");
    assert!(block.len() <= 3100, "бюджет блока памяти превышен: {}", block.len());
}

#[test]
fn unrelated_query_has_no_block() {
    let state = make_state("memory-empty");
    assert!(memory::build_context(&state, "совершенно посторонний вопрос").unwrap().is_none());
}
