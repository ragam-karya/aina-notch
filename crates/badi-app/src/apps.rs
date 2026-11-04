// Work apps (Composio) and media in the app: Settings → Aplikasi kerja (list, switches, linking,
// white-label sign-in), the work-app tools for the Brain and agents with their spoken approvals,
// connect_app, Google Drive uploads from a drop or upload_to_drive with progress in the notch, and
// now playing / media control for the notch. Port of v1's main.js work-app and media parts.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::time::Duration;

use badi_agent::{AppChange, Need};
use badi_apps::composio::{self, Opts, is_connect_key};
use badi_apps::workapps::{self, APPS, approval_for, brands, clip, connect_app_tool, find_app, prompt_line, soon, upload_to_drive_tool};
use badi_apps::media;
use badi_core::notch::{Entry, classify_drop};
use serde_json::{Map, Value, json};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

use crate::i18n::{self, t};
use crate::notch::{self, Action};
use crate::{AppState, talk};

fn state(app: &AppHandle) -> tauri::State<'_, AppState> {
    app.state::<AppState>()
}

fn en(app: &AppHandle) -> bool {
    i18n::lang(app) == "en"
}

/// The Composio options, or None when no key is set.
pub fn opts(app: &AppHandle) -> Option<Opts> {
    let st = state(app);
    let api_key = st.store.secret("composioApiKey").filter(|k| !k.is_empty())?;
    let mut user_id = st.store.get("composioUserId").as_str().unwrap_or("").to_string();
    if user_id.is_empty() {
        use std::hash::{BuildHasher, Hasher};
        let part = || std::collections::hash_map::RandomState::new().build_hasher().finish();
        user_id = format!("badi_{:016x}{:016x}", part(), part());
        let mut patch = Map::new();
        patch.insert("composioUserId".into(), json!(user_id));
        let _ = st.store.remember(&patch);
    }
    // The IANA zone (v1: Intl's timeZone), so "jadwal hari ini" means today here.
    let timezone = iana_time_zone::get_timezone().unwrap_or_default();
    Some(Opts { api_key, user_id, timezone, auth_configs: st.store.get("composioAuthConfigs") })
}

/// The apps switched on in Settings.
pub fn enabled(app: &AppHandle) -> Vec<String> {
    let on = state(app).store.get("appsEnabled");
    APPS.iter().filter(|a| on[a.slug] == true && !soon(a.slug)).map(|a| a.slug.to_string()).collect()
}

fn set_enabled(app: &AppHandle, slug: &str, on: bool) {
    let st = state(app);
    let mut all = st.store.get("appsEnabled");
    if !all.is_object() {
        all = json!({});
    }
    all[slug] = json!(on);
    let mut patch = Map::new();
    patch.insert("appsEnabled".into(), all);
    let _ = st.store.remember(&patch);
}

// Apps whose sign-in page is open (slug, its link, since when): Settings shows them as waiting however
// often it reloads, and switching one on again reopens the same link instead of asking Composio for
// another (new links invalidate the old page and too many in a row are refused with "Too many requests").
static SIGNING_IN: std::sync::Mutex<Vec<(String, String, std::time::Instant)>> = std::sync::Mutex::new(Vec::new());
// Composio's links last 10 minutes; one older than this is not reopened.
const LINK_REUSE: Duration = Duration::from_secs(8 * 60);

fn signing_in(slug: &str, url: Option<String>) {
    let mut all = SIGNING_IN.lock().unwrap();
    all.retain(|s| s.0 != slug);
    if let Some(url) = url {
        all.push((slug.to_string(), url, std::time::Instant::now()));
    }
}

fn is_signing_in(slug: &str) -> bool {
    SIGNING_IN.lock().unwrap().iter().any(|s| s.0 == slug)
}

/// Stops waiting for an app's sign-in: switched off, Batal, or the page was closed without finishing.
fn cancel_sign_in(slug: &str) {
    if is_signing_in(slug) {
        signing_in(slug, None);
        composio::cancel(slug);
    }
}

// A sign-in that Settings is waiting for ended: linked, or not (with why).
fn linked(app: &AppHandle, slug: &str, failed: Option<String>) {
    signing_in(slug, None);
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.emit("apps:changed", json!({ "slug": slug, "linked": failed.is_none(), "error": failed }));
    }
}

// ---------- the spoken question ----------

