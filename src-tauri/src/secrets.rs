//! Хранение API-ключей: Windows Credential Manager через `keyring`,
//! fallback — DPAPI (`CryptProtectData`) с шифроблоком в таблице `settings`.
//!
//! Открытый ключ никогда не попадает в SQLite, логи, события UI или сайдкар.

use anyhow::{anyhow, bail, Context, Result};

use crate::AppState;

const SERVICE: &str = "legamind";

fn account(provider_id: &str) -> String {
    format!("provider:{provider_id}")
}

fn dpapi_setting_key(provider_id: &str) -> String {
    format!("secret_dpapi:{provider_id}")
}

fn entry(provider_id: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, &account(provider_id)).context("Credential Manager недоступен")
}

pub fn set_key(state: &AppState, provider_id: &str, key: &str) -> Result<()> {
    let key = key.trim();
    if key.is_empty() {
        bail!("Пустой ключ");
    }
    match entry(provider_id).and_then(|e| e.set_password(key).map_err(|e| anyhow!(e.to_string()))) {
        Ok(()) => {
            // Если раньше был DPAPI-фолбэк, убираем дубль.
            let _ = state.db.setting_remove(&dpapi_setting_key(provider_id));
            Ok(())
        }
        Err(err) => {
            // Fallback: DPAPI, шифроблок в settings. Открытый текст на диск не попадает.
            let blob = dpapi::protect(key.as_bytes()).map_err(|e| {
                anyhow!("Не удалось сохранить ключ: Credential Manager ({err}), DPAPI ({e})")
            })?;
            state
                .db
                .setting_set(&dpapi_setting_key(provider_id), &dpapi::encode(&blob))?;
            Ok(())
        }
    }
}

pub fn get_key(state: &AppState, provider_id: &str) -> Result<Option<String>> {
    if let Ok(entry) = entry(provider_id) {
        match entry.get_password() {
            Ok(secret) => return Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => {}
            Err(_) => {}
        }
    }
    match state.db.setting_get(&dpapi_setting_key(provider_id))? {
        Some(encoded) if !encoded.is_empty() => {
            let blob = dpapi::decode(&encoded)?;
            let plain = dpapi::unprotect(&blob)?;
            Ok(Some(String::from_utf8(plain).context("DPAPI: не UTF-8")?))
        }
        _ => Ok(None),
    }
}

pub fn has_key(state: &AppState, provider_id: &str) -> bool {
    get_key(state, provider_id).ok().flatten().is_some()
}

pub fn delete_key(state: &AppState, provider_id: &str) -> Result<()> {
    if let Ok(entry) = entry(provider_id) {
        // NoEntry — нормальная ситуация, ключа могло не быть.
        let _ = entry.delete_credential();
    }
    state.db.setting_remove(&dpapi_setting_key(provider_id))?;
    Ok(())
}

#[cfg(windows)]
pub(crate) mod dpapi {
    use anyhow::{bail, Result};
    use std::ptr;
    use winapi::shared::minwindef::DWORD;
    use winapi::um::dpapi::{CryptProtectData, CryptUnprotectData};
    use winapi::um::wincrypt::DATA_BLOB;
    use winapi::um::winbase::LocalFree;

    /// Флаг «без UI» (winapi не экспортирует его в используемом наборе фич).
    const CRYPTPROTECT_UI_FORBIDDEN: DWORD = 0x1;

    fn blob(data: &[u8]) -> DATA_BLOB {
        DATA_BLOB {
            cbData: data.len() as DWORD,
            pbData: data.as_ptr() as *mut u8,
        }
    }

    fn take(out: DATA_BLOB) -> Vec<u8> {
        if out.pbData.is_null() {
            return vec![];
        }
        let bytes = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec() };
        unsafe { LocalFree(out.pbData as *mut _) };
        bytes
    }

    pub fn protect(data: &[u8]) -> Result<Vec<u8>> {
        unsafe {
            let mut input = blob(data);
            let mut output = DATA_BLOB { cbData: 0, pbData: ptr::null_mut() };
            let ok = CryptProtectData(
                &mut input,
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            );
            if ok == 0 {
                bail!("CryptProtectData: {}", std::io::Error::last_os_error());
            }
            Ok(take(output))
        }
    }

    pub fn unprotect(data: &[u8]) -> Result<Vec<u8>> {
        unsafe {
            let mut input = blob(data);
            let mut output = DATA_BLOB { cbData: 0, pbData: ptr::null_mut() };
            let ok = CryptUnprotectData(
                &mut input,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            );
            if ok == 0 {
                bail!("CryptUnprotectData: {}", std::io::Error::last_os_error());
            }
            Ok(take(output))
        }
    }

    pub fn encode(bytes: &[u8]) -> String {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    pub fn decode(text: &str) -> Result<Vec<u8>> {
        use base64::Engine;
        Ok(base64::engine::general_purpose::STANDARD.decode(text)?)
    }
}

#[cfg(not(windows))]
pub(crate) mod dpapi {
    use anyhow::{bail, Result};
    pub fn protect(_: &[u8]) -> Result<Vec<u8>> {
        bail!("DPAPI доступен только на Windows")
    }
    pub fn unprotect(_: &[u8]) -> Result<Vec<u8>> {
        bail!("DPAPI доступен только на Windows")
    }
    pub fn encode(_: &[u8]) -> String {
        String::new()
    }
    pub fn decode(_: &str) -> Result<Vec<u8>> {
        Ok(vec![])
    }
}
