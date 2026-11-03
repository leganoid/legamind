//! Opt-in облачное хранение: WebDAV + AES-256-GCM с парольной фразой.
//! Парольная фраза не сохраняется; без неё копию не расшифровать.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{bail, Result};
use argon2::Argon2;
use serde::Serialize;
use tauri::{AppHandle, State};

use crate::AppState;

const ENABLED_KEY: &str = "cloud_enabled";
const URL_KEY: &str = "cloud_url";
const USER_KEY: &str = "cloud_user";
const SECRET_PROVIDER: &str = "webdav";
const MAGIC: &[u8] = b"LMCLOUD1";
const REMOTE_NAME: &str = "legamind-backup.enc";

#[derive(Debug, Clone, Serialize)]
pub struct CloudConfig {
    pub enabled: bool,
    pub url: String,
    pub user: String,
    pub has_password: bool,
}

fn config(state: &AppState) -> Result<CloudConfig> {
    Ok(CloudConfig {
        enabled: state
            .db
            .setting_get(ENABLED_KEY)?
            .map(|value| value == "1")
            .unwrap_or(false),
        url: state.db.setting_get(URL_KEY)?.unwrap_or_default(),
        user: state.db.setting_get(USER_KEY)?.unwrap_or_default(),
        has_password: crate::secrets::has_key(state, SECRET_PROVIDER),
    })
}

fn remote_url(cfg: &CloudConfig) -> String {
    format!("{}/{}", cfg.url.trim_end_matches('/'), REMOTE_NAME)
}

fn derive_key(passphrase: &str, salt: &[u8]) -> Result<[u8; 32]> {
    let mut key = [0u8; 32];
    Argon2::default()
        .hash_password_into(passphrase.as_bytes(), salt, &mut key)
        .map_err(|e| anyhow::anyhow!("Argon2: {e}"))?;
    Ok(key)
}

fn encrypt_backup(plain: &[u8], passphrase: &str) -> Result<Vec<u8>> {
    let salt = uuid::Uuid::new_v4().into_bytes();
    let nonce_bytes = uuid::Uuid::new_v4().into_bytes();
    let nonce = Nonce::from_slice(&nonce_bytes[..12]);
    let key = derive_key(passphrase, &salt)?;
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| anyhow::anyhow!("AES: {e}"))?;
    let ciphertext = cipher
        .encrypt(nonce, plain)
        .map_err(|e| anyhow::anyhow!("AES encrypt: {e}"))?;
    let mut out = Vec::with_capacity(MAGIC.len() + salt.len() + 12 + ciphertext.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&salt);
    out.extend_from_slice(&nonce_bytes[..12]);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

fn decrypt_backup(data: &[u8], passphrase: &str) -> Result<Vec<u8>> {
    if !data.starts_with(MAGIC) {
        bail!("Файл не является копией Legamind");
    }
    let rest = &data[MAGIC.len()..];
    if rest.len() < 16 + 12 {
        bail!("Повреждённая копия");
    }
    let salt = &rest[..16];
    let nonce = Nonce::from_slice(&rest[16..28]);
    let key = derive_key(passphrase, salt)?;
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| anyhow::anyhow!("AES: {e}"))?;
    cipher
        .decrypt(nonce, &rest[28..])
        .map_err(|_| anyhow::anyhow!("Неверная парольная фраза или повреждённая копия"))
}

fn auth(cfg: &CloudConfig, state: &AppState, request: reqwest::RequestBuilder) -> Result<reqwest::RequestBuilder> {
    let password = crate::secrets::get_key(state, SECRET_PROVIDER)?
        .ok_or_else(|| anyhow::anyhow!("Пароль WebDAV не задан"))?;
    Ok(request.basic_auth(&cfg.user, Some(password)))
}

