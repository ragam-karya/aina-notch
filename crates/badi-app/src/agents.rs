// Sprites in the app: Badi spawns one for each background task ([DELEGATE:kind|task], the Agents
// window, routines). Every task is a new instance of a kind (Ciro 1, Ciro 2 at once); its state comes
// from the task itself (starting, working, waiting, needs_approval, success, error, cancelled) and is
// shown as a card in the overlay's Sprite panel, the notch's peek and the Agents window. Also focus mode
// ([FOCUS], a dropped folder) and the Sprites' questions in the notch. The engines and the storage are
// in badi-agent. Port of v1's main.js agent/focus parts and the runner half of src/agents.js.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use badi_agent::agents::{self as store, Engine, Store, SystemParts, now_ms, parse_blocks, pick_engine, recent_chat, routine_due};
use badi_agent::brain::{self, Session, Setup};
use badi_agent::engines::{self, AgentHost, ClaudeCodeRun, DESKTOP_TOOLS, LocalTools};
use badi_agent::diff::{self, FileDiff, GitSnap};
use badi_agent::sprites::{self, Kind};
use badi_agent::{Need, focus};
use badi_core::notch::{agent_line, task_order, task_status};
use badi_llm::prompts::{TONE, language_instruction};
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

use crate::i18n::{self, t};
use crate::notch::{self, Action};
use crate::{AppState, talk, turn, windows};

/// One Sprite at work (or just finished, while its card lingers).
pub struct Sprite {
    id: u64,
    kind: &'static Kind,
    num: u32,
    task: String,
    state: &'static str,
    started: i64,
    ended: Option<i64>,
    steps: Vec<String>, // its latest activity lines, oldest first
    result: String,
    stop: Arc<AtomicBool>,
    routine: Option<String>, // "slug/id" of the routine that started it
    diffs: Vec<FileDiff>,    // what it changed, edit by edit (the whole task as a git diff once done)
    git: Option<GitSnap>,    // the repo it works in, as it was when it started
    project: Option<PathBuf>, // the focus folder it works in
}

/// A finished task after its card left: the notch's task history (memory only, never on disk).
struct Past {
    id: u64,
    kind: &'static Kind,
    name: String,
    task: String,
    state: &'static str,
    started: i64,
    ended: i64,
    result: String,
    project: Option<PathBuf>,
}

impl Sprite {
    fn live(&self) -> bool {
        self.ended.is_none()
    }
}

#[derive(Default)]
pub struct Agents {
    sprites: Vec<Sprite>,
    next_id: u64,
    batch: usize,                   // Sprites spawned since none were running ("all Sprites are done")
    last_run: HashMap<String, i64>, // routine "slug/id" -> when it last ran (or was first seen)
    // A finished Sprite's diffs, kept in memory for an hour after its card is gone (never on disk), so
    // the Sprites window can still open them.
    kept: HashMap<u64, (i64, Vec<FileDiff>)>,
    history: VecDeque<Past>, // newest first, at most HISTORY_KEPT
}

const HISTORY_KEPT: usize = 8;

const KEEP_DIFFS_MS: i64 = 60 * 60_000;

// At most this many Sprites work at once; each one costs model calls.
const MAX_LIVE: usize = 8;
const STEPS_KEPT: usize = 4;

// One question at a time, from any Sprite; ASKER is the Sprite whose question is up now, so stopping
// it also answers its question.
static ASKING: Mutex<()> = Mutex::new(());
static ASKER: Mutex<Option<u64>> = Mutex::new(None);
// Instances of one kind share its agent.json; one writer at a time.
static WRITING: Mutex<()> = Mutex::new(());

fn state(app: &AppHandle) -> tauri::State<'_, AppState> {
    app.state::<AppState>()
}

pub fn agents_store() -> Store {
    Store::new(badi_store::data_dir().join("agents"))
}

fn emit(app: &AppHandle, channel: &str, payload: Value) {
    if let Some(w) = app.get_webview_window("agents") {
        let _ = w.emit(channel, payload);
    }
}

fn changed(app: &AppHandle) {
    emit(app, "agents:changed", Value::Null);
    notch::send_status(app);
    send_sprites(app);
}

// "Ciro" alone, "Ciro 2" while another Ciro card is up.
fn name_in(a: &Agents, s: &Sprite) -> String {
    let alone = !a.sprites.iter().any(|o| o.id != s.id && o.kind.id == s.kind.id);
    sprites::display_name(s.kind, s.num, alone)
}

fn name_of(app: &AppHandle, id: u64) -> String {
    let st = state(app);
    let a = st.agents.lock().unwrap();
    a.sprites.iter().find(|s| s.id == id).map(|s| name_in(&a, s)).unwrap_or_else(|| "Sprite".into())
}

fn set_state(app: &AppHandle, id: u64, to: &'static str) {
    {
        let st = state(app);
        let mut a = st.agents.lock().unwrap();
        match a.sprites.iter_mut().find(|s| s.id == id && s.live()) {
            Some(s) if s.state != to => s.state = to,
            _ => return,
        }
    }
    send_sprites(app);
}

fn log(app: &AppHandle, id: u64, chunk: &str) {
    let line = agent_line(chunk);
    let slug = {
        let st = state(app);
        let mut a = st.agents.lock().unwrap();
        let Some(s) = a.sprites.iter_mut().find(|s| s.id == id) else { return };
        if std::env::var_os("BADI_LOG").is_some() {
            eprint!("[{} #{id}] {chunk}", s.kind.id);
        }
        let fresh = !line.is_empty() && s.live() && s.steps.last() != Some(&line);
        if fresh {
            if s.state == "starting" {
                s.state = "working";
            }
            s.steps.push(line);
            let extra = s.steps.len().saturating_sub(STEPS_KEPT);
            s.steps.drain(..extra);
        }
        (s.kind.id, fresh)
    };
    emit(app, "agents:log", json!({ "slug": slug.0, "chunk": chunk }));
    if slug.1 {
        notch::send_status(app);
        send_sprites(app);
    }
}

