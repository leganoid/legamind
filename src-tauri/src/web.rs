//! Доступ локальных моделей к интернету: глобальный поиск и чтение страниц.
//!
//! Провайдеры: DuckDuckGo HTML (без ключа, по умолчанию), SearxNG (свой инстанс),
//! Tavily и Brave (API-ключи). Результаты вставляются в system-промпт как
//! «актуальные данные», а ссылки сохраняются для показа источников.

use std::collections::BTreeMap;

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::AppState;

const DDG_URL: &str = "https://html.duckduckgo.com/html/";
const BING_URL: &str = "https://www.bing.com/search";
const YAHOO_URL: &str = "https://search.yahoo.com/search";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                          (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36";
const MAX_PAGE_CHARS: usize = 6000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WebConfigView {
    pub enabled: bool,
    pub provider: String,
    pub searxng_url: String,
    pub google_cx: String,
    pub has_tavily: bool,
    pub has_brave: bool,
    pub has_google: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct WebTestResult {
    pub provider: String,
    pub results: Vec<WebResult>,
    pub error: Option<String>,
}

fn setting_bool(state: &AppState, key: &str, default: bool) -> bool {
    match state.db.setting_get(key) {
        Ok(Some(value)) => value == "1" || value.eq_ignore_ascii_case("true"),
        _ => default,
    }
}

fn setting_string(state: &AppState, key: &str, default: &str) -> String {
    state
        .db
        .setting_get(key)
        .ok()
        .flatten()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| default.to_string())
}

/// Интернет включён по умолчанию: пользователь может выключить в настройках.
pub fn enabled(state: &AppState) -> bool {
    setting_bool(state, "web_enabled", true)
}

pub fn provider(state: &AppState) -> String {
    setting_string(state, "web_provider", "auto")
}

fn web_key(state: &AppState, name: &str) -> Option<String> {
    crate::secrets::get_key(state, &format!("web:{name}")).ok().flatten()
}

#[tauri::command]
pub async fn web_config_get(state: State<'_, AppState>) -> Result<WebConfigView, String> {
    Ok(WebConfigView {
        enabled: enabled(&state),
        provider: provider(&state),
        searxng_url: setting_string(&state, "web_searxng_url", ""),
        google_cx: setting_string(&state, "web_google_cx", ""),
        has_tavily: web_key(&state, "tavily").is_some(),
        has_brave: web_key(&state, "brave").is_some(),
        has_google: web_key(&state, "google").is_some(),
    })
}

#[derive(Debug, Clone, Deserialize)]
pub struct WebConfigInput {
    pub enabled: Option<bool>,
    pub provider: Option<String>,
    pub searxng_url: Option<String>,
    pub google_cx: Option<String>,
    pub tavily_key: Option<String>,
    pub brave_key: Option<String>,
    pub google_key: Option<String>,
}

#[tauri::command]
pub async fn web_config_set(
    state: State<'_, AppState>,
    input: WebConfigInput,
) -> Result<WebConfigView, String> {
    if let Some(enabled) = input.enabled {
        state
            .db
            .setting_set("web_enabled", if enabled { "1" } else { "0" })
            .map_err(|e| e.to_string())?;
    }
    if let Some(provider) = input.provider {
        let provider = provider.trim().to_lowercase();
        if ![
            "auto",
            "yahoo",
            "wikipedia",
            "stackexchange",
            "github",
            "bing",
            "ddg",
            "searxng",
            "tavily",
            "brave",
            "google",
            "yandex",
        ]
        .contains(&provider.as_str())
        {
            return Err("Неизвестный поисковый провайдер".into());
        }
        state
            .db
            .setting_set("web_provider", &provider)
            .map_err(|e| e.to_string())?;
    }
    if let Some(url) = input.searxng_url {
        state
            .db
            .setting_set("web_searxng_url", url.trim())
            .map_err(|e| e.to_string())?;
    }
    if let Some(key) = input.tavily_key {
        if key.trim().is_empty() {
            let _ = crate::secrets::delete_key(&state, "web:tavily");
        } else {
            crate::secrets::set_key(&state, "web:tavily", key.trim()).map_err(|e| e.to_string())?;
        }
    }
    if let Some(key) = input.brave_key {
        if key.trim().is_empty() {
            let _ = crate::secrets::delete_key(&state, "web:brave");
        } else {
            crate::secrets::set_key(&state, "web:brave", key.trim()).map_err(|e| e.to_string())?;
        }
    }
    if let Some(cx) = input.google_cx {
        state
            .db
            .setting_set("web_google_cx", cx.trim())
            .map_err(|e| e.to_string())?;
    }
    if let Some(key) = input.google_key {
        if key.trim().is_empty() {
            let _ = crate::secrets::delete_key(&state, "web:google");
        } else {
            crate::secrets::set_key(&state, "web:google", key.trim()).map_err(|e| e.to_string())?;
        }
    }
    web_config_get(state).await
}

#[tauri::command]
pub async fn web_search(
    state: State<'_, AppState>,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<WebResult>, String> {
    search(&state, &query, limit.unwrap_or(5).clamp(1, 10))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn web_test(state: State<'_, AppState>) -> Result<WebTestResult, String> {
    let provider_name = provider(&state);
    match search(&state, "OpenVINO GenAI", 3).await {
        Ok(results) => Ok(WebTestResult {
            provider: provider_name,
            results,
            error: None,
        }),
        Err(err) => Ok(WebTestResult {
            provider: provider_name,
            results: vec![],
            error: Some(err.to_string()),
        }),
    }
}

#[tauri::command]
pub async fn web_fetch_page(
    state: State<'_, AppState>,
    url: String,
) -> Result<String, String> {
    fetch_page(&state, &url).await.map_err(|e| e.to_string())
}

// ------------------------------------------------------------------ поиск

pub async fn search(state: &AppState, query: &str, limit: usize) -> Result<Vec<WebResult>> {
    let query = query.trim();
    if query.chars().count() < 2 {
        bail!("Слишком короткий поисковый запрос");
    }
    let limit = limit.clamp(1, 10);
    match provider(state).as_str() {
        "searxng" => {
            let base = setting_string(state, "web_searxng_url", "");
            if base.is_empty() {
                bail!("Укажите адрес SearxNG в настройках интернета");
            }
            search_searxng(state, &base, query, limit).await
        }
        "tavily" => {
            let key = web_key(state, "tavily").ok_or_else(|| anyhow!("Нет ключа Tavily"))?;
            search_tavily(state, &key, query, limit).await
        }
        "brave" => {
            let key = web_key(state, "brave").ok_or_else(|| anyhow!("Нет ключа Brave"))?;
            search_brave(state, &key, query, limit).await
        }
        "google" => {
            let key = web_key(state, "google").ok_or_else(|| {
                anyhow!("Нужен ключ Google Custom Search (и CX) в настройках интернета")
            })?;
            let cx = setting_string(state, "web_google_cx", "");
            if cx.is_empty() {
                bail!("Укажите CX (Programmable Search Engine ID) в настройках интернета");
            }
            search_google(state, &key, &cx, query, limit).await
        }
        "yandex" => search_yandex(state, query, limit).await,
        "ddg" => search_ddg(state, query, limit).await,
        "bing" => search_bing(state, query, limit).await,
        "yahoo" => search_yahoo(state, query, limit).await,
        "wikipedia" => search_wikipedia(state, query, limit).await,
        "stackexchange" => search_stackexchange(state, query, limit).await,
        "github" => search_github(state, query, limit).await,
        _ => auto_search(state, query, limit).await,
    }
}

/// Без ключей доступны Wikipedia, Stack Exchange и GitHub. Если пользователь
/// добавил ключ Tavily/Brave или свой SearxNG — они идут первыми.
async fn auto_search(state: &AppState, query: &str, limit: usize) -> Result<Vec<WebResult>> {
    let mut errors: Vec<String> = Vec::new();

    if let Some(key) = web_key(state, "tavily") {
        match search_tavily(state, &key, query, limit).await {
            Ok(results) if !results.is_empty() => return Ok(results),
            Ok(_) => errors.push("tavily: пусто".into()),
            Err(err) => errors.push(format!("tavily: {err}")),
        }
    }
    if let Some(key) = web_key(state, "brave") {
        match search_brave(state, &key, query, limit).await {
            Ok(results) if !results.is_empty() => return Ok(results),
            Ok(_) => errors.push("brave: пусто".into()),
            Err(err) => errors.push(format!("brave: {err}")),
        }
    }
    let google_cx = setting_string(state, "web_google_cx", "");
    if let Some(key) = web_key(state, "google") {
        if !google_cx.is_empty() {
            match search_google(state, &key, &google_cx, query, limit).await {
                Ok(results) if !results.is_empty() => return Ok(results),
                Ok(_) => errors.push("google: пусто".into()),
                Err(err) => errors.push(format!("google: {err}")),
            }
        }
    }
    let searxng = setting_string(state, "web_searxng_url", "");
    if !searxng.is_empty() {
        match search_searxng(state, &searxng, query, limit).await {
            Ok(results) if !results.is_empty() => return Ok(results),
            Ok(_) => errors.push("searxng: пусто".into()),
            Err(err) => errors.push(format!("searxng: {err}")),
        }
    }
    match search_wikipedia(state, query, limit).await {
        Ok(results) if !results.is_empty() => return Ok(results),
        Ok(_) => errors.push("wikipedia: пусто".into()),
        Err(err) => errors.push(format!("wikipedia: {err}")),
    }
    match search_stackexchange(state, query, limit).await {
        Ok(results) if !results.is_empty() => return Ok(results),
        Ok(_) => errors.push("stackexchange: пусто".into()),
        Err(err) => errors.push(format!("stackexchange: {err}")),
    }
    match search_github(state, query, limit).await {
        Ok(results) if !results.is_empty() => return Ok(results),
        Ok(_) => errors.push("github: пусто".into()),
        Err(err) => errors.push(format!("github: {err}")),
    }
    match search_yandex(state, query, limit).await {
        Ok(results) if !results.is_empty() => return Ok(results),
        Ok(_) => errors.push("yandex: пусто".into()),
        Err(err) => errors.push(format!("yandex: {err}")),
    }
    // Yahoo — только как последний резерв: выдача нестабильна и может блокироваться.
    match search_yahoo(state, query, limit).await {
        Ok(results) if !results.is_empty() => return Ok(results),
        Ok(_) => errors.push("yahoo: пусто".into()),
        Err(err) => errors.push(format!("yahoo: {err}")),
    }
    Err(anyhow!(
        "Поиск не дал результатов. Добавьте ключ Tavily/Brave или свой SearxNG в настройках. Попытки: {}",
        errors.join("; ")
    ))
}

fn encode_path(text: &str) -> String {
    let mut out = String::new();
    for byte in text.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn truncate_chars(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        text.to_string()
    } else {
        let cut: String = text.chars().take(limit).collect();
        format!("{}…", cut.trim_end())
    }
}

fn wikipedia_lang(query: &str) -> &'static str {
    if query.chars().any(|c| {
        let code = c as u32;
        (0x0400..=0x04FF).contains(&code)
    }) {
        "ru"
    } else {
        "en"
    }
}

async fn search_wikipedia(state: &AppState, query: &str, limit: usize) -> Result<Vec<WebResult>> {
    let lang = wikipedia_lang(query);
    let endpoint = format!("https://{lang}.wikipedia.org/w/api.php");
    let response = state
        .client
        .get(&endpoint)
        .query(&[
            ("action", "query"),
            ("list", "search"),
            ("srsearch", query),
            ("srlimit", &limit.to_string()),
            ("format", "json"),
            ("utf8", "1"),
        ])
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await?;
    if !response.status().is_success() {
        bail!("Wikipedia вернула HTTP {}", response.status());
    }
    let value: serde_json::Value = response.json().await?;
    Ok(parse_wikipedia(&value, lang, limit))
}

pub fn parse_wikipedia(value: &serde_json::Value, lang: &str, limit: usize) -> Vec<WebResult> {
    let mut out = Vec::new();
    let Some(items) = value
        .get("query")
        .and_then(|v| v.get("search"))
        .and_then(|v| v.as_array())
    else {
        return out;
    };
    for item in items.iter().take(limit) {
        let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if title.is_empty() {
            continue;
        }
        let snippet = item
            .get("snippet")
            .and_then(|v| v.as_str())
            .map(|text| strip_tags(text).trim().to_string())
            .unwrap_or_default();
        let url = format!(
            "https://{lang}.wikipedia.org/wiki/{}",
            encode_path(&title.replace(' ', "_"))
        );
        out.push(WebResult { title, url, snippet });
    }
    out
}

async fn search_stackexchange(
    state: &AppState,
    query: &str,
    limit: usize,
) -> Result<Vec<WebResult>> {
    let response = state
        .client
        .get("https://api.stackexchange.com/2.3/search/advanced")
        .query(&[
            ("order", "desc"),
            ("sort", "relevance"),
            ("q", query),
            ("site", "stackoverflow"),
            ("pagesize", &limit.to_string()),
            ("filter", "withbody"),
        ])
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await?;
    if !response.status().is_success() {
        bail!("Stack Exchange вернул HTTP {}", response.status());
    }
    let value: serde_json::Value = response.json().await?;
    Ok(parse_stackexchange(&value, limit))
}

pub fn parse_stackexchange(value: &serde_json::Value, limit: usize) -> Vec<WebResult> {
    let mut out = Vec::new();
    let Some(items) = value.get("items").and_then(|v| v.as_array()) else {
        return out;
    };
    for item in items.iter().take(limit) {
        let title = strip_tags(item.get("title").and_then(|v| v.as_str()).unwrap_or(""))
            .trim()
            .to_string();
        let url = item.get("link").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if title.is_empty() || !url.starts_with("http") {
            continue;
        }
        let body = item.get("body").and_then(|v| v.as_str()).unwrap_or("");
        let snippet = truncate_chars(strip_tags(body).trim(), 280);
        out.push(WebResult { title, url, snippet });
    }
    out
}

async fn search_github(state: &AppState, query: &str, limit: usize) -> Result<Vec<WebResult>> {
    let response = state
        .client
        .get("https://api.github.com/search/repositories")
        .query(&[
            ("q", query),
            ("sort", "stars"),
            ("order", "desc"),
            ("per_page", &limit.to_string()),
        ])
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await?;
    if !response.status().is_success() {
        bail!("GitHub вернул HTTP {}", response.status());
    }
    let value: serde_json::Value = response.json().await?;
    Ok(parse_github(&value, limit))
}

pub fn parse_github(value: &serde_json::Value, limit: usize) -> Vec<WebResult> {
    let mut out = Vec::new();
    let Some(items) = value.get("items").and_then(|v| v.as_array()) else {
        return out;
    };
    for item in items.iter().take(limit) {
        let title = item
            .get("full_name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let url = item
            .get("html_url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if title.is_empty() || !url.starts_with("http") {
            continue;
        }
        let snippet = item
            .get("description")
            .and_then(|v| v.as_str())
            .map(|text| truncate_chars(text, 280))
            .unwrap_or_default();
        out.push(WebResult { title, url, snippet });
    }
    out
}

async fn search_bing(state: &AppState, query: &str, limit: usize) -> Result<Vec<WebResult>> {
    let response = state
        .client
        .get(BING_URL)
        .query(&[("q", query), ("setlang", "ru")])
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .header(reqwest::header::ACCEPT_LANGUAGE, "ru,en;q=0.8")
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await?;
    if !response.status().is_success() {
        bail!("Bing вернул HTTP {}", response.status());
    }
    let html = response.text().await?;
    Ok(parse_bing(&html, limit))
}

async fn search_ddg(state: &AppState, query: &str, limit: usize) -> Result<Vec<WebResult>> {
    let response = state
        .client
        .get(DDG_URL)
        .query(&[("q", query)])
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .header(reqwest::header::ACCEPT_LANGUAGE, "ru,en;q=0.8")
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await?;
    if !response.status().is_success() {
        bail!("DuckDuckGo вернул HTTP {}", response.status());
    }
    let html = response.text().await?;
    Ok(parse_ddg(&html, limit))
}

async fn search_searxng(
    state: &AppState,
    base: &str,
    query: &str,
    limit: usize,
) -> Result<Vec<WebResult>> {
    let url = format!("{}/search", base.trim_end_matches('/'));
    let response = state
        .client
        .get(&url)
        .query(&[("q", query), ("format", "json")])
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await?;
    if !response.status().is_success() {
        bail!("SearxNG вернул HTTP {}", response.status());
    }
    let value: serde_json::Value = response.json().await?;
    let mut out = Vec::new();
    if let Some(results) = value.get("results").and_then(|v| v.as_array()) {
        for item in results.iter().take(limit) {
            let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let url = item.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let snippet = item.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string();
            if !url.is_empty() {
                out.push(WebResult { title, url, snippet });
            }
        }
    }
    Ok(out)
}

async fn search_tavily(
    state: &AppState,
    key: &str,
    query: &str,
    limit: usize,
) -> Result<Vec<WebResult>> {
    let body = serde_json::json!({
        "api_key": key,
        "query": query,
        "max_results": limit,
        "search_depth": "basic",
        "include_answer": false,
    });
    let response = state
        .client
        .post("https://api.tavily.com/search")
        .json(&body)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await?;
    if !response.status().is_success() {
        bail!("Tavily вернул HTTP {}", response.status());
    }
    let value: serde_json::Value = response.json().await?;
    let mut out = Vec::new();
    if let Some(results) = value.get("results").and_then(|v| v.as_array()) {
        for item in results {
            let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let url = item.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let snippet = item.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string();
            if !url.is_empty() {
                out.push(WebResult { title, url, snippet });
            }
        }
    }
    Ok(out.into_iter().take(limit).collect())
}

async fn search_brave(
    state: &AppState,
    key: &str,
    query: &str,
    limit: usize,
) -> Result<Vec<WebResult>> {
    let response = state
        .client
        .get("https://api.search.brave.com/res/v1/web/search")
        .query(&[("q", query), ("count", &limit.to_string())])
        .header("X-Subscription-Token", key)
        .header(reqwest::header::ACCEPT, "application/json")
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await?;
    if !response.status().is_success() {
        bail!("Brave вернул HTTP {}", response.status());
    }
    let value: serde_json::Value = response.json().await?;
    let mut out = Vec::new();
    if let Some(results) = value
        .get("web")
        .and_then(|v| v.get("results"))
        .and_then(|v| v.as_array())
    {
        for item in results.iter().take(limit) {
            let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let url = item.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let snippet = item
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if !url.is_empty() {
                out.push(WebResult { title, url, snippet });
            }
        }
    }
    Ok(out)
}

// ------------------------------------------------------------------ разбор HTML

fn decode_entity(entity: &str) -> String {
    match entity {
        "amp" => "&".into(),
        "lt" => "<".into(),
        "gt" => ">".into(),
        "quot" => "\"".into(),
        "apos" => "'".into(),
        "nbsp" => " ".into(),
        _ => {
            if let Some(hex) = entity.strip_prefix("#x").or_else(|| entity.strip_prefix("#X")) {
                if let Ok(code) = u32::from_str_radix(hex, 16) {
                    if let Some(ch) = char::from_u32(code) {
                        return ch.to_string();
                    }
                }
            }
            if let Some(dec) = entity.strip_prefix('#') {
                if let Ok(code) = dec.parse::<u32>() {
                    if let Some(ch) = char::from_u32(code) {
                        return ch.to_string();
                    }
                }
            }
            format!("&{entity};")
        }
    }
}

pub fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'&' {
            if let Some(end) = text[index..].find(';') {
                if end <= 10 {
                    let entity = &text[index + 1..index + end];
                    out.push_str(&decode_entity(entity));
                    index += end + 1;
                    continue;
                }
            }
        }
        let ch = text[index..].chars().next().unwrap_or('&');
        out.push(ch);
        index += ch.len_utf8();
    }
    out
}

