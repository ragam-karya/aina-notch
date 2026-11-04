// The window.badi bridge: v1's ipcMain handlers, channel for channel.

use serde_json::{Map, Value, json};
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};

use crate::i18n::{asset_json, strings};
use crate::{AppState, notch, windows};

// Same allowlists as v1's preload.js (shim.js checks them too).
const INVOKE: [&str; 46] = [
    "config:get", "config:set", "config:clearKey", "test:anthropic", "ask",
    "agents:list", "agents:get", "agents:create", "agents:remove", "agents:send", "agents:stop",
    "agents:folder", "agents:routine", "agents:engine", "sprites:diff", "sprites:edit", "open",
    "cloud:status", "cloud:login", "cloud:logout", "cloud:checkout", "cloud:deleteAccount", "cloud:open",
    "apps:list", "apps:connect", "apps:disconnect", "apps:toggle", "apps:brands", "apps:brand", "apps:unbrand",
    "extension:info", "extension:newCode", "extension:folder",
    "dictation:info",
    "focus:get", "focus:start", "focus:update", "focus:end", "focus:pick",
    "reminder:get", "reminder:save", "reminder:delete", "reminder:cities",
    "location:get", "location:detect", "location:search",
];

fn arg(args: &[Value], i: usize) -> Value {
    args.get(i).cloned().unwrap_or(Value::Null)
}

#[tauri::command]
pub async fn badi_invoke(app: AppHandle, ch: String, args: Vec<Value>) -> Result<Value, String> {
    if !INVOKE.contains(&ch.as_str()) {
        return Err(format!("blocked: {ch}"));
    }
    let state = app.state::<AppState>();
    let store = &state.store;
    match ch.as_str() {
        _ if ch.starts_with("focus:") => crate::focus::invoke(&app, &ch, &arg(&args, 0)),
        _ if ch.starts_with("reminder:") => crate::reminders::invoke(&app, &ch, &arg(&args, 0)),
        _ if ch.starts_with("location:") => crate::weather::invoke(&app, &ch, &arg(&args, 0)),
        "config:get" => Ok(store.public_view()),
        "config:set" => {
            let patch = match arg(&args, 0) {
                Value::Object(m) => m,
                _ => Map::new(),
            };
            let before = store.get("language");
            let place_before: Vec<Value> = ["locationMode", "locationCity", "locationShare"].iter().map(|k| store.get(k)).collect();
            store.save(&patch)?;
            if store.get("language") != before {
                send_island_labels(&app, None);
            }
            if patch.contains_key("notchShortcuts") {
                notch::send_status(&app);
            }
            if patch.keys().any(|k| k.starts_with("prayer") || k.starts_with("reminder")) {
                crate::reminders::settings_changed(&app);
            }
            // A different place: the weather (and a followed prayer city) look again.
            if ["locationMode", "locationCity", "locationShare"].iter().map(|k| store.get(k)).collect::<Vec<_>>() != place_before {
                crate::weather::settings_changed(&app);
            }
            // The pomodoro settings apply from the next phase; a running one keeps its length.
            if ["pomodoro", "pomodoroPreset", "pomodoroPresets"].iter().any(|k| patch.contains_key(*k)) {
                notch::load_pomodoro(&mut app.state::<crate::AppState>().timers.lock().unwrap(), store);
                notch::send_status(&app);
            }
            // What the saved settings change right away (v1: hotkeys, wake word, launch at login,
            // the tray tooltip, voice prefetch).
            crate::settings::apply(&app, &patch);
            Ok(store.public_view())
        }
        "config:clearKey" => {
            store.clear_key(arg(&args, 0).as_str().unwrap_or(""))?;
            Ok(store.public_view())
        }
        "open" => {
            let target = arg(&args, 0);
            let target = target.as_str().ok_or("nothing to open")?;
            open_path(target)?;
            Ok(Value::Bool(true))
        }
        "extension:info" => Ok(json!({
            "code": store.secret("extensionToken").unwrap_or_default(),
            "port": store.get("extensionPort"),
            "status": state.bridge.status(),
            "folder": crate::settings::extension_for_browser(&app, false).map(|p| p.display().to_string()).unwrap_or_default(),
        })),
        "extension:folder" => {
            let dir = crate::settings::extension_for_browser(&app, true).ok_or("Folder ekstensi tidak ditemukan.")?;
            open_path(&dir.display().to_string())?;
            Ok(Value::Bool(true))
        }
        // Where dictations are saved (Settings shows it).
        "dictation:info" => Ok(json!({ "folder": crate::dictation::dir(&app).display().to_string() })),
        "test:anthropic" => {
            let a = app.clone();
            tauri::async_runtime::spawn_blocking(move || crate::settings::test_anthropic(&a)).await.map_err(|e| e.to_string())?
        }
        "extension:newCode" => {
            let code = pairing_code();
            let mut patch = Map::new();
            patch.insert("extensionToken".into(), code.clone().into());
            store.save(&patch)?;
            state.bridge.disconnect();
            Ok(json!({ "code": code, "status": state.bridge.status() }))
        }
        // Read-only lists answer empty until their modules are ported, so Settings and Agents still open.
        c if c.starts_with("agents:") || c.starts_with("sprites:") => crate::agents::invoke(&app, c, &arg(&args, 0)),
        c if c.starts_with("apps:") => {
            // Composio calls go over the network; keep them off the IPC thread.
            let (a, c, x) = (app.clone(), c.to_string(), arg(&args, 0));
            tauri::async_runtime::spawn_blocking(move || crate::apps::invoke(&a, &c, &x)).await.map_err(|e| e.to_string())?
        }
        c if c.starts_with("cloud:") => {
            let (a, c, x) = (app.clone(), c.to_string(), arg(&args, 0));
            tauri::async_runtime::spawn_blocking(move || crate::cloud::invoke(&a, &c, &x)).await.map_err(|e| e.to_string())?
        }
        "ask" => {
            // The bar closes first, so it is never in the screenshot sent with the question.
            if let Some(w) = app.get_webview_window("ask") {
                let _ = w.destroy();
            }
            crate::talk::ask_typed(&app, arg(&args, 0).as_str().unwrap_or("").trim());
            Ok(Value::Null)
        }
        _ => Err(format!("unknown channel: {ch}")),
    }
}

