// Hold-to-talk and marking while talking (no click). Port of the hotkey wiring and the ink section of
// v1's main.js (onTalkDown/onPttEnd, startInk, onInkMove, checkDwell, setMark, startEarlyCapture,
// captureForQuestion, lookUpMarkedElement).
//
// While the user holds the talk keys, the cursor trail is ink: circling something, underlining it, or
// holding still on it (pointing) marks what the question is about. The overlays stay click-through.
// Screenshots are taken while the user still talks (300 ms in, and again for every mark), and UI
// Automation names what is under the mark. On release the question is transcribed and goes to
// turn.rs; Esc, Enter and a spoken answer also answer a question Badi is waiting on.

use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, LazyLock, Mutex};
use std::thread::sleep;
use std::time::{Duration, Instant};

use badi_input::gesture::{self, DWELL_MS, Mark, TrailPoint};
use badi_input::hook::{self, Input};
use badi_input::hotkeys::{Event, Hotkeys, Kind};
use badi_screen::capture::{Shot, capture_all};
use badi_screen::{Display, Rect};
use badi_voice::tts::Voice;
use serde_json::{Value, json};
use tauri::{AppHandle, Manager};

use crate::notch::now_ms;
use crate::{AppState, turn, windows};

const INK_MAX_POINTS: usize = 1500;
const MARK_LOOKUP: Duration = Duration::from_millis(1500);
const MARK_LOOKUP_WAIT: Duration = Duration::from_millis(800);
// Shorter holds are taps, not questions (v1: under 500 ms is too short to transcribe).
pub const MIN_SPEECH: Duration = Duration::from_millis(500);

type Shots = Arc<Mutex<Option<Result<Vec<Shot>, String>>>>;

#[derive(Default)]
pub struct Talk {
    listening: bool,
    ink: Vec<TrailPoint>,
    seg_start: usize,
    still_announced: bool,
    mark: Option<Mark>,
    // The capture taken while talking, for the mark it was taken for.
    early: Option<(Option<Mark>, Shots)>,
    // What UI Automation says is under the mark.
    under: Option<(Mark, Receiver<Option<String>>)>,
    started: Option<Instant>,
}

fn state(app: &AppHandle) -> tauri::State<'_, AppState> {
    app.state::<AppState>()
}

/// The talk keys are held (a question is being recorded).
pub fn listening(app: &AppHandle) -> bool {
    state(app).talk.lock().unwrap().listening
}

/// The displays in the overlay space, for captures.
pub fn displays(app: &AppHandle) -> Vec<Display> {
    state(app)
        .overlays
        .lock()
        .unwrap()
        .iter()
        .map(|o| {
            let (x, y, w, h) = o.bounds();
            Display { id: o.label.clone(), phys: (o.pos.x, o.pos.y, o.size.width, o.size.height), scale: o.scale, bounds: Rect { x, y, width: w, height: h }, primary: o.primary }
        })
        .collect()
}

// A physical desktop point -> (overlay label, overlay space).
pub fn to_overlay(app: &AppHandle, x: i32, y: i32) -> Option<(String, f64, f64)> {
    let p = tauri::PhysicalPosition::new(x as f64, y as f64);
    let st = state(app);
    let overlays = st.overlays.lock().unwrap();
    let o = overlays.iter().find(|o| o.contains(p))?;
    let (lx, ly) = o.local(p);
    Some((o.label.clone(), o.origin.0 + lx, o.origin.1 + ly))
}

// The overlay space -> physical desktop px (for UI Automation).
fn to_physical(app: &AppHandle, x: f64, y: f64) -> Option<(f64, f64)> {
    let st = state(app);
    let overlays = st.overlays.lock().unwrap();
    let o = overlays.iter().find(|o| {
        let (bx, by, bw, bh) = o.bounds();
        x >= bx && y >= by && x < bx + bw && y < by + bh
    })?;
    Some((o.pos.x as f64 + (x - o.origin.0) * o.scale, o.pos.y as f64 + (y - o.origin.1) * o.scale))
}

