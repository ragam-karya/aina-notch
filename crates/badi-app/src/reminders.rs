// The Reminder Center behind the notch's reminder page: one repository the Agenda, Timeline and
// Calendar views all read. Local reminders live in config.json (the timers own them); Google Calendar
// events are fetched through Composio for the month on screen when the person has linked and switched
// on Google Calendar, kept in memory only, and merged by stable id; a failed sync keeps the local
// events and the last remote ones, says so, and isn't tried again until the person asks (Coba lagi,
// Sambungkan ulang). Prayer times come from myQuran once a month and are
// kept in config.json. Changes to a Google event go to Google first and show only once it confirms.

use std::collections::HashSet;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use badi_apps::gcal;
use badi_core::calendar::{self, CalEvent, CalendarInfo, from_local, google_event, merge, month_range, next_after, sync_error};
use badi_core::prayer;
use badi_core::timers::Reminder;
use chrono::{Datelike, Local, NaiveDate};
use serde_json::{Map, Value, json};
use tauri::{AppHandle, Manager};

use crate::notch::{self, now_ms};
use crate::{AppState, i18n, windows};

const FRESH_MS: i64 = 5 * 60_000; // a month's remote events are fetched again after this
const AUTO_REFRESH: Duration = Duration::from_secs(10 * 60);
const MAX_CALENDARS: usize = 10;
const PRAYER_RETRY_MS: i64 = 10 * 60_000;

#[derive(Default)]
struct Cal {
    generation: u64, // the newest sync; an older one finishing late is ignored
    month: Option<(i32, u32)>,
    remote: Vec<CalEvent>,
    remote_range: Option<(i64, i64)>,
    fetched: i64,
    calendars: Vec<CalendarInfo>,
    sync: &'static str, // off | loading | ok | auth | scope | rate | offline | error
    sync_message: String,
    synced_at: Option<i64>,
    pending: HashSet<String>,
    prayer_busy: bool,
    prayer_tried: i64,
    prayer_error: String,
    notified: HashSet<String>,
    demo: bool, // BADI_DEMO=reminders: remote events given, never fetched
}

static CAL: LazyLock<Mutex<Cal>> = LazyLock::new(|| Mutex::new(Cal { sync: "off", ..Default::default() }));

fn state(app: &AppHandle) -> tauri::State<'_, AppState> {
    app.state::<AppState>()
}

fn en(app: &AppHandle) -> bool {
    i18n::lang(app) == "en"
}

fn month_of(cal: &Cal) -> (i32, u32) {
    cal.month.unwrap_or_else(|| {
        let t = Local::now();
        (t.year(), t.month())
    })
}

fn range(month: (i32, u32)) -> (i64, i64) {
    month_range(&Local, month.0, month.1)
}

/// The next occurrence of a repeating reminder after now (local time zone).
fn next_of(r: &Reminder) -> Option<i64> {
    next_after(&Local, r.at, r.repeat.as_deref().unwrap_or(""), now_ms())
}

// ---------- local ----------

fn local_events(app: &AppHandle, from: i64, to: i64) -> Vec<CalEvent> {
    let st = state(app);
    let t = st.timers.lock().unwrap();
    t.reminders.iter().flat_map(|r| from_local(&Local, r, from, to, 62)).collect()
}

/// Saves the reminders and shows the change everywhere.
pub fn changed(app: &AppHandle) {
    let st = state(app);
    let list = serde_json::to_value(&st.timers.lock().unwrap().reminders).unwrap_or(Value::Null);
    let mut patch = Map::new();
    patch.insert("reminders".into(), list);
    let _ = st.store.remember(&patch);
    publish(app);
    notch::send_status(app);
}

// The stored reminder behind an event id ("local:<id>" or a repeat's "local:<id>@<ms>").
fn local_id(id: &str) -> Option<&str> {
    id.strip_prefix("local:").map(|r| r.split('@').next().unwrap_or(r))
}

// ---------- Google ----------