#[tauri::command]
pub fn badi_send(app: AppHandle, window: WebviewWindow, ch: String, payload: Value) {
    if std::env::var_os("BADI_LOG").is_some() && ch != "island:interactive" && ch != "cursor:zone" {
        eprintln!("[ipc] {} {ch} {payload}", window.label());
    }
    match ch.as_str() {
        // Where this overlay's page wants the cursor (the notch, the Sprite cards): moves elsewhere aren't sent.
        "cursor:zone" => crate::cursor::set_zone(window.label(), &payload),
        // The notch is hovered: this overlay takes clicks until the cursor leaves it.
        "island:interactive" => {
            let on = payload.as_bool().unwrap_or(false);
            // The Brain's own mouse moves never grab the notch.
            if on && badi_input::hook::muted() {
                return;
            }
            let _ = window.set_ignore_cursor_events(!on);
            crate::apps::watch_media(&app, on);
            if on {
                crate::windows::overlays_busy(&app);
            } else if crate::turn::phase(&app) == "idle" {
                crate::windows::overlays_quiet(&app);
            }
        }
        "island:act" => {
            let id = payload.as_str().unwrap_or("").to_string();
            // Actions may wait (a question's answer, a file check); keep the IPC thread free.
            std::thread::spawn(move || notch::act(&app, &id));
        }
        "drop:hover" => notch::drop_hover(&app, payload.as_bool().unwrap_or(false)),
        "drop:files" => {
            let paths: Vec<String> = payload.as_array().into_iter().flatten().filter_map(|p| p.as_str().map(String::from)).collect();
            std::thread::spawn(move || notch::handle_drop(&app, &paths));
        }
        "overlay:stepDone" => {
            if let Some(id) = payload.as_u64() {
                crate::turn::step_done(&app, id);
            }
        }
        // The realtime worker: model events, the connection's state, and the mic level for the wave.
        "rt:event" if window.label() == "worker" => crate::realtime::on_event(&app, &payload),
        "rt:state" if window.label() == "worker" => crate::realtime::on_state(&app, &payload),
        "record:level" if window.label() == "worker" => windows::broadcast(&app, "level", payload.as_f64().unwrap_or(0.0)),
        // BADI_DEMO=mascot: the overlay probe's summaries (perf_probe.js); ignored otherwise.
        "perf:sample" => crate::demo::perf_sample(window.label(), &payload),
        _ => {}
    }
}

/// A page finished loading and registered its listeners: send what v1 sent on did-finish-load.
#[tauri::command]
pub fn badi_ready(app: AppHandle, window: WebviewWindow) {
    let label = window.label().to_string();
    {
        // Its window was kept hidden until now (no white flash while it loaded).
        let (a, l) = (app.clone(), label.clone());
        std::thread::spawn(move || windows::page_ready(&a, &l));
    }
    if label == "worker" {
        return crate::realtime::worker_ready(&app);
    }
    if !label.starts_with("overlay-") {
        return;
    }
    send_island_labels(&app, Some(&label));
    let _ = app.emit_to(label.as_str(), "notch:status", notch::status(&app));
    let _ = app.emit_to(label.as_str(), "island:lottie", json!({
        "cloud": asset_json(&app, "assets/cloud.json"),
        "file-upload": asset_json(&app, "assets/file-upload.json"),
    }));
    let _ = app.emit_to(label.as_str(), "island:panel", notch::current_panel(&app));
    crate::apps::send_media_extras(&app, &label);
    crate::agents::send_sprites(&app);
    let _ = app.emit_to(label.as_str(), "calendar:data", crate::reminders::payload(&app));
}

pub fn send_island_labels(app: &AppHandle, label: Option<&str>) {
    let labels = strings(app)["island"].clone();
    match label {
        Some(l) => {
            let _ = app.emit_to(l, "islandLabels", labels);
        }
        None => windows::broadcast(app, "islandLabels", labels),
    }
}

// A 6-digit code for pairing the browser extension.
pub fn pairing_code() -> String {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let mixed = (nanos ^ (std::process::id() as u128).rotate_left(17)).wrapping_mul(6364136223846793005);
    format!("{:06}", (mixed >> 32) % 1_000_000)
}

// Opens a file, folder, http(s) URL or mailto: link with its default app (v1: shell.openPath /
// openExternal).
fn open_path(target: &str) -> Result<(), String> {
    let is_url = target.starts_with("http://") || target.starts_with("https://") || target.starts_with("mailto:");
    if !is_url && !std::path::Path::new(target).exists() {
        return Err(format!("not found: {target}"));
    }
    let target = if is_url { target.to_string() } else { badi_agent::apps::visible_path(target) };
    std::process::Command::new("explorer").arg(target).spawn().map(|_| ()).map_err(|e| e.to_string())
}