// The overlay whose display holds an overlay-space point.
fn overlay_at(app: &AppHandle, x: f64, y: f64) -> Option<String> {
    let st = state(app);
    let overlays = st.overlays.lock().unwrap();
    overlays
        .iter()
        .find(|o| {
            let (bx, by, bw, bh) = o.bounds();
            x >= bx && y >= by && x < bx + bw && y < by + bh
        })
        .map(|o| o.label.clone())
}

// The hotkey state machine; Settings reconfigures it in place when a combo changes.
static KEYS: LazyLock<Mutex<Hotkeys>> = LazyLock::new(|| Mutex::new(Hotkeys::default()));

/// Applies the talk and dictation combos from the settings (at start, and when Settings saves them).
pub fn configure_keys(app: &AppHandle) {
    let store = &state(app).store;
    let combo = |k: &str| store.get(k).as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect::<Vec<_>>()).unwrap_or_default();
    KEYS.lock().unwrap().configure(&combo("pttCombo"), &combo("dictationCombo"));
}

/// Starts the keyboard hook and the hotkey state machine.
pub fn start(app: AppHandle) {
    let Some(events) = hook::start() else {
        // Without the hook nothing Badi listens for can work: say so instead of failing silently.
        eprintln!("[talk] the keyboard hook is not available");
        windows::broadcast(&app, "error", "aina tidak bisa membaca tombol keyboard. Coba jalankan ulang aina.");
        return;
    };
    configure_keys(&app);
    std::thread::spawn(move || {
        for input in events {
            let fired = match input {
                Input::Key { vk, down: true } => KEYS.lock().unwrap().key_down(vk),
                Input::Key { vk, down: false } => KEYS.lock().unwrap().key_up(vk),
                Input::MouseDown { x, y, .. } => {
                    if let Some((_, ox, oy)) = to_overlay(&app, x, y) {
                        turn::on_mouse_down(&app, ox, oy);
                    }
                    vec![]
                }
            };
            for e in fired {
                if std::env::var_os("BADI_LOG").is_some() {
                    eprintln!("[keys] {e:?}");
                }
                match e {
                    // Through the realtime engine when it is on (it falls back to the classic path).
                    Event::Start(Kind::Ptt) => crate::realtime::talk_down(&app),
                    Event::End(Kind::Ptt) => {
                        let app = app.clone();
                        std::thread::spawn(move || crate::realtime::talk_up(&app));
                    }
                    Event::Cancel(Kind::Ptt) => cancel(&app),
                    // Esc answers a yes/no question with no. Otherwise (nothing asked, or a question
                    // that wants words) it stops everything, the task too.
                    Event::Escape => {
                        let confirm = crate::notch::pending_kind(&app).as_deref() == Some("confirm");
                        if !(confirm && crate::notch::answer(&app, crate::notch::Answer { approved: false, text: String::new() })) {
                            crate::brain::stop(&app);
                            crate::notch::answer(&app, crate::notch::Answer { approved: false, text: String::new() });
                            cancel(&app);
                        }
                    }
                    // Enter says yes to a waiting yes/no question.
                    Event::Enter => {
                        if crate::notch::pending_kind(&app).as_deref() == Some("confirm") {
                            crate::notch::answer(&app, crate::notch::Answer { approved: true, text: String::new() });
                        }
                    }
                    Event::Start(Kind::Dictation) => crate::dictation::key_down(&app),
                    Event::End(Kind::Dictation) => {
                        let app = app.clone();
                        std::thread::spawn(move || crate::dictation::key_up(&app));
                    }
                    Event::Cancel(Kind::Dictation) => crate::dictation::cancel(&app),
                }
            }
        }
    });
}

