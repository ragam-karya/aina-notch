// The smart notch's state in the core: the panel it shows (a question with buttons, a drop zone, an
// upload, a notice), the drop window over it, questions that wait for the user, and the timers.
// Port of the notch section of v1's main.js (showPanel, clearPanel, showNotice, requestDrop,
// closeDrop, waitForUser, notchAction); the notch itself is v1's island.js, unchanged.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{Sender, channel};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use aina_core::notch::{Entry, classify_drop};
use aina_core::timers::{Event, PomodoroConfig, Timers, builtin_presets};
use serde_json::{Map, Value, json};
use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager};

use crate::i18n::{self, t};
use crate::{AppState, windows};

pub type Run = Arc<dyn Fn(&AppHandle) + Send + Sync>;

pub struct Action {
    pub id: String,
    pub label: String,
    pub tone: String,
    pub run: Option<Run>,
}

impl Action {
    pub fn new(id: &str, label: impl Into<String>, tone: &str, run: Option<Run>) -> Action {
        Action { id: id.into(), label: label.into(), tone: tone.into(), run }
    }
}

#[derive(Default)]
pub struct Notch {
    seq: u64,
    panel: Option<Value>,
    on: HashMap<String, Run>,
    drop: Option<(String, u64)>, // purpose, panel id (or, while quiet, a token)
    drop_quiet: bool,            // armed, but the zone shows only when something is dragged over it
    pending: Option<Sender<Answer>>,
    pending_kind: String, // "confirm" (yes/no) or "ask" (free text)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Answer {
    pub approved: bool,
    pub text: String,
}

// The drop zone part of the notch (same size as .dropzone in overlay.html).
const DROP_ZONE: (f64, f64) = (360.0, 128.0);
// While a quiet drop waits: the invisible target over the resting notch.
const DROP_TARGET: (f64, f64) = (240.0, 48.0);
const DROP_LABEL: &str = "drop";

pub fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn state(app: &AppHandle) -> tauri::State<'_, AppState> {
    app.state::<AppState>()
}

/// The notch follows the display under the cursor when something starts there (v1 pinSubtitleDisplay).
pub fn pin_display(app: &AppHandle) {
    if let Some(o) = windows::overlay_at_cursor(app) {
        windows::broadcast(app, "subtitleDisplay", json!({ "x": o.origin.0, "y": o.origin.1 }));
    }
}

/// Shows a panel; returns its id. `extra` carries kind-specific fields (purpose, progress…).
pub fn show_panel(app: &AppHandle, kind: &str, title: &str, lines: &[String], actions: Vec<Action>, ttl_ms: u64, extra: Map<String, Value>) -> u64 {
    let panel = {
        let st = state(app);
        let mut n = st.notch.lock().unwrap();
        n.seq += 1;
        n.on = actions.iter().filter_map(|a| a.run.clone().map(|r| (a.id.clone(), r))).collect();
        let mut p = json!({
            "id": n.seq,
            "kind": kind,
            "title": title,
            "lines": lines,
            "actions": actions.iter().map(|a| json!({ "id": a.id, "label": a.label, "tone": a.tone })).collect::<Vec<_>>(),
        });
        p.as_object_mut().unwrap().extend(extra);
        n.panel = Some(p.clone());
        p
    };
    let id = panel["id"].as_u64().unwrap_or(0);
    windows::overlays_busy(app);
    pin_display(app);
    windows::broadcast(app, "island:panel", panel);
    if ttl_ms > 0 {
        let app = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(ttl_ms));
            clear_panel_id(&app, id);
        });
    }
    id
}

fn clear_where(app: &AppHandle, matches: impl Fn(&Value) -> bool) {
    let cleared = {
        let st = state(app);
        let mut n = st.notch.lock().unwrap();
        if n.panel.as_ref().is_some_and(&matches) {
            n.panel = None;
            n.on.clear();
            true
        } else {
            false
        }
    };
    if cleared {
        windows::broadcast(app, "island:panel", Value::Null);
        if crate::turn::phase(app) == "idle" {
            windows::overlays_quiet(app);
        }
    }
}

pub fn clear_panel_id(app: &AppHandle, id: u64) {
    clear_where(app, |p| p["id"].as_u64() == Some(id));
}

pub fn clear_panel_kind(app: &AppHandle, kind: &str) {
    clear_where(app, |p| p["kind"] == kind);
}

pub fn current_panel(app: &AppHandle) -> Value {
    state(app).notch.lock().unwrap().panel.clone().unwrap_or(Value::Null)
}

/// A passing notice (an agent finished, an upload is done) never covers a question or a drop zone.
pub fn show_notice(app: &AppHandle, kind: &str, title: &str, lines: &[String], actions: Vec<Action>, ttl_ms: u64) {
    let busy = state(app).notch.lock().unwrap().panel.as_ref().is_some_and(|p| matches!(p["kind"].as_str(), Some("confirm" | "drop" | "upload")));
    if !busy && !crate::focus::quiet(app, kind) {
        show_panel(app, kind, title, lines, actions, ttl_ms, Map::new());
    }
}