pub fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut depth = 0i32;
    for ch in html.chars() {
        match ch {
            '<' => depth += 1,
            '>' => depth = (depth - 1).max(0),
            _ if depth == 0 => out.push(ch),
            _ => {}
        }
    }
    decode_entities(&out)
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("");
            if let Ok(value) = u8::from_str_radix(hex, 16) {
                out.push(value);
                index += 3;
                continue;
            }
        }
        if bytes[index] == b'+' {
            out.push(b' ');
        } else {
            out.push(bytes[index]);
        }
        index += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// Достаёт реальный адрес из редиректа DuckDuckGo (`/l/?uddg=...`).
pub fn unwrap_ddg_url(href: &str) -> String {
    if let Some(query_start) = href.find("uddg=") {
        let rest = &href[query_start + 5..];
        let end = rest.find('&').unwrap_or(rest.len());
        return percent_decode(&rest[..end]);
    }
    href.to_string()
}

fn extract_attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let pattern = format!("{name}=\"");
    let start = tag.find(&pattern)? + pattern.len();
    let end = tag[start..].find('"')? + start;
    Some(&tag[start..end])
}

/// Разбор выдачи DuckDuckGo HTML. Вынесен отдельно, чтобы тестировать на фикстурах.
pub fn parse_ddg(html: &str, limit: usize) -> Vec<WebResult> {
    let mut titles: Vec<(String, String)> = Vec::new();
    let mut snippets: Vec<String> = Vec::new();

    for chunk in html.split("class=\"result__a\"").skip(1) {
        // href идёт до class в разметке DDG, поэтому смотрим на весь тег вокруг.
        let tag_end = match chunk.find('>') {
            Some(pos) => pos,
            None => continue,
        };
        let tag_start = chunk[..tag_end].rfind('<').unwrap_or(0);
        let tag = &chunk[tag_start..tag_end];
        let href = extract_attr(tag, "href").unwrap_or("");
        let after = &chunk[tag_end + 1..];
        let text_end = after.find("</a>").unwrap_or(after.len());
        let title = strip_tags(&after[..text_end]).trim().to_string();
        if !href.is_empty() && !title.is_empty() {
            titles.push((title, unwrap_ddg_url(href)));
        }
    }

    for chunk in html.split("class=\"result__snippet\"").skip(1) {
        let after = match chunk.find('>') {
            Some(pos) => &chunk[pos + 1..],
            None => continue,
        };
        let end = after.find("</a>").unwrap_or(after.len());
        let text = strip_tags(&after[..end]).trim().to_string();
        if !text.is_empty() {
            snippets.push(text);
        }
    }

    let mut out = Vec::new();
    for (index, (title, url)) in titles.into_iter().enumerate() {
        if url.is_empty() || !url.starts_with("http") {
            continue;
        }
        out.push(WebResult {
            title,
            url,
            snippet: snippets.get(index).cloned().unwrap_or_default(),
        });
        if out.len() >= limit {
            break;
        }
    }
    out
}

