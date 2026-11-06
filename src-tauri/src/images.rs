//! Генерация изображений через локальный OpenVINO Text2Image.

use tauri::State;

use crate::AppState;

#[derive(Debug, Clone, serde::Serialize)]
pub struct TranslatedPrompt {
    pub prompt: String,
    pub negative: String,
}

/// Переводит промпт и негативный промпт на английский для SD/CLIP.
#[tauri::command]
pub async fn image_translate(
    state: State<'_, AppState>,
    prompt: String,
    negative: String,
    model: String,
) -> Result<TranslatedPrompt, String> {
    let prompt = prompt.trim().to_string();
    if prompt.is_empty() {
        return Err("Пустой промпт".into());
    }
    if model.trim().is_empty() {
        return Err("Выберите модель-переводчик".into());
    }
    let system = "Ты — переводчик промптов для Stable Diffusion. Переведи описание изображения на английский язык, сохранив все детали, стиль и композицию. Не добавляй пояснений.\n\
Верни ТОЛЬКО JSON: {\"prompt\":\"...\",\"negative\":\"...\"}.\n\
prompt — английский промпт. negative — английский негативный промпт; если негатив пустой, используй \"blurry, low quality, deformed, extra limbs, text, watermark\".";
    let user = format!("prompt: {prompt}\nnegative: {negative}");
    let messages = vec![
        crate::models::ChatMessage::new("system", system),
        crate::models::ChatMessage::new("user", user),
    ];
    let params = crate::models::ChatParams {
        max_tokens: 500,
        temperature: 0.1,
        think: false,
        reasoning: None,
    };
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let mut text = String::new();
    let mut on_status = |_: &crate::models::ChatStatus| {};
    let outcome = crate::models::Router { state: &state }
        .stream(
            &model,
            &messages,
            &params,
            &cancel,
            &mut |chunk| text.push_str(chunk),
            &mut on_status,
        )
        .await
        .map_err(|e| e.to_string())?;
    let answer = if outcome.text.trim().is_empty() { text } else { outcome.text };
    let start = answer.find('{').ok_or_else(|| "Модель не вернула JSON".to_string())?;
    let end = answer.rfind('}').ok_or_else(|| "Модель не вернула JSON".to_string())?;
    if end <= start {
        return Err("Модель вернула некорректный JSON".into());
    }
    let value: serde_json::Value = serde_json::from_str(&answer[start..=end])
        .map_err(|e| format!("Не удалось разобрать перевод: {e}"))?;
    let field = |key: &str| {
        value
            .get(key)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string()
    };
    let translated = TranslatedPrompt {
        prompt: field("prompt"),
        negative: field("negative"),
    };
    if translated.prompt.is_empty() {
        return Err("Модель вернула пустой перевод".into());
    }
    Ok(translated)
}

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