struct Google {
    opts: Option<badi_apps::composio::Opts>,
    enabled: bool,
}

fn google(app: &AppHandle) -> Google {
    Google { opts: crate::apps::opts(app), enabled: crate::apps::enabled(app).iter().any(|s| s == "googlecalendar") }
}

/// Fetches the month's Google events (unless fresh, or `force`). Runs in the background; publishes
/// "loading" now and the result when it arrives, if no newer sync started meanwhile. After a failure
/// only a forced refresh (the person's Coba lagi) tries again: no timer, page or month change does.
pub fn refresh(app: &AppHandle, force: bool) {
    let g = google(app);
    let (Some(o), true) = (g.opts, g.enabled) else {
        let mut c = CAL.lock().unwrap();
        if c.demo {
            return;
        }
        c.remote.clear();
        c.remote_range = None;
        c.sync = "off";
        drop(c);
        return publish(app);
    };
    let (my, from, to) = {
        let mut c = CAL.lock().unwrap();
        let (from, to) = range(month_of(&c));
        let fresh = c.remote_range == Some((from, to)) && now_ms() - c.fetched < FRESH_MS && c.sync == "ok";
        let failed = !matches!(c.sync, "ok" | "off" | "loading");
        if c.demo || (!force && (fresh || failed || c.sync == "loading")) {
            return;
        }
        c.generation += 1;
        c.sync = "loading";
        (c.generation, from, to)
    };
    publish(app);
    let app = app.clone();
    std::thread::spawn(move || {
        let zone = iana_time_zone::get_timezone().unwrap_or_default();
        let result = (|| -> Result<(Vec<CalendarInfo>, Vec<CalEvent>), String> {
            let calendars = gcal::calendars(&o)?;
            let now = now_ms();
            let mut events = Vec::new();
            for cal in calendars.iter().filter(|c| c.selected || c.primary).take(MAX_CALENDARS) {
                let raw = gcal::events(&o, &Local, &zone, &cal.id, from, to)?;
                events.extend(raw.iter().filter_map(|r| google_event(&Local, cal, r, now)));
            }
            Ok((calendars, merge([events])))
        })();
        {
            let mut c = CAL.lock().unwrap();
            if c.generation != my {
                return;
            }
            match result {
                Ok((calendars, events)) => {
                    c.calendars = calendars;
                    c.remote = events;
                    c.remote_range = Some((from, to));
                    c.fetched = now_ms();
                    c.synced_at = Some(c.fetched);
                    c.sync = "ok";
                    c.sync_message.clear();
                }
                // The last remote events stay (shown as not fresh); local ones are never touched.
                Err(e) => {
                    if std::env::var_os("BADI_LOG").is_some() {
                        eprintln!("[calendar] sync failed: {}", e.chars().take(200).collect::<String>());
                    }
                    c.sync = sync_error(&e);
                    c.sync_message = e.chars().take(160).collect();
                }
            }
        }
        publish(&app);
        notch::send_status(&app);
    });
}

fn remote_event(id: &str) -> Option<CalEvent> {
    CAL.lock().unwrap().remote.iter().find(|e| e.id == id).cloned()
}

fn set_pending(id: &str, on: bool) {
    let mut c = CAL.lock().unwrap();
    if on {
        c.pending.insert(id.to_string());
    } else {
        c.pending.remove(id);
    }
}

// A change to a Google event in the background: pending until Google answers, then a fresh fetch (or
// a notice saying why it didn't happen).
fn remote_change(app: &AppHandle, id: &str, run: impl FnOnce(&badi_apps::composio::Opts) -> Result<(), String> + Send + 'static) {
    let Some(o) = crate::apps::opts(app) else { return };
    set_pending(id, true);
    publish(app);
    let (app, id) = (app.clone(), id.to_string());
    std::thread::spawn(move || {
        let r = run(&o);
        set_pending(&id, false);
        match r {
            Ok(()) => refresh(&app, true),
            Err(e) => {
                let what = sync_error(&e);
                notch::show_notice(&app, "info", &i18n::t(&app, &format!("calSyncFailed_{what}")), &[e.chars().take(120).collect()], vec![], 9000);
                publish(&app);
            }
        }
    });
}

