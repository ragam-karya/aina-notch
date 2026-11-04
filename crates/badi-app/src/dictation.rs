// Dictation (Ctrl+Win by default). A quick press starts it and the next press ends it, so the hands
// are free for other work in between; holding the keys works too (released = done). The words are
// saved as a note: a Markdown file in Documents\Badi\Dikte (notes.rs), and shown in the notch. Claude Haiku
// tidies them first when that's on (punctuation, filler words, spoken corrections) and sums up a long
// one. Recording follows v1's main.js onDictationStart/onDictationEnd.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use badi_llm::claude::{self, Request};
use serde_json::json;
use tauri::{AppHandle, Manager};

use crate::i18n::t;
use crate::notch::{self, Action};
use crate::notes::{self, Note};
use crate::{AppState, turn, windows};

/// Voice text is handled by the small, fast model.
const MODEL: &str = "claude-haiku-5-5";
const CLEANUP: &str = "You clean up dictated text before it is saved as the user's note. Fix punctuation, capitalization and obvious speech-recognition mistakes, remove filler words and false starts, and apply spoken corrections (\"no wait, make it Tuesday\"). Split it into paragraphs where the topic changes. Keep the user's wording, language and meaning. Output only the cleaned text, nothing else.";
const SUMMARY: &str = "Summarize the user's dictated note in the same language it is written in: one short sentence on what it is about, then up to five bullet points (\"- \") with the key points, decisions, dates and to-dos. Use only what the note says. Output only the summary.";

// When the current dictation started (None = not dictating).
static STARTED: Mutex<Option<Instant>> = Mutex::new(None);
// The press that ends a running dictation also releases the keys; that release is not a new one.
static SKIP_UP: AtomicBool = AtomicBool::new(false);
// Which dictation the time limit belongs to.
static RUN: AtomicU64 = AtomicU64::new(0);
/// Released sooner than this, the press only switched dictation on; held longer, it was hold-to-talk.
const TAP: Duration = Duration::from_millis(600);
/// A dictation left running ends by itself (speech to text takes up to 25 MB, ±13 minutes).
const MAX: Duration = Duration::from_secs(10 * 60);

pub fn dictating() -> bool {
    STARTED.lock().unwrap().is_some()
}

/// The dictation keys pressed: start, or end the one running.
pub fn key_down(app: &AppHandle) {
    if dictating() {
        SKIP_UP.store(true, Ordering::SeqCst);
        let app = app.clone();
        std::thread::spawn(move || up(&app));
    } else {
        down(app);
    }
}

/// The dictation keys released: a hold ends there; a quick press leaves it running.
pub fn key_up(app: &AppHandle) {
    if SKIP_UP.swap(false, Ordering::SeqCst) {
        return;
    }
    let held = STARTED.lock().unwrap().map(|s| s.elapsed());
    if held.is_some_and(|h| h >= TAP) {
        up(app);
    }
}

fn state(app: &AppHandle) -> tauri::State<'_, AppState> {
    app.state::<AppState>()
}

pub fn down(app: &AppHandle) {
    crate::notch::clear_panel_kind(app, "dictation"); // a new question or dictation closes the last note
    if !crate::cloud::require(app) {
        return;
    }
    let st = state(app);
    if crate::talk::listening(app) {
        return; // already holding the talk keys
    }
    turn::cancel(app);
    *STARTED.lock().unwrap() = Some(Instant::now());
    crate::notch::pin_display(app);
    turn::set_phase(app, "listening");
    windows::broadcast(app, "mode", "dictation");
    turn::chime(app, "start");
    st.mic.start();
    let run = RUN.fetch_add(1, Ordering::SeqCst) + 1;
    let a = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(MAX);
        if RUN.load(Ordering::SeqCst) == run && dictating() {
            up(&a);
        }
    });
}

pub fn cancel(app: &AppHandle) {
    SKIP_UP.store(false, Ordering::SeqCst);
    if STARTED.lock().unwrap().take().is_some() {
        let _ = state(app).mic.stop();
        windows::broadcast(app, "mode", "companion");
        turn::set_phase(app, "idle");
    }
}

