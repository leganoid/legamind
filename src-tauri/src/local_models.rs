//! Пользовательские локальные модели: добавление папки OpenVINO IR в конфиг движка.

use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::AppState;

const SETTING_KEY: &str = "local_models";

#[derive(Debug, Clone, Serialize)]
pub struct LocalModelRow {
    pub id: String,
    pub label: String,
    pub path: String,
    pub device: String,
    pub max_tokens: u32,
    pub thinks: bool,
    pub size_gb: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AddLocalModelInput {
    pub path: String,
    pub label: String,
    pub device: Option<String>,
    pub max_tokens: Option<u32>,
    pub thinks: Option<bool>,
}

fn read_map(state: &AppState) -> Result<serde_json::Map<String, serde_json::Value>> {
    let raw = state.db.setting_get(SETTING_KEY)?.unwrap_or_default();
    if raw.trim().is_empty() {
        return Ok(serde_json::Map::new());
    }
    let value: serde_json::Value = serde_json::from_str(&raw)?;
    Ok(value.as_object().cloned().unwrap_or_default())
}

fn write_map(state: &AppState, map: &serde_json::Map<String, serde_json::Value>) -> Result<()> {
    state
        .db
        .setting_set(SETTING_KEY, &serde_json::to_string(map)?)?;
    Ok(())
}

/// Находит папки OpenVINO IR в C:\Models и добавляет их в конфиг.
/// Уже добавленные по пути модели не дублируются.
pub fn discover_models(state: &AppState) -> Result<usize> {
    let configured = state
        .db
        .setting_get("local_models_dir")
        .ok()
        .flatten()
        .filter(|value| !value.trim().is_empty());
    let root = match configured {
        Some(value) => PathBuf::from(value),
        None => {
            let default = PathBuf::from(r"C:\Models");
            if default.is_dir() {
                default
            } else {
                return Ok(0);
            }
        }
    };
    if !root.is_dir() {
        return Ok(0);
    }
    let mut map = read_map(state)?;
    let mut known: Vec<String> = map
        .values()
        .filter_map(|value| value.get("path").and_then(|v| v.as_str()))
        .map(|value| value.replace('/', "\\").to_lowercase())
        .collect();
    let mut added = 0usize;
    let Ok(entries) = std::fs::read_dir(&root) else {
        return Ok(0);
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let has_vlm = path.join("openvino_language_model.xml").exists();
        let has_image = path.join("model_index.json").exists()
            && (path.join("unet").is_dir() || path.join("transformer").is_dir());
        if !path.is_dir()
            || (!path.join("openvino_model.xml").exists() && !has_vlm && !has_image)
        {
            continue;
        }
        let key = path.to_string_lossy().replace('/', "\\").to_lowercase();
        if known.contains(&key) {
            // У уже добавленной VLM мог не быть флага vlm — обновляем.
            if has_vlm || has_image {
                let existing = map.iter_mut().find(|(_, value)| {
                    value
                        .get("path")
                        .and_then(|v| v.as_str())
                        .map(|value| value.replace('/', "\\").to_lowercase() == key)
                        .unwrap_or(false)
                });
                if let Some((_, value)) = existing {
                    if has_vlm && value.get("vlm").and_then(|v| v.as_bool()) != Some(true) {
                        value["vlm"] = serde_json::json!(true);
                        added += 1;
                    }
                    if has_image && value.get("image").and_then(|v| v.as_bool()) != Some(true) {
                        value["image"] = serde_json::json!(true);
                        added += 1;
                    }
                }
            }
            continue;
        }
        let base = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("model")
            .to_string();
        let mut id = base.clone();
        let mut suffix = 2;
        while map.contains_key(&id) {
            id = format!("{base}-{suffix}");
            suffix += 1;
        }
        let max_tokens = std::fs::read_to_string(path.join("config.json"))
            .ok()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
            .and_then(|value| value.get("max_position_embeddings").and_then(|v| v.as_u64()))
            .unwrap_or(32768)
            .clamp(512, 32768) as u32;
        let lower = base.to_lowercase();
        let thinks = lower.contains("qwen3") || lower.contains("reason");
        let size_gb = (dir_size(&path, 0) as f64 / 1_000_000_000.0 * 100.0).round() / 100.0;
        map.insert(
            id,
            serde_json::json!({
                "path": path.to_string_lossy(),
                "label": base,
                "device": "CPU",
                "max_tokens": max_tokens,
                "size_gb": size_gb,
                "thinks": thinks,
                "vlm": has_vlm,
                "image": has_image
            }),
        );
        known.push(key);
        added += 1;
    }
    if added > 0 {
        write_map(state, &map)?;
    }
    Ok(added)
}

fn dir_size(path: &Path, depth: usize) -> u64 {
    if depth > 3 {
        return 0;
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    let mut total = 0u64;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            total += dir_size(&path, depth + 1);
        } else if let Ok(metadata) = entry.metadata() {
            total += metadata.len();
        }
    }
    total
}