// ---------- prayer times ----------

fn prayer_city(app: &AppHandle) -> Option<(String, String)> {
    let c = state(app).store.get("prayerCity");
    Some((c["id"].as_str().filter(|s| !s.is_empty())?.to_string(), c["name"].as_str().unwrap_or("").to_string()))
}

fn prayer_on(app: &AppHandle) -> bool {
    state(app).store.get("prayerEnabled") == true
}

fn prayer_cache(app: &AppHandle) -> prayer::Cache {
    serde_json::from_value(state(app).store.get("prayerCache")).unwrap_or_default()
}

/// Makes sure today and tomorrow are in the cache: fetches their month(s) from myQuran when not
/// (at most once every 10 minutes after a failure).
pub fn ensure_prayer(app: &AppHandle) {
    let Some((city, _)) = prayer_city(app).filter(|_| prayer_on(app)) else { return };
    let today = Local::now().date_naive();
    let tomorrow = today.succ_opt().unwrap_or(today);
    let cache = prayer_cache(app);
    let need: Vec<NaiveDate> = [today, tomorrow].into_iter().filter(|d| !cache.has(&city, *d)).collect();
    if need.is_empty() {
        return;
    }
    {
        let mut c = CAL.lock().unwrap();
        if c.prayer_busy || now_ms() - c.prayer_tried < PRAYER_RETRY_MS {
            return;
        }
        c.prayer_busy = true;
        c.prayer_tried = now_ms();
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let mut months: Vec<(i32, u32)> = need.iter().map(|d| (d.year(), d.month())).collect();
        months.dedup();
        let mut error = String::new();
        let mut cache = prayer_cache(&app);
        for (y, m) in months {
            let got = reqwest::blocking::Client::builder().timeout(Duration::from_secs(20)).build().map_err(|e| e.to_string()).and_then(|c| c.get(prayer::month_url(&city, y, m)).send().and_then(|r| r.json::<Value>()).map_err(|e| e.to_string())).and_then(|b| prayer::parse_month(&b));
            match got {
                Ok(days) => cache.add(&city, &format!("{y}-{m:02}"), days, today, now_ms()),
                Err(e) => error = e,
            }
        }
        let mut patch = Map::new();
        patch.insert("prayerCache".into(), serde_json::to_value(&cache).unwrap_or(Value::Null));
        let _ = state(&app).store.remember(&patch);
        {
            let mut c = CAL.lock().unwrap();
            c.prayer_busy = false;
            c.prayer_error = error;
        }
        publish(&app);
        notch::send_status(&app);
    });
}

fn prayer_done(app: &AppHandle) -> Vec<String> {
    let d = state(app).store.get("prayerDone");
    if d["day"].as_str() != Some(&Local::now().date_naive().format("%Y-%m-%d").to_string()) {
        return vec![];
    }
    d["keys"].as_array().into_iter().flatten().filter_map(|k| k.as_str().map(String::from)).collect()
}

fn toggle_prayer_done(app: &AppHandle, key: &str) {
    let mut keys = prayer_done(app);
    match keys.iter().position(|k| k == key) {
        Some(i) => {
            keys.remove(i);
        }
        None => keys.push(key.to_string()),
    }
    let mut patch = Map::new();
    patch.insert("prayerDone".into(), json!({ "day": Local::now().date_naive().format("%Y-%m-%d").to_string(), "keys": keys }));
    let _ = state(app).store.remember(&patch);
    publish(app);
}