pub fn on_talk_down(app: &AppHandle) {
    crate::notch::clear_panel_kind(app, "dictation"); // a new question or dictation closes the last note
    // A dictation left running ends (and is saved) first; it shares the microphone.
    if crate::dictation::dictating() {
        let a = app.clone();
        std::thread::spawn(move || crate::dictation::up(&a));
        return;
    }
    // Everything runs through Badi Cloud: signed out, say so instead of listening for nothing.
    if !crate::cloud::require(app) {
        return;
    }
    {
        let st = state(app);
        let mut t = st.talk.lock().unwrap();
        if t.listening {
            return;
        }
        *t = Talk { listening: true, started: Some(Instant::now()), ..Talk::default() };
        st.mic.start();
    }
    // Pressing the keys mid-answer cuts Badi off; a new question replaces a walkthrough and stops a
    // running task, unless the task is waiting for an answer: then this hold is the answer.
    if !crate::notch::pending(app) {
        crate::brain::stop(app);
    }
    turn::cancel(app);
    turn::end_guide(app);
    turn::chime(app, "start");
    turn::prewarm(app);
    crate::notch::pin_display(app);
    windows::broadcast(app, "clear", Value::Null);
    windows::broadcast(app, "mode", "companion");
    turn::set_phase(app, "listening");
    windows::broadcast(app, "ink", json!({ "reset": true }));
    // The first screenshot, 300 ms in (the mark, if any, refreshes it).
    let a = app.clone();
    std::thread::spawn(move || {
        sleep(Duration::from_millis(300));
        let listening = state(&a).talk.lock().unwrap().listening;
        if listening {
            early_capture(&a);
        }
    });
    // Pointing is holding still: checked every 100 ms while listening.
    let a = app.clone();
    std::thread::spawn(move || {
        loop {
            sleep(Duration::from_millis(100));
            if !state(&a).talk.lock().unwrap().listening {
                return;
            }
            check_dwell(&a);
        }
    });
}

/// A cursor move. While Badi is busy (listening, thinking, speaking…, not idle or guiding, as in v1's
/// startFollow) its cursor trails the mouse on every display; while the talk keys are held the trail
/// is also ink.
pub fn on_move(app: &AppHandle, px: i32, py: i32) {
    let Some((label, x, y)) = to_overlay(app, px, py) else { return };
    turn::on_hover(app, x, y);
    let listening = state(app).talk.lock().unwrap().listening;
    let phase = turn::phase(app);
    if listening || !matches!(phase.as_str(), "idle" | "guiding") {
        // Overlay space is shared: each overlay subtracts its own origin, so one message serves all.
        windows::broadcast(app, "mouse", json!({ "x": x, "y": y }));
    }
    if !listening {
        return;
    }
    let (dwell_cleared, circle) = {
        let st = state(app);
        let mut t = st.talk.lock().unwrap();
        t.ink.push(TrailPoint { x, y, t: now_ms() as f64 });
        if t.ink.len() > INK_MAX_POINTS {
            let extra = t.ink.len() - INK_MAX_POINTS;
            t.ink.drain(..extra);
            t.seg_start = 0;
        }
        let cleared = std::mem::take(&mut t.still_announced);
        // A loop closes when the cursor comes back near an earlier point of the current stroke.
        let seg = &t.ink[t.seg_start..];
        let last = *seg.last().unwrap();
        let mut circle = None;
        if seg.len() >= 9 {
            for i in (0..=seg.len() - 9).rev() {
                if (seg[i].x - last.x).hypot(seg[i].y - last.y) > 24.0 {
                    continue;
                }
                if let Some(c) = gesture::circle_of(&seg[i..]) {
                    circle = Some(c);
                    break;
                }
            }
        }
        if circle.is_some() {
            t.seg_start = t.ink.len();
        }
        (cleared, circle)
    };
    if dwell_cleared {
        windows::emit_overlay(app, &label, "ink", json!({ "dwell": null }));
    }
    windows::emit_overlay(app, &label, "ink", json!({ "x": x, "y": y }));
    if let Some(c) = circle {
        set_mark(app, c);
    }
}

// Pointing: holding still. A ring starts filling after a moment; at 0.6 s it counts.
fn check_dwell(app: &AppHandle) {
    let now = now_ms() as f64;
    let (announce, point) = {
        let st = state(app);
        let mut t = st.talk.lock().unwrap();
        let seg = &t.ink[t.seg_start..];
        let Some(last) = seg.last().copied() else { return };
        let point = gesture::dwell_of(seg, now);
        let announce = (!t.still_announced && now - last.t > 150.0).then_some(last);
        if announce.is_some() {
            t.still_announced = true;
        }
        if point.is_some() {
            t.seg_start = t.ink.len();
        }
        (announce, point)
    };
    if let Some(last) = announce
        && let Some(label) = overlay_at(app, last.x, last.y) {
            windows::emit_overlay(app, &label, "ink", json!({ "dwell": { "x": last.x, "y": last.y, "ms": DWELL_MS - (now - last.t) } }));
        }
    if let Some(p) = point {
        set_mark(app, p);
    }
}

