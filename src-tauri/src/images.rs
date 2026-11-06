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
    let value: Option<serde_json::Value> = match (answer.find('{'), answer.rfind('}')) {
        (Some(start), Some(end)) if end > start => {
            serde_json::from_str::<serde_json::Value>(&answer[start..=end]).ok()
        }
        _ => None,
    };
    if let Some(value) = value {
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
        return Ok(translated);
    }

    // Небольшие локальные модели часто не выдают JSON. Просим простой текст.
    let fallback_system = "Translate the image description into English. Return ONLY the English translation, without quotes or explanations. If a negative list is present, put it after a new line starting with NEGATIVE:.";
    let fallback_messages = vec![
        crate::models::ChatMessage::new("system", fallback_system),
        crate::models::ChatMessage::new(
            "user",
            format!("{prompt}\n\nNegative list: {negative}"),
        ),
    ];
    let fallback_params = crate::models::ChatParams {
        max_tokens: 300,
        temperature: 0.1,
        think: false,
        reasoning: None,
    };
    let mut plain = String::new();
    let mut ignored = |_: &crate::models::ChatStatus| {};
    let second = crate::models::Router { state: &state }
        .stream(
            &model,
            &fallback_messages,
            &fallback_params,
            &cancel,
            &mut |chunk| plain.push_str(chunk),
            &mut ignored,
        )
        .await
        .map_err(|e| e.to_string())?;
    let raw = if second.text.trim().is_empty() { plain } else { second.text };
    let mut prompt_en = String::new();
    let mut negative_en = String::new();
    for line in raw.lines() {
        let trimmed = line.trim().trim_matches('"').trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("NEGATIVE:") {
            negative_en = rest.trim().to_string();
        } else if prompt_en.is_empty() {
            prompt_en = trimmed.to_string();
        }
    }
    if prompt_en.is_empty() {
        return Err("Модель не смогла перевести промпт".into());
    }
    Ok(TranslatedPrompt {
        prompt: prompt_en,
        negative: if negative_en.is_empty() {
            "blurry, low quality, deformed, extra limbs, text, watermark".to_string()
        } else {
            negative_en
        },
    })
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