/// Проверяет, что папка похожа на OpenVINO IR модель.
pub fn validate_model_dir(path: &Path) -> Result<()> {
    if !path.is_dir() {
        bail!("Папка не найдена");
    }
    let mut has_xml = false;
    let mut has_bin = false;
    for entry in std::fs::read_dir(path)?.flatten() {
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if name.ends_with(".xml") {
            has_xml = true;
        }
        if name.ends_with(".bin") {
            has_bin = true;
        }
    }
    if !has_xml || !has_bin {
        bail!("В папке нет OpenVINO IR файлов (.xml и .bin)");
    }
    Ok(())
}

#[tauri::command]
pub async fn local_models_list(state: State<'_, AppState>) -> Result<Vec<LocalModelRow>, String> {
    let map = read_map(&state).map_err(|e| e.to_string())?;
    let rows = map
        .into_iter()
        .map(|(id, value)| LocalModelRow {
            id,
            label: value
                .get("label")
                .and_then(|v| v.as_str())
                .unwrap_or("Локальная модель")
                .to_string(),
            path: value
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            device: value
                .get("device")
                .and_then(|v| v.as_str())
                .unwrap_or("CPU")
                .to_string(),
            max_tokens: value
                .get("max_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(2048) as u32,
            thinks: value
                .get("thinks")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            size_gb: value
                .get("size_gb")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0),
        })
        .collect();
    Ok(rows)
}

#[tauri::command]
pub async fn local_model_add(
    state: State<'_, AppState>,
    input: AddLocalModelInput,
) -> Result<LocalModelRow, String> {
    let path = PathBuf::from(input.path.trim());
    validate_model_dir(&path).map_err(|e| e.to_string())?;
    let label = input.label.trim();
    if label.is_empty() {
        return Err("Укажите название модели".into());
    }
    let device = input
        .device
        .unwrap_or_else(|| "CPU".to_string())
        .trim()
        .to_uppercase();
    if !["CPU", "GPU", "NPU"].contains(&device.as_str()) {
        return Err("Устройство: CPU, GPU или NPU".into());
    }
    let max_tokens = input.max_tokens.unwrap_or(2048).clamp(512, 32768);
    let size_gb = dir_size(&path, 0) as f64 / 1_000_000_000.0;
    let vlm = path.join("openvino_language_model.xml").exists();
    let image = path.join("model_index.json").exists()
        && (path.join("unet").is_dir() || path.join("transformer").is_dir());
    let id = format!(
        "custom-{}",
        crate::db::new_id().split('-').next().unwrap_or("model")
    );
    let mut map = read_map(&state).map_err(|e| e.to_string())?;
    map.insert(
        id.clone(),
        serde_json::json!({
            "path": path.to_string_lossy(),
            "label": label,
            "device": device,
            "max_tokens": max_tokens,
            "size_gb": (size_gb * 100.0).round() / 100.0,
            "thinks": input.thinks.unwrap_or(false),
            "vlm": vlm,
            "image": image
        }),
    );
    write_map(&state, &map).map_err(|e| e.to_string())?;
    Ok(LocalModelRow {
        id,
        label: label.to_string(),
        path: path.to_string_lossy().to_string(),
        device,
        max_tokens,
        thinks: input.thinks.unwrap_or(false),
        size_gb: (size_gb * 100.0).round() / 100.0,
    })
}

#[tauri::command]
pub async fn local_model_remove(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let mut map = read_map(&state).map_err(|e| e.to_string())?;
    if map.remove(&id).is_none() {
        return Err("Модель не найдена".into());
    }
    write_map(&state, &map).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_openvino_dir() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-data")
            .join("model-add");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(validate_model_dir(&dir).is_err());
        std::fs::write(dir.join("openvino_model.xml"), b"<xml/>").unwrap();
        std::fs::write(dir.join("openvino_model.bin"), b"bin").unwrap();
        assert!(validate_model_dir(&dir).is_ok());
        assert!(dir_size(&dir, 0) > 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