fn mark_anchor(m: &Mark) -> (f64, f64) {
    match *m {
        Mark::Point { x, y, .. } => (x, y),
        // An underline's words are just above the line.
        Mark::Underline { b } => ((b.x1 + b.x2) / 2.0, b.y1 - 12.0),
        Mark::Circle { b } => ((b.x1 + b.x2) / 2.0, (b.y1 + b.y2) / 2.0),
    }
}

// The newest mark wins: shown cleanly (an ellipse, a line, a dot), the screenshot refreshed for it,
// and what is under it looked up by name.
fn set_mark(app: &AppHandle, mark: Mark) {
    state(app).talk.lock().unwrap().mark = Some(mark);
    let (ax, ay) = mark_anchor(&mark);
    if let Some(label) = overlay_at(app, ax, ay) {
        windows::emit_overlay(app, &label, "gesture", mark);
    }
    early_capture(app);
    let (tx, rx) = channel();
    state(app).talk.lock().unwrap().under = Some((mark, rx));
    if let Some((x, y)) = to_physical(app, ax, ay) {
        let a = app.clone();
        std::thread::spawn(move || {
            let r = state(&a).uia.request("element_at", json!({ "x": x.round(), "y": y.round() }), Some(MARK_LOOKUP)).ok().and_then(|v| v.as_str().map(String::from));
            let _ = tx.send(r);
        });
    }
}

// "Melihat area": Badi's cursor flies to the marked region and scans it while the question is heard.
fn scan_mark(app: &AppHandle) {
    let Some(mark) = state(app).talk.lock().unwrap().mark else { return };
    let (ax, ay) = mark_anchor(&mark);
    if let Some(label) = overlay_at(app, ax, ay) {
        windows::emit_overlay(app, &label, "scan", gesture::region_of(&mark));
    }
}

fn early_capture(app: &AppHandle) {
    let mark = state(app).talk.lock().unwrap().mark;
    {
        let st = state(app);
        let t = st.talk.lock().unwrap();
        if t.early.as_ref().is_some_and(|(m, _)| *m == mark) {
            return;
        }
    }
    let slot: Shots = Arc::new(Mutex::new(None));
    state(app).talk.lock().unwrap().early = Some((mark, slot.clone()));
    let a = app.clone();
    std::thread::spawn(move || {
        let r = capture(&a, mark.as_ref());
        *slot.lock().unwrap() = Some(r);
    });
}

/// Screenshots for a question asked in a realtime session (with the last mark, if any).
pub fn capture_now(app: &AppHandle) -> Result<Vec<Shot>, String> {
    let mark = state(app).talk.lock().unwrap().mark;
    capture(app, mark.as_ref())
}

/// Throws away the hold's recording (a realtime turn took it), keeping the phase as it is.
pub fn discard(app: &AppHandle) {
    let st = state(app);
    if std::mem::take(&mut st.talk.lock().unwrap().listening) {
        let _ = st.mic.stop();
    }
    windows::broadcast(app, "ink", json!({ "reset": true }));
}

/// When the current hold started.
pub fn held_ms(app: &AppHandle) -> u128 {
    state(app).talk.lock().unwrap().started.map(|s| s.elapsed().as_millis()).unwrap_or(0)
}

fn capture(app: &AppHandle, mark: Option<&Mark>) -> Result<Vec<Shot>, String> {
    let cursor = hook::last_cursor().and_then(|(x, y)| to_overlay(app, x, y)).map(|(_, x, y)| (x, y)).unwrap_or((f64::NAN, f64::NAN));
    capture_all(&displays(app), cursor, mark, true)
}

