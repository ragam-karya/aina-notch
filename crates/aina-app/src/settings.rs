// What saving Settings changes right away, and aina's start and quit (v1 main.js config:set side
// effects, updateTrayTooltip, the whenReady tail and will-quit): the hotkeys, launch at login, the tray
// tooltip, the voice lines fetched ahead, the startup greeting or first-run Settings, and the browser
// extension's setup.

use std::path::PathBuf;
use std::time::Duration;

use aina_llm::persona;
use serde_json::{Map, Value, json};
use tauri::{AppHandle, Emitter, Manager};

use crate::{AppState, talk, turn, windows};

fn state(app: &AppHandle) -> tauri::State<'_, AppState> {
    app.state::<AppState>()
}

/// After Settings saved `patch`.
pub fn apply(app: &AppHandle, patch: &Map<String, Value>) {
    if patch.contains_key("pttCombo") || patch.contains_key("dictationCombo") {
        talk::configure_keys(app);
    }
    if patch.contains_key("launchAtLogin") {
        launch_at_login(state(app).store.get("launchAtLogin").as_bool() == Some(true));
    }
    if patch.contains_key("wakeWord") {
        crate::wake::update(app);
    }
    if ["tts", "elevenlabsVoice", "openaiVoice", "systemVoice", "language", "userName"].iter().any(|k| patch.contains_key(*k)) {
        prefetch(app);
    }
    update_tray(app);
}

/// "Jalan saat Windows mulai": the current user's Run key (no admin rights), pointing at this exe; the
/// package's StartupTask in the MSIX build.
#[cfg(windows)]
pub fn launch_at_login(on: bool) {
    if crate::package::packaged() {
        if let Err(e) = crate::package::set_startup(on) {
            eprintln!("[startup] {e}");
        }
        return;
    }
    use ::windows::Win32::System::Registry::{HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegSetValueExW};
    use ::windows::core::HSTRING;
    let Ok(exe) = std::env::current_exe() else { return };
    unsafe {
        let mut key = HKEY::default();
        if RegCreateKeyExW(HKEY_CURRENT_USER, &HSTRING::from(r"Software\Microsoft\Windows\CurrentVersion\Run"), None, None, REG_OPTION_NON_VOLATILE, KEY_WRITE, None, &mut key, None).is_err() {
            return;
        }
        let name = HSTRING::from("aina");
        if on {
            let value = format!("\"{}\"", exe.display());
            let data: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
            let bytes = std::slice::from_raw_parts(data.as_ptr() as *const u8, data.len() * 2);
            let _ = RegSetValueExW(key, &name, None, REG_SZ, Some(bytes));
        } else {
            let _ = RegDeleteValueW(key, &name);
        }
        let _ = RegCloseKey(key);
    }
}

#[cfg(not(windows))]
pub fn launch_at_login(_: bool) {}

/// The tray tooltip: what aina is doing, the focus folder and the talk keys (v1 updateTrayTooltip).
pub fn update_tray(app: &AppHandle) {
    let Some(tray) = app.tray_by_id("aina") else { return };
    let label = match turn::phase(app).as_str() {
        "listening" => "mendengarkan…",
        "transcribing" | "looking" => "menyalin suara…",
        "thinking" => "berpikir…",
        "speaking" => "menjawab…",
        "guiding" => "memandu…",
        "acting" => "mengerjakan…",
        "confirm" => "menunggu izinmu…",
        "asking" => "menunggu jawabanmu…",
        _ => "siap",
    };
    let combo: Vec<String> = state(app).store.get("pttCombo").as_array().into_iter().flatten().filter_map(|v| v.as_str()).map(|k| if k == "Meta" { "Win".to_string() } else { k.to_string() }).collect();
    let focus = crate::agents::current_focus(app).and_then(|f| f.file_name().map(|n| format!("\nFokus: {}", n.to_string_lossy()))).unwrap_or_default();
    let _ = tray.set_tooltip(Some(format!("aina: {label}{focus}\nTahan {} untuk bicara", combo.join("+"))));
}

/// The filler and every wake reply are fetched ahead, so they play the instant they're needed.
pub fn prefetch(app: &AppHandle) {
    let a = app.clone();
    std::thread::spawn(move || {
        let st = state(&a);
        let en = crate::i18n::lang(&a) == "en";
        let name = st.store.get("userName").as_str().unwrap_or("").to_string();
        // The out-of-credits line too. Each is fetched once per voice (free on aina Cloud) and kept on
        // disk, so a start normally asks for nothing.
        let mut lines = persona::ack_lines(en, &name);
        if crate::cloud::signed_in(&a) {
            lines.push(persona::out_line(en));
        }
        for line in lines {
            // Paused while a question is being answered, so it never competes with a real reply.
            while turn::phase(&a) != "idle" {
                std::thread::sleep(Duration::from_millis(500));
            }
            if talk::voice(&a).is_none_or(|v| matches!(v, aina_voice::tts::Voice::System { .. })) {
                return; // a Windows voice needs nothing fetched
            }
            if talk::line_clip(&a, &line).is_none() {
                return; // offline, or the cloud's daily cap: try again next time
            }
        }
    });
}