// A file the Sprite changed: kept as a diff (at most MAX_DIFFS) and shown as an activity step,
// "✎ main.rs +3 −1", like Coucou's live diff.
fn add_diff(app: &AppHandle, id: u64, d: FileDiff) {
    let line = d.line();
    let slug = {
        let st = state(app);
        let mut a = st.agents.lock().unwrap();
        let Some(s) = a.sprites.iter_mut().find(|s| s.id == id && s.live()) else { return };
        if s.diffs.len() >= diff::MAX_DIFFS {
            return;
        }
        s.diffs.push(d);
        if s.state == "starting" {
            s.state = "working";
        }
        s.steps.push(line.clone());
        let extra = s.steps.len().saturating_sub(STEPS_KEPT);
        s.steps.drain(..extra);
        s.kind.id
    };
    emit(app, "agents:log", json!({ "slug": slug, "chunk": format!("{line}\n") }));
    send_sprites(app);
}

/// The files a Sprite changed, for its card and its message: [{ i, name, path, added, removed }].
fn changes(diffs: &[FileDiff]) -> Vec<Value> {
    diffs.iter().enumerate().map(|(i, d)| d.summary(i)).collect()
}

/// "3 file · +42 −7".
fn changes_line(app: &AppHandle, diffs: &[FileDiff]) -> String {
    let files: HashSet<&str> = diffs.iter().map(|d| d.path.as_str()).collect();
    let (add, del) = diffs.iter().fold((0, 0), |(a, r), d| (a + d.added, r + d.removed));
    format!("{} {} · +{add} −{del}", files.len(), if i18n::lang(app) == "en" { if files.len() == 1 { "file" } else { "files" } } else { "file" })
}

/// A Sprite's diff in full (sprites:diff), while its card is up or for an hour after it finished.
fn diff_of(app: &AppHandle, sprite: u64, i: usize) -> Option<Value> {
    let st = state(app);
    let mut a = st.agents.lock().unwrap();
    let now = now_ms();
    a.kept.retain(|_, (at, _)| now - *at < KEEP_DIFFS_MS);
    let list = a.sprites.iter().find(|s| s.id == sprite).map(|s| &s.diffs).or(a.kept.get(&sprite).map(|k| &k.1))?;
    list.get(i).map(FileDiff::to_json)
}

// Opens a changed file in VS Code at its first change when it is installed, else in its default app.
fn open_in_editor(path: &str, line: u64) -> Result<(), String> {
    if !Path::new(path).is_file() {
        return Err(format!("not found: {path}"));
    }
    let code = badi_agent::files::hidden("where").arg("code.cmd").output().ok().filter(|o| o.status.success());
    if let Some(o) = code {
        let exe = String::from_utf8_lossy(&o.stdout).lines().next().unwrap_or("").trim().to_string();
        if !exe.is_empty() {
            let mut c = badi_agent::files::hidden("cmd.exe");
            c.args(["/c", &exe, "-g", &format!("{path}:{}", line.max(1))]);
            return c.spawn().map(|_| ()).map_err(|e| e.to_string());
        }
    }
    badi_agent::apps::shell_open(path).map(|_| ())
}

/// A Sprite kind's meta with its running flag, as the Agents window expects.
fn with_running(app: &AppHandle, mut meta: Value) -> Value {
    let slug = meta["slug"].as_str().unwrap_or("").to_string();
    meta["running"] = json!(state(app).agents.lock().unwrap().sprites.iter().any(|s| s.live() && s.kind.id == slug));
    if let Some(k) = sprites::kind(&slug) {
        meta["icon"] = json!(k.icon);
        meta["purpose"] = json!(k.role[lang_ix(app)]);
    }
    meta
}

fn lang_ix(app: &AppHandle) -> usize {
    if i18n::lang(app) == "en" { 0 } else { 1 }
}

/// The working Sprites for the notch's peek: { slug, name, line }.
pub fn notch_agents(app: &AppHandle) -> Vec<Value> {
    let st = state(app);
    let a = st.agents.lock().unwrap();
    a.sprites
        .iter()
        .filter(|s| s.live())
        .map(|s| json!({ "slug": s.kind.id, "name": name_in(&a, s), "line": s.steps.last().cloned().unwrap_or_default() }))
        .collect()
}

/// Every Sprite card for the overlay's panel, newest first.
pub fn sprites_status(app: &AppHandle) -> Value {
    let ix = lang_ix(app);
    let st = state(app);
    let a = st.agents.lock().unwrap();
    let cards: Vec<Value> = a
        .sprites
        .iter()
        .rev()
        .map(|s| {
            let status = if s.state == "working" { s.kind.working[ix].to_string() } else { i18n::sprite_state(app, s.state) };
            json!({
                "id": s.id, "kind": s.kind.id, "name": name_in(&a, s), "icon": s.kind.icon, "role": s.kind.role[ix],
                "state": s.state, "status": status, "task": s.task, "steps": s.steps, "result": s.result,
                "started": s.started, "ended": s.ended, "live": s.live(), "changes": changes(&s.diffs),
            })
        })
        .collect();
    json!({ "sprites": cards, "now": now_ms() })
}

/// The panel is on the primary display's overlay.
pub fn send_sprites(app: &AppHandle) {
    let label = {
        let st = state(app);
        let overlays = st.overlays.lock().unwrap();
        overlays.iter().find(|o| o.primary).or(overlays.first()).map(|o| o.label.clone())
    };
    if let Some(l) = label {
        let _ = app.emit_to(l.as_str(), "sprites:status", sprites_status(app));
    }
}

// ---------- focus mode ----------

