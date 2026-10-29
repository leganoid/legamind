//! ModelRouter — единая точка выбора модели.
//!
//! Любая модель имеет id `provider/model`. Локальные (`local/*`) идут в Python-сайдкар,
//! облачные — HTTP к провайдеру. Оба пути отдают одинаковый поток `delta/done/error`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Result};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::secrets;
use crate::AppState;

pub const LOCAL_PREFIX: &str = "local";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn new(role: &str, content: impl Into<String>) -> Self {
        Self { role: role.to_string(), content: content.into() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatParams {
    pub max_tokens: u32,
    pub temperature: f64,
    pub think: bool,
    /// Уровень рассуждений: off/on для локальных, low/high/max для облачных.
    pub reasoning: Option<String>,
}

impl Default for ChatParams {
    fn default() -> Self {
        Self { max_tokens: 1024, temperature: 0.7, think: false, reasoning: None }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub label: String,
    pub provider: String,
    pub kind: String, // local | cloud
    pub device: Option<String>,
    pub size_gb: Option<f64>,
    pub loaded: bool,
    pub thinks: bool,
    /// Предел контекста в токенах (для бюджета и индикатора).
    pub context: Option<u32>,
    /// Уровни рассуждений от провайдера (например ["low","high","max"]).
    pub efforts: Vec<String>,
}

pub struct LocalDef {
    pub id: &'static str,
    pub label: &'static str,
    pub device: &'static str,
    pub size_gb: f64,
    pub thinks: bool,
    pub context_tokens: u32,
}

/// Реестр локальных моделей — синхронизирован с `engine/registry.py`.
/// `context_tokens` — сколько токенов промпта реально принимает пайплайн.
pub const LOCAL_MODELS: &[LocalDef] = &[
    LocalDef { id: "qwen3-14b", label: "Qwen3-14B", device: "GPU", size_gb: 9.06, thinks: true, context_tokens: 32768 },
    LocalDef { id: "coder-30b", label: "Qwen3-Coder-30B-A3B", device: "GPU", size_gb: 15.2, thinks: false, context_tokens: 65536 },
    LocalDef { id: "coder-7b", label: "Qwen2.5-Coder-7B", device: "GPU", size_gb: 4.2, thinks: false, context_tokens: 32768 },
    // NPU-пайплайн ограничен MAX_PROMPT_LEN=4096 (см. engine/manager.py)
    LocalDef { id: "qwen-1.5b", label: "Qwen2.5-1.5B", device: "NPU", size_gb: 0.87, thinks: false, context_tokens: 4096 },
];

pub fn local_models() -> Vec<ModelInfo> {
    LOCAL_MODELS
        .iter()
        .map(|m| ModelInfo {
            id: format!("{LOCAL_PREFIX}/{}", m.id),
            label: m.label.to_string(),
            provider: LOCAL_PREFIX.to_string(),
            kind: "local".to_string(),
            device: Some(m.device.to_string()),
            size_gb: Some(m.size_gb),
            loaded: false,
            thinks: m.thinks,
            context: Some(m.context_tokens),
            efforts: Vec::new(),
        })
        .collect()
}

pub fn local_context_tokens(model_id: &str) -> Option<u32> {
    let short = model_id.strip_prefix("local/").unwrap_or(model_id);
    LOCAL_MODELS.iter().find(|m| m.id == short).map(|m| m.context_tokens)
}

pub fn local_label(model_id: &str) -> Option<&'static str> {
    let short = model_id.strip_prefix("local/").unwrap_or(model_id);
    LOCAL_MODELS.iter().find(|m| m.id == short).map(|m| m.label)
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatOutcome {
    pub text: String,
    pub tokens: u32,
    pub seconds: f64,
    pub stopped: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatStatus {
    pub phase: String,
    pub label: Option<String>,
    pub device: Option<String>,
    pub seconds: Option<f64>,
}

/// Грубая оценка длины текста в токенах — для облачных моделей, где нет нашего
/// токенизатора. Кириллица «дороже» латиницы примерно в 1,5 раза.
pub fn estimate_tokens(text: &str) -> u32 {
    let mut ascii = 0f64;
    let mut cyrillic = 0f64;
    let mut other = 0f64;
    for ch in text.chars() {
        if ch.is_ascii() {
            ascii += 1.0;
        } else if ch.is_alphabetic() {
            cyrillic += 1.0;
        } else {
            other += 1.0;
        }
    }
    ((ascii / 3.8) + (cyrillic / 2.3) + (other / 2.0)).ceil() as u32
}

/// Точная длина текстов в токенах для локальной модели (через сайдкар).
pub async fn count_local_tokens(
    state: &AppState,
    model_id: &str,
    texts: &[String],
) -> Result<Vec<u32>> {
    let base = state.engine_base().await?;
    let short = model_id.strip_prefix("local/").unwrap_or(model_id);
    let resp = state
        .client
        .post(format!("{base}/tokenize"))
        .json(&json!({ "model": short, "texts": texts }))
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .map_err(|e| anyhow!("Не удалось посчитать токены: {e}"))?;
    if !resp.status().is_success() {
        bail!("Сайдкар не посчитал токены ({})", resp.status());
    }
    let value: Value = resp.json().await.map_err(|e| anyhow!("Некорректный ответ /tokenize: {e}"))?;
    let counts = value
        .get("counts")
        .and_then(|c| c.as_array())
        .map(|items| items.iter().filter_map(|v| v.as_u64()).map(|v| v as u32).collect::<Vec<_>>())
        .unwrap_or_default();
    if counts.len() != texts.len() {
        bail!("Сайдкар вернул {} счётчиков на {} текстов", counts.len(), texts.len());
    }
    Ok(counts)
}

pub struct Router<'a> {
    pub state: &'a AppState,
}

fn split_model(model_id: &str) -> Result<(&str, &str)> {
    model_id
        .split_once('/')
        .filter(|(p, m)| !p.is_empty() && !m.is_empty())
        .ok_or_else(|| anyhow!("Некорректный идентификатор модели: {model_id}"))
}

impl Router<'_> {
    pub async fn stream(
        &self,
        model_id: &str,
        messages: &[ChatMessage],
        params: &ChatParams,
        cancel: &AtomicBool,
        on_delta: &mut (dyn FnMut(&str) + Send),
        on_status: &mut (dyn FnMut(&ChatStatus) + Send),
    ) -> Result<ChatOutcome> {
        let (provider, model) = split_model(model_id)?;
        if provider == LOCAL_PREFIX {
            self.stream_local(model, messages, params, cancel, on_delta, on_status).await
        } else {
            self.stream_cloud(provider, model, messages, params, cancel, on_delta, on_status).await
        }
    }

    // ------------------------------------------------------------------ локальные
    async fn stream_local(
        &self,
        model: &str,
        messages: &[ChatMessage],
        params: &ChatParams,
        cancel: &AtomicBool,
        on_delta: &mut (dyn FnMut(&str) + Send),
        on_status: &mut (dyn FnMut(&ChatStatus) + Send),
    ) -> Result<ChatOutcome> {
        let base = self.state.engine_base().await?;
        let body = json!({
            "model": model,
            "messages": messages,
            "max_tokens": params.max_tokens,
            "temperature": params.temperature,
            "think": params.think,
        });
        let resp = self
            .state
            .client
            .post(format!("{base}/chat"))
            .json(&body)
            .timeout(Duration::from_secs(300))
            .send()
            .await
            .map_err(|e| anyhow!("Сайдкар недоступен: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            let msg = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(|s| s.to_string()))
                .unwrap_or_else(|| format!("Ошибка сайдкара ({status})"));
            bail!(msg);
        }
        self.consume_openai_stream(resp, cancel, on_delta, on_status, None).await
    }

    // ------------------------------------------------------------------ облачные
    async fn stream_cloud(
        &self,
        provider_id: &str,
        model: &str,
        messages: &[ChatMessage],
        params: &ChatParams,
        cancel: &AtomicBool,
        on_delta: &mut (dyn FnMut(&str) + Send),
        on_status: &mut (dyn FnMut(&ChatStatus) + Send),
    ) -> Result<ChatOutcome> {
        let provider = self
            .state
            .db
            .provider_get(provider_id)?
            .ok_or_else(|| anyhow!("Провайдер «{provider_id}» не найден. Добавьте его на экране «Провайдеры»."))?;

        if !provider.enabled {
            bail!("Провайдер «{}» отключён", provider.label);
        }
        let key = secrets::get_key(&self.state, provider_id)?
            .ok_or_else(|| anyhow!("Ключ провайдера «{}» не найден. Откройте «Провайдеры» и введите ключ.", provider.label))?;
        let base = provider.base_url.trim_end_matches('/').to_string();
        let timeout = Duration::from_secs(provider.timeout_s.max(10) as u64);

        let mut attempt = 0;
        on_status(&ChatStatus {
            phase: "connecting".to_string(),
            label: Some(provider.label.clone()),
            device: None,
            seconds: None,
        });
        loop {
            attempt += 1;
            let result = match provider.kind.as_str() {
                "anthropic" => {
                    self.call_anthropic(
                        &base, model, &key, messages, params, timeout, cancel, on_delta, on_status,
                    )
                    .await
                }
                _ => {
                    self.call_openai_compatible(
                        &base, model, &key, messages, params, timeout, cancel, on_delta, on_status,
                    )
                    .await
                }
            };
            match result {
                Err(CloudError::RateLimited) if attempt == 1 => {
                    // 429: один повтор через 2 с, затем понятная ошибка.
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
                Err(CloudError::RateLimited) => bail!("Превышен лимит провайдера (429). Попробуйте позже."),
                Err(CloudError::Other(e)) => return Err(e),
                Ok(outcome) => return Ok(outcome),
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn call_openai_compatible(
        &self,
        base: &str,
        model: &str,
        key: &str,
        messages: &[ChatMessage],
        params: &ChatParams,
        timeout: Duration,
        cancel: &AtomicBool,
        on_delta: &mut (dyn FnMut(&str) + Send),
        on_status: &mut (dyn FnMut(&ChatStatus) + Send),
    ) -> std::result::Result<ChatOutcome, CloudError> {
        let mut body = json!({
            "model": model,
            "messages": messages,
            "stream": true,
            "max_tokens": params.max_tokens,
            "temperature": params.temperature,
        });
        if let Some(effort) = params.reasoning.as_deref() {
            if matches!(effort, "low" | "high" | "max") {
                body["reasoning_effort"] = json!(effort);
            }
        }
        let resp = self
            .state
            .client
            .post(format!("{base}/chat/completions"))
            .header("Authorization", format!("Bearer {key}"))
            .header("Content-Type", "application/json")
            .json(&body)
            .timeout(timeout)
            .send()
            .await
            .map_err(|e| CloudError::Other(anyhow!(network_error(e))))?;

        check_status(resp.status())?;
        self.consume_openai_stream(resp, cancel, on_delta, on_status, Some(model.to_string()))
            .await
            .map_err(CloudError::Other)
    }

    #[allow(clippy::too_many_arguments)]
    async fn call_anthropic(
        &self,
        base: &str,
        model: &str,
        key: &str,
        messages: &[ChatMessage],
        params: &ChatParams,
        timeout: Duration,
        cancel: &AtomicBool,
        on_delta: &mut (dyn FnMut(&str) + Send),
        on_status: &mut (dyn FnMut(&ChatStatus) + Send),
    ) -> std::result::Result<ChatOutcome, CloudError> {
        let system: String = messages
            .iter()
            .filter(|m| m.role == "system")
            .map(|m| m.content.clone())
            .collect::<Vec<_>>()
            .join("\n");
        let dialog: Vec<Value> = messages
            .iter()
            .filter(|m| m.role != "system")
            .map(|m| json!({"role": m.role, "content": m.content}))
            .collect();
        let mut body = json!({
            "model": model,
            "messages": dialog,
            "max_tokens": params.max_tokens,
            "temperature": params.temperature,
            "stream": true,
        });
        if !system.is_empty() {
            body["system"] = json!(system);
        }
        let resp = self
            .state
            .client
            .post(format!("{base}/v1/messages"))
            .header("x-api-key", key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&body)
            .timeout(timeout)
            .send()
            .await
            .map_err(|e| CloudError::Other(anyhow!(network_error(e))))?;

        check_status(resp.status())?;
        self.consume_anthropic_stream(resp, cancel, on_delta, on_status)
            .await
            .map_err(CloudError::Other)
    }

    /// Разбор SSE OpenAI-совместимого формата (`choices[0].delta.content`).
    async fn consume_openai_stream(
        &self,
        resp: reqwest::Response,
        cancel: &AtomicBool,
        on_delta: &mut (dyn FnMut(&str) + Send),
        on_status: &mut (dyn FnMut(&ChatStatus) + Send),
        _model: Option<String>,
    ) -> Result<ChatOutcome> {
        let started = Instant::now();
        let mut stream = resp.bytes_stream();
        let mut buffer = String::new();
        let mut utf8_tail: Vec<u8> = Vec::new();
        let mut text = String::new();
        let mut tokens = 0u32;
        let mut stopped = false;
        let mut reported_seconds: Option<f64> = None;

        while let Some(chunk) = stream.next().await {
            if cancel.load(Ordering::Relaxed) {
                stopped = true;
                break;
            }
            let chunk = chunk.map_err(|e| anyhow!("Обрыв потока: {e}"))?;
            buffer.push_str(&decode_stream_chunk(&mut utf8_tail, &chunk));
            while let Some(pos) = buffer.find("\n\n") {
                let event: String = buffer.drain(..pos + 2).collect();
                for line in event.lines() {
                    let line = line.trim_start();
                    let Some(data) = line.strip_prefix("data:") else { continue };
                    let data = data.trim();
                    if data == "[DONE]" {
                        continue;
                    }
                    let Ok(value) = serde_json::from_str::<Value>(data) else { continue };
                    if let Some(err) = value.get("error") {
                        bail!("Ошибка провайдера: {}", error_text(err));
                    }
                    // Статусы сайдкара: loading / ready / generating.
                    if let Some(phase) = value.get("status").and_then(|v| v.as_str()) {
                        on_status(&ChatStatus {
                            phase: phase.to_string(),
                            label: value.get("label").and_then(|v| v.as_str()).map(|s| s.to_string()),
                            device: value.get("device").and_then(|v| v.as_str()).map(|s| s.to_string()),
                            seconds: value.get("seconds").and_then(|v| v.as_f64()),
                        });
                        continue;
                    }
                    // Формат сайдкара: {"delta": "..."} и финальный {"done": true, "tokens": N}.
                    if let Some(delta) = value.get("delta").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
                        text.push_str(delta);
                        tokens += 1;
                        on_delta(delta);
                    }
                    // Формат OpenAI-совместимых провайдеров.
                    if let Some(delta) = value
                        .pointer("/choices/0/delta/content")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                    {
                        text.push_str(delta);
                        tokens += 1;
                        on_delta(delta);
                    }
                    if let Some(usage) = value.get("usage").and_then(|u| u.get("completion_tokens")).and_then(|v| v.as_u64()) {
                        tokens = tokens.max(usage as u32);
                    }
                    if value.get("done").and_then(|v| v.as_bool()).unwrap_or(false) {
                        if let Some(total) = value.get("tokens").and_then(|v| v.as_u64()) {
                            tokens = total as u32;
                        }
                        reported_seconds = value.get("seconds").and_then(|v| v.as_f64());
                    }
                }
            }
        }
        Ok(ChatOutcome {
            text,
            tokens,
            seconds: reported_seconds.unwrap_or_else(|| started.elapsed().as_secs_f64()),
            stopped,
        })
    }

    /// Разбор SSE Anthropic (`content_block_delta` -> `delta.text`).
    async fn consume_anthropic_stream(
        &self,
        resp: reqwest::Response,
        cancel: &AtomicBool,
        on_delta: &mut (dyn FnMut(&str) + Send),
        _on_status: &mut (dyn FnMut(&ChatStatus) + Send),
    ) -> Result<ChatOutcome> {
        let started = Instant::now();
        let mut stream = resp.bytes_stream();
        let mut buffer = String::new();
        let mut utf8_tail: Vec<u8> = Vec::new();
        let mut text = String::new();
        let mut tokens = 0u32;
        let mut stopped = false;

        while let Some(chunk) = stream.next().await {
            if cancel.load(Ordering::Relaxed) {
                stopped = true;
                break;
            }
            let chunk = chunk.map_err(|e| anyhow!("Обрыв потока: {e}"))?;
            buffer.push_str(&decode_stream_chunk(&mut utf8_tail, &chunk));
            while let Some(pos) = buffer.find("\n\n") {
                let event: String = buffer.drain(..pos + 2).collect();
                for line in event.lines() {
                    let line = line.trim_start();
                    let Some(data) = line.strip_prefix("data:") else { continue };
                    let Ok(value) = serde_json::from_str::<Value>(data.trim()) else { continue };
                    match value.get("type").and_then(|t| t.as_str()).unwrap_or("") {
                        "content_block_delta" => {
                            if let Some(delta) = value.pointer("/delta/text").and_then(|v| v.as_str()) {
                                text.push_str(delta);
                                tokens += 1;
                                on_delta(delta);
                            }
                        }
                        "message_delta" => {
                            if let Some(out) = value.pointer("/usage/output_tokens").and_then(|v| v.as_u64()) {
                                tokens = out as u32;
                            }
                        }
                        "error" => bail!("Ошибка провайдера: {}", error_text(&value)),
                        _ => {}
                    }
                }
            }
        }
        Ok(ChatOutcome {
            text,
            tokens,
            seconds: started.elapsed().as_secs_f64(),
            stopped,
        })
    }
}

enum CloudError {
    RateLimited,
    Other(anyhow::Error),
}

fn check_status(status: reqwest::StatusCode) -> std::result::Result<(), CloudError> {
    if status.is_success() {
        return Ok(());
    }
    let message = match status.as_u16() {
        401 | 403 => "Ключ недействителен или отозван. Проверьте ключ на экране «Провайдеры».",
        404 => "Модель недоступна у провайдера.",
        429 => return Err(CloudError::RateLimited),
        s if s >= 500 => "Ошибка провайдера. Попробуйте позже.",
        _ => "Провайдер отклонил запрос.",
    };
    Err(CloudError::Other(anyhow!("{message} (HTTP {status})")))
}

fn network_error(err: reqwest::Error) -> String {
    if err.is_timeout() {
        "Провайдер не ответил вовремя (таймаут).".to_string()
    } else if err.is_connect() {
        "Сервер недоступен: нет соединения или неверный адрес.".to_string()
    } else {
        format!("Ошибка сети: {err}")
    }
}

fn error_text(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.to_string();
    }
    value
        .pointer("/error/message")
        .or_else(|| value.get("message"))
        .and_then(|v| v.as_str())
        .unwrap_or("неизвестная ошибка")
        .to_string()
}

/// Декодирует сетевой чанк в строку, не разрывая многобайтные UTF-8 символы.
///
/// TCP/SSE может разрезать кириллицу на границе чанка; если декодировать каждый
/// чанк отдельно через `from_utf8_lossy`, символ превратится в `�`.
fn decode_stream_chunk(pending: &mut Vec<u8>, chunk: &[u8]) -> String {
    pending.extend_from_slice(chunk);
    let valid = match std::str::from_utf8(pending) {
        Ok(_) => pending.len(),
        Err(err) => err.valid_up_to(),
    };
    let text = String::from_utf8_lossy(&pending[..valid]).into_owned();
    pending.drain(..valid);
    text
}

/// Маска ключа для UI: `sk-…ab12` (сам ключ никогда не покидает хранилище).
pub fn mask_key(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.len() <= 8 {
        return "…".to_string();
    }
    let head: String = chars.iter().take(3).collect();
    let tail: String = chars.iter().skip(chars.len() - 4).collect();
    format!("{head}…{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_id_split() {
        assert_eq!(split_model("local/qwen-1.5b").unwrap(), ("local", "qwen-1.5b"));
        assert_eq!(split_model("openrouter/meta/llama").unwrap(), ("openrouter", "meta/llama"));
        assert!(split_model("broken").is_err());
    }

    #[test]
    fn key_mask_hides_secret() {
        assert_eq!(mask_key("sk-1234567890ab12"), "sk-…ab12");
        assert_eq!(mask_key("short"), "…");
    }

    #[test]
    fn local_label_lookup() {
        assert_eq!(local_label("local/qwen3-14b"), Some("Qwen3-14B"));
        assert_eq!(local_label("local/unknown"), None);
    }

    #[test]
    fn local_context_limits() {
        assert_eq!(local_context_tokens("local/qwen-1.5b"), Some(4096));
        assert_eq!(local_context_tokens("local/qwen3-14b"), Some(32768));
        assert_eq!(local_context_tokens("openai/gpt-4o"), None);
    }

    #[test]
    fn estimate_tokens_grows_with_text() {
        assert_eq!(estimate_tokens(""), 0);
        assert!(estimate_tokens("hello world") >= 2);
        let ru = estimate_tokens("Привет, мир! Это проверка оценки токенов.");
        let en = estimate_tokens("Hello, world! This is a token estimate check.");
        assert!(ru > en, "кириллица должна оцениваться дороже: {ru} vs {en}");
    }
}