/// A work-app action in Badi's words: "Aku mau kirim email (budi@x.com) di Gmail. Boleh?"
pub fn app_question(app: &AppHandle, who: Option<&str>, changes: &[AppChange]) -> String {
    let e = en(app);
    let me = match (who, e) {
        (Some(n), true) => format!("{n} wants to"),
        (Some(n), false) => format!("{n} mau"),
        (None, true) => "I want to".into(),
        (None, false) => "Aku mau".into(),
    };
    if changes.len() > 1 {
        let mut apps: Vec<String> = changes.iter().filter_map(|c| c.app.clone()).collect();
        apps.dedup();
        let apps = if apps.is_empty() { "app".to_string() } else { apps.join(&format!(" {} ", t(app, "and"))) };
        return if e { format!("{me} run {} actions in {apps}. Okay?", changes.len()) } else { format!("{me} jalankan {} aksi di {apps}. Boleh?", changes.len()) };
    }
    let c = &changes[0];
    let key = format!("appVerb_{}", c.verb);
    let verb = Some(t(app, &key)).filter(|v| *v != key).unwrap_or_else(|| c.verb.to_lowercase());
    let target = if c.target.is_empty() { String::new() } else { format!("({})", c.target) };
    let what = [verb, c.object.clone(), target].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ");
    match (&c.app, e) {
        (Some(a), true) => format!("{me} {what} in {a}. Okay?"),
        (Some(a), false) => format!("{me} {what} di {a}. Boleh?"),
        (None, true) => format!("{me} {what}. Okay?"),
        (None, false) => format!("{me} {what}. Boleh?"),
    }
}

// ---------- tools for the Brain and agents ----------

pub struct WorkTools {
    pub tools: Vec<Value>,
    pub line: String,
}

/// The work-app tools for one task, or None when Composio isn't set up or doesn't answer in 8 s (the
/// task then runs without them).
pub fn work_tools(app: &AppHandle) -> Option<WorkTools> {
    let o = opts(app)?;
    let (tx, rx) = channel();
    let o2 = o.clone();
    std::thread::spawn(move || {
        let r = composio::with_session(&o2, |s| Ok((s.tools()?, s.status()?)));
        let _ = tx.send(r);
    });
    let (mut tools, linked) = match rx.recv_timeout(Duration::from_secs(8)) {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => {
            eprintln!("[apps] {e}");
            return None;
        }
        Err(_) => {
            eprintln!("[apps] Composio did not answer in time");
            return None;
        }
    };
    let on = enabled(app);
    let apps: Vec<(String, bool, bool)> = APPS.iter().filter(|a| !soon(a.slug)).map(|a| (a.slug.to_string(), linked.get(a.slug).is_some_and(Option::is_some), on.iter().any(|s| s == a.slug))).collect();
    // Uploading local files needs a project key; the Connect endpoint can't take file contents.
    let can_upload = !is_connect_key(&o.api_key);
    tools.push(connect_app_tool());
    if can_upload {
        tools.push(upload_to_drive_tool());
    }
    let mut line = prompt_line(&apps);
    line.push_str(" The other work apps (Outlook, Teams, Notion, Linear, Jira, ClickUp, HubSpot, Salesforce, Dropbox, GitHub, Slack) are coming soon: if asked, say they aren't available yet instead of trying them.");
    if can_upload {
        line.push_str(" To put files from this PC into Google Drive, use upload_to_drive.");
    }
    Some(WorkTools { tools, line })
}

/// Runs a work-app tool: None when `name` isn't one. `approve(need)` returns why the person said no.
pub fn run_tool(app: &AppHandle, name: &str, input: &Value, approve: &(dyn Fn(&Need) -> Option<String> + Sync)) -> Option<Result<String, String>> {
    match name {
        "connect_app" => Some(connect_app(app, input["app"].as_str().unwrap_or(""))),
        "upload_to_drive" => Some(upload_tool(app, input, approve)),
        n if n.starts_with("COMPOSIO_") => Some((|| {
            let o = opts(app).ok_or("Composio is not set up.")?;
            if n == "COMPOSIO_MULTI_EXECUTE_TOOL" {
                let on = enabled(app);
                let mut off: Vec<String> = input["tools"].as_array().into_iter().flatten().filter_map(|x| workapps::app_of(x["tool_slug"].as_str().unwrap_or(""))).filter(|a| !on.iter().any(|s| s == a)).map(workapps::app_name).collect();
                off.dedup();
                if !off.is_empty() {
                    return Err(format!("{} is switched off in aina's Settings, so it can't be used. Tell the user they can switch it on in Settings → Aplikasi kerja.", off.join(", ")));
                }
            }
            if let Some(need) = approval_for(n, input)
                && let Some(no) = approve(&need)
            {
                return Err(no);
            }
            let r = composio::with_session(&o, |s| s.execute(n, input))?;
            if !r.ok && !r.error.is_empty() {
                return Err(r.error);
            }
            Ok(clip(&match r.data {
                Value::String(s) => s,
                d => d.to_string(),
            }))
        })()),
        _ => None,
    }
}

