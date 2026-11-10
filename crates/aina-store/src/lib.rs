// Persistent settings in %APPDATA%\aina\config.json, plus aina's own two secrets in the Windows
// Credential Manager (service "aina"): the aina Cloud account token and the browser extension's pairing
// code.
//
// aina keeps no provider API keys at all: aina Cloud holds the Anthropic, OpenAI, ElevenLabs and
// Composio keys, and the app sends its account token in their place.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use serde_json::{Map, Value};

pub mod legacy;

pub const SECRET_KEYS: [&str; 2] = ["extensionToken", "ainaCloudToken"];

/// Provider keys, held by aina Cloud only: asking for one gives the account token (or nothing when
/// signed out), and Settings can't save one.
pub const CLOUD_ROUTED: [&str; 4] = ["anthropicApiKey", "openaiApiKey", "elevenlabsApiKey", "composioApiKey"];

const DEFAULTS: &str = include_str!("defaults.json");
const SERVICE: &str = "aina";

/// Where aina keeps its data: %APPDATA%\aina, or AINA_PROFILE for experiments that must not touch the
/// real config.
pub fn data_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("AINA_PROFILE") {
        return PathBuf::from(p);
    }
    let appdata = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    appdata.join("aina")
}

/// Brings over what aina 1.0.0 and 1.0.1 kept under the Badi names (%APPDATA%\BADI, Credential Manager
/// service "BADI"), once, before the Store opens (legacy.rs). Not for an AINA_PROFILE experiment.
pub fn move_from_badi() -> Option<legacy::Moved> {
    if std::env::var_os("AINA_PROFILE").is_some() {
        return None;
    }
    let appdata = PathBuf::from(std::env::var_os("APPDATA")?);
    legacy::move_once(&appdata.join("BADI"), &data_dir(), vault::get_in, vault::set)
}

pub struct Store {
    dir: PathBuf,
    // Plain settings: defaults, then config.json on top.
    config: Mutex<Map<String, Value>>,
    // Secrets read once from the Credential Manager.
    secrets: Mutex<HashMap<String, String>>,
}

impl Store {
    pub fn open(dir: PathBuf) -> Store {
        let mut config = defaults();
        let on_disk = read_json(&dir.join("config.json"));
        for (k, v) in on_disk {
            // Keys never live in the file (secrets are in the Credential Manager, provider keys on
            // aina Cloud); one found there is left as it is, unread.
            if SECRET_KEYS.contains(&k.as_str()) || CLOUD_ROUTED.contains(&k.as_str()) {
                continue;
            }
            config.insert(k, v);
        }
        // v2 of the config: the Brain/agent default moved from Opus 5.5 to Sonnet 5.5. The move is
        // saved, so it happens once and a later choice of Opus stays.
        let mut migrated = Map::new();
        if config.get("configVersion").and_then(Value::as_u64).unwrap_or(1) < 2 {
            if config.get("brainModel").and_then(Value::as_str) == Some("claude-opus-5-5") {
                migrated.insert("brainModel".into(), "claude-sonnet-5-5".into());
            }
            migrated.insert("configVersion".into(), 2.into());
        }

        let mut secrets = HashMap::new();
        for k in SECRET_KEYS {
            if let Some(v) = vault::get(k) {
                secrets.insert(k.to_string(), v);
            }
        }
        let store = Store { dir, config: Mutex::new(config), secrets: Mutex::new(secrets) };
        if !migrated.is_empty() {
            let _ = store.remember(&migrated);
        }
        store
    }

    pub fn file(&self) -> PathBuf {
        self.dir.join("config.json")
    }

    pub fn get(&self, key: &str) -> Value {
        self.config.lock().unwrap().get(key).cloned().unwrap_or(Value::Null)
    }

    /// A key. The provider keys are the aina Cloud account token (the cloud holds the real ones), and
    /// None while signed out.
    pub fn secret(&self, key: &str) -> Option<String> {
        let secrets = self.secrets.lock().unwrap();
        if CLOUD_ROUTED.contains(&key) {
            return secrets.get("ainaCloudToken").cloned();
        }
        secrets.get(key).cloned()
    }

    /// Removes provider keys an earlier v2 build saved in the Credential Manager.
    pub fn forget_provider_keys(&self) {
        for k in CLOUD_ROUTED {
            let _ = vault::delete(k);
        }
    }

    /// The user's own key, never the cloud token (for showing which keys are set).
    pub fn own_secret(&self, key: &str) -> Option<String> {
        self.secrets.lock().unwrap().get(key).cloned()
    }

    /// Settings sent to the UI never include raw keys, only whether they are set.
    pub fn public_view(&self) -> Value {
        let mut view = self.config.lock().unwrap().clone();
        let secrets = self.secrets.lock().unwrap();
        for k in SECRET_KEYS.iter().chain(CLOUD_ROUTED.iter()) {
            view.insert((*k).into(), "".into());
            view.insert(format!("{k}Set"), secrets.contains_key(*k).into());
            view.insert(format!("{k}Demo"), false.into());
        }
        view.insert("demoUntil".into(), "".into());
        Value::Object(view)
    }

    /// Applies a patch from Settings: keys go to the Credential Manager, everything else to config.json.
    /// An empty key means "unchanged".
    pub fn save(&self, patch: &Map<String, Value>) -> Result<Value, String> {
        let mut plain = Map::new();
        for (k, v) in patch {
            if CLOUD_ROUTED.contains(&k.as_str()) {
                continue; // never kept on this PC
            }
            if SECRET_KEYS.contains(&k.as_str()) {
                let s = v.as_str().unwrap_or("").trim();
                if !s.is_empty() {
                    vault::set(k, s)?;
                    self.secrets.lock().unwrap().insert(k.clone(), s.to_string());
                }
            } else {
                plain.insert(k.clone(), v.clone());
            }
        }
        self.remember(&plain)?;
        Ok(self.public_view())
    }

