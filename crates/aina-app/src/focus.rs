// Focus: what the person is working on now, shared by the notch's Focus page, the pomodoro and aina's
// AI (every prompt gets its brief; with a folder and the lock on, the AI changes files only there).
// The model is aina-core's focus module; this keeps it in config.json (focusSession, focusHistory,
// with focusFolder mirroring the folder for the code that only needs that), starts and ends it from
// the notch, voice and dropped folders, and opens the small Focus window for typing (a new context,
// notes, the end-of-session summary, the history).

use std::path::PathBuf;

use aina_core::focus::{self, Record, Session, Summary};
use serde_json::{Map, Value, json};
use tauri::{AppHandle, Manager};

use crate::notch::{self, now_ms};
use crate::{AppState, i18n, windows};

fn state(app: &AppHandle) -> tauri::State<'_, AppState> {
    app.state::<AppState>()
}

/// The running session. One saved before sessions existed (only focusFolder) becomes one now.
pub fn session(app: &AppHandle) -> Option<Session> {
    let store = &state(app).store;
    let saved = store.get("focusSession");
    if saved.is_object() {
        return serde_json::from_value(saved).ok();
    }
    let folder = store.get("focusFolder").as_str().unwrap_or("").to_string();
    if folder.is_empty() || !std::path::Path::new(&folder).is_dir() {
        return None;
    }
    let s = Session::for_folder(&folder, now_ms());
    save_session(app, Some(&s));
    Some(s)
}

fn save_session(app: &AppHandle, s: Option<&Session>) {
    let mut patch = Map::new();
    patch.insert("focusSession".into(), s.map(|s| serde_json::to_value(s).unwrap_or(Value::Null)).unwrap_or(Value::Null));
    patch.insert("focusFolder".into(), json!(s.map(|s| s.folder.as_str()).unwrap_or("")));
    let _ = state(app).store.remember(&patch);
}

pub fn history(app: &AppHandle) -> Vec<Record> {
    serde_json::from_value(state(app).store.get("focusHistory")).unwrap_or_default()
}

/// What every AI prompt is told about the focus ("" without one).
pub fn brief(app: &AppHandle) -> String {
    session(app).map(|s| s.brief()).unwrap_or_default()
}

/// The folder aina's AI must keep its file changes in, when the lock is on.
pub fn locked_folder(app: &AppHandle) -> Option<PathBuf> {
    session(app).and_then(|s| s.locked_folder().map(PathBuf::from))
}

/// Starts `s` (ending the one before into the history). A running pomodoro is left alone.
pub fn start(app: &AppHandle, s: Session) {
    if let Some(old) = session(app) {
        let mut h = history(app);
        focus::end(&old, now_ms(), Summary::default(), &mut h);
        save_history(app, &h);
    }
    save_session(app, Some(&s));
    notch::send_status(app);
}

/// Ends the session with an optional summary; returns its record.
pub fn end(app: &AppHandle, summary: Summary) -> Option<Record> {
    let s = session(app)?;
    let mut h = history(app);
    let r = focus::end(&s, now_ms(), summary, &mut h);
    save_history(app, &h);
    save_session(app, None);
    let line = i18n::focus_ended(app, &r.name, r.ms, r.pomodoros);
    notch::show_notice(app, "focus", &format!("✓ {line}"), &[], vec![], 5000);
    notch::send_status(app);
    Some(r)
}

fn save_history(app: &AppHandle, h: &[Record]) {
    let mut patch = Map::new();
    patch.insert("focusHistory".into(), serde_json::to_value(h).unwrap_or(Value::Null));
    let _ = state(app).store.remember(&patch);
}

/// Changes to the running session from the Focus window or the notch (about, goal, note, lock).
pub fn update(app: &AppHandle, patch: &Value) {
    let Some(mut s) = session(app) else { return };
    let text = |k: &str| patch[k].as_str().map(|v| v.trim().chars().take(400).collect::<String>());
    if let Some(v) = text("about") {
        s.about = v;
    }
    if let Some(v) = text("goal") {
        s.goal = v;
    }
    if let Some(v) = patch["note"].as_str() {
        s.note = v.chars().take(4000).collect();
    }
    if let Some(v) = patch["lock"].as_bool() {
        s.lock = v;
    }
    save_session(app, Some(&s));
    notch::send_status(app);
}

/// A pomodoro focus phase finished while focused: it counts toward the session's actual focus time.
pub fn pomodoro_done(app: &AppHandle, ms: u64) {
    if let Some(mut s) = session(app) {
        s.pomodoros += 1;
        s.pomodoro_ms += ms;
        save_session(app, Some(&s));
    }
}

/// While focused with "do not disturb" on, notices that can wait (a task done, an upload) stay quiet.
pub fn quiet(app: &AppHandle, kind: &str) -> bool {
    matches!(kind, "agentDone" | "uploaded" | "knowledge") && state(app).store.get("focusDnd") == true && session(app).is_some()
}

/// What the notch's Focus page shows: the session (with how long it has run worked out by the page),
/// or the last few contexts to start again with one click.
pub fn status(app: &AppHandle) -> Value {
    let s = session(app);
    let recent: Vec<Value> = focus::recent(&history(app), s.as_ref(), 3).into_iter().map(|(name, folder)| json!({ "name": name, "folder": folder })).collect();
    json!({
        "session": s,
        "recent": recent,
        "summary": state(app).store.get("focusSummary") == true,
        "dnd": state(app).store.get("focusDnd") == true,
    })
}