/// Opens an app's sign-in page (and switches the app on); when it completes, Badi says so.
pub fn connect_app(app: &AppHandle, name: &str) -> Result<String, String> {
    let o = opts(app).ok_or("Composio is not set up. The user can add a Composio API key in Settings.")?;
    let found = find_app(name).ok_or_else(|| format!("\"{name}\" is not one of the apps aina can link: {}.", APPS.iter().map(|a| a.name).collect::<Vec<_>>().join(", ")))?;
    let a = app.clone();
    let shown = found.name;
    let slug = found.slug;
    if soon(slug) {
        return Err(if en(app) { format!("{shown} is coming soon to aina; for now only the Google apps can be linked.") } else { format!("{shown} segera hadir di aina. Untuk sekarang baru aplikasi Google yang bisa disambungkan.") });
    }
    let open = if en(app) { format!("I've opened the {shown} sign-in page in your browser. Finish signing in there.") } else { format!("Aku buka halaman login {shown} di browser. Selesaikan login di sana ya.") };
    // Still waiting for this app: the same page again.
    let fresh = SIGNING_IN.lock().unwrap().iter().find(|s| s.0 == slug && s.2.elapsed() < LINK_REUSE).map(|s| s.1.clone());
    if let Some(url) = fresh {
        badi_agent::apps::shell_open(&url)?;
        return Ok(open);
    }
    let url = composio::connect(&o, slug, move |r| match r {
        Ok(()) => {
            // Asking to link an app means wanting to use it, unless it was switched off while signing in.
            if is_signing_in(slug) {
                set_enabled(&a, slug, true);
            }
            linked(&a, slug, None);
            let body = if en(&a) { format!("{shown} is connected.") } else { format!("{shown} sudah tersambung.") };
            let _ = a.notification().builder().title("aina").body(&body).show();
        }
        // Cancelled here: Settings already shows the switch off and why, so nothing more is said.
        Err(e) if e == composio::CANCELLED_ERR => set_enabled(&a, slug, false),
        // The sign-in failed or wasn't finished in time: the switch goes back off, saying why.
        Err(e) => {
            eprintln!("[apps] connect {shown}: {e}");
            set_enabled(&a, slug, false);
            let why = if en(&a) { format!("{shown} wasn't connected: the sign-in wasn't finished. Switch it on to try again.") } else { format!("{shown} belum tersambung: login-nya tidak selesai. Nyalakan saklarnya lagi untuk mencoba.") };
            linked(&a, slug, Some(why));
        }
    })?;
    signing_in(slug, Some(url.clone()));
    badi_agent::apps::shell_open(&url)?;
    Ok(open)
}

// ---------- Google Drive ----------