pub fn up(app: &AppHandle) {
    let Some(started) = STARTED.lock().unwrap().take() else { return };
    RUN.fetch_add(1, Ordering::SeqCst);
    let st = state(app);
    let rec = st.mic.stop();
    windows::broadcast(app, "mode", "companion");
    let held = started.elapsed();
    let rec = match rec {
        Ok(r) if held.as_millis() >= 350 => r,
        _ => return turn::set_phase(app, "idle"),
    };
    if rec.duration < crate::talk::MIN_SPEECH || rec.samples.is_empty() {
        turn::set_phase(app, "idle");
        return windows::broadcast(app, "error", t(app, "dictationTooShort"));
    }
    turn::set_phase(app, "transcribing");
    let key = st.store.secret("openaiApiKey").unwrap_or_default();
    let model = st.store.get("sttModel").as_str().unwrap_or("gpt-4o-mini-transcribe").to_string();
    let lang = match st.store.get("language").as_str() {
        Some(l @ ("id" | "en")) => Some(l.to_string()),
        _ => None,
    };
    let text = match badi_voice::stt::transcribe(&key, &model, lang.as_deref(), &rec.samples, badi_voice::mic::RATE) {
        Ok(t) => t.trim().to_string(),
        Err(e) => {
            turn::set_phase(app, "idle");
            return windows::broadcast(app, "error", format!("Dikte gagal: {e}"));
        }
    };
    if text.is_empty() {
        turn::set_phase(app, "idle");
        return windows::broadcast(app, "error", t(app, "notHeard"));
    }
    turn::set_phase(app, "thinking");
    let text = if st.store.get("dictationCleanup").as_bool() != Some(false) { ask(app, CLEANUP, &text, 4000).unwrap_or(text) } else { text };
    let summary = if notes::needs_summary(&text) { ask(app, SUMMARY, &text, 800) } else { None };
    let lang = crate::i18n::detect_lang(&text).unwrap_or(if crate::i18n::lang(app) == "en" { "en" } else { "id" });
    let note = Note { created: chrono::Local::now(), duration_s: rec.duration.as_secs(), lang, summary: summary.as_deref(), text: &text };
    let saved = notes::save(&dir(app), &note);
    turn::set_phase(app, "idle");
    match saved {
        Ok(file) => {
            let title = if crate::i18n::lang(app) == "en" { "✓ Dictation saved" } else { "✓ Dikte tersimpan" };
            show_note(app, &file, title, summary.as_deref(), &text, false);
        }
        Err(e) => windows::broadcast(app, "error", format!("Dikte gagal disimpan: {e}")),
    }
}

/// Where dictations go: the `dictationFolder` setting, or Documents\Badi\Dikte.
pub fn dir(app: &AppHandle) -> PathBuf {
    state(app).store.get("dictationFolder").as_str().map(str::trim).filter(|s| !s.is_empty()).map(PathBuf::from).unwrap_or_else(|| badi_agent::files::app_documents().join("Dikte"))
}

// The note in the notch: the summary (when there is one) above the text, scrolling when it's long. It
// stays until closed, so there's time to read it.
/// A note in the notch: just saved, or opened from the notch's list of notes (`back` returns to it).
pub fn show_note(app: &AppHandle, file: &std::path::Path, title: &str, summary: Option<&str>, text: &str, back: bool) {
    let en = crate::i18n::lang(app) == "en";
    let name = file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let copy = {
        let text = text.to_string();
        Action::new("dictation:copy", if en { "Copy" } else { "Salin" }, "", Some(Arc::new(move |_: &AppHandle| copy_text(&text))))
    };
    let open = {
        let file = file.to_path_buf();
        Action::new("dictation:open", if en { "Open file" } else { "Buka file" }, "", Some(Arc::new(move |_: &AppHandle| {
            let _ = badi_agent::apps::shell_open(&file.display().to_string());
        })))
    };
    let close = Action::new("dictation:close", if en { "Close" } else { "Tutup" }, "", Some(Arc::new(|a: &AppHandle| notch::clear_panel_kind(a, "dictation"))));
    let mut actions = vec![copy, open];
    if back {
        actions.push(Action::new("dictation:back", if en { "← Notes" } else { "← Catatan" }, "", Some(Arc::new(|a: &AppHandle| notch::show_notes(a)))));
    }
    actions.push(close);
    let mut extra = serde_json::Map::new();
    extra.insert("note".into(), json!({ "summary": summary.unwrap_or(""), "summaryLabel": if en { "Summary" } else { "Ringkasan" }, "text": text }));
    notch::show_panel(app, "dictation", title, &[name], actions, 0, extra);
}

// The clipboard, through PowerShell (the text goes in an environment variable, never a command line).
fn copy_text(text: &str) {
    let _ = badi_agent::files::hidden("powershell.exe").args(["-NoProfile", "-NonInteractive", "-Command", "Set-Clipboard -Value $env:BADI_TEXT"]).env("BADI_TEXT", text).spawn();
}

// One small Claude call on the dictated words; any failure leaves them as they are.
fn ask(app: &AppHandle, system: &str, text: &str, max_tokens: u32) -> Option<String> {
    let st = state(app);
    let key = st.store.secret("anthropicApiKey")?;
    let model = crate::cloud::model(app, MODEL);
    let r = Request { key: &key, model: &model, effort: "low", thinking: false, system, messages: json!([{ "role": "user", "content": text }]), max_tokens };
    let (out, _) = claude::stream(&r, &|| false, &mut |_| {}).ok()?;
    let out = out.trim().to_string();
    (!out.is_empty()).then_some(out)
}
