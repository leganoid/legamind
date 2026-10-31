//! Локальный RAG по рабочей папке: чанки файлов в FTS5, поиск по BM25.
//!
//! Без внешних моделей: индексируются текстовые файлы проекта, а перед ответом
//! в промпт подмешиваются наиболее релевантные фрагменты с путями.

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::Serialize;
use tauri::State;

use crate::AppState;

const MAX_FILE_BYTES: u64 = 256 * 1024;
const MAX_FILES: usize = 400;
const MAX_CHUNKS: usize = 2000;
const CHUNK_CHARS: usize = 900;
const CHUNK_OVERLAP: usize = 150;
const BLOCK_CHARS: usize = 4500;
const HIT_CHARS: usize = 1200;

const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    ".legamind",
    "dist",
    "build",
    ".venv",
    "venv",
    "__pycache__",
    ".idea",
    ".vscode",
];

const TEXT_EXTS: &[&str] = &[
    "md", "txt", "rs", "ts", "tsx", "js", "jsx", "json", "toml", "py", "css", "html", "yml",
    "yaml", "sql", "ps1", "c", "h", "cpp", "hpp", "cs", "go", "java", "sh", "bat", "ini", "cfg",
    "conf",
];

#[derive(Debug, Clone, Serialize)]
pub struct IndexStats {
    pub files: usize,
    pub chunks: usize,
}

/// Режет текст на перекрывающиеся чанки, по возможности по границе строки.
pub fn chunk_text(text: &str, max_chars: usize, overlap: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return Vec::new();
    }
    if chars.len() <= max_chars {
        return vec![text.to_string()];
    }
    let mut chunks = Vec::new();
    let mut start = 0usize;
    while start < chars.len() {
        let mut end = (start + max_chars).min(chars.len());
        if end < chars.len() {
            let window_start = end.saturating_sub(200).max(start);
            if let Some(pos) = chars[window_start..end].iter().rposition(|ch| *ch == '\n') {
                let candidate = window_start + pos + 1;
                if candidate > start {
                    end = candidate;
                }
            }
        }
        if end <= start {
            end = (start + max_chars).min(chars.len());
        }
        chunks.push(chars[start..end].iter().collect::<String>());
        if end >= chars.len() {
            break;
        }
        start = end.saturating_sub(overlap);
        if start == 0 {
            start = end;
        }
    }
    chunks
}

fn is_text_file(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .map(|ext| TEXT_EXTS.contains(&ext.to_lowercase().as_str()))
        .unwrap_or(false)
}

fn walk(root: &Path, current: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > 8 || out.len() >= MAX_FILES {
        return;
    }
    let Ok(entries) = std::fs::read_dir(current) else {
        return;
    };
    for entry in entries.flatten() {
        if out.len() >= MAX_FILES {
            return;
        }
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            let name = entry.file_name().to_string_lossy().to_string();
            if SKIP_DIRS.contains(&name.as_str()) {
                continue;
            }
            walk(root, &path, out, depth + 1);
        } else if file_type.is_file() {
            if !is_text_file(&path) {
                continue;
            }
            let size = entry.metadata().map(|meta| meta.len()).unwrap_or(0);
            if size == 0 || size > MAX_FILE_BYTES {
                continue;
            }
            out.push(path);
        }
    }
}

/// Полностью переиндексирует рабочую папку.
pub fn index_workdir(db: &crate::db::Db, root: &Path) -> Result<IndexStats> {
    let root_key = root.to_string_lossy().to_string();
    db.rag_clear(&root_key)?;
    let mut files = Vec::new();
    walk(root, root, &mut files, 0);
    files.sort();

    let mut total_chunks = 0usize;
    for file in files.iter() {
        if total_chunks >= MAX_CHUNKS {
            break;
        }
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        let relative = file
            .strip_prefix(root)
            .map(|path| path.to_string_lossy().to_string())
            .unwrap_or_else(|_| file.to_string_lossy().to_string());
        let chunks = chunk_text(&text, CHUNK_CHARS, CHUNK_OVERLAP);
        for (index, chunk) in chunks.iter().enumerate() {
            if total_chunks >= MAX_CHUNKS {
                break;
            }
            db.rag_add_chunk(&root_key, &relative, index as i64, chunk)?;
            total_chunks += 1;
        }
    }
    db.setting_set(
        &index_key(root),
        &chrono::Utc::now().to_rfc3339(),
    )?;
    Ok(IndexStats {
        files: files.len(),
        chunks: total_chunks,
    })
}
/// Обновляет в индексе один файл (после записи/правки/удаления).
pub fn reindex_file(db: &crate::db::Db, root: &Path, file: &Path) -> Result<()> {
    let root_key = root.to_string_lossy().to_string();
    let relative = file
        .strip_prefix(root)
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_else(|_| file.to_string_lossy().to_string());
    db.rag_delete_path(&root_key, &relative)?;
    if !file.is_file() {
        return Ok(());
    }
    let Ok(text) = std::fs::read_to_string(file) else {
        return Ok(());
    };
    for (index, chunk) in chunk_text(&text, CHUNK_CHARS, CHUNK_OVERLAP)
        .iter()
        .enumerate()
    {
        db.rag_add_chunk(&root_key, &relative, index as i64, chunk)?;
    }
    Ok(())
}