/// The focus folder, if one is set and it still exists.
pub fn current_focus(app: &AppHandle) -> Option<PathBuf> {
    let f = state(app).store.get("focusFolder").as_str().unwrap_or("").to_string();
    let p = Path::new(&f);
    (!f.is_empty() && p.is_dir()).then(|| badi_agent::guard::real(p))
}

/// A focus session on `folder` (voice, [FOCUS], a dropped folder), or none for "". Moving to another
/// project ends the one before into the history; a running pomodoro carries on.
pub fn set_focus(app: &AppHandle, folder: &str) {
    if folder.is_empty() {
        crate::focus::end(app, Default::default());
    } else {
        crate::focus::start(app, badi_core::focus::Session::for_folder(folder, notch::now_ms()));
    }
}

/// Finds and sets the focus folder (None clears it); returns the sentence to say.
pub fn focus_folder(app: &AppHandle, phrase: Option<&str>) -> String {
    let Some(phrase) = phrase.filter(|p| !p.trim().is_empty()) else {
        set_focus(app, "");
        return t(app, "focusOff");
    };
    let found = focus::find(phrase);
    let name = phrase.split(['\\', '/']).rfind(|s| !s.is_empty()).unwrap_or(phrase);
    let base = |p: &Path| p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    match found.as_slice() {
        [] => i18n::focus_not_found(app, name),
        [a, b, ..] if base(a) == base(b) => {
            let list: Vec<String> = found.iter().take(4).map(|p| i18n::focus_in(app, &focus::where_label(p))).collect();
            i18n::focus_many(app, found.len(), name, &list.join(", "))
        }
        [first, ..] => {
            set_focus(app, &first.display().to_string());
            show_focus(app, first);
            i18n::focus_on(app, &first.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), &focus::where_label(first))
        }
    }
}

/// The [FOCUS:…] tag: set it and say how it went.
pub fn apply_focus(app: &AppHandle, phrase: Option<String>) {
    let app = app.clone();
    std::thread::spawn(move || {
        turn::set_phase(&app, "thinking");
        let line = focus_folder(&app, phrase.as_deref());
        talk::say(&app, &line);
        turn::set_phase(&app, "idle");
    });
}

// Shows in the notch which folder Badi now works in (the mini pill keeps showing it afterwards).
pub fn show_focus(app: &AppHandle, folder: &Path) {
    let name = folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let labels = i18n::strings(app)["island"].clone();
    let label = |k: &str, d: &str| labels[k].as_str().unwrap_or(d).to_string();
    let end = Action::new("endFocus", label("endFocus", "Selesai fokus"), "", None);
    notch::show_notice(app, "focus", &format!("📁 {} · {name}", label("focus", "Fokus")), &[folder.display().to_string()], vec![end], 6000);
}

/// A folder dropped on the notch's focus zone.
pub fn focus_dropped(app: &AppHandle, folder: &str) {
    set_focus(app, folder);
    let p = Path::new(folder);
    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let line = i18n::focus_on(app, &name, &focus::where_label(p));
    show_focus(app, p);
    let a = app.clone();
    std::thread::spawn(move || talk::say(&a, &line));
}

// ---------- asking the person ----------

// A change outside a Sprite's folder in Badi's words: "Filo mau memindahkan file di Unduhan. Boleh?"
fn agent_question(app: &AppHandle, agent: &str, need: &Need) -> String {
    let en = i18n::lang(app) == "en";
    let file = || need.paths.first().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    match need.kind.as_str() {
        "app" => crate::apps::app_question(app, Some(agent), &need.changes),
        "files" => i18n::file_question(app, Some(agent), &need.op, &need.names, need.place.as_deref(), need.new_name.as_deref()),
        "write" if en => format!("{agent} wants to create {}. Okay?", file()),
        "write" => format!("{agent} mau bikin file {}. Boleh?", file()),
        "edit" if en => format!("{agent} wants to edit {}. Okay?", file()),
        "edit" => format!("{agent} mau ngedit {}. Boleh?", file()),
        _ => {
            let words: Vec<String> = need.acts.iter().map(|a| t(app, &format!("agentAct_{a}"))).collect();
            let and = t(app, "and");
            let list = match words.as_slice() {
                [one] => one.clone(),
                [rest @ .., last] => format!("{} {and} {last}", rest.join(", ")),
                [] => String::new(),
            };
            let place = need.place.clone().unwrap_or_else(|| t(app, "yourFolders"));
            if en { format!("{agent} wants to {list} in {place}. Okay?") } else { format!("{agent} mau {list} di {place}. Boleh?") }
        }
    }
}