fn upload_tool(app: &AppHandle, input: &Value, approve: &(dyn Fn(&Need) -> Option<String> + Sync)) -> Result<String, String> {
    let focus = crate::agents::current_focus(app);
    let list: Vec<PathBuf> = match &input["paths"] {
        Value::Array(a) => a.iter().filter_map(Value::as_str).map(String::from).collect(),
        Value::String(s) => vec![s.clone()],
        _ => vec![],
    }
    .iter()
    .map(|p| badi_agent::files::resolve_user_path(p, focus.as_deref()))
    .collect::<Result<_, _>>()?;
    let mut entries = Vec::new();
    for p in &list {
        let m = std::fs::metadata(p).map_err(|_| format!("{} does not exist. Find the exact path with find_files first.", p.display()))?;
        if let Some(why) = badi_agent::guard::protected_reason(p, None) {
            return Err(format!("{why}; aina doesn't upload from there."));
        }
        entries.push(Entry { path: p.display().to_string(), dir: m.is_dir(), size: m.len() });
    }
    let r = classify_drop(&entries, "upload");
    if r["problem"] == "tooBig" {
        return Err(format!("{} is over 100 MB; aina can't upload it.", r["path"].as_str().unwrap_or("")));
    }
    if r.get("problem").is_some() {
        return Err("Give paths of files, not folders.".into());
    }
    let files: Vec<PathBuf> = r["upload"].as_array().into_iter().flatten().filter_map(Value::as_str).map(PathBuf::from).collect();
    let names = files.iter().filter_map(|f| f.file_name()).map(|n| n.to_string_lossy().into_owned()).collect::<Vec<_>>().join(", ");
    let change = workapps::describe_change("GOOGLEDRIVE_UPLOAD_FILE", &json!({ "name": names }));
    if let Some(no) = approve(&Need { kind: "app".into(), op: "upload".into(), changes: vec![change], detail: names, ..Need::default() }) {
        return Err(no);
    }
    let (ok, say, done) = upload_to_drive(app, &files);
    if !ok {
        return Err(say);
    }
    Ok(format!("Uploaded to Google Drive: {}.", done.iter().map(|(n, link)| match link {
        Some(l) => format!("{n} ({l})"),
        None => n.clone(),
    }).collect::<Vec<_>>().join(", ")))
}

/// Uploads files to the person's Drive with progress in the notch. Returns (ok, what to say, files).
/// A drop into the upload zone is the person's OK; upload_to_drive asks first like any app change.
pub fn upload_to_drive(app: &AppHandle, paths: &[PathBuf]) -> (bool, String, Vec<(String, Option<String>)>) {
    let Some(o) = opts(app) else { return (false, t(app, "driveNoComposio"), vec![]) };
    if is_connect_key(&o.api_key) {
        return (false, t(app, "driveNeedsProjectKey"), vec![]);
    }
    let linked = composio::status(&o).map(|m| m.get("googledrive").copied().unwrap_or(false)).unwrap_or(false);
    if !linked {
        let _ = connect_app(app, "googledrive");
        return (false, t(app, "driveNotLinked"), vec![]);
    }
    set_enabled(app, "googledrive", true); // using it means wanting it on
    // The notch shrinks to a cloud and a progress bar. Composio reports no byte progress, so the bar
    // moves through each file's stages (sending, then saving to Drive) and fills only when it's in Drive.
    let n = paths.len();
    let progress = |i: usize, stage: &str, file: &Path| {
        let name = file.file_name().map(|x| x.to_string_lossy().into_owned()).unwrap_or_default();
        let title = if en(app) { format!("Uploading {name}{}…", if n > 1 { format!(" ({}/{n})", i + 1) } else { String::new() }) } else { format!("Mengunggah {name}{}…", if n > 1 { format!(" ({}/{n})", i + 1) } else { String::new() }) };
        let size = std::fs::metadata(file).map(|m| m.len()).unwrap_or(0);
        let mut extra = Map::new();
        extra.insert("progress".into(), json!({ "i": i, "n": n, "stage": stage, "size": size, "at": notch::now_ms() }));
        notch::show_panel(app, "upload", &title, &[], vec![], 0, extra);
    };
    let mut done = Vec::new();
    for (i, p) in paths.iter().enumerate() {
        progress(i, "send", p);
        match composio::upload(&o, p, &|stage| progress(i, stage, p)) {
            Ok((name, _, link)) => done.push((name, link)),
            Err(e) => {
                notch::clear_panel_kind(app, "upload");
                eprintln!("[apps] drive upload: {e}");
                let say = if e == "NEEDS_PROJECT_KEY" { t(app, "driveNeedsProjectKey") } else if en(app) { format!("The upload failed: {e}") } else { format!("Upload-nya gagal: {e}") };
                return (false, say, done);
            }
        }
    }
    let mut extra = Map::new();
    extra.insert("progress".into(), json!({ "i": n, "n": n, "stage": "done", "size": 0, "at": notch::now_ms() }));
    notch::show_panel(app, "upload", "", &[], vec![], 0, extra);
    std::thread::sleep(Duration::from_millis(450)); // let the bar fill before the notice
    notch::clear_panel_kind(app, "upload");
    let say = i18n::uploaded(app, done.len());
    let actions = match done.last().and_then(|d| d.1.clone()) {
        Some(link) => vec![Action::new("openUpload", t(app, "notchOpen"), "", Some(std::sync::Arc::new(move |_: &AppHandle| {
            let _ = badi_agent::apps::shell_open(&link);
        })))],
        None => vec![],
    };
    let lines: Vec<String> = done.iter().take(3).map(|d| d.0.clone()).collect();
    notch::show_notice(app, "uploaded", &format!("✓ {say}"), &lines, actions, 12_000);
    (true, say, done)
}