// ------------------------------------------------------------------ Google / Яндекс

async fn search_google(
    state: &AppState,
    key: &str,
    cx: &str,
    query: &str,
    limit: usize,
) -> Result<Vec<WebResult>> {
    let response = state
        .client
        .get("https://www.googleapis.com/customsearch/v1")
        .query(&[
            ("key", key),
            ("cx", cx),
            ("q", query),
            ("num", &limit.to_string()),
        ])
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        bail!(
            "Google вернул HTTP {status}: {}",
            truncate_chars(body.trim(), 200)
        );
    }
    let value: serde_json::Value = response.json().await?;
    Ok(parse_google(&value, limit))
}

pub fn parse_google(value: &serde_json::Value, limit: usize) -> Vec<WebResult> {
    let mut out = Vec::new();
    let Some(items) = value.get("items").and_then(|v| v.as_array()) else {
        return out;
    };
    for item in items.iter().take(limit) {
        let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let url = item.get("link").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if title.is_empty() || !url.starts_with("http") {
            continue;
        }
        let snippet = item
            .get("snippet")
            .and_then(|v| v.as_str())
            .map(|text| truncate_chars(text, 300))
            .unwrap_or_default();
        out.push(WebResult { title, url, snippet });
    }
    out
}

async fn search_yandex(state: &AppState, query: &str, limit: usize) -> Result<Vec<WebResult>> {
    let response = state
        .client
        .get("https://yandex.ru/search/")
        .query(&[("text", query), ("lr", "213")])
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .header(reqwest::header::ACCEPT_LANGUAGE, "ru-RU,ru;q=0.9,en;q=0.6")
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await?;
    if !response.status().is_success() {
        bail!("Яндекс вернул HTTP {}", response.status());
    }
    let html = response.text().await?;
    if html.contains("showcaptcha") || html.contains("SmartCaptcha") {
        bail!("Яндекс показывает капчу — используйте ключ Google CSE, Tavily/Brave или свой SearxNG");
    }
    Ok(parse_yandex(&html, limit))
}

