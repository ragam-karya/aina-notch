// "Kerjakan untukku": a [DO:task] runs the Brain (badi-agent) on the display the user is looking at.
// One-time consent, file changes asked out loud in the notch, questions answered by voice, buttons,
// Enter or Esc, progress shown in the notch, and the result spoken. Port of v1's main.js startBrain,
// runBrain, stopBrain, brainFileApprover, waitForUser's Brain side, and the extension bridge setup.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, OnceLock};

use badi_agent::brain::{self, Reason, Session, Setup, SystemInfo};
use badi_agent::bridge::Bridge;
use badi_agent::desktop::Desktop;
use badi_agent::{Host, Need};
use badi_llm::prompts::language_instruction;
use regex::Regex;
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, Manager};

use crate::i18n::{self, t};
use crate::notch::{self, Answer};
use crate::{AppState, talk, turn, windows};

static ALL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(semua(nya)?|seterusnya|all|everything)\b").unwrap());
// A spoken answer to a yes/no question: it is a yes only when it starts with a yes word and has no
// no word in it (v1 main.js YES/NO). Anything else ("hmm, apa maksudnya?") is not an OK.
static YES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^\W*(ya+|iya+|yes|yep|yup|ok(e|ay)?|boleh|lanjut(kan)?|silakan|setuju|gas|sure|go ahead|do it|betul|benar)\b").unwrap());
static NO: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(jangan|tidak|nggak|enggak|ngga|gak|ga|ndak|batal|stop|no|nope|don'?t|cancel)\b").unwrap());

static APP: OnceLock<AppHandle> = OnceLock::new();

fn state(app: &AppHandle) -> tauri::State<'_, AppState> {
    app.state::<AppState>()
}

fn log(line: &str) {
    if std::env::var_os("BADI_LOG").is_some() || std::env::var_os("BADI_TIMING").is_some() {
        eprintln!("[brain] {line}");
    }
}

/// The bridge to the browser extension: 127.0.0.1:extensionPort, paired with the code in Settings.
pub fn new_bridge() -> Bridge {
    Bridge::new(|| APP.get().and_then(|a| a.state::<AppState>().store.secret("extensionToken")).unwrap_or_default())
}

pub fn start_bridge(app: &AppHandle) {
    let _ = APP.set(app.clone());
    let st = state(app);
    let a = app.clone();
    st.bridge.on_status(move |s| {
        if let Some(w) = a.get_webview_window("settings") {
            let _ = w.emit("extension:status", s);
        }
    });
    let port = st.store.get("extensionPort").as_u64().unwrap_or(47821) as u16;
    if let Err(e) = st.bridge.start(port) {
        eprintln!("[bridge] could not listen on port {port}: {e}");
    }
}

/// A spoken or typed reply that clearly means yes ("boleh", "iya, lanjut", "boleh semua").
pub fn said_yes(text: &str) -> bool {
    YES.is_match(text) && !NO.is_match(text)
}

/// "boleh semua": yes for the rest of the task.
pub fn said_all(text: &str) -> bool {
    ALL.is_match(text)
}