fn prayer_today(app: &AppHandle) -> Option<(Vec<(&'static str, i64)>, Option<i64>)> {
    let (city, _) = prayer_city(app)?;
    let cache = prayer_cache(app);
    let today = Local::now().date_naive();
    if cache.city_id != city {
        return None;
    }
    let times = prayer::day(&Local, &cache, today)?;
    let tomorrow = today.succ_opt().and_then(|d| prayer::day(&Local, &cache, d)).map(|t| t[0].1);
    Some((times, tomorrow))
}

fn notify_minutes(app: &AppHandle) -> i64 {
    state(app).store.get("prayerNotify").as_i64().unwrap_or(10)
}

fn prayer_view(app: &AppHandle) -> Value {
    let city = prayer_city(app);
    // Without a city the times can't be right for the person: the page offers to set one up; with one
    // already known (followed from the shared place) it offers to turn the times on. Until the person
    // waves the offer away.
    if !prayer_on(app) || city.is_none() {
        let dismissed = state(app).store.get("prayerGuideDismissed") == true;
        return json!({ "enabled": false, "guide": !dismissed, "suggest": city.map(|c| c.1) });
    }
    let now = now_ms();
    let done = prayer_done(app);
    let today = prayer_today(app);
    let c = CAL.lock().unwrap();
    let times: Vec<Value> = today.as_ref().map(|(t, _)| t.iter().map(|(k, at)| json!({ "key": k, "at": at, "done": done.iter().any(|d| d == k), "prayer": prayer::PRAYERS.contains(k) })).collect()).unwrap_or_default();
    let next = today.as_ref().and_then(|(t, tomorrow)| prayer::next(t, *tomorrow, now)).map(|(k, at, prev)| json!({ "key": k, "at": at, "prev": prev }));
    json!({
        "enabled": true,
        "city": city.map(|c| c.1),
        "method": "Kemenag (Indonesia)",
        "notify": notify_minutes(app),
        "times": times,
        "next": next,
        "loading": c.prayer_busy,
        "error": c.prayer_error,
    })
}

// Each obligatory prayer is announced once, `prayerNotify` minutes before (0: at its time).
fn check_prayer_notices(app: &AppHandle) {
    let minutes = notify_minutes(app);
    if !prayer_on(app) || minutes < 0 {
        return;
    }
    let Some((times, _)) = prayer_today(app) else { return };
    let now = now_ms();
    let day = Local::now().date_naive().format("%Y-%m-%d").to_string();
    for (k, at) in times.into_iter().filter(|(k, _)| prayer::FARD.contains(k)) {
        let from = at - minutes * 60_000;
        if now < from || now > at + 5 * 60_000 {
            continue;
        }
        if !CAL.lock().unwrap().notified.insert(format!("{day}:{k}")) {
            continue;
        }
        let name = prayer_name(app, k);
        let left = ((at - now) as f64 / 60_000.0).round() as i64;
        let line = match (en(app), left > 0) {
            (true, true) => format!("{name} is in {left} minutes."),
            (true, false) => format!("It's time for {name}."),
            (false, true) => format!("{left} menit lagi masuk waktu {name}."),
            (false, false) => format!("Sudah masuk waktu {name}."),
        };
        crate::turn::chime(app, "step");
        notch::show_notice(app, "remind", &format!("🕌 {line}"), &[], vec![], 12_000);
        notch::say_when_free(app, line);
    }
}

fn prayer_name(app: &AppHandle, key: &str) -> String {
    i18n::strings(app)["island"][format!("prayer_{key}")].as_str().unwrap_or(key).to_string()
}

// ---------- what the notch gets ----------

fn prefs(app: &AppHandle) -> Value {
    let st = state(app);
    json!({ "view": st.store.get("reminderView").as_str().unwrap_or("agenda"), "density": st.store.get("reminderDensity").as_str().unwrap_or("compact") })
}

/// The whole page's data for the month on screen.
pub fn payload(app: &AppHandle) -> Value {
    let month = month_of(&CAL.lock().unwrap());
    let (from, to) = range(month);
    let local = local_events(app, from, to);
    let g = google(app);
    let prayer = prayer_view(app);
    let c = CAL.lock().unwrap();
    let remote: Vec<CalEvent> = c
        .remote
        .iter()
        .filter(|e| e.overlaps(from, to))
        .cloned()
        .map(|mut e| {
            if c.pending.contains(&e.id) {
                e.sync = "pending";
                e.actions = calendar::actions_for(&e);
            }
            e
        })
        .collect();
    json!({
        "now": now_ms(),
        "month": format!("{}-{:02}", month.0, month.1),
        "range": [from, to],
        "events": merge([local, remote]),
        "prefs": prefs(app),
        "sync": { "state": c.sync, "message": c.sync_message, "at": c.synced_at, "fresh": c.remote_range == Some((from, to)) },
        "google": {
            "configured": g.opts.is_some() || c.demo,
            "enabled": g.enabled || c.demo,
            "calendars": c.calendars.iter().map(|k| json!({ "id": k.id, "name": k.name, "color": k.color, "writable": k.writable() })).collect::<Vec<_>>(),
        },
        "prayer": prayer,
    })
}

pub fn publish(app: &AppHandle) {
    windows::broadcast(app, "calendar:data", payload(app));
}

/// For the compact notch: the nearest reminder, event or prayer in the next hour.
pub fn next_item(app: &AppHandle) -> Value {
    let now = now_ms();
    let soon = now + 60 * 60_000;
    let mut events = local_events(app, now - 60_000, soon);
    events.extend(CAL.lock().unwrap().remote.iter().filter(|e| e.overlaps(now, soon)).cloned());
    let mut best = calendar::next_up(&events, now).filter(|e| e.due() <= soon).map(|e| json!({ "title": e.title, "at": e.due(), "source": e.source, "kind": e.kind }));
    if prayer_on(app)
        && let Some((times, tomorrow)) = prayer_today(app)
        && let Some((k, at, _)) = prayer::next(&times, tomorrow, now)
        && at <= now + 30 * 60_000
        && best.as_ref().is_none_or(|b| b["at"].as_i64().unwrap_or(i64::MAX) > at)
    {
        best = Some(json!({ "title": prayer_name(app, k), "at": at, "source": "prayer", "kind": "prayer" }));
    }
    best.unwrap_or(Value::Null)
}

// ---------- the notch's buttons (island:act "cal:…") ----------

fn remember(app: &AppHandle, key: &str, v: Value) {
    let mut patch = Map::new();
    patch.insert(key.into(), v);
    let _ = state(app).store.remember(&patch);
}

pub fn act(app: &AppHandle, what: &str) {
    let mut parts = what.splitn(3, ':');
    let (verb, a, b) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let now = now_ms();
    match verb {
        "view" if ["agenda", "timeline", "calendar"].contains(&a) => {
            remember(app, "reminderView", json!(a));
            publish(app);
        }
        "density" if ["compact", "comfortable"].contains(&a) => {
            remember(app, "reminderDensity", json!(a));
            publish(app);
        }
        "month" => {
            let mut ym = a.split('-').filter_map(|x| x.parse::<i64>().ok());
            if let (Some(y), Some(m)) = (ym.next(), ym.next())
                && (1..=12).contains(&m)
            {
                CAL.lock().unwrap().month = Some((y as i32, m as u32));
                publish(app);
                refresh(app, false);
            }
        }
        "open" => {
            refresh(app, false);
            ensure_prayer(app);
        }
        "refresh" => {
            CAL.lock().unwrap().prayer_tried = 0;
            refresh(app, true);
            ensure_prayer(app);
        }
        "add" => open_window(app, "new", "", a),
        "edit" => open_window(app, "edit", what.trim_start_matches("edit:"), ""),
        "connect" => connect_google(app),
        // "done:<event id>", "undo:<event id>", "snooze:<minutes|tomorrow>:<event id>" (ids hold colons).
        "done" | "undo" | "snooze" => {
            let rest = what.split_once(':').map(|x| x.1).unwrap_or("");
            let (b, rest) = if verb == "snooze" { rest.split_once(':').unwrap_or(("10", "")) } else { ("", rest) };
            let Some(id) = local_id(rest).map(String::from) else { return };
            let ok = {
                let st = state(app);
                let mut t = st.timers.lock().unwrap();
                match verb {
                    "done" => t.complete(&id, now, next_of),
                    "undo" => t.reopen(&id, now),
                    _ => {
                        let until = if b == "tomorrow" { calendar::shortcuts(&Local, now)["tomorrow"].as_i64().unwrap_or(now) } else { now + b.parse::<i64>().unwrap_or(10).clamp(1, 1440) * 60_000 };
                        t.snooze(&id, until, now)
                    }
                }
            };
            if ok {
                changed(app);
            }
        }
        "delete" => delete(app, what.trim_start_matches("delete:")),
        "link" => {
            if let Some(link) = remote_event(what.trim_start_matches("link:")).and_then(|e| e.link).filter(|l| l.starts_with("https://")) {
                let _ = badi_agent::apps::shell_open(&link);
            }
        }
        "prayer" if a == "done" && prayer::PRAYERS.contains(&b) => toggle_prayer_done(app, b),
        // The setup guide: to Settings → Pengingat, or away for good.
        "prayer" if a == "setup" => notch::open_settings_at(app, "prayerSetup"),
        "prayer" if a == "enable" && prayer_city(app).is_some() => {
            remember(app, "prayerEnabled", json!(true));
            settings_changed(app);
        }
        "prayer" if a == "dismiss" => {
            remember(app, "prayerGuideDismissed", json!(true));
            publish(app);
        }
        _ => {}
    }
}

fn delete(app: &AppHandle, id: &str) {
    if let Some(lid) = local_id(id) {
        let removed = state(app).timers.lock().unwrap().remove_reminder(lid);
        if removed {
            changed(app);
        }
        return;
    }
    let Some(e) = remote_event(id).filter(|e| e.editable) else { return };
    let (cal, eid) = (e.calendar_id.clone().unwrap_or_default(), e.remote_id.clone().unwrap_or_default());
    remote_change(app, id, move |o| gcal::delete(o, &cal, &eid));
}

/// "Sambungkan Google Calendar": its sign-in when Composio is set up, else Settings → work apps. The
/// next sync after it is the person's own request, so it may run even after a failure.
fn connect_google(app: &AppHandle) {
    if crate::apps::opts(app).is_none() {
        return notch::open_settings_at(app, "appsCard");
    }
    {
        let mut c = CAL.lock().unwrap();
        if !matches!(c.sync, "ok" | "loading") {
            c.sync = "off";
        }
    }
    if let Err(e) = crate::apps::connect_app(app, "Google Calendar") {
        notch::show_notice(app, "info", &e, &[], vec![], 8000);
    }
}

fn open_window(app: &AppHandle, mode: &str, id: &str, day: &str) {
    if let Some(w) = app.get_webview_window("reminder") {
        let _ = w.destroy();
        for _ in 0..40 {
            if app.get_webview_window("reminder").is_none() {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }
    let enc = |s: &str| s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect::<String>();
    let built = windows::builder(app, "reminder", windows::page("reminder.html"))
        .title(if en(app) { "aina · Reminder" } else { "aina · Pengingat" })
        .inner_size(420.0, 600.0)
        .resizable(true)
        .always_on_top(true)
        .visible(false)
        .initialization_script(windows::init_script(&format!("?mode={mode}&id={}&day={}", enc(id), enc(day))))
        .build();
    if built.is_ok() {
        windows::reveal_when_ready(app, "reminder", true);
    }
}

// ---------- the reminder window's calls ----------

fn event_by_id(app: &AppHandle, id: &str) -> Option<CalEvent> {
    if let Some(lid) = local_id(id) {
        let st = state(app);
        let t = st.timers.lock().unwrap();
        return t.reminders.iter().find(|r| r.id == lid).and_then(|r| from_local(&Local, r, i64::MIN / 2, i64::MAX / 2, 1).into_iter().next());
    }
    remote_event(id)
}

pub fn invoke(app: &AppHandle, ch: &str, arg: &Value) -> Result<Value, String> {
    match ch {
        "reminder:get" => {
            let id = arg["id"].as_str().unwrap_or("");
            let g = google(app);
            let c = CAL.lock().unwrap();
            let writable: Vec<Value> = if g.enabled && g.opts.is_some() { c.calendars.iter().filter(|k| k.writable()).map(|k| json!({ "id": k.id, "name": k.name })).collect() } else { vec![] };
            drop(c);
            Ok(json!({
                "now": now_ms(),
                "item": if id.is_empty() { Value::Null } else { serde_json::to_value(event_by_id(app, id)).unwrap_or(Value::Null) },
                "shortcuts": calendar::shortcuts(&Local, now_ms()),
                "google": { "configured": g.opts.is_some(), "enabled": g.enabled, "calendars": writable },
                "prefs": prefs(app),
            }))
        }
        "reminder:save" => save(app, arg),
        "reminder:delete" => {
            delete(app, arg["id"].as_str().unwrap_or(""));
            Ok(json!(true))
        }
        "reminder:cities" => {
            let q = arg["q"].as_str().unwrap_or("").trim();
            if q.chars().count() < 3 {
                return Ok(json!([]));
            }
            let body: Value = reqwest::blocking::Client::builder().timeout(Duration::from_secs(15)).build().map_err(|e| e.to_string())?.get(prayer::search_url(q)).send().and_then(|r| r.json()).map_err(|e| e.to_string())?;
            Ok(json!(prayer::parse_cities(&body).into_iter().map(|(id, name)| json!({ "id": id, "name": name })).collect::<Vec<_>>()))
        }
        _ => Err(format!("unknown: {ch}")),
    }
}

// Creates or changes a reminder (local) or an event (Google). Google answers before this returns: the
// window shows an error instead of claiming it saved.
fn save(app: &AppHandle, arg: &Value) -> Result<Value, String> {
    let en = en(app);
    let title = arg["title"].as_str().unwrap_or("").trim().to_string();
    if title.is_empty() {
        return Err(if en { "Give it a title." } else { "Isi judulnya dulu." }.into());
    }
    let at = arg["at"].as_i64().ok_or(if en { "Pick a time." } else { "Pilih waktunya." })?;
    let end = arg["end"].as_i64().filter(|e| *e > at);
    let all_day = arg["allDay"] == true;
    let note = arg["note"].as_str().unwrap_or("").trim().to_string();
    let notify = arg["notify"] != false;
    let repeat = arg["repeat"].as_str().filter(|r| ["daily", "weekdays", "weekly", "monthly"].contains(r)).map(String::from);
    let id = arg["id"].as_str().unwrap_or("");
    let dest = arg["dest"].as_str().unwrap_or("local");
    let now = now_ms();
    let zone = iana_time_zone::get_timezone().unwrap_or_default();

    // A Google event: changed or created there, never stored locally.
    let google_target = if id.starts_with("google:") { remote_event(id).map(|e| (e.calendar_id.unwrap_or_default(), e.remote_id, e.editable)) } else { dest.strip_prefix("google:").map(|c| (c.to_string(), None, true)) };
    if let Some((cal, remote_id, editable)) = google_target {
        if !editable {
            return Err(if en { "This calendar is read-only, so the event can't be changed from here." } else { "Kalender ini hanya bisa dibaca, jadi acaranya tidak bisa diubah dari sini." }.into());
        }
        if all_day {
            return Err(if en { "All-day events can't be saved to Google from aina yet." } else { "Acara sepanjang hari belum bisa disimpan ke Google dari aina." }.into());
        }
        let o = crate::apps::opts(app).ok_or("Composio is not set up.")?;
        let draft = gcal::Draft { title: &title, description: &note, start: at, end: end.unwrap_or(at + 30 * 60_000), zone: &zone, rrule: repeat.as_deref().and_then(gcal::rrule) };
        let r = match &remote_id {
            Some(eid) => gcal::patch(&o, &Local, &cal, eid, &draft),
            None => gcal::create(&o, &Local, &cal, &draft).map(|_| ()),
        };
        r.map_err(|e| format!("{} ({})", i18n::t(app, &format!("calSyncFailed_{}", sync_error(&e))), e.chars().take(140).collect::<String>()))?;
        refresh(app, true);
        return Ok(json!({ "saved": "google" }));
    }

    let mut r = match local_id(id) {
        Some(lid) => state(app).timers.lock().unwrap().reminders.iter().find(|r| r.id == lid).cloned().ok_or(if en { "That reminder is gone." } else { "Pengingat itu sudah tidak ada." })?,
        None => Reminder::new(String::new(), at, title.clone()),
    };
    r.at = at;
    r.text = title;
    r.note = note;
    r.end = end;
    r.all_day = all_day;
    r.notify = notify;
    r.repeat = repeat;
    r.tz = (!zone.is_empty()).then_some(zone);
    if id.is_empty() {
        r.done = false;
    }
    state(app).timers.lock().unwrap().put(r, Some(now));
    changed(app);
    Ok(json!({ "saved": "local" }))
}

/// Settings saved reminder or prayer options: the page and the prayer times follow at once.
pub fn settings_changed(app: &AppHandle) {
    CAL.lock().unwrap().prayer_tried = 0;
    ensure_prayer(app);
    publish(app);
    notch::send_status(app);
}

// ---------- background ----------

/// Every minute: prayer notices and the prayer cache; every 10 minutes: Google events.
pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(5));
        let mut last_sync = std::time::Instant::now() - AUTO_REFRESH;
        let mut last_day = String::new();
        loop {
            ensure_prayer(&app);
            check_prayer_notices(&app);
            let day = Local::now().date_naive().to_string();
            if last_sync.elapsed() >= AUTO_REFRESH {
                last_sync = std::time::Instant::now();
                refresh(&app, false);
            }
            // A new day: the page's "today", the prayer list and its ticks change.
            if day != last_day {
                last_day = day;
                publish(&app);
            }
            std::thread::sleep(Duration::from_secs(30));
        }
    });
}