/// A button in the notch (island:act).
pub fn act(app: &AppHandle, id: &str) {
    let run = state(app).notch.lock().unwrap().on.get(id).cloned();
    if let Some(run) = run {
        return run(app);
    }
    // The peek's "Login" beside "Silakan login dulu": Settings, at the account section.
    if id == "cloud:settings" {
        return open_settings_at(app, "cloudSection");
    }
    if id == "timerOff" {
        clear_panel_kind(app, "pomodoro");
        state(app).timers.lock().unwrap().stop_timer();
        return send_status(app);
    }
    if let Some(what) = id.strip_prefix("timer:") {
        return timer_action(app, what);
    }
    if let Some(rid) = id.strip_prefix("remindOff:") {
        if state(app).timers.lock().unwrap().remove_reminder(rid) {
            crate::reminders::changed(app);
        }
        return send_status(app);
    }
    // The weather on the left: no place known yet leads to Settings → Lokasi & cuaca.
    if id == "weather:setup" {
        return open_settings_at(app, "locationCard");
    }
    if id == "weather:refresh" {
        return crate::weather::refresh(app, true);
    }
    // The reminder page (Agenda, Timeline, Calendar, prayer times).
    if let Some(what) = id.strip_prefix("cal:") {
        return crate::reminders::act(app, what);
    }
    if id == "stopAgents" {
        crate::agents::stop_all(app);
        return send_status(app);
    }
    // The Focus page (and the older "end focus" button): Selesai ends the session, asking for the
    // summary first when the person turned that on.
    if id == "endFocus" {
        return crate::focus::act(app, "end");
    }
    if let Some(what) = id.strip_prefix("focus:") {
        return crate::focus::act(app, what);
    }
    // The Tasks page's "+N more": every task, in the Agents window.
    if id == "agents" {
        return windows::open_agents(app, None);
    }
    if let Some(slug) = id.strip_prefix("agent:") {
        return windows::open_agents(app, Some(slug));
    }
    if let Some(what) = id.strip_prefix("sprite:") {
        return crate::agents::act(app, what);
    }
    // The music card: volume (0..100), seeking (ms), and Windows' sound output page.
    if let Some(v) = id.strip_prefix("volume:").and_then(|v| v.parse::<f64>().ok()) {
        return crate::apps::set_volume(app, v);
    }
    if let Some(ms) = id.strip_prefix("media:seek:").and_then(|v| v.parse::<i64>().ok()) {
        return crate::apps::seek(app, ms);
    }
    if id == "sound:output" {
        return crate::apps::open_sound_output();
    }
    if let Some(what) = id.strip_prefix("media:").filter(|w| ["shuffle", "repeat", "mute", "open"].contains(w)) {
        return crate::apps::media_extra(app, what);
    }
    if let Some(action) = id.strip_prefix("media:") {
        return crate::apps::media_control(app, action);
    }
    if let Some(n) = id.strip_prefix("shortcut:").and_then(|n| n.parse().ok()) {
        return crate::apps::shortcut(app, n);
    }
    if id == "notes" {
        return show_notes(app);
    }
    if id == "knowledge" {
        return show_knowledge(app);
    }
    if std::env::var_os("AINA_LOG").is_some() {
        eprintln!("[notch] unknown action: {id}");
    }
}

// ---- pomodoro ----

/// The pomodoro settings, today's count of finished focus sessions and the local UTC offset, from
/// config.json into the timers (at start, and again when Settings saves them).
pub fn load_pomodoro(t: &mut Timers, store: &aina_store::Store) {
    t.config = PomodoroConfig::from_value(&store.get("pomodoro"));
    t.stats = serde_json::from_value(store.get("pomodoroStats")).unwrap_or_default();
    t.utc_offset_ms = i64::from(chrono::Local::now().offset().local_minus_utc()) * 1000;
}

/// The presets offered: the built-in patterns, then the person's own (config.json pomodoroPresets).
fn pomodoro_presets(app: &AppHandle) -> Vec<(String, String, PomodoroConfig)> {
    let mut list: Vec<_> = builtin_presets().into_iter().map(|(id, c)| (id.to_string(), c.name(), c)).collect();
    for (i, p) in state(app).store.get("pomodoroPresets").as_array().into_iter().flatten().enumerate() {
        let c = PomodoroConfig::from_value(p);
        let name = p["name"].as_str().filter(|n| !n.trim().is_empty()).map(String::from).unwrap_or_else(|| c.name());
        list.push((format!("custom:{i}"), name, c));
    }
    list
}

