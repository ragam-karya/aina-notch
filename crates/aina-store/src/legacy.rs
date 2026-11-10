// The move from the Badi names (aina 1.0.0 and 1.0.1): their data lived in %APPDATA%\BADI and their
// secrets under the Credential Manager service "BADI" (the account token as badiCloudToken). Once, before
// the Store opens, everything there that %APPDATA%\aina doesn't have yet is copied over: the settings,
// the agents' workspaces (with the paths inside their JSON files pointed at the new folder), the
// browser profile; and the account token and the extension's pairing code go to the "aina" service, so
// an update keeps the person signed in with their agents and settings. Nothing old is deleted (going
// back to 1.0.1 still works); a marker file in the new folder makes it happen only once.
//
// As an MSIX package Windows keeps AppData private to the package, so a Store build finds there what
// the Store's 1.0.x build left, as it should.

use std::fs;
use std::io;
use std::path::Path;

use serde_json::{Map, Value};

/// Written in the new folder once the move is done.
pub const MARKER: &str = ".moved-from-badi";
/// Left behind: the old log, and the WebView2 cache, which the app makes again.
const SKIP: [&str; 2] = ["badi.log", "webview"];
/// (old service, old key) → new key, under the new service.
pub const SECRETS: [(&str, &str, &str); 2] = [("BADI", "badiCloudToken", "ainaCloudToken"), ("BADI", "extensionToken", "extensionToken")];

/// What the move did, for the log.
#[derive(Debug, Default, PartialEq)]
pub struct Moved {
    pub files: usize,
    pub settings: usize,
    pub secrets: usize,
}

/// Moves `old` into `new` once (see the top of this file). `secret_get(service, key)` and
/// `secret_set(key, value)` (under the new service) reach the Credential Manager. None: nothing to do
/// (no old folder, or already done).
pub fn move_once(old: &Path, new: &Path, secret_get: impl Fn(&str, &str) -> Option<String>, secret_set: impl Fn(&str, &str) -> Result<(), String>) -> Option<Moved> {
    if !old.is_dir() || new.join(MARKER).exists() {
        return None;
    }
    let mut moved = Moved::default();
    if fs::create_dir_all(new).is_err() {
        return None;
    }
    // Settings: the old ones under what the new folder already has (a first run of the new build may
    // have written a few).
    let old_cfg = read_map(&old.join("config.json"));
    if !old_cfg.is_empty() {
        let mut cfg = read_map(&new.join("config.json"));
        for (k, v) in old_cfg {
            if !cfg.contains_key(&k) {
                cfg.insert(k, retarget(v, old, new));
                moved.settings += 1;
            }
        }
        if moved.settings > 0 && serde_json::to_string_pretty(&Value::Object(cfg)).ok().and_then(|t| fs::write(new.join("config.json"), t).ok()).is_none() {
            return None;
        }
    }
    // Everything else, file by file, never over a file the new folder has.
    if let Ok(entries) = fs::read_dir(old) {
        for e in entries.flatten() {
            let name = e.file_name();
            let name = name.to_string_lossy();
            if name == "config.json" || SKIP.contains(&name.as_ref()) {
                continue;
            }
            match copy_tree(&e.path(), &new.join(name.as_ref()), old, new) {
                Ok(n) => moved.files += n,
                Err(_) => return None, // tried again on the next start
            }
        }
    }
    for (service, key, new_key) in SECRETS {
        if secret_get("aina", new_key).is_none()
            && let Some(v) = secret_get(service, key)
            && secret_set(new_key, &v).is_ok()
        {
            moved.secrets += 1;
        }
    }
    let _ = fs::write(new.join(MARKER), format!("{} files, {} settings, {} secrets\n", moved.files, moved.settings, moved.secrets));
    Some(moved)
}

/// Copies a file or a folder; a JSON file's paths into `old` now point into `new`. Answers the files
/// copied.
fn copy_tree(from: &Path, to: &Path, old: &Path, new: &Path) -> io::Result<usize> {
    if from.is_dir() {
        fs::create_dir_all(to)?;
        let mut n = 0;
        for e in fs::read_dir(from)?.flatten() {
            n += copy_tree(&e.path(), &to.join(e.file_name()), old, new)?;
        }
        return Ok(n);
    }
    if to.exists() {
        return Ok(0);
    }
    let is_json = from.extension().is_some_and(|x| x.eq_ignore_ascii_case("json"));
    match (is_json, fs::read_to_string(from)) {
        (true, Ok(text)) => fs::write(to, retarget_text(&text, old, new))?,
        _ => {
            fs::copy(from, to)?;
        }
    }
    Ok(1)
}