/// Обновляет файл в индексе, только если индекс уже существует.
pub fn touch_file(db: &crate::db::Db, root: &Path, file: &Path) {
    let root_key = root.to_string_lossy().to_string();
    let has_index = db
        .rag_stats(&root_key)
        .map(|stats| stats.chunks > 0)
        .unwrap_or(false);
    if has_index {
        let _ = reindex_file(db, root, file);
    }
}

/// Ключ настройки с временем последней индексации конкретной папки.
pub fn index_key(root: &Path) -> String {
    format!("rag_indexed_at::{}", root.to_string_lossy())
}

/// Переиндексирует папку, если файлы менялись после последней индексации.
pub fn refresh_if_stale(db: &crate::db::Db, root: &Path) -> Result<bool> {
    let key = index_key(root);
    let stamp = db.setting_get(&key)?;
    let Some(stamp) = stamp else {
        // Индекс мог быть построен старой версией без отметки времени — перестроим один раз.
        let root_key = root.to_string_lossy().to_string();
        let has_index = db
            .rag_stats(&root_key)
            .map(|stats| stats.chunks > 0)
            .unwrap_or(false);
        if has_index {
            index_workdir(db, root)?;
            return Ok(true);
        }
        return Ok(false);
    };
    let Ok(indexed_at) = chrono::DateTime::parse_from_rfc3339(&stamp) else {
        return Ok(false);
    };
    let indexed_at = indexed_at.with_timezone(&chrono::Utc);

    let mut files = Vec::new();
    walk(root, root, &mut files, 0);
    let mut newest: Option<chrono::DateTime<chrono::Utc>> = None;
    for file in files {
        let Ok(metadata) = std::fs::metadata(&file) else {
            continue;
        };
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        let modified: chrono::DateTime<chrono::Utc> = modified.into();
        if newest.map(|current| modified > current).unwrap_or(true) {
            newest = Some(modified);
        }
    }
    let Some(newest) = newest else {
        return Ok(false);
    };
    if newest <= indexed_at {
        return Ok(false);
    }
    index_workdir(db, root)?;
    Ok(true)
}

/// Блок релевантных фрагментов для system-промпта.
pub fn context_block(db: &crate::db::Db, root: &Path, query: &str, limit: usize) -> Option<String> {
    let root_key = root.to_string_lossy().to_string();
    let hits = db.rag_search(&root_key, query, limit).ok()?;
    if hits.is_empty() {
        return None;
    }
    let mut out = String::from(
        "Релевантные фрагменты проекта (RAG). Используй их и ссылайся на пути файлов:\n",
    );
    for hit in hits {
        let body: String = hit.content.chars().take(HIT_CHARS).collect();
        out.push_str(&format!("\n[{}]\n{}\n", hit.path, body));
        if out.chars().count() > BLOCK_CHARS {
            break;
        }
    }
    Some(out)
}