#[tauri::command]
pub fn cloud_get(state: State<'_, AppState>) -> Result<CloudConfig, String> {
    config(&state).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cloud_set(
    state: State<'_, AppState>,
    enabled: bool,
    url: String,
    user: String,
    password: Option<String>,
) -> Result<CloudConfig, String> {
    let url = url.trim().to_string();
    if enabled && !url.starts_with("https://") && !url.starts_with("http://") {
        return Err("URL WebDAV должен начинаться с http:// или https://".into());
    }
    state
        .db
        .setting_set(ENABLED_KEY, if enabled { "1" } else { "0" })
        .map_err(|e| e.to_string())?;
    state
        .db
        .setting_set(URL_KEY, &url)
        .map_err(|e| e.to_string())?;
    state
        .db
        .setting_set(USER_KEY, user.trim())
        .map_err(|e| e.to_string())?;
    if let Some(password) = password {
        if !password.is_empty() {
            crate::secrets::set_key(&state, SECRET_PROVIDER, &password)
                .map_err(|e| e.to_string())?;
        }
    }
    config(&state).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn cloud_upload(
    state: State<'_, AppState>,
    passphrase: String,
) -> Result<String, String> {
    if passphrase.chars().count() < 8 {
        return Err("Парольная фраза должна быть не короче 8 символов".into());
    }
    let cfg = config(&state).map_err(|e| e.to_string())?;
    if !cfg.enabled || cfg.url.trim().is_empty() {
        return Err("Облачное хранение выключено или URL не задан".into());
    }
    let tmp = state.data_dir.join("cloud-upload.db");
    state.db.backup_to(&tmp).map_err(|e| e.to_string())?;
    let plain = std::fs::read(&tmp).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&tmp);
    let encrypted = encrypt_backup(&plain, &passphrase).map_err(|e| e.to_string())?;
    let request = state
        .client
        .put(remote_url(&cfg))
        .header("Content-Type", "application/octet-stream")
        .body(encrypted)
        .timeout(std::time::Duration::from_secs(120));
    let request = auth(&cfg, &state, request).map_err(|e| e.to_string())?;
    let resp = request.send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("WebDAV вернул {}", resp.status()));
    }
    let _ = crate::logging::log_line("app.log", "cloud backup uploaded");
    Ok(format!("Копия загружена: {}", remote_url(&cfg)))
}

#[tauri::command]
pub async fn cloud_restore(
    app: AppHandle,
    state: State<'_, AppState>,
    passphrase: String,
) -> Result<String, String> {
    let cfg = config(&state).map_err(|e| e.to_string())?;
    if cfg.url.trim().is_empty() {
        return Err("URL WebDAV не задан".into());
    }
    let request = state
        .client
        .get(remote_url(&cfg))
        .timeout(std::time::Duration::from_secs(120));
    let request = auth(&cfg, &state, request).map_err(|e| e.to_string())?;
    let resp = request.send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("WebDAV вернул {}", resp.status()));
    }
    let data = resp.bytes().await.map_err(|e| e.to_string())?;
    let plain = decrypt_backup(&data, &passphrase).map_err(|e| e.to_string())?;
    let tmp = state.data_dir.join("cloud-restore.db");
    std::fs::write(&tmp, plain).map_err(|e| e.to_string())?;
    let safety = state
        .data_dir
        .join("backups")
        .join(format!("before-cloud-restore-{}.db", chrono::Local::now().format("%Y%m%d-%H%M%S")));
    crate::backup::write_encrypted_backup(&state.db, &safety).map_err(|e| e.to_string())?;
    state.db.restore_from(&tmp).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&tmp);
    let _ = crate::logging::log_line("app.log", "cloud backup restored");
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        handle.restart();
    });
    Ok(format!(
        "Копия восстановлена. Копия текущей базы: {}",
        safety.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let data = b"sqlite-test-data";
        let encrypted = encrypt_backup(data, "correct horse battery").unwrap();
        assert_ne!(encrypted, data);
        let decrypted = decrypt_backup(&encrypted, "correct horse battery").unwrap();
        assert_eq!(decrypted, data);
        assert!(decrypt_backup(&encrypted, "wrong passphrase").is_err());
    }
}