/// A setting's paths into `old` pointed into `new`.
fn retarget(v: Value, old: &Path, new: &Path) -> Value {
    match v {
        Value::String(s) => Value::String(s.replace(&*old.to_string_lossy(), &new.to_string_lossy())),
        Value::Array(a) => Value::Array(a.into_iter().map(|x| retarget(x, old, new)).collect()),
        Value::Object(m) => Value::Object(m.into_iter().map(|(k, x)| (k, retarget(x, old, new))).collect()),
        other => other,
    }
}

/// JSON text with the paths into `old` (written with \\ inside strings) pointed into `new`.
fn retarget_text(text: &str, old: &Path, new: &Path) -> String {
    let esc = |p: &Path| p.to_string_lossy().replace('\\', "\\\\");
    text.replace(&esc(old), &esc(new))
}

fn read_map(path: &Path) -> Map<String, Value> {
    fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| match v {
            Value::Object(m) => Some(m),
            _ => None,
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("aina-legacy-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn moves_settings_agents_and_secrets_once_and_keeps_the_old_folder() {
        let root = temp("move");
        let (old, new) = (root.join("BADI"), root.join("aina"));
        fs::create_dir_all(old.join("agents").join("ciro").join("output")).unwrap();
        fs::create_dir_all(old.join("webview")).unwrap();
        let out = old.join("agents").join("ciro").join("output").join("hero.html");
        fs::write(&out, "<h1>hi</h1>").unwrap();
        let agent = serde_json::json!({ "files": [out.to_string_lossy()] }).to_string();
        fs::write(old.join("agents").join("ciro").join("agent.json"), agent).unwrap();
        fs::write(old.join("webview").join("cache"), "x").unwrap();
        fs::write(old.join("badi.log"), "old log").unwrap();
        fs::write(old.join("config.json"), r#"{"brainModel":"claude-sonnet-5-5","language":"id","notesDir":"OLD"}"#.replace("OLD", &old.join("notes").to_string_lossy().replace('\\', "\\\\"))).unwrap();
        // The new build already ran once and saved a choice of its own.
        fs::create_dir_all(&new).unwrap();
        fs::write(new.join("config.json"), r#"{"language":"en"}"#).unwrap();

        let vault: RefCell<HashMap<(String, String), String>> = RefCell::new(HashMap::from([(("BADI".into(), "badiCloudToken".into()), "bct_abc".into())]));
        let get = |s: &str, k: &str| vault.borrow().get(&(s.to_string(), k.to_string())).cloned();
        let set = |k: &str, v: &str| {
            vault.borrow_mut().insert(("aina".into(), k.into()), v.into());
            Ok(())
        };

        let moved = move_once(&old, &new, get, set).unwrap();
        assert_eq!(moved, Moved { files: 2, settings: 2, secrets: 1 });
        let cfg = read_map(&new.join("config.json"));
        assert_eq!(cfg["language"], "en", "the new build's own choice stays");
        assert_eq!(cfg["brainModel"], "claude-sonnet-5-5");
        assert_eq!(cfg["notesDir"], new.join("notes").to_string_lossy().as_ref());
        let agent: Value = serde_json::from_str(&fs::read_to_string(new.join("agents").join("ciro").join("agent.json")).unwrap()).unwrap();
        assert_eq!(agent["files"][0], new.join("agents").join("ciro").join("output").join("hero.html").to_string_lossy().as_ref());
        assert!(new.join("agents").join("ciro").join("output").join("hero.html").exists());
        assert!(!new.join("webview").exists() && !new.join("badi.log").exists());
        assert_eq!(vault.borrow().get(&("aina".into(), "ainaCloudToken".into())).map(String::as_str), Some("bct_abc"));
        assert!(old.join("config.json").exists(), "nothing old is deleted");

        // Once only.
        assert_eq!(move_once(&old, &new, |_, _| None, |_, _| Ok(())), None);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_signed_in_new_build_keeps_its_token_and_nothing_old_means_nothing_to_do() {
        let root = temp("keep");
        let (old, new) = (root.join("BADI"), root.join("aina"));
        assert_eq!(move_once(&old, &new, |_, _| None, |_, _| Ok(())), None);
        fs::create_dir_all(&old).unwrap();
        let get = |s: &str, k: &str| match (s, k) {
            ("aina", "ainaCloudToken") => Some("bct_new".to_string()),
            ("BADI", "badiCloudToken") => Some("bct_old".to_string()),
            _ => None,
        };
        let moved = move_once(&old, &new, get, |_, _| panic!("the new token is kept")).unwrap();
        assert_eq!(moved.secrets, 0);
        assert!(new.join(MARKER).exists());
        let _ = fs::remove_dir_all(&root);
    }
}