/// The notch's timer controls. Skip ends only the current phase, Reset puts the current phase back to
/// its full length, and Stop (timerOff) ends the whole cycle.
/// The reminder that the next pomodoro phase is ready (auto-start off): it stays in the notch, its
/// Start button starting it, until it is started (here or on the Timer page) or the cycle stops.
fn show_next_phase(app: &AppHandle, next: &str, min: u64) {
    let en = i18n::lang(app) == "en";
    let brk = next != "work";
    let (title, line, start) = match (en, brk) {
        (true, true) => ("Focus session done".to_string(), format!("Your {min}-minute break is ready."), "Start break"),
        (true, false) => ("Break's over".to_string(), "The next focus session is ready.".to_string(), "Start focus"),
        (false, true) => ("Fokus selesai".to_string(), format!("Istirahat {min} menit siap dimulai."), "Mulai istirahat"),
        (false, false) => ("Istirahat selesai".to_string(), "Sesi fokus berikutnya siap.".to_string(), "Mulai fokus"),
    };
    let actions = vec![
        Action::new("pomo:stop", if en { "Stop" } else { "Stop" }, "danger", Some(Arc::new(|a: &AppHandle| act(a, "timerOff")))),
        Action::new("pomo:start", start, "primary", Some(Arc::new(|a: &AppHandle| timer_action(a, "start")))),
    ];
    let mut extra = Map::new();
    extra.insert("next".into(), json!(next));
    // Shown like a notice (never over a question or a drop), but it stays until it is answered.
    let busy = state(app).notch.lock().unwrap().panel.as_ref().is_some_and(|p| matches!(p["kind"].as_str(), Some("confirm" | "drop" | "upload")));
    if !busy {
        show_panel(app, "pomodoro", &title, &[line], actions, 0, extra);
    }
}

fn timer_action(app: &AppHandle, what: &str) {
    let now = now_ms();
    let st = state(app);
    // Starting, skipping, resetting or stopping answers the "next phase is ready" reminder.
    if matches!(what, "start" | "skip" | "reset") {
        clear_panel_kind(app, "pomodoro");
    }
    if let Some(id) = what.strip_prefix("preset:") {
        // The pattern's lengths and cycle; the person's auto-start and notification choices stay.
        if let Some((_, _, c)) = pomodoro_presets(app).into_iter().find(|(pid, _, _)| pid == id) {
            let mut t = st.timers.lock().unwrap();
            t.config = PomodoroConfig { auto_start: t.config.auto_start, notify: t.config.notify, ..c };
            let mut patch = Map::new();
            patch.insert("pomodoro".into(), serde_json::to_value(&t.config).unwrap_or(Value::Null));
            patch.insert("pomodoroPreset".into(), json!(id));
            let _ = st.store.save(&patch);
        }
        return send_status(app);
    }
    if what == "settings" {
        return open_pomodoro_settings(app);
    }
    // A pomodoro started while focused is for that work: it carries the focus's name.
    let label = crate::focus::session(app).map(|s| s.name).unwrap_or_default();
    let skipped = {
        let mut t = st.timers.lock().unwrap();
        match what {
            // Start: a held phase goes on, otherwise a new cycle begins.
            "start" => {
                if !t.resume(now) {
                    t.start_pomodoro(now, &label);
                }
                None
            }
            "pause" => {
                t.pause(now);
                None
            }
            "resume" => {
                t.resume(now);
                None
            }
            "reset" => {
                t.reset(now);
                None
            }
            "skip" => t.skip(now),
            _ => None,
        }
    };
    if skipped.is_some() {
        crate::turn::chime(app, "step");
    }
    send_status(app);
}

/// Settings, opened at its pomodoro section.
fn open_pomodoro_settings(app: &AppHandle) {
    open_settings_at(app, "pomodoroCard");
}

/// Settings, scrolled to one of its cards (pomodoroCard, remindersCard, appsCard…).
pub fn open_settings_at(app: &AppHandle, card: &'static str) {
    use tauri::Emitter;
    let fresh = app.get_webview_window("settings").is_none();
    windows::open_settings(app);
    let a = app.clone();
    std::thread::spawn(move || {
        if fresh {
            std::thread::sleep(Duration::from_millis(1500)); // the page registers its listeners first
        }
        if let Some(w) = a.get_webview_window("settings") {
            let _ = w.emit("settings:focus", card);
        }
    });
}

fn save_pomodoro_stats(app: &AppHandle) {
    let st = state(app);
    let stats = {
        let mut t = st.timers.lock().unwrap();
        if !t.stats_changed {
            return;
        }
        t.stats_changed = false;
        t.stats.clone()
    };
    let mut patch = Map::new();
    patch.insert("pomodoroStats".into(), serde_json::to_value(stats).unwrap_or(Value::Null));
    let _ = st.store.remember(&patch);
}

// ---- status (the hover peek and the mini pill) ----

pub fn status(app: &AppHandle) -> Value {
    let st = state(app);
    let store = &st.store;
    let shortcuts: Vec<Value> = store
        .get("notchShortcuts")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|s| s["label"].is_string() && s["prompt"].is_string())
        .take(4)
        .enumerate()
        .map(|(i, s)| json!({ "id": format!("shortcut:{i}"), "label": s["label"] }))
        .collect();
    let mut snap = st.timers.lock().unwrap().snapshot(now_ms());
    // The pomodoro page's preset switch: every preset by id and name, and the one chosen.
    snap["pomodoro"]["presets"] = pomodoro_presets(app).into_iter().map(|(id, name, _)| json!({ "id": id, "name": name })).collect();
    snap["pomodoro"]["preset"] = store.get("pomodoroPreset");
    json!({
        "agents": crate::agents::notch_agents(app),
        "tasks": crate::agents::notch_tasks(app),
        "timer": snap["timer"],
        "reminders": snap["reminders"],
        "pomodoro": snap["pomodoro"],
        // The focus's name (also for a context without a folder), and the Focus page's details.
        "focus": crate::focus::session(app).map(|s| s.name),
        "focusInfo": crate::focus::status(app),
        "media": aina_apps::media::LAST.lock().unwrap().clone(),
        "shortcuts": shortcuts,
        "cloud": crate::cloud::notch_usage(),
        "notes": crate::notes::count(&crate::dictation::dir(app)),
        "knowledge": crate::knowledge::items(app).len(),
        "nextReminder": crate::reminders::next_item(app),
        "weather": crate::weather::status(),
    })
}