/// The Focus page's buttons (island:act "focus:…").
pub fn act(app: &AppHandle, what: &str) {
    match what {
        "pick" => {
            if let Some(folder) = pick_folder() {
                start(app, Session::for_folder(&folder, now_ms()));
            }
        }
        "new" | "note" | "history" => open_window(app, what),
        "end" => {
            if state(app).store.get("focusSummary") == true {
                open_window(app, "end");
            } else {
                end(app, Summary::default());
            }
        }
        "open" => {
            if let Some(s) = session(app).filter(|s| !s.folder.is_empty()) {
                let _ = aina_agent::apps::shell_open(&s.folder);
            }
        }
        "lock" => {
            if let Some(s) = session(app) {
                update(app, &json!({ "lock": !s.lock }));
            }
        }
        _ => {
            if let Some(i) = what.strip_prefix("recent:").and_then(|i| i.parse::<usize>().ok())
                && let Some((name, folder)) = focus::recent(&history(app), session(app).as_ref(), 3).into_iter().nth(i)
            {
                let s = if folder.is_empty() { Session { name, started_at: now_ms(), lock: true, ..Default::default() } } else { Session::for_folder(&folder, now_ms()) };
                start(app, s);
            }
        }
    }
}

/// The small Focus window: "new" (a context, with or without a folder), "note", "end" (the
/// optional summary) or "history".
pub fn open_window(app: &AppHandle, mode: &str) {
    // A window already open is replaced; its label is free only once it has really gone.
    if let Some(w) = app.get_webview_window("focus") {
        let _ = w.destroy();
        for _ in 0..40 {
            if app.get_webview_window("focus").is_none() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
    }
    let built = windows::builder(app, "focus", windows::page("focus.html"))
        .title("aina · Fokus")
        .inner_size(440.0, 560.0)
        .resizable(true)
        .always_on_top(true)
        .visible(false)
        .initialization_script(windows::init_script(&format!("?mode={mode}")))
        .build();
    if built.is_ok() {
        windows::reveal_when_ready(app, "focus", true);
    }
}

/// The Focus window's calls (focus:get / focus:start / focus:update / focus:end / focus:pick).
pub fn invoke(app: &AppHandle, ch: &str, arg: &Value) -> Result<Value, String> {
    match ch {
        "focus:get" => {
            let h = history(app);
            let week = focus::totals(&h, now_ms() - 7 * 86_400_000);
            Ok(json!({
                "session": session(app),
                "now": now_ms(),
                "history": h.iter().take(20).collect::<Vec<_>>(),
                "week": week.into_iter().map(|(name, ms, n)| json!({ "name": name, "ms": ms, "sessions": n })).collect::<Vec<_>>(),
            }))
        }
        "focus:start" => {
            let name = arg["name"].as_str().unwrap_or("").trim().to_string();
            let folder = arg["folder"].as_str().unwrap_or("").trim().to_string();
            if name.is_empty() && folder.is_empty() {
                return Err("Isi nama atau pilih folder.".into());
            }
            let mut s = if folder.is_empty() { Session { started_at: now_ms(), lock: true, ..Default::default() } } else { Session::for_folder(&folder, now_ms()) };
            if !name.is_empty() {
                s.name = name;
            }
            s.about = arg["about"].as_str().unwrap_or("").trim().into();
            s.goal = arg["goal"].as_str().unwrap_or("").trim().into();
            s.lock = arg["lock"].as_bool().unwrap_or(true);
            start(app, s);
            close_window(app);
            Ok(Value::Null)
        }
        "focus:update" => {
            update(app, arg);
            Ok(Value::Null)
        }
        "focus:end" => {
            let summary: Summary = serde_json::from_value(arg.clone()).unwrap_or_default();
            let r = end(app, summary);
            close_window(app);
            Ok(json!(r))
        }
        "focus:pick" => {
            // The dialog blocks; this runs on an async command's thread.
            Ok(pick_folder().map(Value::String).unwrap_or(Value::Null))
        }
        _ => Err(format!("unknown channel: {ch}")),
    }
}

/// Closes the Focus window once its call has answered.
fn close_window(app: &AppHandle) {
    let a = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(150));
        if let Some(w) = a.get_webview_window("focus") {
            let _ = w.destroy();
        }
    });
}

/// Windows' own folder picker. None when cancelled.
pub fn pick_folder() -> Option<String> {
    #[cfg(windows)]
    {
        std::thread::spawn(|| unsafe {
            use ::windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree};
            use ::windows::Win32::UI::Shell::{FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog, SIGDN_FILESYSPATH};
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let d: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
            d.SetOptions(d.GetOptions().ok()? | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM).ok()?;
            let _ = d.SetTitle(&::windows::core::HSTRING::from("aina · Pilih folder untuk fokus"));
            d.Show(None).ok()?; // an error when cancelled
            let p = d.GetResult().ok()?.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
            let s = p.to_string().ok();
            CoTaskMemFree(Some(p.0 as _));
            s
        })
        .join()
        .ok()
        .flatten()
    }
    #[cfg(not(windows))]
    None
}