// Changes outside a Sprite's folder need the person's OK, unless Settings allows them; "boleh semua"
// covers the rest of the task. Asked out loud once Badi is free, one question at a time; the card says
// "waiting" while it queues and "needs your OK" while the question is up.
fn approver<'a>(app: &'a AppHandle, id: u64, project: Option<&'a Path>, stop: &'a AtomicBool, all: &'a AtomicBool) -> impl Fn(&Need) -> Option<String> + Sync + 'a {
    move |need: &Need| {
        // The focus lock comes first: no "allow all" opens what the person keeps to the focus folder.
        if let Some(f) = crate::focus::locked_folder(app)
            && let Some(why) = badi_agent::guard::outside_focus(&need.paths, &f)
        {
            return Some(why);
        }
        if all.load(Ordering::SeqCst) || state(app).store.get("agentFileAccess").as_bool() == Some(true) {
            return None;
        }
        if let Some(p) = project
            && need.kind == "files"
            && need.op != "delete"
            && need.paths.iter().all(|x| badi_agent::guard::inside(x, p))
        {
            return None;
        }
        let first: String = need.detail.lines().next().unwrap_or("").chars().take(160).collect();
        log(app, id, &format!("? {first}\n"));
        let declined = || Some("The person declined this change. Do not try it another way; tell them what you wanted to do so they can decide.".to_string());
        set_state(app, id, "waiting");
        let _turn = ASKING.lock().unwrap_or_else(|e| e.into_inner());
        while crate::brain::running(app) || notch::pending(app) || turn::phase(app) != "idle" {
            if stop.load(Ordering::SeqCst) {
                return declined();
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        if stop.load(Ordering::SeqCst) {
            return declined();
        }
        *ASKER.lock().unwrap() = Some(id);
        set_state(app, id, "needs_approval");
        let a = notch::wait_for_user(app, "confirm", &agent_question(app, &name_of(app, id), need), true, 120_000);
        *ASKER.lock().unwrap() = None;
        set_state(app, id, "working");
        if stop.load(Ordering::SeqCst) {
            return declined();
        }
        if a.approved && crate::brain::said_all(&a.text) {
            all.store(true, Ordering::SeqCst);
        }
        if a.approved { None } else { declined() }
    }
}

// ---------- running a task ----------

/// [DELEGATE:kind|task]: Badi spawns that kind of Sprite (Ciro in focus mode, else Mino, when none is
/// named). In focus mode it works in that folder.
pub fn delegate(app: &AppHandle, arg: &str) {
    let project = current_focus(app);
    let (kind, task) = sprites::parse_delegate(arg, project.is_some());
    spawn(app, kind, &task, None, project);
}

/// Starts a new Sprite of a kind on a task, in the background.
pub fn spawn(app: &AppHandle, kind: &'static Kind, task: &str, routine: Option<(String, String)>, project: Option<PathBuf>) {
    if let Err(e) = {
        let _w = WRITING.lock().unwrap_or_else(|e| e.into_inner());
        agents_store().ensure_sprite(kind)
    } {
        return windows::broadcast(app, "error", e);
    }
    let Some((id, stop)) = add(app, kind, task, routine.as_ref().map(|r| r.0.clone()), project.clone()) else { return };
    turn::chime(app, "agent-start");
    let (app, task) = (app.clone(), task.to_string());
    let title = routine.map(|r| r.1);
    std::thread::spawn(move || {
        // On Badi Cloud a background task is billed as one "latar", which may finish past a limit.
        let reply = badi_net::with_task("latar", &badi_net::task_id(), || run(&app, id, kind, &task, title.as_deref(), project, &stop));
        finish(&app, id, kind, reply, title.is_some(), &stop);
    });
}

// A new card ("starting"), unless MAX_LIVE Sprites already work.
fn add(app: &AppHandle, kind: &'static Kind, task: &str, routine: Option<String>, project: Option<PathBuf>) -> Option<(u64, Arc<AtomicBool>)> {
    let stop = Arc::new(AtomicBool::new(false));
    let id = {
        let st = state(app);
        let mut a = st.agents.lock().unwrap();
        let live = a.sprites.iter().filter(|s| s.live()).count();
        if live >= MAX_LIVE {
            drop(a);
            emit(app, "agents:error", json!(i18n::sprites_busy(app, MAX_LIVE)));
            notch::show_notice(app, "info", &i18n::sprites_busy(app, MAX_LIVE), &[], vec![], 6000);
            return None;
        }
        if live == 0 {
            a.batch = 0;
        }
        a.batch += 1;
        // Ids stay unique across restarts: a saved message points at its Sprite's diffs by id.
        if a.next_id == 0 {
            a.next_id = now_ms() as u64 * 100;
        }
        a.next_id += 1;
        let id = a.next_id;
        let used: Vec<u32> = a.sprites.iter().filter(|s| s.kind.id == kind.id).map(|s| s.num).collect();
        let s = Sprite {
            id,
            kind,
            num: sprites::next_number(&used),
            task: task.to_string(),
            state: "starting",
            started: now_ms(),
            ended: None,
            steps: vec![],
            result: String::new(),
            stop: stop.clone(),
            routine,
            diffs: vec![],
            git: None,
            project,
        };
        a.sprites.push(s);
        id
    };
    changed(app);
    Some((id, stop))
}

/// Stops a Sprite: its card says "stopped" and its question, if one is up, is answered "no".
pub fn stop(app: &AppHandle, id: u64) {
    {
        let st = state(app);
        let mut a = st.agents.lock().unwrap();
        let Some(s) = a.sprites.iter_mut().find(|s| s.id == id && s.live()) else { return };
        s.stop.store(true, Ordering::SeqCst);
        s.state = "cancelled";
        s.ended = Some(now_ms());
    }
    if *ASKER.lock().unwrap() == Some(id) {
        notch::answer(app, notch::Answer { approved: false, text: String::new() });
    }
    linger(app, id, 4000);
    changed(app);
}

fn live_ids(app: &AppHandle, kind: Option<&str>) -> Vec<u64> {
    state(app).agents.lock().unwrap().sprites.iter().filter(|s| s.live() && kind.is_none_or(|k| s.kind.id == k)).map(|s| s.id).collect()
}

pub fn any_running(app: &AppHandle) -> bool {
    !live_ids(app, None).is_empty()
}

pub fn stop_all(app: &AppHandle) {
    for id in live_ids(app, None) {
        stop(app, id);
    }
}

// A finished card stays a moment (longer when it failed), then leaves the panel.
fn linger(app: &AppHandle, id: u64, ms: u64) {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(ms));
        dismiss(&app, id);
    });
}

// The card leaves the panel; the task goes on in the notch's history.
fn dismiss(app: &AppHandle, id: u64) {
    {
        let st = state(app);
        let mut a = st.agents.lock().unwrap();
        let Some(i) = a.sprites.iter().position(|s| s.id == id && !s.live()) else { return };
        let name = name_in(&a, &a.sprites[i]);
        let s = a.sprites.remove(i);
        let past = Past { id: s.id, kind: s.kind, name, task: s.task, state: s.state, started: s.started, ended: s.ended.unwrap_or(s.started), result: s.result, project: s.project };
        a.history.push_front(past);
        a.history.truncate(HISTORY_KEPT);
    }
    send_sprites(app);
    notch::send_status(app);
}