pub fn send_status(app: &AppHandle) {
    windows::broadcast(app, "notch:status", status(app));
}

fn save_reminders(app: &AppHandle) {
    let st = state(app);
    let list = serde_json::to_value(&st.timers.lock().unwrap().reminders).unwrap_or(Value::Null);
    let mut patch = Map::new();
    patch.insert("reminders".into(), list);
    let _ = st.store.remember(&patch);
    crate::reminders::publish(app);
}

/// Says a line once aina is free (a timer or reminder never cuts into an answer), waiting up to a minute.
pub fn say_when_free(app: &AppHandle, line: String) {
    let a = app.clone();
    std::thread::spawn(move || {
        for _ in 0..120 {
            if crate::turn::phase(&a) == "idle" && !pending(&a) {
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        pin_display(&a);
        crate::talk::say(&a, &line);
        windows::broadcast(&a, "linger", 6000);
        if crate::turn::phase(&a) == "idle" {
            crate::turn::set_phase(&a, "idle"); // the notch leaves "speaking"
        }
    });
}

/// Ticks the timers once a second while something is set (v1: setInterval while timers.busy()).
pub fn start_timers(app: AppHandle) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(1));
            let (events, saved) = {
                let st = state(&app);
                let mut t = st.timers.lock().unwrap();
                if !t.busy() {
                    continue;
                }
                // Repeating reminders step by the local calendar (clock changes included).
                t.tick_with(now_ms(), |r| aina_core::calendar::next_after(&chrono::Local, r.at, r.repeat.as_deref().unwrap_or(""), now_ms()))
            };
            if saved {
                save_reminders(&app);
            }
            for e in &events {
                match e {
                    Event::Timer { label, pomodoro } => {
                        let (auto, notify, today, focus_ms) = {
                            let st = state(&app);
                            let t = st.timers.lock().unwrap();
                            (t.config.auto_start, t.config.notify, t.done_today(now_ms()), t.config.phase_ms("work"))
                        };
                        // A finished pomodoro focus phase counts toward the focus session's real time.
                        if matches!(pomodoro, Some(("work", _, _))) {
                            crate::focus::pomodoro_done(&app, focus_ms);
                        }
                        // Each end sounds its own: a timer rings, a focus session falls (rest now), a
                        // break rises (back to it), the whole cycle plays a small fanfare.
                        let (line, chime) = match pomodoro {
                            None => (i18n::timer_done(&app, label), "timer-done"),
                            Some((_, "done", _)) => (i18n::pomodoro_done(&app, today), "cycle-done"),
                            Some((_, "short" | "long", min)) if auto => (i18n::pomodoro_break(&app, *min), "focus-done"),
                            Some((_, "short" | "long", min)) => (i18n::pomodoro_break_ready(&app, *min), "focus-done"),
                            Some(_) if auto => (t(&app, "pomodoroWork"), "break-done"),
                            Some(_) => (i18n::pomodoro_work_ready(&app), "break-done"),
                        };
                        // Without auto-start the next phase waits: the notch stays open on it, with its
                        // Start button, until it is started or stopped.
                        let waiting = !auto && matches!(pomodoro, Some((_, next, _)) if *next != "done");
                        if waiting && let Some((_, next, min)) = pomodoro {
                            show_next_phase(&app, next, *min);
                        }
                        // A plain timer always says so; a pomodoro only with its notifications on.
                        if pomodoro.is_none() || notify {
                            crate::turn::chime(&app, chime);
                            if !waiting {
                                show_notice(&app, "timer", &format!("⏱ {line}"), &[], vec![], 8000);
                            }
                            if pomodoro.is_some() {
                                use tauri_plugin_notification::NotificationExt;
                                let _ = app.notification().builder().title("aina").body(&line).show();
                            }
                            say_when_free(&app, line);
                        }
                    }
                    Event::Remind { reminder, late } => {
                        crate::turn::chime(&app, "step");
                        show_notice(&app, "remind", &format!("⏰ {}", reminder.text), &[], vec![], 15000);
                        let en = i18n::lang(&app) == "en";
                        let line = match (en, late) {
                            (true, true) => format!("You missed a reminder earlier: {}.", reminder.text),
                            (true, false) => format!("Reminder: {}.", reminder.text),
                            (false, true) => format!("Tadi ada pengingat yang kelewat: {}.", reminder.text),
                            (false, false) => format!("Pengingat: {}.", reminder.text),
                        };
                        say_when_free(&app, line);
                    }
                }
            }
            if !events.is_empty() {
                save_pomodoro_stats(&app);
                send_status(&app);
            }
        }
    });
}

