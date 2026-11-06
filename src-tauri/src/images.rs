//! Генерация изображений через локальный OpenVINO Text2Image.

use tauri::State;

use crate::AppState;

#[tauri::command]
pub async fn image_generate(
    state: State<'_, AppState>,
    model: String,
    prompt: String,
    negative: String,
    width: u32,
    height: u32,
    steps: u32,
    seed: i64,
    guidance_scale: f64,
) -> Result<String, String> {
    if prompt.trim().is_empty() {
        return Err("Пустой промпт".into());
    }
    let base = state.engine_base().await.map_err(|e| e.to_string())?;
    let short = model.strip_prefix("local/").unwrap_or(&model);
    let resp = state
        .client
        .post(format!("{base}/images/generate"))
        .json(&serde_json::json!({
            "model": short,
            "prompt": prompt,
            "negative": negative,
            "width": width,
            "height": height,
            "steps": steps,
            "seed": seed,
            "guidance_scale": guidance_scale
        }))
        .timeout(std::time::Duration::from_secs(600))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        let msg = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|value| value.get("error").and_then(|v| v.as_str()).map(|s| s.to_string()))
            .unwrap_or_else(|| format!("движок вернул {status}"));
        return Err(msg);
    }
    let value: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let image = value
        .get("image")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "движок не вернул изображение".to_string())?;
    Ok(format!("data:image/png;base64,{image}"))
}
