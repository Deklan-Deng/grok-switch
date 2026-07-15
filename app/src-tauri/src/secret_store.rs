//! Local token vault (CC Switch style).
//!
//! Tokens live in the app data directory as a restricted JSON file:
//!   macOS: ~/Library/Application Support/GrokTokenSwitcher/tokens.json
//!
//! Directory name kept as `GrokTokenSwitcher` for backward compatibility with
//! earlier local builds. Display product name is **Grok Switch**.
//!
//! On first use we one-shot migrate any leftover Keychain entries (legacy)
//! so existing users don't lose secrets, then stop using Keychain for new ops.

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use thiserror::Error;
use uuid::Uuid;

const LEGACY_KEYCHAIN_SERVICE: &str = "local.groktokenswitcher.xai";

#[derive(Debug, Error)]
pub enum SecretError {
    #[error("本地密钥存储失败：{0}")]
    Io(#[from] std::io::Error),
    #[error("本地密钥解析失败：{0}")]
    Json(#[from] serde_json::Error),
    #[error("本地密钥存储锁损坏")]
    Lock,
}

fn vault_path() -> PathBuf {
    let base = dirs::data_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("GrokTokenSwitcher").join("tokens.json")
}

fn migration_marker_path() -> PathBuf {
    let base = dirs::data_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("GrokTokenSwitcher").join(".keychain-migrated")
}

fn read_map() -> Result<HashMap<String, String>, SecretError> {
    let path = vault_path();
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let raw = fs::read_to_string(&path)?;
    if raw.trim().is_empty() {
        return Ok(HashMap::new());
    }
    Ok(serde_json::from_str(&raw)?)
}

fn write_map(map: &HashMap<String, String>) -> Result<(), SecretError> {
    let path = vault_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let raw = serde_json::to_string_pretty(map)?;
    let tmp = path.with_extension("json.tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(raw.as_bytes())?;
        f.sync_all()?;
    }
    fs::rename(&tmp, &path)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn load_profile_ids_from_disk() -> Option<Vec<String>> {
    let base = dirs::data_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."));
    let path = base.join("GrokTokenSwitcher").join("profiles.json");
    let raw = fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let mut ids = Vec::new();
    let arr = v
        .get("profiles")
        .and_then(|p| p.as_array())
        .or_else(|| v.as_array());
    if let Some(arr) = arr {
        for item in arr {
            if let Some(id) = item.get("id").and_then(|x| x.as_str()) {
                ids.push(id.to_string());
            }
        }
    }
    Some(ids)
}

fn keychain_load(account: &str) -> Option<String> {
    use keyring::Entry;
    let entry = Entry::new(LEGACY_KEYCHAIN_SERVICE, account).ok()?;
    match entry.get_password() {
        Ok(v) => Some(v),
        Err(_) => None,
    }
}

fn keychain_delete(account: &str) {
    use keyring::Entry;
    if let Ok(entry) = Entry::new(LEGACY_KEYCHAIN_SERVICE, account) {
        let _ = entry.delete_credential();
    }
}

/// One-shot: pull any tokens still in the legacy Keychain into the local vault.
fn migrate_from_keychain_once(map: &mut HashMap<String, String>) {
    if migration_marker_path().exists() {
        return;
    }

    let mut ids: Vec<String> = map.keys().cloned().collect();
    if let Some(profiles) = load_profile_ids_from_disk() {
        for id in profiles {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }

    let mut migrated = 0usize;
    for id in &ids {
        if map.get(id).map(|s| !s.is_empty()).unwrap_or(false) {
            continue;
        }
        if let Some(token) = keychain_load(id) {
            if !token.is_empty() {
                map.insert(id.clone(), token);
                migrated += 1;
            }
        }
    }

    // Best-effort delete legacy keychain entries so macOS stops prompting.
    for id in map.keys().chain(ids.iter()) {
        keychain_delete(id);
    }

    let _ = write_map(map);
    let _ = fs::write(migration_marker_path(), format!("migrated={migrated}\n"));
}

struct Vault {
    map: HashMap<String, String>,
}

impl Vault {
    fn open() -> Result<Self, SecretError> {
        let mut map = read_map()?;
        migrate_from_keychain_once(&mut map);
        map = read_map()?;
        Ok(Self { map })
    }

    fn persist(&self) -> Result<(), SecretError> {
        write_map(&self.map)
    }
}

static VAULT: Mutex<Option<Vault>> = Mutex::new(None);

fn with_vault<T>(f: impl FnOnce(&mut Vault) -> Result<T, SecretError>) -> Result<T, SecretError> {
    let mut guard = VAULT.lock().map_err(|_| SecretError::Lock)?;
    if guard.is_none() {
        *guard = Some(Vault::open()?);
    }
    f(guard.as_mut().expect("vault just initialized"))
}

pub fn save_token(id: Uuid, token: &str) -> Result<(), SecretError> {
    with_vault(|v| {
        let key = id.to_string();
        if token.is_empty() {
            v.map.remove(&key);
        } else {
            v.map.insert(key.clone(), token.to_string());
        }
        keychain_delete(&key);
        v.persist()
    })
}

pub fn load_token(id: Uuid) -> Result<Option<String>, SecretError> {
    with_vault(|v| {
        let key = id.to_string();
        if let Some(t) = v.map.get(&key).cloned() {
            if !t.is_empty() {
                return Ok(Some(t));
            }
        }
        // Fallback: promote leftover keychain entry once.
        if let Some(token) = keychain_load(&key) {
            if !token.is_empty() {
                v.map.insert(key.clone(), token.clone());
                let _ = v.persist();
                keychain_delete(&key);
                return Ok(Some(token));
            }
        }
        Ok(None)
    })
}

pub fn delete_token(id: Uuid) -> Result<(), SecretError> {
    with_vault(|v| {
        let key = id.to_string();
        v.map.remove(&key);
        keychain_delete(&key);
        v.persist()
    })
}

pub fn has_token(id: Uuid) -> bool {
    matches!(load_token(id), Ok(Some(v)) if !v.is_empty())
}