// ---- lists in the notch: the notes, the knowledge base ----

/// A row of a list panel: clicking it runs `open`; `remove` is the × at its end.
pub struct Row {
    pub main: String,
    pub sub: String,
    pub open: Option<Run>,
    pub remove: Option<Run>,
}

// A list panel, scrolling when long. Each row's actions are registered like the panel's buttons.
fn show_list(app: &AppHandle, kind: &str, title: &str, rows: Vec<Row>, actions: Vec<Action>) {
    let mut extra = Map::new();
    let mut runs: Vec<(String, Run)> = Vec::new();
    let list: Vec<Value> = rows
        .into_iter()
        .enumerate()
        .map(|(i, r)| {
            let open = r.open.map(|run| {
                runs.push((format!("{kind}:open:{i}"), run));
                format!("{kind}:open:{i}")
            });
            let remove = r.remove.map(|run| {
                runs.push((format!("{kind}:remove:{i}"), run));
                format!("{kind}:remove:{i}")
            });
            json!({ "main": r.main, "sub": r.sub, "act": open, "remove": remove })
        })
        .collect();
    extra.insert("list".into(), Value::Array(list));
    show_panel(app, kind, title, &[], actions, 0, extra);
    let st = state(app);
    let mut n = st.notch.lock().unwrap();
    n.on.extend(runs);
}

fn close_action(app: &AppHandle, kind: &'static str) -> Action {
    let en = i18n::lang(app) == "en";
    Action::new(&format!("{kind}:close"), if en { "Close" } else { "Tutup" }, "", Some(Arc::new(move |a: &AppHandle| clear_panel_kind(a, kind))))
}

/// The notes (dictations), newest first; a click opens one in the notch.
pub fn show_notes(app: &AppHandle) {
    let en = i18n::lang(app) == "en";
    let dir = crate::dictation::dir(app);
    let rows = crate::notes::list(&dir, 40)
        .into_iter()
        .map(|(file, r)| {
            let when = chrono::DateTime::from_timestamp_millis(r.created_ms).map(|t| t.with_timezone(&chrono::Local).format("%d/%m %H.%M").to_string()).unwrap_or_default();
            let first = if r.summary.is_empty() { &r.text } else { &r.summary };
            let main: String = first.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(80).collect();
            let sub = format!("{when} · {} {}", crate::notes::words(&r.text), if en { "words" } else { "kata" });
            let open: Run = Arc::new(move |a: &AppHandle| {
                crate::dictation::show_note(a, &file, &format!("📝 {when}"), (!r.summary.is_empty()).then_some(r.summary.as_str()), &r.text, true);
            });
            Row { main, sub, open: Some(open), remove: None }
        })
        .collect();
    let folder = Action::new("notes:folder", "Folder", "", Some(Arc::new(move |_: &AppHandle| {
        let _ = std::fs::create_dir_all(&dir);
        let _ = aina_agent::apps::shell_open(&dir.display().to_string());
    })));
    show_list(app, "notes", if en { "📝 Notes" } else { "📝 Catatan" }, rows, vec![folder, close_action(app, "notes")]);
}

/// What is in the knowledge base; × takes an item out (the files themselves stay).
pub fn show_knowledge(app: &AppHandle) {
    let en = i18n::lang(app) == "en";
    let rows = crate::knowledge::items(app)
        .into_iter()
        .map(|p| {
            let sub = p.parent().map(|d| d.display().to_string()).unwrap_or_default();
            let main = format!("{} {}", if p.is_dir() { "📁" } else { "📄" }, crate::knowledge::name(&p));
            let open_path = p.clone();
            let open: Run = Arc::new(move |_: &AppHandle| {
                let _ = aina_agent::apps::shell_open(&open_path.display().to_string());
            });
            let remove: Run = Arc::new(move |a: &AppHandle| {
                crate::knowledge::remove(a, &p);
                show_knowledge(a);
            });
            Row { main, sub, open: Some(open), remove: Some(remove) }
        })
        .collect();
    show_list(app, "knowledge", if en { "📚 Knowledge" } else { "📚 Pengetahuan" }, rows, vec![close_action(app, "knowledge")]);
}

// ---- drops: "fokus di folder ini" / "upload file ini ke Drive" ----
// The notch becomes the drop zone. A click-through overlay can't take an OS drop, so while aina waits
// for one, an invisible window lies over the zone part of the notch and passes the drop on.

fn show_drop_zone(app: &AppHandle, purpose: &str) -> u64 {
    let folder = purpose == "focus";
    let mut extra = Map::new();
    extra.insert("purpose".into(), purpose.into());
    let cancel: Run = Arc::new(close_drop);
    let en = i18n::lang(app) == "en";
    // A drop nobody asked for: aina works out what it is for.
    let (title, hint) = match purpose {
        "auto" if en => ("Drop it here".to_string(), "A project folder: I'll work there. Files or documents: I'll keep them as my knowledge.".to_string()),
        "auto" => ("Taruh di sini".to_string(), "Folder proyek: aku fokus kerja di situ. File atau dokumen: aku simpan jadi pengetahuanku.".to_string()),
        _ => (t(app, if folder { "dropFolderTitle" } else { "dropFilesTitle" }), t(app, if folder { "dropFolderHint" } else { "dropFilesHint" })),
    };
    show_panel(
        app,
        "drop",
        &title,
        &[hint],
        vec![Action::new("cancelDrop", t(app, "notchCancel"), "", Some(cancel))],
        0,
        extra,
    )
}