/// Runs a failed or stopped task again, as a new Sprite of the same kind on the same task and folder.
fn retry(app: &AppHandle, id: u64) {
    let again = {
        let st = state(app);
        let mut a = st.agents.lock().unwrap();
        let card = a.sprites.iter().position(|s| s.id == id && !s.live()).map(|i| a.sprites.remove(i)).map(|s| (s.kind, s.task, s.project));
        card.or_else(|| a.history.iter().position(|p| p.id == id).and_then(|i| a.history.remove(i)).map(|p| (p.kind, p.task, p.project)))
    };
    if let Some((kind, task, project)) = again {
        spawn(app, kind, &task, None, project.filter(|p| p.is_dir()));
    }
}

/// The notch's Tasks page: every task on a card (working, or just finished) in the order it should be
/// looked at, and the history of the ones whose cards have left. A Sprite runs on this PC, so stopping
/// it ends a local process ("local"); it reports no percentage, so no task carries a progress.
pub fn notch_tasks(app: &AppHandle) -> Value {
    let ix = lang_ix(app);
    let folder = |p: &Option<PathBuf>| p.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned());
    let focus = current_focus(app);
    let in_focus = |p: &Option<PathBuf>| p.is_some() && *p == focus;
    let short = |e: &str| -> String {
        let e = e.trim_start_matches("Couldn't finish this: ").split_whitespace().collect::<Vec<_>>().join(" ");
        if e.chars().count() > 90 { format!("{}…", e.chars().take(89).collect::<String>().trim_end()) } else { e }
    };
    let st = state(app);
    let a = st.agents.lock().unwrap();
    let mut list: Vec<&Sprite> = a.sprites.iter().collect();
    list.sort_by_key(|s| task_order(task_status(s.state), s.started, s.ended));
    let list: Vec<Value> = list
        .into_iter()
        .map(|s| {
            let status = task_status(s.state);
            json!({
                "id": s.id, "slug": s.kind.id, "name": name_in(&a, s), "role": s.kind.role[ix], "task": s.task, "status": status,
                "line": s.steps.last().cloned().unwrap_or_default(), "error": if status == "failed" { short(&s.result) } else { String::new() },
                "started": s.started, "ended": s.ended, "project": folder(&s.project), "inFocus": in_focus(&s.project),
                "routine": s.routine.is_some(), "stop": s.live().then_some("local"), "retry": matches!(status, "failed" | "cancelled"),
            })
        })
        .collect();
    let history: Vec<Value> = a
        .history
        .iter()
        .map(|p| {
            let status = task_status(p.state);
            json!({
                "id": p.id, "slug": p.kind.id, "name": p.name, "role": p.kind.role[ix], "task": p.task, "status": status,
                "error": if status == "failed" { short(&p.result) } else { String::new() }, "started": p.started, "ended": p.ended,
                "project": folder(&p.project), "inFocus": in_focus(&p.project), "retry": matches!(status, "failed" | "cancelled"),
            })
        })
        .collect();
    json!({ "list": list, "history": history, "focus": folder(&focus) })
}

/// A button on a Sprite card or the notch's Tasks page (island:act "sprite:open:N" | "sprite:stop:N" |
/// "sprite:retry:N" | "sprite:dismiss:N").
/// Closing a card never stops its task; only the stop button does.
pub fn act(app: &AppHandle, what: &str) {
    let Some((verb, id)) = what.split_once(':') else { return };
    let Ok(id) = id.parse::<u64>() else { return };
    match verb {
        "open" => {
            let slug = state(app).agents.lock().unwrap().sprites.iter().find(|s| s.id == id).map(|s| s.kind.id);
            windows::open_agents(app, slug);
        }
        "stop" => stop(app, id),
        "retry" => retry(app, id),
        "dismiss" => dismiss(app, id),
        _ => {}
    }
}

struct Reply {
    text: String,
    stats: Option<Value>,
    failed: bool,
}

impl Reply {
    fn failed(text: impl Into<String>) -> Reply {
        Reply { text: text.into(), stats: None, failed: true }
    }
}

// v1 Meter.line().
fn stats_line(s: &Value) -> String {
    let n = |k: &str| s[k].as_u64().unwrap_or(0);
    format!("{} turns · {} tools · {} screenshots · {} s · ~${:.3}", n("turns"), n("tools"), n("screenshots"), (n("ms") as f64 / 1000.0).round(), s["usd"].as_f64().unwrap_or(0.0))
}