/// Разбор выдачи Яндекса (`li.serp-item`, `OrganicTitle-Link`, `OrganicText`).
pub fn parse_yandex(html: &str, limit: usize) -> Vec<WebResult> {
    let mut out = Vec::new();
    let chunks: Vec<&str> = if html.contains("serp-item") {
        html.split("class=\"serp-item\"").skip(1).collect()
    } else {
        html.split("OrganicTitle-Link").skip(1).collect()
    };
    for chunk in chunks {
        let Some(anchor_pos) = chunk.find("<a ") else {
            continue;
        };
        let Some(tag_rel_end) = chunk[anchor_pos..].find('>') else {
            continue;
        };
        let tag = &chunk[anchor_pos..anchor_pos + tag_rel_end];
        let href = extract_attr(tag, "href").unwrap_or("");
        if !href.starts_with("http") {
            continue;
        }
        let after = &chunk[anchor_pos + tag_rel_end + 1..];
        let title_end = after.find("</a>").unwrap_or(after.len());
        let title = strip_tags(&after[..title_end]).trim().to_string();
        if title.is_empty() {
            continue;
        }
        let snippet = chunk
            .find("OrganicText")
            .and_then(|start| {
                let end = chunk[start..].find('>')?;
                let rest = &chunk[start + end + 1..];
                let close = rest.find("</div>").or_else(|| rest.find("</span>"))?;
                Some(truncate_chars(strip_tags(&rest[..close]).trim(), 300))
            })
            .unwrap_or_default();
        out.push(WebResult {
            title,
            url: href.to_string(),
            snippet,
        });
        if out.len() >= limit {
            break;
        }
    }
    out
}