// Places the invisible drop window: the whole zone, or only the target over the resting notch.
fn place_drop_window(app: &AppHandle, zone: bool) {
    let (Some(win), Some(o)) = (app.get_webview_window(DROP_LABEL), windows::overlay_at_cursor(app)) else { return };
    let (x, y, w, _) = o.bounds();
    let (zw, zh) = if zone { DROP_ZONE } else { DROP_TARGET };
    let _ = win.set_position(LogicalPosition::new(x + (w - zw) / 2.0, y));
    let _ = win.set_size(LogicalSize::new(zw, zh));
}

/// aina waits for a drop. For a folder (focus) it waits quietly: aina says the folder can be dragged
/// onto the notch, and the zone with its animation opens only once something is dragged over it.
/// Files for Drive open the zone at once.
pub fn request_drop(app: &AppHandle, purpose: &str) {
    close_drop(app);
    let quiet = purpose == "focus" || purpose == "auto";
    let id = if quiet { now_ms() as u64 } else { show_drop_zone(app, purpose) };
    {
        let st = state(app);
        let mut n = st.notch.lock().unwrap();
        n.drop = Some((purpose.into(), id));
        n.drop_quiet = quiet;
    }

    let Some(o) = windows::overlay_at_cursor(app) else { return };
    let (x, y, w, _) = o.bounds();
    let protect = std::env::var_os("AINA_DEMO").is_none();
    let built = windows::builder(app, DROP_LABEL, windows::page("drop.html"))
        .title("aina")
        .transparent(true)
        .decorations(false)
        .resizable(false)
        .skip_taskbar(true)
        .always_on_top(true)
        .shadow(false)
        .focused(false)
        .focusable(false)
        .visible(false)
        .content_protected(protect)
        .initialization_script(windows::init_script(""))
        .build();
    if let Ok(win) = built {
        let (zw, zh) = if quiet { DROP_TARGET } else { DROP_ZONE };
        let _ = win.set_position(LogicalPosition::new(x + (w - zw) / 2.0, y));
        let _ = win.set_size(LogicalSize::new(zw, zh));
        let _ = win.show();
    }
    // Nobody dropped anything in 90 s: the zone closes by itself.
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(90));
        let still = state(&app).notch.lock().unwrap().drop.as_ref().map(|d| d.1) == Some(id);
        if still {
            close_drop(&app);
        }
    });
}

/// Something is dragged over the drop window (or left it). A quiet drop opens its zone now.
pub fn drop_hover(app: &AppHandle, on: bool) {
    let quiet = {
        let st = state(app);
        let n = st.notch.lock().unwrap();
        n.drop.as_ref().filter(|_| n.drop_quiet).map(|d| d.0.clone())
    };
    if let Some(purpose) = quiet {
        if on {
            show_drop_zone(app, &purpose);
            place_drop_window(app, true);
        } else {
            clear_panel_kind(app, "drop");
            place_drop_window(app, false);
        }
    }
    windows::broadcast(app, "island:dropHover", on);
}

pub fn close_drop(app: &AppHandle) {
    if state(app).notch.lock().unwrap().drop.take().is_none() {
        return;
    }
    if let Some(w) = app.get_webview_window(DROP_LABEL) {
        let _ = w.destroy();
    }
    clear_panel_kind(app, "drop");
}

/// Files or a folder dropped on the zone: a folder becomes the focus folder, files go to Google Drive.
pub fn handle_drop(app: &AppHandle, paths: &[String]) {
    let Some(purpose) = state(app).notch.lock().unwrap().drop.as_ref().map(|d| d.0.clone()) else { return };
    let entries: Vec<Entry> = paths
        .iter()
        .filter_map(|p| std::fs::metadata(p).ok().map(|m| Entry { path: p.clone(), dir: m.is_dir(), size: m.len() }))
        .collect();
    if purpose == "auto" {
        close_drop(app);
        return auto_dropped(app, &entries);
    }
    let verdict = classify_drop(&entries, &purpose);
    if std::env::var_os("AINA_LOG").is_some() {
        eprintln!("[drop] {purpose} {verdict}");
    }
    // The wrong kind keeps the zone open, with a spoken hint.
    if let Some(problem) = verdict["problem"].as_str() {
        let line = match problem {
            "tooBig" => {
                let name = std::path::Path::new(verdict["path"].as_str().unwrap_or("")).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                if crate::i18n::lang(app) == "en" { format!("{name} is over 100 MB, so I can't upload it yet.") } else { format!("{name} lebih dari 100 MB, jadi belum bisa aku upload.") }
            }
            "needFolder" => t(app, "dropNeedFolder"),
            "oneFolder" => t(app, "dropOneFolder"),
            "needFiles" => t(app, "dropNeedFiles"),
            _ => t(app, "dropEmpty"),
        };
        return crate::talk::say(app, &line);
    }
    close_drop(app);
    if let Some(folder) = verdict["focus"].as_str() {
        crate::agents::focus_dropped(app, folder);
    }
    if let Some(list) = verdict["upload"].as_array() {
        let files = list.iter().filter_map(Value::as_str).map(std::path::PathBuf::from).collect();
        crate::apps::dropped_files(app, files, verdict["skipped"].as_u64().unwrap_or(0));
    }
}