/// Files dropped on the upload zone.
pub fn dropped_files(app: &AppHandle, paths: Vec<PathBuf>, skipped: u64) {
    let (ok, mut say, _) = upload_to_drive(app, &paths);
    if ok && skipped > 0 {
        say.push(' ');
        say.push_str(&if en(app) { format!("I skipped {skipped} folder{} and uploaded only the files.", if skipped > 1 { "s" } else { "" }) } else { format!("{skipped} folder aku lewati, yang aku upload cuma file-nya.") });
    }
    talk::say(app, &say);
}

// ---------- Settings → Aplikasi kerja ----------

fn brands_view(app: &AppHandle) -> Value {
    let st = state(app);
    let own = st.store.get("composioAuthConfigs");
    let key = st.store.secret("composioApiKey").unwrap_or_default();
    json!({
        "kind": if key.is_empty() { Value::Null } else if is_connect_key(&key) { json!("connect") } else { json!("project") },
        "redirect": workapps::REDIRECT_URI,
        "brands": brands().iter().map(|b| json!({ "id": b.id, "name": b.name, "console": b.console, "on": b.slugs.iter().all(|s| own[*s].is_string()) })).collect::<Vec<_>>(),
    })
}

pub fn invoke(app: &AppHandle, ch: &str, arg: &Value) -> Result<Value, String> {
    let st = state(app);
    match ch {
        "apps:list" => {
            let on = enabled(app);
            let waiting: Vec<String> = SIGNING_IN.lock().unwrap().iter().map(|s| s.0.clone()).collect();
            let with = |linked: &dyn Fn(&str) -> bool| -> Vec<Value> {
                APPS.iter()
                    .map(|a| {
                        let connected = linked(a.slug);
                        // On but not linked (an old sign-in that never finished): shown off, so the switch matches the status.
                        json!({ "slug": a.slug, "name": a.name, "soon": soon(a.slug), "connected": connected, "enabled": connected && on.iter().any(|s| s == a.slug), "pending": !connected && waiting.iter().any(|s| s == a.slug) })
                    })
                    .collect()
            };
            let Some(o) = opts(app) else { return Ok(json!({ "ready": false, "apps": with(&|_| false) })) };
            Ok(match composio::status(&o) {
                Ok(m) => json!({ "ready": true, "apps": with(&|s| m.get(s).copied().unwrap_or(false)) }),
                Err(e) => json!({ "ready": false, "error": e, "apps": with(&|_| false) }),
            })
        }
        // The on/off switch: on links the app first when it isn't linked yet; off keeps the link.
        "apps:toggle" => {
            let slug = arg["slug"].as_str().unwrap_or("");
            let on = arg["on"].as_bool().unwrap_or(false);
            if on && soon(slug) {
                return Err(if en(app) { "Coming soon.".into() } else { "Segera hadir.".into() });
            }
            if on && arg["connected"] != true {
                return connect_app(app, slug).map(Value::String);
            }
            if !on {
                cancel_sign_in(slug);
            }
            set_enabled(app, slug, on);
            Ok(Value::Null)
        }
        "apps:connect" => connect_app(app, arg.as_str().unwrap_or("")).map(Value::String),
        "apps:disconnect" => {
            let slug = arg.as_str().unwrap_or("");
            set_enabled(app, slug, false);
            // Batal on a sign-in still open: nothing is linked yet, so only the waiting stops.
            if is_signing_in(slug) {
                cancel_sign_in(slug);
                return Ok(Value::Null);
            }
            if let Some(o) = opts(app) {
                composio::disconnect(&o, slug)?;
            }
            Ok(Value::Null)
        }
        "apps:brands" => Ok(brands_view(app)),
        "apps:brand" => {
            let o = opts(app).ok_or("Simpan Composio API key dulu.")?;
            let id = arg["id"].as_str().unwrap_or("");
            let all = brands();
            let b = all.iter().find(|b| b.id == id).ok_or_else(|| format!("Unknown provider \"{id}\"."))?;
            let mut own = st.store.get("composioAuthConfigs").as_object().cloned().unwrap_or_default();
            let old: Vec<String> = b.slugs.iter().filter_map(|s| own.get(*s).and_then(Value::as_str).map(String::from)).collect();
            // The secret goes to Composio only.
            let made = composio::brand(&o, &b.slugs, arg["clientId"].as_str().unwrap_or(""), arg["clientSecret"].as_str().unwrap_or(""))?;
            if !old.is_empty() {
                composio::unbrand(&o, &old);
            }
            own.extend(made);
            let mut patch = Map::new();
            patch.insert("composioAuthConfigs".into(), Value::Object(own));
            let _ = st.store.remember(&patch);
            composio::forget();
            Ok(brands_view(app))
        }
        "apps:unbrand" => {
            let id = arg.as_str().unwrap_or("");
            let mut own = st.store.get("composioAuthConfigs").as_object().cloned().unwrap_or_default();
            if let Some(b) = brands().iter().find(|b| b.id == id) {
                let ids: Vec<String> = b.slugs.iter().filter_map(|s| own.get(*s).and_then(Value::as_str).map(String::from)).collect();
                if let (false, Some(o)) = (ids.is_empty(), opts(app)) {
                    composio::unbrand(&o, &ids);
                }
                for s in &b.slugs {
                    own.remove(*s);
                }
            }
            let mut patch = Map::new();
            patch.insert("composioAuthConfigs".into(), Value::Object(own));
            let _ = st.store.remember(&patch);
            composio::forget();
            Ok(brands_view(app))
        }
        _ => Err(format!("unknown channel: {ch}")),
    }
}