// ------------------------------------------------------------------ Yahoo

/// Yahoo оборачивает ссылки в `r.search.yahoo.com/...;RU=<url>/RK=...`.
pub fn decode_yahoo_url(href: &str) -> String {
    if let Some(pos) = href.find("RU=") {
        let start = pos + 3;
        let end = href[start..]
            .find("/RK=")
            .map(|offset| start + offset)
            .unwrap_or(href.len());
        let decoded = percent_decode(&href[start..end]);
        if decoded.starts_with("http") {
            return decoded;
        }
    }
    href.to_string()
}

/// Разбор выдачи Yahoo (`div.dd.algo`).
pub fn parse_yahoo(html: &str, limit: usize) -> Vec<WebResult> {
    let mut out = Vec::new();
    for chunk in html.split("class=\"dd algo").skip(1) {
        let Some(anchor_start) = chunk.find("<a ") else {
            continue;
        };
        let Some(anchor_rel_end) = chunk[anchor_start..].find('>') else {
            continue;
        };
        let tag = &chunk[anchor_start..anchor_start + anchor_rel_end];
        let href = extract_attr(tag, "href").unwrap_or("");
        if href.is_empty() {
            continue;
        }
        let title = chunk
            .find("<h3")
            .and_then(|start| {
                let end = chunk[start..].find('>')?;
                let rest = &chunk[start + end + 1..];
                let close = rest.find("</h3>")?;
                Some(strip_tags(&rest[..close]).trim().to_string())
            })
            .unwrap_or_default();
        if title.is_empty() {
            continue;
        }
        let snippet = chunk
            .find("<p")
            .and_then(|start| {
                let end = chunk[start..].find('>')?;
                let rest = &chunk[start + end + 1..];
                let close = rest.find("</p>")?;
                Some(strip_tags(&rest[..close]).trim().to_string())
            })
            .unwrap_or_default();
        let url = decode_yahoo_url(href);
        if !url.starts_with("http") {
            continue;
        }
        out.push(WebResult {
            title,
            url,
            snippet: truncate_chars(&snippet, 300),
        });
        if out.len() >= limit {
            break;
        }
    }
    out
}

