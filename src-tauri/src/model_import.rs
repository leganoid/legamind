//! Мастер локальных моделей: проверка IR-папки и загрузка готовых OpenVINO IR
//! с Hugging Face.

use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use crate::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct ModelInspection {
    pub path: String,
    pub label: String,
    pub size_gb: f64,
    pub has_xml: bool,
    pub has_bin: bool,
    pub has_tokenizer: bool,
    pub context_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HfFile {
    pub path: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct HfPreview {
    pub repo: String,
    pub files: Vec<HfFile>,
    pub total_bytes: u64,
    pub ir_ready: bool,
    pub tokenizer_ready: bool,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DownloadProgress {
    pub repo: String,
    pub file: String,
    pub downloaded: u64,
    pub total: u64,
    pub file_index: usize,
    pub file_count: usize,
    pub done: bool,
}

fn inspect_dir(path: &Path) -> Result<ModelInspection> {
    crate::local_models::validate_model_dir(path)?;
    let mut has_xml = false;
    let mut has_bin = false;
    let mut has_tokenizer = false;
    for entry in std::fs::read_dir(path)?.flatten() {
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if name.ends_with(".xml") {
            has_xml = true;
            if name.contains("tokenizer") {
                has_tokenizer = true;
            }
        }
        if name.ends_with(".bin") {
            has_bin = true;
        }
        if name == "tokenizer.json" || name == "tokenizer_config.json" {
            has_tokenizer = true;
        }
    }
    let context_tokens = read_context(path);
    let label = path
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| "Локальная модель".to_string());
    Ok(ModelInspection {
        path: path.to_string_lossy().to_string(),
        label,
        size_gb: (dir_size(path) as f64 / 1_000_000_000.0 * 100.0).round() / 100.0,
        has_xml,
        has_bin,
        has_tokenizer,
        context_tokens,
    })
}

fn dir_size(path: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| {
            let path = entry.path();
            if path.is_dir() {
                dir_size(&path)
            } else {
                entry.metadata().map(|meta| meta.len()).unwrap_or(0)
            }
        })
        .sum()
}

fn read_context(path: &Path) -> Option<u32> {
    let text = std::fs::read_to_string(path.join("config.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    for key in ["max_position_embeddings", "max_sequence_length"] {
        if let Some(number) = value.get(key).and_then(|v| v.as_u64()) {
            return Some(number as u32);
        }
    }
    value
        .get("text_config")
        .and_then(|cfg| cfg.get("max_position_embeddings"))
        .and_then(|v| v.as_u64())
        .map(|value| value as u32)
}

#[tauri::command]
pub async fn inspect_local_model(path: String) -> Result<ModelInspection, String> {
    inspect_dir(Path::new(path.trim())).map_err(|e| e.to_string())
}

fn validate_repo(repo: &str) -> Result<()> {
    let repo = repo.trim();
    let (owner, name) = repo
        .split_once('/')
        .ok_or_else(|| anyhow::anyhow!("Укажите репозиторий в формате owner/model"))?;
    if owner.is_empty() || name.is_empty() {
        bail!("Укажите репозиторий в формате owner/model");
    }
    if matches!(owner, "." | "..") || matches!(name, "." | "..") {
        bail!("Недопустимое имя репозитория");
    }
    if owner.starts_with('.') || name.starts_with('.') {
        bail!("Недопустимое имя репозитория");
    }
    let ok = |value: &str| {
        value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    if !ok(owner) || !ok(name) {
        bail!("Недопустимые символы в имени репозитория");
    }
    Ok(())
}

#[derive(Deserialize)]
struct TreeEntry {
    #[serde(rename = "type")]
    kind: String,
    path: String,
    size: Option<u64>,
}

async fn hf_tree(state: &AppState, repo: &str) -> Result<Vec<HfFile>> {
    validate_repo(repo)?;
    let url = format!("https://huggingface.co/api/models/{repo}/tree/main");
    let resp = state
        .client
        .get(url)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .context("Hugging Face не ответил")?;
    if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        bail!("Репозиторий требует авторизации Hugging Face (gated/private)");
    }
    if !resp.status().is_success() {
        bail!("Hugging Face вернул {}", resp.status());
    }
    let entries: Vec<TreeEntry> = resp.json().await.context("Некорректный ответ Hugging Face")?;
    Ok(entries
        .into_iter()
        .filter(|entry| entry.kind == "file")
        .filter(|entry| entry.path != "README.md" && entry.path != ".gitattributes")
        .map(|entry| HfFile {
            path: entry.path,
            size: entry.size.unwrap_or(0),
        })
        .collect())
}

#[tauri::command]
pub async fn hf_model_preview(
    state: State<'_, AppState>,
    repo: String,
) -> Result<HfPreview, String> {
    let repo = repo.trim().to_string();
    let files = hf_tree(&state, &repo).await.map_err(|e| e.to_string())?;
    let has = |name: &str| files.iter().any(|file| file.path == name);
    let ir_ready = has("openvino_model.xml") && has("openvino_model.bin");
    let tokenizer_ready = has("tokenizer.json")
        || has("openvino_tokenizer.xml")
        || has("tokenizer_config.json");
    if !ir_ready {
        return Err(
            "В репозитории нет готового OpenVINO IR (openvino_model.xml/.bin). \
             Нужен репозиторий с уже сконвертированной моделью."
                .into(),
        );
    }
    let label = repo.split('/').next_back().unwrap_or(&repo).to_string();
    Ok(HfPreview {
        repo,
        total_bytes: files.iter().map(|file| file.size).sum(),
        files,
        ir_ready,
        tokenizer_ready,
        label,
    })
}

fn safe_dir_name(repo: &str) -> String {
    let mut name: String = repo
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if name.len() > 120 {
        name.truncate(120);
    }
    name
}

#[tauri::command]
pub async fn hf_model_download(
    app: AppHandle,
    state: State<'_, AppState>,
    repo: String,
) -> Result<String, String> {
    let repo = repo.trim().to_string();
    let files = hf_tree(&state, &repo).await.map_err(|e| e.to_string())?;
    if !files.iter().any(|file| file.path == "openvino_model.xml") {
        return Err("В репозитории нет openvino_model.xml".into());
    }
    let target = state.data_dir.join("models").join(safe_dir_name(&repo));
    std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
    let total: u64 = files.iter().map(|file| file.size).sum();
    let file_count = files.len();
    let mut downloaded_total = 0u64;

    for (index, file) in files.iter().enumerate() {
        let url = format!(
            "https://huggingface.co/{repo}/resolve/main/{}",
            file.path
        );
        let mut resp = state
            .client
            .get(&url)
            .timeout(std::time::Duration::from_secs(3600))
            .send()
            .await
            .map_err(|e| format!("{}: {e}", file.path))?;
        if !resp.status().is_success() {
            return Err(format!(
                "{}: Hugging Face вернул {}",
                file.path,
                resp.status()
            ));
        }
        let path = target.join(&file.path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut out = std::fs::File::create(&path).map_err(|e| e.to_string())?;
        let mut downloaded_file = 0u64;
        let mut last_emit = 0u64;
        while let Some(chunk) = resp.chunk().await.map_err(|e| e.to_string())? {
            use std::io::Write;
            out.write_all(&chunk).map_err(|e| e.to_string())?;
            downloaded_file += chunk.len() as u64;
            downloaded_total += chunk.len() as u64;
            if downloaded_file - last_emit >= 2 * 1024 * 1024 {
                last_emit = downloaded_file;
                let _ = app.emit(
                    "model:download",
                    DownloadProgress {
                        repo: repo.clone(),
                        file: file.path.clone(),
                        downloaded: downloaded_total,
                        total,
                        file_index: index,
                        file_count,
                        done: false,
                    },
                );
            }
        }
        let _ = app.emit(
            "model:download",
            DownloadProgress {
                repo: repo.clone(),
                file: file.path.clone(),
                downloaded: downloaded_total,
                total,
                file_index: index + 1,
                file_count,
                done: false,
            },
        );
    }

    let _ = app.emit(
        "model:download",
        DownloadProgress {
            repo: repo.clone(),
            file: String::new(),
            downloaded: total,
            total,
            file_index: file_count,
            file_count,
            done: true,
        },
    );
    let _ = crate::logging::log_line(
        "app.log",
        &format!("hf model downloaded: {repo} -> {}", target.display()),
    );
    Ok(target.to_string_lossy().to_string())
}

/// Проверка папки для мастера (публичная, чтобы использовать в тестах).
pub fn inspect_for_test(path: &Path) -> Result<ModelInspection> {
    inspect_dir(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inspects_ir_directory() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-data")
            .join("model-inspect");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("openvino_model.xml"), b"<xml/>").unwrap();
        std::fs::write(dir.join("openvino_model.bin"), b"bin").unwrap();
        std::fs::write(dir.join("tokenizer.json"), b"{}").unwrap();
        std::fs::write(
            dir.join("config.json"),
            br#"{"max_position_embeddings": 8192}"#,
        )
        .unwrap();
        let info = inspect_for_test(&dir).unwrap();
        assert!(info.has_xml && info.has_bin && info.has_tokenizer);
        assert_eq!(info.context_tokens, Some(8192));
        assert!(info.size_gb >= 0.0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn validates_repo_names() {
        assert!(validate_repo("OpenVINO/Qwen2.5-1.5B-Instruct-int4-ov").is_ok());
        assert!(validate_repo("broken").is_err());
        assert!(validate_repo("../evil").is_err());
    }

    #[test]
    fn safe_names_do_not_escape() {
        assert_eq!(safe_dir_name("a/b"), "a_b");
        assert!(!safe_dir_name("../x").contains('/'));
    }
}