/// Constant-time compare (login states).
pub fn same_text(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

pub fn running(app: &AppHandle) -> bool {
    state(app).brain.lock().unwrap().is_some()
}

/// Stops the running task (Esc, a new hold, tray → stop all). A pending question is answered "no".
pub fn stop(app: &AppHandle) {
    if let Some(flag) = state(app).brain.lock().unwrap().take() {
        flag.store(true, Ordering::SeqCst);
        notch::answer(app, Answer { approved: false, text: String::new() });
    }
}

pub fn start(app: &AppHandle, task: &str) {
    stop(app);
    let (a, task) = (app.clone(), task.to_string());
    std::thread::spawn(move || run(&a, &task));
}

// The one-time OK to control the computer (v1: a native message box).
#[cfg(windows)]
fn consent(app: &AppHandle) -> bool {
    use ::windows::Win32::UI::WindowsAndMessaging::{IDOK, MB_ICONQUESTION, MB_OKCANCEL, MB_SETFOREGROUND, MB_TOPMOST, MessageBoxW};
    use ::windows::core::HSTRING;
    let text = format!("{}\n\n{}", t(app, "consentTitle"), t(app, "consentBody"));
    let ok = unsafe { MessageBoxW(None, &HSTRING::from(text), &HSTRING::from("aina"), MB_OKCANCEL | MB_ICONQUESTION | MB_TOPMOST | MB_SETFOREGROUND) } == IDOK;
    if ok {
        let mut patch = serde_json::Map::new();
        patch.insert("computerControl".into(), json!(true));
        let _ = state(app).store.save(&patch);
    }
    ok
}

// Progress updates can be long; the notch shows the first sentence.
fn headline(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut end = None;
    let chars: Vec<char> = flat.chars().collect();
    for (i, c) in chars.iter().enumerate().take(160) {
        if ".!?".contains(*c) && chars.get(i + 1).is_none_or(|n| n.is_whitespace()) {
            end = Some(i + 1);
            break;
        }
    }
    match end {
        Some(n) => chars[..n].iter().collect(),
        None if chars.len() > 157 => format!("{}…", chars[..157].iter().collect::<String>()),
        None => flat,
    }
}

struct AppHost {
    app: AppHandle,
    all: AtomicBool,
    focus: Option<std::path::PathBuf>,
}

impl AppHost {
    // Asks in the notch, then goes back to "acting".
    fn wait(&self, kind: &str, question: &str, all: bool, limit_ms: u64) -> Answer {
        let a = notch::wait_for_user(&self.app, kind, question, all, limit_ms);
        if running(&self.app) {
            turn::set_phase(&self.app, "acting");
        }
        a
    }
}

impl Host for AppHost {
    fn approve(&self, need: &Need) -> Option<String> {
        // The focus lock comes first: no "allow all" opens what the person keeps to the focus folder.
        if let Some(f) = crate::focus::locked_folder(&self.app)
            && let Some(why) = badi_agent::guard::outside_focus(&need.paths, &f)
        {
            return Some(why);
        }
        if self.all.load(Ordering::SeqCst) || state(&self.app).store.get("agentFileAccess").as_bool() == Some(true) {
            return None;
        }
        // In the focus folder creating, editing, moving and renaming are free; deleting still asks.
        if let Some(f) = &self.focus
            && need.kind == "files"
            && need.op != "delete"
            && need.paths.iter().all(|p| badi_agent::guard::inside(p, f))
        {
            return None;
        }
        let q = if need.kind == "app" {
            crate::apps::app_question(&self.app, None, &need.changes)
        } else {
            i18n::file_question(&self.app, None, &need.op, &need.names, need.place.as_deref(), need.new_name.as_deref())
        };
        let a = self.wait("confirm", &q, true, 60_000);
        if a.approved && ALL.is_match(&a.text) {
            self.all.store(true, Ordering::SeqCst);
        }
        if a.approved {
            return None;
        }
        let said = if a.text.is_empty() { String::new() } else { format!(" and said: \"{}\"", a.text) };
        Some(format!("The user declined{said}. Don't do it another way."))
    }

    fn confirm(&self, question: &str) -> (bool, String) {
        let a = self.wait("confirm", question, false, 60_000);
        (a.approved, a.text)
    }

    fn ask(&self, question: &str) -> String {
        self.wait("ask", question, false, 120_000).text
    }

    fn action_at(&self, x: i32, y: i32, kind: &str) {
        if let Some((label, gx, gy)) = talk::to_overlay(&self.app, x, y) {
            windows::emit_overlay(&self.app, &label, "actionAt", json!({ "x": gx, "y": gy, "kind": kind }));
        }
    }

    fn progress(&self, text: &str) {
        if !notch::pending(&self.app) {
            windows::broadcast(&self.app, "text", json!({ "text": headline(text), "ms": 400 }));
        }
    }

    fn action(&self, line: &str) {
        log(line);
    }
}

fn run(app: &AppHandle, task: &str) {
    let st = state(app);
    // Demos skip the consent box only in a test profile, never on the real one.
    let demo = std::env::var_os("BADI_DEMO").is_some() && std::env::var_os("BADI_PROFILE").is_some();
    if st.store.get("computerControl").as_bool() != Some(true) && !demo && !consent(app) {
        talk::say(app, &t(app, "controlOff"));
        return turn::set_phase(app, "idle");
    }
    let get = |k: &str, d: &str| st.store.get(k).as_str().unwrap_or(d).to_string();
    let (model, effort) = (crate::cloud::model(app, &get("brainModel", "claude-sonnet-5-5")), get("brainEffort", "medium"));
    let max_steps = st.store.get("brainMaxSteps").as_u64().unwrap_or(60) as u32;
    let Some(key) = st.store.secret("anthropicApiKey") else {
        crate::cloud::require(app);
        return turn::set_phase(app, "idle");
    };

    // The display the user is looking at: the one under the cursor.
    let displays = talk::displays(app);
    let (cx, cy) = badi_input::send::cursor();
    let Some(display) = displays
        .iter()
        .find(|d| cx >= d.phys.0 && cy >= d.phys.1 && cx < d.phys.0 + d.phys.2 as i32 && cy < d.phys.1 + d.phys.3 as i32)
        .or_else(|| displays.iter().find(|d| d.primary))
        .or(displays.first())
        .cloned()
    else {
        return windows::broadcast(app, "error", i18n::brain_unavailable(app, "no display"));
    };
    let focus = crate::agents::current_focus(app);
    let host = AppHost { app: app.clone(), all: AtomicBool::new(false), focus: focus.clone() };
    let focus_text = focus.as_ref().map(|f| f.display().to_string());
    let shown = app.clone();
    let desktop = Desktop::new(display, displays.len() > 1, Box::new(move |x, y, kind| {
        if let Some((label, gx, gy)) = talk::to_overlay(&shown, x, y) {
            windows::emit_overlay(&shown, &label, "actionAt", json!({ "x": gx, "y": gy, "kind": kind }));
        }
    }));
    let browser = st.bridge.connected();
    let work = crate::apps::work_tools(app);
    let today = chrono::Local::now().format("%a %b %d %Y").to_string();
    let system = brain::build_system(&SystemInfo {
        screen_info: &desktop.describe(),
        browser,
        today: &today,
        language_instruction: &language_instruction(&get("language", "auto")),
        focus: focus_text.as_deref(),
        focus_brief: &crate::focus::brief(app),
        apps: work.as_ref().map(|w| w.line.as_str()).unwrap_or(""),
    });
    let stop = Arc::new(AtomicBool::new(false));
    *st.brain.lock().unwrap() = Some(stop.clone());
    let call = move |body: &Value, cancelled: &dyn Fn() -> bool| brain::claude_call(&key, body, cancelled);
    let mut session = Session::new(Setup { model: &model, effort: &effort, system, tools: brain::build_tools(browser, work.as_ref().map(|w| w.tools.clone()).unwrap_or_default()), max_steps }, &call, &host, stop.clone());
    session.desktop = Some(&desktop);
    session.bridge = browser.then_some(&st.bridge);
    session.uia = Some(&st.uia);
    session.base_folder = focus;
    let approve = |need: &Need| host.approve(need);
    let extra = |name: &str, input: &Value| crate::apps::run_tool(app, name, input, &approve);
    if work.is_some() {
        session.extra = Some(&extra);
    }

    log(&format!("task: {task}"));
    turn::set_phase(app, "acting");
    // On Badi Cloud the task's calls are billed as one "kerjakan", which may finish past a limit.
    // The knowledge base: named, plus the passages matching the task (the file tools can't read PDFs).
    let kb = [crate::knowledge::summary_line(app), crate::knowledge::context_for(app, task).unwrap_or_default()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n\n");
    let task = if kb.is_empty() { task.to_string() } else { format!("{task}\n\n{kb}") };
    let outcome = badi_net::with_task("kerjakan", &badi_net::task_id(), || session.run(&task));
    st.bridge.end_session();
    st.uia.stop(); // the UI Automation worker only runs during a task
    log(&format!("end: {:?} after {} steps · {}", outcome.reason, outcome.steps, session.meter.line()));
    {
        let mut current = st.brain.lock().unwrap();
        if !current.as_ref().is_some_and(|f| Arc::ptr_eq(f, &stop)) {
            return; // a newer task took over
        }
        *current = None;
    }
    let phase = turn::phase(app);
    // Stopped because the user started talking or typing a new question: stay out of the way.
    if outcome.reason == Reason::Stopped && phase != "acting" && phase != "idle" {
        return;
    }
    let line = match outcome.reason {
        Reason::Done | Reason::Truncated if !outcome.text.is_empty() => outcome.text,
        Reason::Done | Reason::Truncated => t(app, "brainDone"),
        Reason::Stopped => t(app, "brainStopped"),
        Reason::Limit => t(app, "brainLimit"),
        Reason::Refusal => t(app, "brainRefused"),
        Reason::Error(e) => {
            windows::broadcast(app, "error", i18n::brain_failed(app, &e));
            return turn::set_phase(app, "idle");
        }
    };
    talk::say(app, &line);
    windows::broadcast(app, "linger", 9000);
    turn::set_phase(app, "idle");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_clear_yes_approves() {
        for yes in ["boleh", "Iya, lanjut", "oke", "ok gas", "yes please", "boleh semua", "Silakan."] {
            assert!(said_yes(yes), "{yes}");
        }
        for not in ["hmm, apa maksudnya?", "jangan", "boleh jangan dulu", "nggak", "tunggu", "", "Lihat area yang aku tandai.", "ya jangan", "oke tapi tidak sekarang", "no"] {
            assert!(!said_yes(not), "{not}");
        }
        assert!(said_all("boleh semua"));
    }
}