    /// Forgets one key (Settings → clear).
    pub fn clear_key(&self, key: &str) -> Result<(), String> {
        if !SECRET_KEYS.contains(&key) {
            return Err(format!("not a key: {key}"));
        }
        vault::delete(key)?;
        self.secrets.lock().unwrap().remove(key);
        Ok(())
    }

    /// Updates plain fields on disk. The file is read again first so fields this version doesn't know
    /// are kept as they are.
    pub fn remember(&self, patch: &Map<String, Value>) -> Result<(), String> {
        if patch.is_empty() {
            return Ok(());
        }
        let mut cfg = self.config.lock().unwrap();
        let mut on_disk = read_json(&self.file());
        for (k, v) in patch {
            if SECRET_KEYS.contains(&k.as_str()) || CLOUD_ROUTED.contains(&k.as_str()) {
                continue;
            }
            cfg.insert(k.clone(), v.clone());
            on_disk.insert(k.clone(), v.clone());
        }
        fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        let text = serde_json::to_string_pretty(&Value::Object(on_disk)).map_err(|e| e.to_string())?;
        fs::write(self.file(), text).map_err(|e| e.to_string())
    }
}

fn defaults() -> Map<String, Value> {
    match serde_json::from_str(DEFAULTS) {
        Ok(Value::Object(m)) => m,
        _ => Map::new(),
    }
}

fn read_json(path: &PathBuf) -> Map<String, Value> {
    fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| match v {
            Value::Object(m) => Some(m),
            _ => None,
        })
        .unwrap_or_default()
}

// The Windows Credential Manager. Elsewhere (tests on CI runners without it) secrets live only in memory.
#[cfg(windows)]
mod vault {
    use super::SERVICE;

    fn entry(key: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(SERVICE, key).map_err(|e| e.to_string())
    }

    pub fn get(key: &str) -> Option<String> {
        get_in(SERVICE, key)
    }

    /// A secret under another service (the Badi names' "BADI", for the move).
    pub fn get_in(service: &str, key: &str) -> Option<String> {
        keyring::Entry::new(service, key).ok()?.get_password().ok().filter(|v| !v.is_empty())
    }

    pub fn set(key: &str, value: &str) -> Result<(), String> {
        entry(key)?.set_password(value).map_err(|e| e.to_string())
    }

    pub fn delete(key: &str) -> Result<(), String> {
        match entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

#[cfg(not(windows))]
mod vault {
    pub fn get(_: &str) -> Option<String> {
        None
    }
    pub fn get_in(_: &str, _: &str) -> Option<String> {
        None
    }
    pub fn set(_: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
    pub fn delete(_: &str) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("aina-store-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn reads_the_config_and_keeps_what_it_does_not_know() {
        let dir = temp_dir("config");
        fs::write(dir.join("config.json"), r#"{ "language": "en", "anthropicApiKey": "enc:AAAA", "configVersion": 1, "brainModel": "claude-opus-5-5", "futureField": 7 }"#).unwrap();
        let s = Store::open(dir.clone());
        assert_eq!(s.get("language"), "en");
        assert_eq!(s.get("brainModel"), "claude-sonnet-5-5");
        assert_eq!(s.get("tts"), "elevenlabs"); // a default
        let view = s.public_view();
        assert_eq!(view["anthropicApiKey"], "");
        assert_eq!(view["anthropicApiKeySet"], false);

        let mut patch = Map::new();
        patch.insert("language".into(), "id".into());
        s.remember(&patch).unwrap();
        let on_disk = read_json(&dir.join("config.json"));
        assert_eq!(on_disk["language"], "id");
        assert_eq!(on_disk["anthropicApiKey"], "enc:AAAA"); // a key found in the file is left, unread
        assert_eq!(on_disk["futureField"], 7);
        // The migration was saved: choosing Opus again survives the next start.
        assert_eq!(on_disk["configVersion"], 2);
        let mut opus = Map::new();
        opus.insert("brainModel".into(), "claude-opus-5-5".into());
        s.remember(&opus).unwrap();
        assert_eq!(Store::open(dir).get("brainModel"), "claude-opus-5-5");
    }

    #[test]
    fn provider_keys_are_never_kept_and_are_the_cloud_token_when_signed_in() {
        let dir = temp_dir("keys");
        let s = Store::open(dir.clone());
        s.secrets.lock().unwrap().clear(); // signed out here, whatever this PC's Credential Manager holds
        let mut patch = Map::new();
        patch.insert("anthropicApiKey".into(), "sk-ant-x".into());
        patch.insert("language".into(), "en".into());
        s.save(&patch).unwrap();
        assert_eq!(s.secret("anthropicApiKey"), None);
        assert!(!fs::read_to_string(dir.join("config.json")).unwrap().contains("sk-ant"));
        // Signed in (set in memory here; the real token lives in the Credential Manager).
        s.secrets.lock().unwrap().insert("ainaCloudToken".into(), "bct_1".into());
        assert_eq!(s.secret("openaiApiKey").as_deref(), Some("bct_1"));
        assert_eq!(s.secret("composioApiKey").as_deref(), Some("bct_1"));
    }
}