// ---------- BADI_DEMO=reminders ----------

/// Remote events as if fetched from Google (no request is made).
pub fn demo_remote(app: &AppHandle, calendars: Vec<CalendarInfo>, raw: Vec<(usize, Value)>) {
    {
        let mut c = CAL.lock().unwrap();
        c.demo = true;
        let now = now_ms();
        c.remote = merge([raw.iter().filter_map(|(i, r)| google_event(&Local, &calendars[*i], r, now)).collect::<Vec<_>>()]);
        let (from, to) = range(month_of(&c));
        c.remote_range = Some((from, to));
        c.calendars = calendars;
        c.sync = "ok";
        c.synced_at = Some(now);
    }
    publish(app);
    notch::send_status(app);
}

pub fn demo_sync(app: &AppHandle, s: &'static str, message: &str) {
    {
        let mut c = CAL.lock().unwrap();
        c.sync = s;
        c.sync_message = message.into();
    }
    publish(app);
}

pub fn demo_prayer(app: &AppHandle, city: (&str, &str), days: std::collections::BTreeMap<String, Vec<String>>) {
    let mut cache = prayer::Cache::default();
    cache.add(city.0, "demo", days, Local::now().date_naive(), now_ms());
    let mut patch = Map::new();
    patch.insert("prayerEnabled".into(), json!(true));
    patch.insert("prayerCity".into(), json!({ "id": city.0, "name": city.1 }));
    patch.insert("prayerCache".into(), serde_json::to_value(cache).unwrap_or(Value::Null));
    let _ = state(app).store.remember(&patch);
    publish(app);
}