/// The browser extension's folder: bundled next to the app, or the repo's while developing.
pub fn extension_dir(app: &AppHandle) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(r) = app.path().resource_dir() {
        candidates.push(r.join("extension"));
    }
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../extension"));
    candidates.into_iter().find(|p| p.join("manifest.json").is_file()).map(|p| aina_agent::guard::real(&p))
}

/// The folder a browser loads the extension from. The MSIX package's own folder can't be opened by a
/// browser, so there it is copied to Documents\aina\extension when the person opens it (`copy`); Settings
/// only shows where that will be.
pub fn extension_for_browser(app: &AppHandle, copy: bool) -> Option<PathBuf> {
    let dir = extension_dir(app)?;
    if !crate::package::packaged() {
        return Some(dir);
    }
    if copy { crate::package::shared_extension(&dir) } else { Some(crate::package::shared_extension_dir()) }
}

/// `aina.exe --setup-extension` (the installer's shortcut): Settings opens on the extension section,
/// with its folder open beside it.
pub fn setup_extension(app: &AppHandle) {
    let fresh = app.get_webview_window("settings").is_none();
    windows::open_settings(app);
    let a = app.clone();
    std::thread::spawn(move || {
        if fresh {
            std::thread::sleep(Duration::from_millis(1500)); // the page registers its listeners first
        }
        if let Some(w) = a.get_webview_window("settings") {
            let _ = w.emit("settings:focus", "extension");
        }
        if let Some(dir) = extension_for_browser(&a, true) {
            let _ = std::process::Command::new("explorer").arg(dir).spawn();
        }
    });
}

/// After start: first run (or no way to answer yet) opens Settings; otherwise aina greets.
pub fn on_start(app: &AppHandle) {
    let st = state(app);
    // The pairing code exists from the start, so Settings always has one to show.
    if st.store.secret("extensionToken").is_none() {
        let mut patch = Map::new();
        patch.insert("extensionToken".into(), json!(crate::ipc::pairing_code()));
        let _ = st.store.save(&patch);
    }
    launch_at_login(st.store.get("launchAtLogin").as_bool() == Some(true));
    update_tray(app);
    prefetch(app);
    let kb = app.clone();
    std::thread::spawn(move || crate::knowledge::warm(&kb));
    let onboarded = st.store.get("onboarded").as_bool() == Some(true);
    if std::env::args().any(|a| a == "--setup-extension") {
        setup_extension(app);
        remember(app, json!({ "onboarded": true }));
        return;
    }
    if !onboarded {
        windows::open_settings(app);
        remember(app, json!({ "onboarded": true }));
        return;
    }
    // Signed out, the notch is free and aina stays quiet in the tray: the greeting needs aina Cloud's
    // voice, and asking aina shows the sign-in panel (cloud::require).
    if !crate::cloud::signed_in(app) {
        return;
    }
    let get_i = |k: &str| st.store.get(k).as_i64().unwrap_or(0);
    let (launches, last_seen) = (get_i("launches"), get_i("lastSeenAt"));
    let now = crate::notch::now_ms();
    let line = persona::greeting(
        &persona::Greeting {
            en: crate::i18n::lang(app) == "en",
            now_ms: now,
            hour: chrono::Timelike::hour(&chrono::Local::now()),
            launches: launches.max(0) as u64,
            last_seen_ms: last_seen,
            name: st.store.get("userName").as_str().unwrap_or(""),
            wake: st.store.get("wakeWord").as_bool() == Some(true),
        },
        persona::rand(),
    );
    remember(app, json!({ "launches": launches + 1, "lastSeenAt": now }));
    let a = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(1500)); // the overlays load first
        if turn::phase(&a) != "idle" {
            return; // the user already started something: no greeting over it
        }
        crate::notch::pin_display(&a);
        talk::say_line(&a, &line); // free, and kept on disk after the first time
        // Only the notch's "speaking" is undone; a question asked meanwhile keeps its phase.
        if turn::phase(&a) == "idle" {
            windows::broadcast(&a, "linger", 6000);
            turn::set_phase(&a, "idle");
        }
    });
}

/// Where an installed aina writes its error lines (main.rs log_to_file).
pub fn log_path() -> PathBuf {
    aina_store::data_dir().join("aina.log")
}

/// On quit: when aina was last running, for the next greeting.
pub fn on_quit(app: &AppHandle) {
    remember(app, json!({ "lastSeenAt": crate::notch::now_ms() }));
    crate::wake::stop();
}

fn remember(app: &AppHandle, v: Value) {
    if let Value::Object(m) = v {
        let _ = state(app).store.remember(&m);
    }
}

/// Settings' "Tes" button for the Anthropic key: one tiny call (through aina Cloud when signed in).
pub fn test_anthropic(app: &AppHandle) -> Result<Value, String> {
    let st = state(app);
    let key = st.store.secret("anthropicApiKey").ok_or("Masuk ke aina Cloud dulu (Settings → Akun).")?;
    let model = crate::cloud::model(app, st.store.get("model").as_str().unwrap_or("claude-opus-5"));
    let r = aina_llm::claude::Request { key: &key, model: &model, effort: "low", thinking: false, system: "Reply with exactly: ok", messages: json!([{ "role": "user", "content": "ping" }]), max_tokens: 2000 };
    let (text, _) = aina_llm::claude::stream(&r, &|| false, &mut |_| {})?;
    Ok(json!(text.trim()))
}