fn run(app: &AppHandle, id: u64, kind: &'static Kind, text: &str, from_routine: Option<&str>, project: Option<PathBuf>, stop: &Arc<AtomicBool>) -> Reply {
    let s = agents_store();
    let slug = kind.id;
    let mut msg = json!({ "role": "user", "text": text, "sprite": name_of(app, id) });
    if let Some(title) = from_routine {
        msg["fromRoutine"] = json!(title); // the Agents window shows "Routine: <title>"
    }
    {
        let _w = WRITING.lock().unwrap_or_else(|e| e.into_inner());
        let _ = s.add_message(slug, msg);
    }
    // Read after the new message is in: recent_chat leaves out the last message (this one), so the
    // previous reply is still part of the context ("simpan itu").
    let Some(meta) = s.get(slug) else { return Reply::failed("Couldn't finish this: no such Sprite.") };
    emit(app, "agents:changed", Value::Null);
    let st = state(app);
    let get = |k: &str, d: &str| st.store.get(k).as_str().unwrap_or(d).to_string();
    let (model, effort) = (crate::cloud::model(app, &get("brainModel", "claude-sonnet-5-5")), get("brainEffort", "medium"));
    let Some(key) = st.store.secret("anthropicApiKey") else {
        return Reply::failed("Masuk ke aina Cloud dulu (Settings → Akun), lalu kirim lagi tugasnya.");
    };
    let own = badi_agent::guard::real(&s.dir(slug));
    let project = project.filter(|p| p.is_dir());
    let dir = project.clone().unwrap_or_else(|| own.clone());
    let agents_md = std::fs::read_to_string(own.join("AGENTS.md")).unwrap_or_default();
    let desktop = st.store.get("computerControl").as_bool() == Some(true);
    let all = AtomicBool::new(false);
    let approve = approver(app, id, project.as_deref(), stop, &all);
    let logf = |line: &str| log(app, id, &format!("{line}\n"));
    let on_diff = |d: FileDiff| add_diff(app, id, d);
    // In a project that is a git repo, the whole task becomes one git diff when it ends.
    if project.is_some()
        && let Some(snap) = diff::git_snapshot(&dir)
        && let Some(s) = st.agents.lock().unwrap().sprites.iter_mut().find(|s| s.id == id)
    {
        s.git = Some(snap);
    }
    // The knowledge base: named, plus the passages matching the task.
    let kb = [crate::knowledge::summary_line(app), crate::knowledge::context_for(app, text).unwrap_or_default()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n\n");
    let prompt = format!("{}{text}{}", recent_chat(meta["messages"].as_array().map(Vec::as_slice).unwrap_or(&[]), 8), if kb.is_empty() { String::new() } else { format!("\n\n{kb}") });
    let lang = language_instruction(&get("language", "auto"));
    let today = chrono::Local::now().format("%a %b %d %Y").to_string();
    let tone = TONE.join("\n");
    // Work apps (Composio) outside focus mode; project work doesn't get them.
    let work = if project.is_none() { crate::apps::work_tools(app) } else { None };
    let apps_line = work.as_ref().map(|w| w.line.clone()).unwrap_or_default();
    if std::env::var_os("BADI_LOG").is_some() {
        eprintln!("[{slug} #{id}] work-app tools: {}", work.as_ref().map(|w| w.tools.len()).unwrap_or(0));
    }
    let system = |engine: Engine| {
        store::agent_system(&SystemParts { agents_md: &agents_md, dir: &dir, desktop, engine, project: project.as_deref(), notes_dir: &own, tone: &tone, language_instruction: &lang, today: &today, apps: &apps_line })
    };
    let work_tool = |name: &str, input: &Value| if work.is_some() { crate::apps::run_tool(app, name, input, &approve) } else { None };

    let choice = get("agentEngine", "auto");
    let claude = engines::find_claude_code();
    let codex = if choice == "codex" { engines::find_codex() } else { None };
    let engine = pick_engine(&choice, claude.is_some(), codex.is_some());

    // Badi's own tools (desktop and files), run the same way for every engine.
    let host = AgentHost { approve: &approve, log: &logf, dir: dir.clone() };
    let never = |_: &Value, _: &dyn Fn() -> bool| -> Result<Value, String> { Err("not used".into()) };
    let mut tools_session = Session::new(Setup { model: &model, effort: &effort, system: String::new(), tools: vec![], max_steps: 0 }, &never, &host, stop.clone());
    tools_session.uia = Some(&st.uia);
    tools_session.base_folder = project.clone();
    let run_tool = |name: &str, input: &Value| -> Result<Vec<Value>, String> {
        if DESKTOP_TOOLS.contains(&name) && st.store.get("computerControl").as_bool() != Some(true) {
            return Err("The person has not allowed aina to control the computer. Do the task without their apps, or tell them to turn it on in Settings.".into());
        }
        if let Some(r) = work_tool(name, input) {
            return r.map(|t| vec![json!({ "type": "text", "text": t })]);
        }
        tools_session.run_tool(name, input)
    };

    let local = || -> Result<(String, Value), String> {
        let tools = LocalTools { dir: dir.clone(), focus: project.is_some(), stop: stop.clone(), approve: &approve, on_diff: Some(&on_diff) };
        let extra = |name: &str, input: &Value| tools.run(name, input).or_else(|| work_tool(name, input));
        let call = |body: &Value, cancelled: &dyn Fn() -> bool| brain::claude_call(&key, body, cancelled);
        let max_steps = st.store.get("brainMaxSteps").as_u64().unwrap_or(60) as u32;
        let mut session = Session::new(Setup { model: &model, effort: &effort, system: system(Engine::Local), tools: engines::agent_tools(desktop, work.as_ref().map(|w| w.tools.clone()).unwrap_or_default()), max_steps }, &call, &host, stop.clone());
        session.max_time = Duration::from_secs(30 * 60);
        session.uia = Some(&st.uia);
        session.base_folder = project.clone();
        session.extra = Some(&extra);
        let r = session.run(&prompt);
        let line = session.meter.summary();
        match r.reason {
            brain::Reason::Stopped => Err("stopped".into()),
            brain::Reason::Refusal => Err("Claude declined this task.".into()),
            brain::Reason::Error(e) => Err(e),
            brain::Reason::Limit if r.text.is_empty() => Ok(("I ran out of steps before finishing. Tell me to continue and I will pick up from here.".into(), line)),
            _ => Ok((if r.text.is_empty() { "Done.".into() } else { r.text }, line)),
        }
    };

    let result: Result<(String, Option<Value>), String> = match (engine, &claude, &codex) {
        (Engine::Codex, _, Some(codex)) => engines::run_codex(codex, &dir, &format!("{text}\n\n({lang})"), &logf, stop).map(|t| (t, None)),
        (Engine::ClaudeCode, Some(exe), _) => {
            let mut tools: Vec<Value> = brain::own_tools()
                .into_iter()
                .filter(|t| {
                    let n = t["name"].as_str().unwrap_or("");
                    (desktop && DESKTOP_TOOLS.contains(&n)) || ["find_files", "list_folder", "move_paths", "copy_paths", "rename_path", "create_folder", "delete_paths"].contains(&n)
                })
                .collect();
            tools.extend(work.as_ref().map(|w| w.tools.clone()).unwrap_or_default());
            for t in &mut tools {
                if let Some(o) = t.as_object_mut() {
                    o.remove("eager_input_streaming");
                    o.remove("cache_control");
                }
            }
            let r = ClaudeCodeRun {
                exe,
                key: &key,
                model: &model,
                effort: &effort,
                dir: &dir,
                focus: project.is_some(),
                append: &system(Engine::ClaudeCode),
                prompt: &prompt,
                tools,
                run_tool: &run_tool,
                approve: &approve,
                log: &logf,
                stop,
                on_diff: Some(&on_diff),
            };
            match engines::run_claude_code(&r) {
                Ok((t, line)) => Ok((t, Some(line))),
                // A Claude Code that can't start falls back to Badi's own engine, unless it had already
                // begun working or the user stopped it.
                Err(e) if !e.started && !stop.load(Ordering::SeqCst) => {
                    logf(&format!("(Claude Code could not start: {}. Using aina's own engine.)", e.message));
                    local().map(|(t, l)| (t, Some(l)))
                }
                Err(e) => Err(e.message),
            }
        }
        _ => local().map(|(t, l)| (t, Some(l))),
    };
    match result {
        Ok((text, stats)) => Reply { text, stats, failed: false },
        Err(e) => Reply::failed(format!("Couldn't finish this: {e}")),
    }
}

fn finish(app: &AppHandle, id: u64, kind: &'static Kind, reply: Reply, from_routine: bool, stop: &Arc<AtomicBool>) {
    let s = agents_store();
    let slug = kind.id;
    if let Some(stats) = &reply.stats {
        log(app, id, &format!("· {}\n", stats_line(stats)));
    }
    let parsed = parse_blocks(&reply.text);
    let name = name_of(app, id);
    // In a git repo the task's diff is the repo against its state at the start: that also catches what
    // commands and formatters changed. Edits outside the repo stay as they were caught.
    let snap = state(app).agents.lock().unwrap().sprites.iter_mut().find(|s| s.id == id).and_then(|s| s.git.take());
    let from_git = snap.map(|g| diff::git_changes(&g)).unwrap_or_default();
    let diffs = {
        let st = state(app);
        let mut a = st.agents.lock().unwrap();
        match a.sprites.iter_mut().find(|s| s.id == id) {
            Some(s) => {
                if !from_git.is_empty() {
                    let covered: HashSet<String> = from_git.iter().map(|d| d.path.to_lowercase()).collect();
                    let outside: Vec<FileDiff> = s.diffs.drain(..).filter(|d| !covered.contains(&d.path.to_lowercase())).collect();
                    s.diffs = from_git.into_iter().chain(outside).take(diff::MAX_DIFFS).collect();
                }
                s.diffs.clone()
            }
            None => from_git,
        }
    };
    if !diffs.is_empty() {
        state(app).agents.lock().unwrap().kept.insert(id, (now_ms(), diffs.clone()));
    }
    {
        let _w = WRITING.lock().unwrap_or_else(|e| e.into_inner());
        // Work files go with the task, unless another instance of this kind is still using tmp/.
        if live_ids(app, Some(slug)).iter().all(|&o| o == id) {
            s.clear_tmp(slug);
        }
        let mut msg = json!({ "role": "assistant", "text": parsed.body, "nextActions": parsed.next_actions, "artifacts": parsed.artifacts, "sprite": name });
        if let Some(stats) = reply.stats {
            msg["stats"] = stats;
        }
        // Only the file names and counts are saved; the diffs themselves stay in memory (kept).
        if !diffs.is_empty() {
            msg["changes"] = json!(changes(&diffs).into_iter().map(|mut c| {
                if let Some(o) = c.as_object_mut() {
                    o.remove("newFile");
                }
                c
            }).collect::<Vec<_>>());
            msg["spriteId"] = json!(id);
        }
        let _ = s.add_message(slug, msg);
        if let (Some(r), false) = (&parsed.routine, from_routine) {
            let _ = s.add_routine(slug, r);
        }
    }
    let summary = parsed.body.split_inclusive(['.', '!', '?']).next().unwrap_or("").trim().to_string();
    let cancelled = stop.load(Ordering::SeqCst);
    // The card ends only once the reply is saved, so whoever waits for it finds it.
    let (all_done, batch) = {
        let st = state(app);
        let mut a = st.agents.lock().unwrap();
        if let Some(sp) = a.sprites.iter_mut().find(|sp| sp.id == id) {
            if sp.live() {
                sp.state = if cancelled { "cancelled" } else if reply.failed { "error" } else { "success" };
                sp.ended = Some(now_ms());
            }
            sp.result = if reply.failed { reply.text.clone() } else { summary.clone() };
        }
        (!a.sprites.iter().any(Sprite::live), a.batch)
    };
    linger(app, id, if cancelled { 4000 } else if reply.failed { 20_000 } else { 8000 });
    if !cancelled {
        turn::chime(app, "agent-done");
        let title = if reply.failed {
            i18n::sprite_failed(app, &name)
        } else if all_done && batch > 1 {
            format!("✓ {}", i18n::sprites_all_done(app))
        } else {
            format!("✓ {}", i18n::agent_done(app, &name))
        };
        let label = if reply.failed { t(app, "taskSeeError") } else if diffs.is_empty() { t(app, "notchOpen") } else { t(app, "spriteViewChanges") };
        let open = Action::new("openAgent", label, "", Some(Arc::new(move |a: &AppHandle| windows::open_agents(a, Some(slug)))));
        let shown = if reply.failed { reply.text.chars().take(140).collect() } else { summary.clone() };
        let mut lines = if shown.is_empty() { vec![] } else { vec![shown.clone()] };
        if !diffs.is_empty() {
            lines.push(changes_line(app, &diffs));
        }
        notch::show_notice(app, "agentDone", &title, &lines, vec![open], 7000);
        let body = if shown.is_empty() { t(app, "taskDone") } else { shown };
        let _ = app.notification().builder().title(title.trim_start_matches("✓ ")).body(&body).show();
    }
    if !crate::brain::running(app) {
        state(app).uia.stop(); // the Sprite may have used UI Automation
    }
    changed(app);
}

// ---------- routines ----------

/// Runs due routines while the app is open. Periods count on the wall clock, so after the PC sleeps a
/// due routine runs once on waking. A routine runs as a new Sprite of the kind that set it up.
pub fn start_routines(app: AppHandle) {
    std::thread::spawn(move || {
        loop {
            let now = now_ms();
            let s = agents_store();
            for kind in &sprites::KINDS {
                let Some(meta) = s.get(kind.id) else { continue };
                for r in meta["routines"].as_array().into_iter().flatten() {
                    if r["paused"].as_bool() == Some(true) {
                        continue;
                    }
                    let key = format!("{}/{}", kind.id, r["id"].as_str().unwrap_or(""));
                    let due = {
                        let st = state(&app);
                        let mut a = st.agents.lock().unwrap();
                        let last = *a.last_run.entry(key.clone()).or_insert(now);
                        let busy = a.sprites.iter().any(|sp| sp.live() && sp.routine.as_deref() == Some(key.as_str()));
                        let due = routine_due(last, r["every_minutes"].as_i64().unwrap_or(60), now) && !busy;
                        if due {
                            a.last_run.insert(key.clone(), now);
                        }
                        due
                    };
                    if due {
                        let title = r["title"].as_str().unwrap_or("").to_string();
                        spawn(&app, kind, r["task"].as_str().unwrap_or(""), Some((key, title)), None);
                    }
                }
            }
            std::thread::sleep(Duration::from_secs(20));
        }
    });
}

// ---------- BADI_DEMO=sprites: cards with no task behind them ----------

pub fn demo_spawn(app: &AppHandle, kind: &str, task: &str) -> u64 {
    add(app, sprites::kind(kind).unwrap_or(sprites::default_kind(false)), task, None, None).map(|(id, _)| id).unwrap_or(0)
}

pub fn demo_step(app: &AppHandle, id: u64, line: &str) {
    log(app, id, &format!("{line}\n"));
}

pub fn demo_diff(app: &AppHandle, id: u64, d: FileDiff) {
    add_diff(app, id, d);
}

pub fn demo_state(app: &AppHandle, id: u64, to: &'static str) {
    set_state(app, id, to);
}

pub fn demo_end(app: &AppHandle, id: u64, to: &'static str, result: &str) {
    {
        let st = state(app);
        let mut a = st.agents.lock().unwrap();
        if let Some(s) = a.sprites.iter_mut().find(|s| s.id == id && s.live()) {
            s.state = to;
            s.ended = Some(now_ms());
            s.result = result.to_string();
        }
    }
    linger(app, id, 20_000);
    changed(app);
}

// ---------- the Agents window ----------

pub fn invoke(app: &AppHandle, ch: &str, arg: &Value) -> Result<Value, String> {
    let s = agents_store();
    let slug = || arg.as_str().map(String::from).or_else(|| arg["slug"].as_str().map(String::from)).unwrap_or_default();
    match ch {
        "agents:list" => {
            let list = {
                let _w = WRITING.lock().unwrap_or_else(|e| e.into_inner());
                s.sprites()
            };
            Ok(Value::Array(list.into_iter().map(|m| with_running(app, m)).collect()))
        }
        "agents:get" => Ok(s.get(&slug()).map(|m| with_running(app, m)).unwrap_or(Value::Null)),
        // The Sprites are a fixed set; they can't be made or deleted.
        "agents:create" | "agents:remove" => Err(t(app, "spritesFixed")),
        "agents:send" => {
            let kind = sprites::kind(&slug()).ok_or("no such Sprite")?;
            spawn(app, kind, arg["text"].as_str().unwrap_or(""), None, current_focus(app));
            Ok(Value::Null)
        }
        "agents:stop" => {
            for id in live_ids(app, Some(&slug())) {
                stop(app, id);
            }
            Ok(Value::Null)
        }
        "agents:folder" => badi_agent::apps::shell_open(&s.dir(&slug()).display().to_string()).map(|_| Value::Null),
        "agents:routine" => {
            let r = {
                let _w = WRITING.lock().unwrap_or_else(|e| e.into_inner());
                s.set_routine(&slug(), arg["id"].as_str().unwrap_or(""), &arg["patch"])
            };
            changed(app);
            r.map(|_| Value::Null)
        }
        // A Sprite's diff in full, and opening its file in an editor.
        "sprites:diff" => Ok(diff_of(app, arg["sprite"].as_u64().unwrap_or(0), arg["i"].as_u64().unwrap_or(0) as usize).unwrap_or(Value::Null)),
        "sprites:edit" => open_in_editor(arg["path"].as_str().unwrap_or(""), arg["line"].as_u64().unwrap_or(1)).map(|_| Value::Null),
        "agents:engine" => {
            let choice = state(app).store.get("agentEngine").as_str().unwrap_or("auto").to_string();
            let codex = choice == "codex" && engines::find_codex().is_some();
            Ok(json!(pick_engine(&choice, engines::find_claude_code().is_some(), codex).id()))
        }
        _ => Err(format!("unknown channel: {ch}")),
    }
}