async fn search_yahoo(state: &AppState, query: &str, limit: usize) -> Result<Vec<WebResult>> {
    let response = state
        .client
        .get(YAHOO_URL)
        .query(&[("p", query)])
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .header(reqwest::header::ACCEPT_LANGUAGE, "en-US,en;q=0.9,ru;q=0.8")
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await?;
    if !response.status().is_success() {
        bail!("Yahoo вернул HTTP {}", response.status());
    }
    let html = response.text().await?;
    Ok(parse_yahoo(&html, limit))
}

// ------------------------------------------------------------------ Bing

fn base64url_decode(input: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut buffer: u32 = 0;
    let mut bits = 0u32;
    for ch in input.chars() {
        let value = match ch {
            'A'..='Z' => ch as u32 - 'A' as u32,
            'a'..='z' => ch as u32 - 'a' as u32 + 26,
            '0'..='9' => ch as u32 - '0' as u32 + 52,
            '-' | '+' => 62,
            '_' | '/' => 63,
            '=' => break,
            _ => return None,
        };
        buffer = (buffer << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
        }
    }
    Some(out)
}

/// Bing заворачивает ссылки в `bing.com/ck/a?...&u=a1<base64url>`.
pub fn decode_bing_url(href: &str) -> String {
    if let Some(pos) = href.find("u=a1") {
        let start = pos + 4;
        let end = href[start..]
            .find('&')
            .map(|offset| start + offset)
            .unwrap_or(href.len());
        if let Some(bytes) = base64url_decode(&href[start..end]) {
            if let Ok(url) = String::from_utf8(bytes) {
                if url.starts_with("http") {
                    return url;
                }
            }
        }
    }
    href.to_string()
}

/// Разбор выдачи Bing (`li.b_algo`).
pub fn parse_bing(html: &str, limit: usize) -> Vec<WebResult> {
    let mut out = Vec::new();
    for chunk in html.split("class=\"b_algo\"").skip(1) {
        let Some(anchor_start) = chunk.find("<a ") else {
            continue;
        };
        let Some(anchor_rel_end) = chunk[anchor_start..].find('>') else {
            continue;
        };
        let tag = &chunk[anchor_start..anchor_start + anchor_rel_end];
        let href = extract_attr(tag, "href").unwrap_or("");
        let after = &chunk[anchor_start + anchor_rel_end + 1..];
        let title_end = after.find("</a>").unwrap_or(after.len());
        let title = strip_tags(&after[..title_end]).trim().to_string();
        if href.is_empty() || title.is_empty() {
            continue;
        }
        let snippet = chunk
            .find("<p")
            .and_then(|start| {
                let end = chunk[start..].find('>')?;
                let rest = &chunk[start + end + 1..];
                let close = rest.find("</p>")?;
                Some(strip_tags(&rest[..close]).trim().to_string())
            })
            .unwrap_or_default();
        out.push(WebResult {
            title,
            url: decode_bing_url(href),
            snippet,
        });
        if out.len() >= limit {
            break;
        }
    }
    out
}

// ------------------------------------------------------------------ чтение страниц

pub fn host_is_private(host: &str) -> bool {
    let host = host.trim().trim_start_matches('[').trim_end_matches(']').to_lowercase();
    if host.is_empty() || host == "localhost" || host.ends_with(".localhost") || host.ends_with(".local") {
        return true;
    }
    if let Some(rest) = host.strip_prefix("::ffff:") {
        return host_is_private(rest);
    }
    let octets: Vec<&str> = host.split('.').collect();
    if octets.len() == 4 {
        if let (Ok(a), Ok(b)) = (octets[0].parse::<u8>(), octets[1].parse::<u8>()) {
            if a == 10 || a == 127 || a == 0 || a == 169 && b == 254 {
                return true;
            }
            if a == 192 && b == 168 {
                return true;
            }
            if a == 172 && (16..=31).contains(&b) {
                return true;
            }
            if a == 100 && (64..=127).contains(&b) {
                return true;
            }
        }
    }
    host == "::1"
}