// The keys were released: the ink ends (an unfinished stroke may still be an underline), and the
// screenshots for this question are the early ones if they still match the mark, else fresh ones.
pub fn on_talk_up(app: &AppHandle) {
    // Esc or a new hold while this question is still being heard drops it.
    let turn_gen = turn::generation();
    crate::wake::forget();
    let underline = {
        let st = state(app);
        let mut t = st.talk.lock().unwrap();
        if !t.listening {
            return;
        }
        t.listening = false;
        if t.mark.is_none() { gesture::underline_of(&t.ink[t.seg_start..]) } else { None }
    };
    if let Some(line) = underline {
        set_mark(app, line);
    }
    // Speech to text runs alongside the capture.
    turn::set_phase(app, if underline.is_some() || state(app).talk.lock().unwrap().mark.is_some() { "looking" } else { "transcribing" });
    scan_mark(app);
    // An answer to a waiting question needs no "sent" chime or filler.
    let answering = crate::notch::pending(app);
    turn::begin_turn(app, turn_gen, !answering, !answering);
    let heard = {
        let a = app.clone();
        std::thread::spawn(move || {
            let st = state(&a);
            let rec = st.mic.stop()?;
            if rec.duration < MIN_SPEECH || rec.samples.is_empty() {
                return Ok(String::new());
            }
            let key = st.store.secret("openaiApiKey").unwrap_or_default();
            let model = st.store.get("sttModel").as_str().unwrap_or("gpt-4o-mini-transcribe").to_string();
            let lang = match st.store.get("language").as_str() {
                Some("id") => Some("id"),
                Some("en") => Some("en"),
                _ => None,
            };
            badi_voice::stt::transcribe(&key, &model, lang, &rec.samples, badi_voice::mic::RATE)
        })
    };
    let (mark, early, under, started) = {
        let st = state(app);
        let mut t = st.talk.lock().unwrap();
        (t.mark, t.early.take(), t.under.take(), t.started)
    };
    let waited_from = Instant::now();
    let mut shots = None;
    if let Some((m, slot)) = early.filter(|(m, _)| *m == mark) {
        let _ = m;
        while shots.is_none() && waited_from.elapsed() < Duration::from_secs(5) {
            shots = slot.lock().unwrap().clone();
            if shots.is_none() {
                sleep(Duration::from_millis(10));
            }
        }
    }
    let mut shots = shots.unwrap_or_else(|| capture(app, mark.as_ref()));
    // The element's name joins the mark's description when the lookup is back in time.
    let named = under.filter(|(m, _)| Some(*m) == mark).and_then(|(_, rx)| rx.recv_timeout(MARK_LOOKUP_WAIT).ok().flatten());
    if let (Ok(list), Some(name)) = (shots.as_mut(), &named)
        && let Some(m) = list.iter_mut().find_map(|s| s.mark.as_mut()) {
            m.text = format!("{} Under it: {name}.", m.text);
        }
    let ready_ms = waited_from.elapsed().as_millis();
    let held_ms = started.map(|s| s.elapsed().as_millis()).unwrap_or(0);

    let heard = heard.join().unwrap_or_else(|_| Err("speech to text crashed".into()));
    let stt_ms = waited_from.elapsed().as_millis();
    if turn::generation() != turn_gen {
        return; // cancelled while listening back
    }
    // After a hold that asked nothing, a question Badi is waiting on stays up.
    let settle = |app: &AppHandle| turn::set_phase(app, crate::notch::pending_phase(app).unwrap_or("idle"));
    let shots = match shots {
        Ok(s) => s,
        Err(e) => {
            windows::broadcast(app, "error", e);
            return settle(app);
        }
    };
    let marked = shots.iter().any(|s| s.mark.is_some());
    let pending = crate::notch::pending(app);
    let transcript = match heard {
        Ok(t) if !t.is_empty() => t,
        // A marked region with nothing said still asks about it (never as an answer to a question).
        Ok(_) if marked && !pending => crate::i18n::t(app, "markOnly"),
        Ok(_) if held_ms < 350 => return settle(app), // a tap
        Ok(_) => {
            let line = if held_ms < MIN_SPEECH.as_millis() { "tooShort" } else { "notHeard" };
            windows::broadcast(app, "error", crate::i18n::t(app, line));
            return settle(app);
        }
        Err(e) => {
            crate::cloud::turn_error(app, e);
            return settle(app);
        }
    };
    crate::i18n::note_spoken(app, &transcript);
    windows::broadcast(app, "transcript", &transcript);
    // The answer to a question the task is waiting on.
    if pending {
        crate::notch::answer_text(app, &transcript);
        return;
    }
    if std::env::var_os("BADI_LOG").is_some() {
        eprintln!("[talk] held {held_ms} ms; after release: capture {ready_ms} ms, speech to text {stt_ms} ms: {transcript}");
    }
    turn::run_companion(app, &transcript, shots, turn_gen);
}