// A drop nobody asked for: a project folder becomes the focus folder, anything else the knowledge
// base. Either notice has a button for the other choice, in case the guess was wrong.
fn auto_dropped(app: &AppHandle, entries: &[Entry]) {
    let en = i18n::lang(app) == "en";
    let project = |p: &str| {
        let names: Vec<String> = std::fs::read_dir(p).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        aina_core::knowledge::is_project(&names)
    };
    let verdict = aina_core::knowledge::auto_drop(entries, project);
    if std::env::var_os("AINA_LOG").is_some() {
        eprintln!("[drop] auto {verdict}");
    }
    if let Some(folder) = verdict["focus"].as_str() {
        crate::agents::focus_dropped(app, folder);
        // focus_dropped shows its notice; this one replaces it, with the way to "knowledge" instead.
        let f = std::path::PathBuf::from(folder);
        let as_kb: Run = Arc::new(move |a: &AppHandle| {
            let _ = crate::agents::focus_folder(a, None);
            kb_added(a, std::slice::from_ref(&f));
        });
        let name = crate::knowledge::name(std::path::Path::new(folder));
        let labels = i18n::strings(app)["island"].clone();
        let focus = labels["focus"].as_str().unwrap_or("Fokus").to_string();
        let end = labels["endFocus"].as_str().unwrap_or("Selesai fokus").to_string();
        let instead = if en { "Use as knowledge" } else { "Jadikan pengetahuan" };
        show_notice(app, "focus", &format!("📁 {focus} · {name}"), &[folder.to_string()], vec![Action::new("dropAsKnowledge", instead, "", Some(as_kb)), Action::new("endFocus", end, "", None)], 8000);
        return;
    }
    let paths: Vec<std::path::PathBuf> = verdict["knowledge"].as_array().into_iter().flatten().filter_map(Value::as_str).map(std::path::PathBuf::from).collect();
    if !paths.is_empty() {
        kb_added(app, &paths);
    }
}

// "📚 Added to my knowledge": the names, the way to see them all, and (for one folder) to work in it.
fn kb_added(app: &AppHandle, paths: &[std::path::PathBuf]) {
    let en = i18n::lang(app) == "en";
    let added = crate::knowledge::add(app, paths);
    let names: Vec<String> = paths.iter().map(|p| crate::knowledge::name(p)).collect();
    let mut actions = vec![Action::new("knowledge", if en { "See all" } else { "Lihat semua" }, "", None)];
    if let [only] = paths
        && only.is_dir()
    {
        let f = only.clone();
        actions.push(Action::new("dropAsFocus", if en { "Work in it instead" } else { "Jadikan fokus" }, "", Some(Arc::new(move |a: &AppHandle| {
            crate::knowledge::remove(a, &aina_agent::guard::real(&f));
            crate::agents::focus_dropped(a, &f.display().to_string());
        }))));
    }
    let title = match (added.is_empty(), en) {
        (true, true) => "📚 Already in my knowledge",
        (true, false) => "📚 Sudah ada di pengetahuanku",
        (false, true) => "📚 Added to my knowledge",
        (false, false) => "📚 Masuk ke pengetahuanku",
    };
    show_notice(app, "knowledge", title, &[names.join(", ")], actions, 8000);
    let line = if en { format!("Got it, I'll use {} when it helps.", names.join(", ")) } else { format!("Oke, {} aku pakai sebagai rujukan ya.", names.join(", ")) };
    let a = app.clone();
    std::thread::spawn(move || {
        crate::knowledge::warm(&a);
        crate::talk::say(&a, &line);
    });
}

// ---- a drag that comes to the notch ----
// Files dragged from anywhere toward the top of the screen arm a quiet drop zone over the resting notch
// (`request_drop("auto")`); the zone opens once they are over it. A click-through overlay can't take an
// OS drop, so the invisible drop window exists only during such a drag and goes once the button is up.

static DRAG: std::sync::Mutex<Option<bool>> = std::sync::Mutex::new(None); // Some(the press began away from the notch)
const NEAR_TOP: f64 = 150.0;
const NEAR_HALF_WIDTH: f64 = 320.0;

/// A cursor move (physical px), from the mouse hook.
pub fn watch_drag(app: &AppHandle, x: i32, y: i32) {
    let held = aina_input::send::left_button_down();
    let arm = {
        let mut d = DRAG.lock().unwrap();
        match (held, *d) {
            (false, _) => {
                *d = None;
                false
            }
            (true, None) => {
                *d = Some(!near_notch(app, x, y));
                false
            }
            (true, Some(from_away)) => from_away && near_notch(app, x, y),
        }
    };
    if !arm || state(app).notch.lock().unwrap().drop.is_some() || crate::talk::listening(app) {
        return;
    }
    request_drop(app, "auto");
    // Disarmed once the button is up, dropped or not (a drop arrives right at the release).
    let a = app.clone();
    std::thread::spawn(move || {
        while aina_input::send::left_button_down() {
            std::thread::sleep(Duration::from_millis(80));
        }
        std::thread::sleep(Duration::from_millis(600));
        let auto = state(&a).notch.lock().unwrap().drop.as_ref().is_some_and(|d| d.0 == "auto");
        if auto {
            close_drop(&a);
        }
    });
}