pub fn extract_readable_text(html: &str) -> String {
    let mut text = html.to_string();
    for (open, close) in [
        ("<script", "</script>"),
        ("<style", "</style>"),
        ("<noscript", "</noscript>"),
        ("<svg", "</svg>"),
    ] {
        loop {
            let lower = text.to_lowercase();
            let Some(start) = lower.find(open) else { break };
            let Some(end_rel) = lower[start..].find(close) else {
                text.truncate(start);
                break;
            };
            let end = start + end_rel + close.len();
            text.replace_range(start..end, " ");
        }
    }
    let mut cleaned = strip_tags(&text);
    cleaned = cleaned
        .replace(['\u{00a0}', '\t', '\r'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if cleaned.chars().count() > MAX_PAGE_CHARS {
        cleaned = cleaned.chars().take(MAX_PAGE_CHARS).collect();
    }
    cleaned
}

pub async fn fetch_page(state: &AppState, url: &str) -> Result<String> {
    let parsed = reqwest::Url::parse(url).map_err(|_| anyhow!("Некорректный URL"))?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        bail!("Разрешены только http/https");
    }
    let host = parsed.host_str().unwrap_or("");
    if host_is_private(host) {
        bail!("Запрещён доступ к локальным адресам");
    }
    let response = state
        .client
        .get(parsed)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .timeout(std::time::Duration::from_secs(25))
        .send()
        .await?;
    if !response.status().is_success() {
        bail!("Страница вернула HTTP {}", response.status());
    }
    let html = response.text().await?;
    Ok(extract_readable_text(&html))
}

// ------------------------------------------------------------------ интеграция в чат

/// Самодостаточный поисковый запрос из сообщения пользователя.
pub fn query_from_user(text: &str) -> String {
    let mut query = text.trim().to_string();
    for prefix in [
        "найди в интернете",
        "поищи в интернете",
        "поищи",
        "найди",
        "погугли",
        "search the web for",
        "search for",
        "look up",
    ] {
        let lower = query.to_lowercase();
        if lower.starts_with(prefix) {
            query = query[prefix.len()..].trim_start_matches([':', ' ', '—', '-']).to_string();
            break;
        }
    }
    query = query.trim_matches(|c: char| c == '?' || c == '.' || c == '!' || c == ' ').to_string();
    query.chars().take(300).collect()
}

/// Блок для system-промпта с результатами поиска.
pub fn prompt_block(query: &str, results: &[WebResult]) -> String {
    let date = chrono::Local::now().format("%d.%m.%Y");
    let mut out = format!(
        "Актуальные данные из интернета (запрос: «{query}», дата: {date}). \
         Используй их, если они относятся к вопросу, и не выдумывай факты сверх них:\n"
    );
    for (index, result) in results.iter().enumerate() {
        out.push_str(&format!(
            "[{}] {} — {}\n{}\n",
            index + 1,
            result.title,
            result.url,
            result.snippet
        ));
    }
    out.trim_end().to_string()
}

/// Стоит ли вообще искать: короткие реплики («привет», «спасибо») не ищем.
pub fn should_search(text: &str) -> bool {
    let query = query_from_user(text);
    if query.chars().count() < 10 {
        return false;
    }
    let words = query.split_whitespace().count();
    words >= 3 || query.chars().count() >= 25
}

/// Поиск для чата: возвращает блок и источники; ошибки не роняют ответ модели.
pub async fn context_for(state: &AppState, user_text: &str) -> (Option<String>, Vec<WebResult>) {
    if !enabled(state) || !should_search(user_text) {
        return (None, vec![]);
    }
    let query = query_from_user(user_text);
    match search(state, &query, 5).await {
        Ok(results) if !results.is_empty() => (Some(prompt_block(&query, &results)), results),
        Ok(_) => {
            crate::logging::log_line("app.log", &format!("web search: пусто по запросу «{query}»"));
            (None, vec![])
        }
        Err(err) => {
            crate::logging::log_line(
                "app.log",
                &format!("web search error по запросу «{query}»: {err}"),
            );
            (None, vec![])
        }
    }
}

/// Проверка конфигурации: используется в UI.
pub fn provider_status(state: &AppState) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    map.insert("provider".into(), provider(state));
    map.insert("enabled".into(), if enabled(state) { "1".into() } else { "0".into() });
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"
    <div class="result results_links results_links_deep web-result">
      <div class="links_main links_deep result__body">
        <h2 class="result__title">
          <a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fdocs.openvino.ai%2Fgenai&amp;rut=abc">OpenVINO&nbsp;GenAI docs</a>
        </h2>
        <a class="result__snippet" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fdocs.openvino.ai%2Fgenai">Run&nbsp;LLM locally with OpenVINO GenAI.</a>
      </div>
    </div>
    <div class="result results_links results_links_deep web-result">
        <a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fgithub.com%2Fopenvinotoolkit%2Fopenvino.genai&amp;rut=x">GitHub - openvinotoolkit/openvino.genai</a>
        <a class="result__snippet">Examples and&nbsp;demos.</a>
    </div>
    "#;

    #[test]
    fn parses_bing_results_and_redirects() {
        let html = r#"
        <li class="b_algo"><h2><a href="https://example.com/page" h="ID=SERP,1">Example <strong>Title</strong></a></h2>
          <div class="b_caption"><p class="b_lineclamp2">Snippet text here.</p></div></li>
        <li class="b_algo"><h2><a href="https://www.bing.com/ck/a?!&amp;&amp;p=abc&amp;u=a1aHR0cHM6Ly9leGFtcGxlLm9yZy9i&amp;ntb=1">Redirected</a></h2>
          <p>Second snippet</p></li>
        "#;
        let results = parse_bing(html, 5);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].url, "https://example.com/page");
        assert!(results[0].title.contains("Example Title"));
        assert!(results[0].snippet.contains("Snippet text"));
        assert_eq!(results[1].url, "https://example.org/b");
    }

    #[test]
    fn parses_google_cse_json() {
        let value: serde_json::Value = serde_json::from_str(
            r#"{"items":[{"title":"OpenVINO GenAI","link":"https://docs.openvino.ai/genai","snippet":"Install via PyPI."}]}"#,
        )
        .unwrap();
        let results = parse_google(&value, 5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://docs.openvino.ai/genai");
        assert!(results[0].snippet.contains("PyPI"));
    }

    #[test]
    fn parses_yandex_html() {
        let html = r#"
        <li class="serp-item">
          <div class="organic">
            <a class="OrganicTitle-Link" href="https://example.com/a">Пример <b>заголовка</b></a>
            <div class="OrganicText"><span class="OrganicTextContentSpan">Описание результата.</span></div>
          </div>
        </li>
        "#;
        let results = parse_yandex(html, 5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://example.com/a");
        assert_eq!(results[0].title, "Пример заголовка");
        assert!(results[0].snippet.contains("Описание результата"));
    }

    #[test]
    fn parses_yahoo_results_and_redirects() {
        let html = r#"
        <div class="dd algo algo-sr relsrch richAlgo">
          <div class="compTitle"><a class="d-ib" href="https://r.search.yahoo.com/_ylt=A2;RU=https%3a%2f%2fdocs.openvino.ai%2fgenai/RK=2/RS=x">
            <h3 class="title"><span>OpenVINO GenAI</span></h3></a></div>
          <div class="compText"><p class="lh-16">Install <b>OpenVINO</b> GenAI via PyPI.</p></div>
        </div>
        <div class="dd algo algo-sr relsrch richAlgo">
          <a href="https://r.search.yahoo.com/_ylt=B;RU=https%3a%2f%2fgithub.com%2fopenvinotoolkit%2fopenvino.genai/RK=1/RS=y">
            <h3>GitHub - openvinotoolkit/openvino.genai</h3></a>
          <p>Examples and demos.</p>
        </div>
        "#;
        let results = parse_yahoo(html, 5);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].url, "https://docs.openvino.ai/genai");
        assert_eq!(results[0].title, "OpenVINO GenAI");
        assert!(results[0].snippet.contains("via PyPI"));
        assert_eq!(results[1].url, "https://github.com/openvinotoolkit/openvino.genai");
    }

    #[test]
    fn parses_wikipedia_json() {
        let value: serde_json::Value = serde_json::from_str(
            r#"{"query":{"search":[{"title":"OpenVINO","snippet":"<span class=\"searchmatch\">OpenVINO</span> — набор инструментов для инференса"}]}}"#,
        )
        .unwrap();
        let results = parse_wikipedia(&value, "ru", 5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://ru.wikipedia.org/wiki/OpenVINO");
        assert!(results[0].snippet.contains("набор инструментов"));
    }

    #[test]
    fn parses_stackexchange_json() {
        let value: serde_json::Value = serde_json::from_str(
            r#"{"items":[{"title":"How to read a file &amp; close it?","link":"https://stackoverflow.com/q/1","body":"<p>Use <code>with open()</code> and it closes automatically.</p>"}]}"#,
        )
        .unwrap();
        let results = parse_stackexchange(&value, 5);
        assert_eq!(results.len(), 1);
        assert!(results[0].title.contains("How to read a file & close it?"));
        assert!(results[0].snippet.contains("with open()"));
        assert_eq!(results[0].url, "https://stackoverflow.com/q/1");
    }

    #[test]
    fn parses_github_json() {
        let value: serde_json::Value = serde_json::from_str(
            r#"{"items":[{"full_name":"openvinotoolkit/openvino","html_url":"https://github.com/openvinotoolkit/openvino","description":"OpenVINO toolkit"}]}"#,
        )
        .unwrap();
        let results = parse_github(&value, 5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "openvinotoolkit/openvino");
        assert_eq!(results[0].url, "https://github.com/openvinotoolkit/openvino");
    }

    #[test]
    fn parses_ddg_results() {
        let results = parse_ddg(FIXTURE, 5);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].url, "https://docs.openvino.ai/genai");
        assert!(results[0].title.contains("OpenVINO GenAI"));
        assert!(results[0].snippet.contains("Run LLM locally"));
        assert_eq!(results[1].url, "https://github.com/openvinotoolkit/openvino.genai");
    }

    #[test]
    fn decodes_entities_and_urls() {
        assert_eq!(decode_entities("a &amp; b &#x41; &lt;tag&gt;"), "a & b A <tag>");
        assert_eq!(
            unwrap_ddg_url("//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fa%3Fb%3D1&rut=z"),
            "https://example.com/a?b=1"
        );
    }

    #[test]
    fn private_hosts_are_blocked() {
        for host in ["localhost", "127.0.0.1", "10.1.2.3", "192.168.0.1", "172.20.0.1", "169.254.1.1", "::1", "printer.local"] {
            assert!(host_is_private(host), "{host} должен быть заблокирован");
        }
        for host in ["example.com", "8.8.8.8", "docs.openvino.ai"] {
            assert!(!host_is_private(host), "{host} должен быть разрешён");
        }
    }

    #[test]
    fn strips_html_and_scripts() {
        let html = "<html><script>var x=1;</script><style>.a{}</style><p>Привет&nbsp;мир</p></html>";
        let text = extract_readable_text(html);
        assert!(!text.contains("var x"), "{text}");
        assert!(text.contains("Привет мир"), "{text}");
    }

    #[test]
    fn query_is_cleaned() {
        assert_eq!(query_from_user("Найди: погода в Москве?"), "погода в Москве");
        assert_eq!(query_from_user("  просто вопрос  "), "просто вопрос");
    }

    #[test]
    fn short_replies_do_not_trigger_search() {
        assert!(!should_search("привет"));
        assert!(!should_search("спасибо!"));
        assert!(should_search("What is the latest OpenVINO GenAI release?"));
        assert!(should_search("Найди последнюю версию OpenVINO GenAI"));
    }

    #[test]
    fn prompt_block_has_links() {
        let results = vec![WebResult {
            title: "T".into(),
            url: "https://example.com".into(),
            snippet: "S".into(),
        }];
        let block = prompt_block("q", &results);
        assert!(block.contains("https://example.com"));
        assert!(block.contains("[1]"));
    }
}