#[tauri::command]
pub async fn rag_reindex(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<IndexStats, String> {
    let session = state
        .db
        .session_get(&session_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Сессия не найдена".to_string())?;
    let workdir = session
        .workdir
        .ok_or_else(|| "У сессии не задана рабочая папка".to_string())?;
    let stats = index_workdir(&state.db, Path::new(&workdir)).map_err(|e| e.to_string())?;
    let _ = crate::logging::log_line(
        "app.log",
        &format!(
            "rag: indexed {} chunks from {} files ({workdir})",
            stats.chunks, stats.files
        ),
    );
    Ok(stats)
}

#[tauri::command]
pub async fn rag_status(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<crate::db::RagStats, String> {
    let workdir = state
        .db
        .session_get(&session_id)
        .map_err(|e| e.to_string())?
        .and_then(|session| session.workdir);
    match workdir {
        Some(root) => state
            .db
            .rag_stats(&root)
            .map_err(|e| e.to_string()),
        None => Ok(crate::db::RagStats { chunks: 0, files: 0 }),
    }
}

#[tauri::command]
pub async fn rag_get_enabled(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(state
        .db
        .setting_get("rag_enabled")
        .map_err(|e| e.to_string())?
        .map(|value| value != "0")
        .unwrap_or(true))
}

#[tauri::command]
pub async fn rag_set_enabled(
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<bool, String> {
    state
        .db
        .setting_set("rag_enabled", if enabled { "1" } else { "0" })
        .map_err(|e| e.to_string())?;
    Ok(enabled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_respect_size_and_overlap() {
        let text = "строка\n".repeat(500);
        let chunks = chunk_text(&text, 900, 150);
        assert!(chunks.len() > 1);
        assert!(chunks.iter().all(|chunk| chunk.chars().count() <= 900));
        assert!(chunks.iter().all(|chunk| !chunk.trim().is_empty()));
    }

    #[test]
    fn reindex_file_updates_index() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-data")
            .join("rag-inc");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.md");
        std::fs::write(&file, "версия один").unwrap();

        let db = crate::db::Db::open_memory().unwrap();
        let root = dir.to_string_lossy().to_string();
        index_workdir(&db, &dir).expect("index");
        assert!(db.rag_search(&root, "уникальное", 5).unwrap().is_empty());

        std::fs::write(&file, "версия два уникальное").unwrap();
        reindex_file(&db, &dir, &file).expect("reindex");
        let hits = db.rag_search(&root, "уникальное", 5).unwrap();
        assert!(!hits.is_empty());
        assert!(hits[0].content.contains("уникальное"));

        std::fs::remove_file(&file).unwrap();
        reindex_file(&db, &dir, &file).expect("remove");
        assert!(db.rag_search(&root, "уникальное", 5).unwrap().is_empty());
        assert_eq!(db.rag_stats(&root).unwrap().files, 0);
    }

    #[test]
    fn refresh_reindexes_stale_project() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-data")
            .join("rag-stale");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.md");
        std::fs::write(&file, "старое содержимое").unwrap();

        let db = crate::db::Db::open_memory().unwrap();
        index_workdir(&db, &dir).expect("index");
        let root = dir.to_string_lossy().to_string();
        assert!(db.rag_search(&root, "уникальное", 5).unwrap().is_empty());

        db.setting_set(&index_key(&dir), "2000-01-01T00:00:00+00:00")
            .unwrap();
        std::fs::write(&file, "свежее уникальное содержимое").unwrap();

        assert!(refresh_if_stale(&db, &dir).expect("refresh"));
        assert!(!db.rag_search(&root, "уникальное", 5).unwrap().is_empty());
        assert!(!refresh_if_stale(&db, &dir).expect("fresh"));
    }

    #[test]
    fn index_is_isolated_per_project() {
        let base = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-data")
            .join("rag-isolation");
        let _ = std::fs::remove_dir_all(&base);
        let one = base.join("one");
        let two = base.join("two");
        std::fs::create_dir_all(&one).unwrap();
        std::fs::create_dir_all(&two).unwrap();
        std::fs::write(one.join("a.md"), "проект альфа уникальное").unwrap();
        std::fs::write(two.join("b.md"), "проект бета уникальное").unwrap();

        let db = crate::db::Db::open_memory().unwrap();
        index_workdir(&db, &one).expect("index one");
        index_workdir(&db, &two).expect("index two");

        let root_one = one.to_string_lossy().to_string();
        let root_two = two.to_string_lossy().to_string();
        let hits_one = db.rag_search(&root_one, "уникальное", 5).unwrap();
        let hits_two = db.rag_search(&root_two, "уникальное", 5).unwrap();
        assert!(hits_one.iter().all(|hit| hit.path.contains("a.md")));
        assert!(hits_two.iter().all(|hit| hit.path.contains("b.md")));
        assert_eq!(db.rag_stats(&root_one).unwrap().files, 1);
        assert_eq!(db.rag_stats(&root_two).unwrap().files, 1);
    }

    #[test]
    fn indexes_and_finds_files() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-data")
            .join("rag");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("src").join("main.rs"),
            "fn main() { println!(\"legamind crc32 zstd dump\"); }\n",
        )
        .unwrap();
        std::fs::write(dir.join("notes.md"), "проект про дампы прошивки и CRC32\n").unwrap();
        std::fs::write(dir.join("skip.bin"), [0u8, 1, 2]).unwrap();

        let db = crate::db::Db::open_memory().unwrap();
        let stats = index_workdir(&db, &dir).expect("index");
        assert_eq!(stats.files, 2);
        assert!(stats.chunks >= 2);

        let root = dir.to_string_lossy().to_string();
        let hits = db.rag_search(&root, "CRC32", 5).expect("search");
        assert!(!hits.is_empty());
        assert!(hits.iter().any(|hit| hit.path.contains("notes.md")));
        let block = context_block(&db, &dir, "zstd dump", 5).expect("block");
        assert!(block.contains("main.rs"), "{block}");
    }
}