// ---------- media ----------

static WATCHING: AtomicBool = AtomicBool::new(false);
/// The track seen last (its key), so a new one is noticed once.
static TRACK: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());
/// The current track's synced lyrics, sent to the notch as `media:lyrics` { key, lines: [[ms, text]] | null }.
static LYRICS: std::sync::Mutex<Value> = std::sync::Mutex::new(Value::Null);

/// Follows what Windows plays from start to quit, through its media events (polling every 3 s when
/// they can't be had), so the notch's compact pill knows a song is on without being hovered. A new
/// track sends its cover and opens the notch's track card for a moment (`media:track`); its lyrics
/// follow from LRCLIB unless notchLyrics is off. Nothing of it is written to disk.
pub fn start_media_watch(app: AppHandle) {
    let (tx, rx) = channel();
    let events = media::watch(tx);
    std::thread::spawn(move || {
        loop {
            if events {
                if rx.recv().is_err() {
                    return;
                }
                // A change comes as a burst of events; read once it settles.
                std::thread::sleep(Duration::from_millis(150));
                while rx.try_recv().is_ok() {}
            } else {
                std::thread::sleep(Duration::from_secs(3));
            }
            media_changed(&app);
        }
    });
}

/// BADI_DEMO=notch: a pretend song stays put (Windows' real session is not read meanwhile).
static DEMO_MEDIA: AtomicBool = AtomicBool::new(false);

/// BADI_DEMO=notch: shows `now` as what plays, with its cover and lyrics, as a new track would.
pub fn demo_media(app: &AppHandle, now: Value, art: Value, lines: Value) {
    DEMO_MEDIA.store(true, Ordering::SeqCst);
    let key = now["key"].as_str().unwrap_or("").to_string();
    *media::LAST.lock().unwrap() = now;
    TRACK.lock().unwrap().clone_from(&key);
    *LYRICS.lock().unwrap() = json!({ "key": key, "lines": lines });
    crate::windows::broadcast(app, "media:art", json!({ "key": key, "src": art }));
    crate::windows::broadcast(app, "media:lyrics", LYRICS.lock().unwrap().clone());
    crate::windows::broadcast(app, "media:track", key);
    notch::send_status(app);
}

fn media_changed(app: &AppHandle) {
    if DEMO_MEDIA.load(Ordering::SeqCst) {
        return notch::send_status(app);
    }
    let now = media::now();
    let key = now["key"].as_str().unwrap_or("").to_string();
    let fresh = {
        let mut last = TRACK.lock().unwrap();
        let fresh = *last != key;
        last.clone_from(&key);
        fresh
    };
    if fresh && !key.is_empty() {
        crate::windows::broadcast(app, "media:art", json!({ "key": key, "src": media::art(&key) }));
        if now["playing"] == true {
            crate::windows::broadcast(app, "media:track", key.clone());
        }
        fetch_lyrics(app, &now);
    }
    notch::send_status(app);
}