/// A typed question (the Ask bar, a notch shortcut): the screen and the text go to Claude, as in v1's
/// askTyped. A question Badi is waiting on gets the text as its answer instead.
pub fn ask_typed(app: &AppHandle, text: &str) {
    if crate::notch::pending(app) {
        crate::notch::answer_text(app, text);
        return;
    }
    if !crate::cloud::require(app) {
        return;
    }
    crate::i18n::note_spoken(app, text);
    crate::brain::stop(app);
    turn::cancel(app);
    turn::end_guide(app);
    windows::broadcast(app, "clear", Value::Null);
    turn::set_phase(app, "thinking");
    let turn_gen = turn::generation();
    turn::begin_turn(app, turn_gen, true, true);
    let (a, text) = (app.clone(), text.to_string());
    std::thread::spawn(move || {
        // The Ask bar closes first, so it isn't in the screenshot.
        sleep(Duration::from_millis(120));
        let cursor = badi_input::send::cursor();
        let at = to_overlay(&a, cursor.0, cursor.1).map(|(_, x, y)| (x, y)).unwrap_or((f64::NAN, f64::NAN));
        match capture_all(&displays(&a), at, None, true) {
            Ok(shots) => {
                windows::broadcast(&a, "transcript", &text);
                turn::run_companion(&a, &text, shots, turn_gen);
            }
            Err(e) => {
                windows::broadcast(&a, "error", e);
                turn::set_phase(&a, "idle");
            }
        }
    });
}

// When ElevenLabs last refused (a wrong key or voice): for a while Badi speaks with the next voice
// instead of answering in silence, and says so once.
static ELEVEN_REFUSED: LazyLock<Mutex<Option<Instant>>> = LazyLock::new(|| Mutex::new(None));
const ELEVEN_REST: Duration = Duration::from_secs(10 * 60);

fn eleven_resting() -> bool {
    ELEVEN_REFUSED.lock().unwrap().is_some_and(|at| at.elapsed() < ELEVEN_REST)
}

/// The voice for this conversation (v1 ttsMode): ElevenLabs, else OpenAI, else a Windows voice; None = off.
pub fn voice(app: &AppHandle) -> Option<Voice> {
    let store = &state(app).store;
    let get = |k: &str| store.get(k).as_str().unwrap_or("").to_string();
    // The language of the answer (with "auto", the question's), so names and numbers sound right.
    let lang = crate::i18n::lang(app).to_string();
    let eleven = store.secret("elevenlabsApiKey").filter(|_| !eleven_resting());
    let openai = store.secret("openaiApiKey");
    let system = Voice::System { name: get("systemVoice"), lang: lang.clone() };
    // On Badi Cloud every plan has both voices; the one picked in Settings is used (OpenAI costs fewer credits).
    if let (Some(plan_voice), Some(token)) = (crate::cloud::voice(app), store.secret("openaiApiKey")) {
        return match get("tts").as_str() {
            "off" => None,
            "system" => Some(system),
            _ if plan_voice == "elevenlabs" && !eleven_resting() => Some(Voice::Eleven { key: token, voice: get("elevenlabsVoice"), model: get("elevenlabsModel"), lang }),
            _ => Some(Voice::OpenAi { key: token, voice: get("openaiVoice"), model: "gpt-4o-mini-tts".into() }),
        };
    }
    match get("tts").as_str() {
        "off" => None,
        "system" => Some(system),
        mode => Some(match (mode, eleven, openai) {
            ("elevenlabs", Some(key), _) => Voice::Eleven { key, voice: get("elevenlabsVoice"), model: get("elevenlabsModel"), lang },
            (_, _, Some(key)) => Voice::OpenAi { key, voice: get("openaiVoice"), model: get("openaiTtsModel") },
            _ => system,
        }),
    }
}