// Near the notch: the top band of a display, around its middle.
fn near_notch(app: &AppHandle, x: i32, y: i32) -> bool {
    let Some((_, lx, ly)) = crate::talk::to_overlay(app, x, y) else { return false };
    let st = state(app);
    let overlays = st.overlays.lock().unwrap();
    overlays.iter().any(|o| {
        let (bx, by, bw, bh) = o.bounds();
        lx >= bx && lx < bx + bw && ly >= by && ly < by + bh && ly - by < NEAR_TOP && (lx - (bx + bw / 2.0)).abs() < NEAR_HALF_WIDTH
    })
}

// ---- questions that wait for the user ----

/// A question is waiting for its answer.
pub fn pending(app: &AppHandle) -> bool {
    state(app).notch.lock().unwrap().pending.is_some()
}

/// The kind of the waiting question: "confirm", "ask", or None.
pub fn pending_kind(app: &AppHandle) -> Option<String> {
    let st = state(app);
    let n = st.notch.lock().unwrap();
    n.pending.as_ref().map(|_| n.pending_kind.clone())
}

/// The phase that shows the waiting question ("confirm" or "asking"), to go back to after a hold.
pub fn pending_phase(app: &AppHandle) -> Option<&'static str> {
    pending_kind(app).map(|k| if k == "confirm" { "confirm" } else { "asking" })
}

/// A spoken or typed reply to the waiting question. A yes/no question is approved only by a clear
/// yes (v1: YES and not NO); anything unclear counts as not approved, and the words go back to the task.
pub fn answer_text(app: &AppHandle, text: &str) -> bool {
    let approved = pending_kind(app).as_deref() == Some("confirm") && crate::brain::said_yes(text);
    answer(app, Answer { approved, text: text.to_string() })
}

/// Answers the waiting question (Enter, Esc, a spoken answer). False when nothing was asked.
pub fn answer(app: &AppHandle, a: Answer) -> bool {
    let tx = state(app).notch.lock().unwrap().pending.clone();
    match tx {
        Some(tx) => tx.send(a).is_ok(),
        None => false,
    }
}

/// Asks out loud with the notch's buttons, and blocks until answered (buttons, voice, typing, Enter,
/// Esc) or `limit_ms` passes. Run it off the main thread. Port of v1's waitForUser.
pub fn wait_for_user(app: &AppHandle, kind: &str, question: &str, all: bool, limit_ms: u64) -> Answer {
    let (tx, rx) = channel::<Answer>();
    {
        let st = state(app);
        let mut n = st.notch.lock().unwrap();
        n.pending = Some(tx.clone());
        n.pending_kind = kind.to_string();
    }
    let answer = |approved: bool, text: &'static str| -> Run {
        Arc::new(move |app: &AppHandle| {
            if let Some(tx) = state(app).notch.lock().unwrap().pending.clone() {
                let _ = tx.send(Answer { approved, text: text.into() });
            }
        })
    };
    let actions = if kind == "confirm" {
        let mut a = vec![Action::new("allow", t(app, "notchAllow"), "primary", Some(answer(true, "")))];
        if all {
            a.push(Action::new("allowAll", t(app, "notchAllowAll"), "", Some(answer(true, "semua"))));
        }
        a.push(Action::new("deny", t(app, "notchDeny"), "danger", Some(answer(false, ""))));
        a
    } else {
        vec![Action::new("type", t(app, "notchTypeAnswer"), "", Some(Arc::new(|app: &AppHandle| windows::open_ask(app))))]
    };
    show_panel(app, "confirm", "", &[], actions, 0, Map::new());
    crate::turn::set_phase(app, if kind == "confirm" { "confirm" } else { "asking" });
    windows::broadcast(app, "hint", t(app, if kind == "confirm" { "confirmHint" } else { "askHint" }));
    // The question is spoken (and typed out with the voice); a button can answer before it ends.
    {
        let (a, q) = (app.clone(), question.to_string());
        std::thread::spawn(move || crate::talk::say(&a, &q));
    }

    let got = rx.recv_timeout(Duration::from_millis(limit_ms)).unwrap_or(Answer {
        approved: false,
        text: if kind == "confirm" { format!("(no answer within {} seconds)", limit_ms / 1000) } else { String::new() },
    });
    state(app).notch.lock().unwrap().pending = None;
    state(app).player.stop(); // answered before aina finished asking
    windows::broadcast(app, "hint", "");
    clear_panel_kind(app, "confirm");
    crate::turn::set_phase(app, "idle");
    windows::broadcast(app, "linger", 2500); // the answered question tucks away instead of staying up
    got
}