fn fetch_lyrics(app: &AppHandle, now: &Value) {
    let key = now["key"].as_str().unwrap_or("").to_string();
    *LYRICS.lock().unwrap() = json!({ "key": key, "lines": null });
    if state(app).store.get("notchLyrics") == false {
        return;
    }
    let (app, now) = (app.clone(), now.clone());
    std::thread::spawn(move || {
        let s = |k: &str| now[k].as_str().unwrap_or("").to_string();
        let lines = media::lyrics(&s("title"), &s("artist"), &s("album"), now["dur"].as_i64());
        let msg = json!({ "key": key, "lines": lines.map(|l| l.into_iter().map(|(t, text)| json!([t, text])).collect::<Vec<_>>()) });
        // Only if the song is still the same one.
        if *TRACK.lock().unwrap() == key {
            *LYRICS.lock().unwrap() = msg.clone();
            crate::windows::broadcast(&app, "media:lyrics", msg);
        }
    });
}

/// For an overlay that just loaded: the current cover and lyrics.
pub fn send_media_extras(app: &AppHandle, label: &str) {
    let key = TRACK.lock().unwrap().clone();
    if key.is_empty() {
        return;
    }
    let _ = app.emit_to(label, "media:art", json!({ "key": key, "src": media::art(&key) }));
    let _ = app.emit_to(label, "media:lyrics", LYRICS.lock().unwrap().clone());
}

/// The notch's volume bar: 0..100.
pub fn set_volume(app: &AppHandle, percent: f64) {
    media::set_volume((percent / 100.0) as f32);
    media::now();
    notch::send_status(app);
}

/// The notch's progress bar: jump to `ms` into the track.
pub fn seek(app: &AppHandle, ms: i64) {
    media::seek(ms);
    std::thread::sleep(Duration::from_millis(250));
    media::now();
    notch::send_status(app);
}

/// The notch's output button: Windows' own sound output page (choosing a device has no public API).
pub fn open_sound_output() {
    let _ = badi_agent::apps::shell_open("ms-settings:sound");
}

/// The music page's other controls: shuffle, repeat (none → list → track), mute, and the player itself.
pub fn media_extra(app: &AppHandle, what: &str) {
    let now = media::LAST.lock().unwrap().clone();
    match what {
        "shuffle" => {
            media::set_shuffle(now["shuffle"] != true);
        }
        "repeat" => {
            media::set_repeat(match now["repeat"].as_str() {
                Some("none") | None => "list",
                Some("list") => "track",
                _ => "none",
            });
        }
        "mute" => {
            media::toggle_mute();
        }
        "open" => {
            media::open_source(now["source"].as_str().unwrap_or(""));
            return;
        }
        _ => return,
    }
    std::thread::sleep(Duration::from_millis(250));
    media_changed(app);
}

/// While the notch is hovered, what's playing is also re-read every 2 s (position, volume).
pub fn watch_media(app: &AppHandle, on: bool) {
    if !on {
        WATCHING.store(false, Ordering::SeqCst);
        return;
    }
    if WATCHING.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        while WATCHING.load(Ordering::SeqCst) {
            media_changed(&app);
            std::thread::sleep(Duration::from_secs(2));
        }
    });
}

/// [MEDIA:…] and the notch's media buttons. Falls back to the media keys without a session.
pub fn media_control(app: &AppHandle, action: &str) {
    if let Ok(false) = media::control(action) {
        badi_input::hook::mute(500);
        let combo = badi_input::keys::Combo { mods: vec![], key: badi_input::keys::Key::Vk(media::media_key(action)) };
        let _ = badi_input::send::press_combo(&combo, 1);
    }
    std::thread::sleep(Duration::from_millis(300));
    media_changed(app);
}

/// The notch shortcut N: its prompt asked as a typed question.
pub fn shortcut(app: &AppHandle, n: usize) {
    let list = state(app).store.get("notchShortcuts");
    if let Some(prompt) = list.as_array().into_iter().flatten().filter(|s| s["label"].is_string() && s["prompt"].is_string()).nth(n).and_then(|s| s["prompt"].as_str()) {
        talk::ask_typed(app, prompt);
    }
}