/// A line's audio in `v`. When ElevenLabs refuses, the line is fetched again in the next voice (so it
/// still sounds), later lines skip ElevenLabs for a while, and the notch says why once.
pub fn synthesize(app: &AppHandle, text: &str, v: &Voice) -> Result<badi_voice::Clip, String> {
    let st = state(app);
    // A reply already speaking in ElevenLabs when it refused goes on in the next voice.
    if matches!(v, Voice::Eleven { .. })
        && eleven_resting()
        && let Some(next) = voice(app)
    {
        return st.tts.synthesize(text, &next);
    }
    match st.tts.synthesize(text, v) {
        Err(e) if matches!(v, Voice::Eleven { .. }) && badi_voice::tts::is_refused(&e) => {
            eprintln!("[voice] {e}");
            let first = ELEVEN_REFUSED.lock().unwrap().replace(Instant::now()).is_none_or(|at| at.elapsed() >= ELEVEN_REST);
            if first {
                crate::notch::show_notice(app, "info", "Suara ElevenLabs sedang menolak akses, jadi aina pakai suara cadangan dulu.", &[], vec![], 6000);
            }
            match voice(app) {
                Some(next) if !matches!(next, Voice::Eleven { .. }) => st.tts.synthesize(text, &next),
                _ => Err(e),
            }
        }
        r => r,
    }
}

/// Speaks a line; the text shows in the notch the moment the voice starts, typed out over its length.
/// Without a voice (or when it fails) the line only shows as text.
pub fn say(app: &AppHandle, text: &str) {
    let clip = voice(app).map(|v| synthesize(app, text, &v));
    say_clip(app, text, clip);
}

/// Speaks a line without calling any paid service: the voice's clip fetched earlier (settings::prefetch),
/// else Windows' own voice.
pub fn say_free(app: &AppHandle, text: &str) {
    let st = state(app);
    let cached = voice(app).and_then(|v| st.tts.cached(text, &v));
    let clip = match cached {
        Some(c) => Ok(c),
        None => {
            let name = st.store.get("systemVoice").as_str().unwrap_or("").to_string();
            st.tts.synthesize(text, &Voice::System { name, lang: crate::i18n::lang(app).to_string() })
        }
    };
    say_clip(app, text, Some(clip));
}

fn say_clip(app: &AppHandle, text: &str, clip: Option<Result<badi_voice::Clip, String>>) {
    let st = state(app);
    let shown = |ms: u128| {
        windows::broadcast(app, "phase", "speaking");
        windows::broadcast(app, "voice", true);
        windows::broadcast(app, "text", json!({ "text": text, "ms": ms }));
    };
    match clip {
        Some(Ok(clip)) => {
            let a = app.clone();
            let t = text.to_string();
            st.player.play(clip, move |d| {
                windows::broadcast(&a, "phase", "speaking");
                windows::broadcast(&a, "voice", true);
                windows::broadcast(&a, "text", json!({ "text": t, "ms": d.as_millis() }));
            });
            windows::broadcast(app, "voice", false);
        }
        Some(Err(e)) => {
            eprintln!("[voice] {e}");
            shown(text.chars().count() as u128 * 40);
        }
        None => shown(text.chars().count() as u128 * 40),
    }
}

/// Esc, or a real key during the hold (the user is typing a shortcut): nothing is asked, and Esc also
/// stops Badi talking.
pub fn cancel(app: &AppHandle) {
    let st = state(app);
    crate::dictation::cancel(app); // Esc drops a dictation left running too
    crate::wake::forget();
    crate::realtime::stop(app);
    turn::cancel(app);
    turn::end_guide(app);
    if std::mem::take(&mut st.talk.lock().unwrap().listening) {
        let _ = st.mic.stop();
    }
    windows::broadcast(app, "ink", json!({ "reset": true }));
    windows::broadcast(app, "clear", Value::Null);
    turn::set_phase(app, "idle");
}
