// A companion turn: the question and screenshots go to Claude, the streamed reply is cut into steps
// (the tags before a sentence, plus that sentence), each sentence's voice is requested as soon as it is
// known, and a step's visuals and subtitle go out the moment its voice starts. The next step waits for
// both its speech and its drawings. Action tags run after the reply. Guided mode (TARGET/HOVER) waits
// for the click, recaptures and asks for the next step. Port of v1's main.js runCompanion,
// streamReply, playStep, beginGuide/advanceGuide/onGlobalMouseDown, and llm.js askCompanion/askFollowUp.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::sleep;
use std::time::{Duration, Instant};

use badi_llm::claude::{self, Request, Usage};
use badi_llm::prompts;
use badi_llm::tags::{Rect, Shot as LlmShot, Tag, extract_tags, to_global};
use badi_llm::{Step, StepSplitter};
use badi_screen::capture::{Shot, capture_all};
use badi_voice::Clip;
use serde_json::{Value, json};
use tauri::{AppHandle, Manager};

use crate::i18n::t;
use crate::notch::{self, now_ms};
use crate::{AppState, talk, windows};

// Bumped by Esc, a new question or a new hold: everything of an older generation stops.
static GEN: AtomicU64 = AtomicU64::new(0);

// A sentence waits at most this long for its cloud audio; then it is shown as text and Badi moves on.
const AUDIO_WAIT: Duration = Duration::from_secs(7);
const DRAWING_WAIT: Duration = Duration::from_secs(20);
const GIVE_UP: Duration = Duration::from_secs(45);
const HISTORY_TURNS: usize = 6;
const HISTORY_TTL: Duration = Duration::from_secs(10 * 60);
const WARM_EVERY: Duration = Duration::from_secs(4 * 60);
const GUIDE_IDLE: Duration = Duration::from_secs(90);

pub struct Guide {
    id: u64,
    task: String,
    steps: Vec<String>,
    visual: Value, // the TARGET/HOVER in the overlay space
    rounds: u32,
    hover_since: Option<Instant>,
}

#[derive(Default)]
pub struct Turns {
    phase: Phase,
    history: Vec<(String, String)>,
    last_turn: Option<Instant>,
    warmed: Option<Instant>,
    step_seq: u64,
    waiters: HashMap<u64, Sender<()>>,
    guide: Option<Guide>,
    guide_seq: u64,
}

/// The phase starts "idle": an empty one counted as busy, so until the first turn every mouse move
/// redrew the overlays (Badi's cursor following it) and the start greeting never played.
pub struct Phase(String);

impl Default for Phase {
    fn default() -> Phase {
        Phase("idle".into())
    }
}

impl std::ops::Deref for Phase {
    type Target = String;
    fn deref(&self) -> &String {
        &self.0
    }
}

fn state(app: &AppHandle) -> tauri::State<'_, AppState> {
    app.state::<AppState>()
}

pub fn set_phase(app: &AppHandle, phase: &str) {
    let changed = {
        let st = state(app);
        let mut t = st.turns.lock().unwrap();
        let changed = *t.phase != phase;
        t.phase = Phase(phase.into());
        changed
    };
    windows::broadcast(app, "phase", phase);
    if changed {
        crate::settings::update_tray(app);
        if phase == "idle" {
            windows::overlays_quiet(app);
        } else {
            windows::overlays_busy(app);
        }
        // Badi's cursor starts out next to the mouse even before it moves (talk::on_move follows it).
        if !matches!(phase, "idle" | "guiding") {
            let (cx, cy) = badi_input::send::cursor();
            if let Some((_, x, y)) = crate::talk::to_overlay(app, cx, cy) {
                windows::broadcast(app, "mouse", serde_json::json!({ "x": x, "y": y }));
            }
        }
        if std::env::var_os("BADI_LOG").is_some() {
            eprintln!("[phase] {phase}");
        }
    }
}

pub fn phase(app: &AppHandle) -> String {
    state(app).turns.lock().unwrap().phase.to_string()
}

/// A short chime (v1's worker chimes): "start", "sent", "step", "agent-start", "agent-done", and the
/// pomodoro's "focus-done" and "break-done".
pub fn chime(_app: &AppHandle, name: &str) {
    badi_voice::chime::play(name);
}

/// The current turn generation: a turn started with it plays only while it is still current.
pub fn generation() -> u64 {
    GEN.load(Ordering::SeqCst)
}

/// From the moment a question is sent until Badi's voice starts, something always shows Badi is on it
/// (v1 beginTurn): a "sent" chime, a short spoken filler after 1.5 s (only if its audio is already in
/// memory, so it never delays the answer), a timer after 6 s, an Esc hint after 20 s, and after 45 s
/// (counted from the question, speech to text included) the turn gives up with a clear message.
pub fn begin_turn(app: &AppHandle, turn_gen: u64, sent_chime: bool, filler: bool) {
    if sent_chime {
        chime(app, "sent");
    }
    let a = app.clone();
    std::thread::spawn(move || {
        let t0 = Instant::now();
        let mut filler_done = !filler;
        let mut shown = 0;
        loop {
            sleep(Duration::from_millis(100));
            let waiting = matches!(phase(&a).as_str(), "transcribing" | "looking" | "thinking");
            if GEN.load(Ordering::SeqCst) != turn_gen || !waiting {
                break;
            }
            let ms = t0.elapsed().as_millis() as u64;
            if !filler_done && ms >= 1500 {
                filler_done = true;
                play_filler(&a);
            }
            let s = ms / 1000;
            if s >= GIVE_UP.as_secs() {
                cancel(&a);
                windows::broadcast(&a, "elapsed", Value::Null);
                windows::broadcast(&a, "error", t(&a, "tookTooLong"));
                return set_phase(&a, "idle");
            }
            if s >= 6 && s != shown {
                shown = s;
                windows::broadcast(&a, "elapsed", json!({ "s": s, "hint": if s >= 20 { t(&a, "escHint") } else { String::new() } }));
            }
        }
        if shown > 0 {
            windows::broadcast(&a, "elapsed", Value::Null);
        }
    });
}

// "Bentar, aku lihat dulu ya.": spoken if its audio is ready this instant, else shown as text (and
// fetched for next time).
fn play_filler(app: &AppHandle) {
    let line = badi_llm::persona::filler(crate::i18n::lang(app) == "en").to_string();
    let st = state(app);
    let voice = talk::voice(app);
    match voice.as_ref().and_then(|v| st.tts.cached(&line, v)) {
        Some(clip) => {
            let (a, shown, l) = (app.clone(), app.clone(), line.clone());
            std::thread::spawn(move || {
                state(&a).player.play(clip, move |d| windows::broadcast(&shown, "text", json!({ "text": l, "ms": d.as_millis() })));
            });
        }
        None => {
            windows::broadcast(app, "text", json!({ "text": line, "ms": line.chars().count() * 40 }));
            if let Some(v) = voice {
                let a = app.clone();
                std::thread::spawn(move || {
                    let _ = state(&a).tts.synthesize(&line, &v);
                });
            }
        }
    }
}

/// Stops whatever Badi is saying or about to say (Esc, a new hold).
pub fn cancel(app: &AppHandle) {
    GEN.fetch_add(1, Ordering::SeqCst);
    let st = state(app);
    st.player.stop();
    for (_, w) in st.turns.lock().unwrap().waiters.drain() {
        let _ = w.send(());
    }
}

/// The overlay finished drawing a step (overlay:stepDone).
pub fn step_done(app: &AppHandle, id: u64) {
    if let Some(w) = state(app).turns.lock().unwrap().waiters.remove(&id) {
        let _ = w.send(());
    }
}

struct Model {
    key: String,
    model: String,
    effort: String,
    thinking: bool,
    system: String,
}

impl Model {
    fn of(app: &AppHandle) -> Model {
        let store = &state(app).store;
        let s = |k: &str, d: &str| store.get(k).as_str().filter(|v| !v.is_empty()).unwrap_or(d).to_string();
        Model {
            key: store.secret("anthropicApiKey").unwrap_or_default(),
            model: crate::cloud::model(app, &s("model", "claude-opus-5")),
            effort: s("effort", "low"),
            thinking: store.get("thinking").as_bool().unwrap_or(false),
            system: prompts::companion_system(&s("userName", ""), &s("focusFolder", ""), &crate::focus::brief(app), &s("language", "auto")),
        }
    }

    fn request(&self, messages: Value) -> Request<'_> {
        Request { key: &self.key, model: &self.model, effort: &self.effort, thinking: self.thinking, system: &self.system, messages, max_tokens: 16_000 }
    }
}

/// Readies Claude's prompt cache while the user is still talking (at most every 4 minutes).
pub fn prewarm(app: &AppHandle) {
    {
        let st = state(app);
        let mut t = st.turns.lock().unwrap();
        if t.warmed.is_some_and(|w| w.elapsed() < WARM_EVERY) {
            return;
        }
        t.warmed = Some(Instant::now());
    }
    let m = Model::of(app);
    let a = app.clone();
    std::thread::spawn(move || {
        if m.key.is_empty() || claude::prewarm(&m.request(Value::Null)).is_err() {
            state(&a).turns.lock().unwrap().warmed = None;
        }
    });
}

fn screenshot_blocks(shots: &[Shot]) -> Vec<Value> {
    let mut blocks = Vec::new();
    for s in shots {
        let cursor = s.cursor.as_ref().map(|c| format!(" Mouse cursor is at {},{}.", c.x, c.y)).unwrap_or_default();
        blocks.push(json!({ "type": "text", "text": format!("screen{} ({}x{} px).{cursor}", s.index, s.width, s.height) }));
        blocks.push(json!({ "type": "image", "source": { "type": "base64", "media_type": s.media_type, "data": s.base64 } }));
        if let Some(m) = &s.mark {
            blocks.push(json!({ "type": "text", "text": format!("{} Zoomed view of that spot (coordinates in tags still refer to the full screenshot above):", m.text) }));
            blocks.push(json!({ "type": "image", "source": { "type": "base64", "media_type": "image/jpeg", "data": m.crop_base64 } }));
        }
    }
    blocks
}

// Recent turns, so "what about that one?" works across holds (screenshots are not kept).
fn history(app: &AppHandle) -> (Vec<Value>, Option<u128>) {
    let st = state(app);
    let mut t = st.turns.lock().unwrap();
    if t.last_turn.is_some_and(|l| l.elapsed() > HISTORY_TTL) {
        t.history.clear();
    }
    let msgs = t.history.iter().flat_map(|(u, a)| [json!({ "role": "user", "content": u }), json!({ "role": "assistant", "content": if a.is_empty() { "(no reply)" } else { a } })]).collect();
    (msgs, t.last_turn.map(|l| l.elapsed().as_millis()))
}

/// The recent turns as text, for a realtime session's context (v1 llm.historyText); "" when none.
pub fn history_text(app: &AppHandle) -> String {
    let (msgs, _) = history(app);
    let lines: Vec<String> = msgs
        .iter()
        .map(|m| format!("{}: {}", if m["role"] == "assistant" { "aina" } else { "User" }, m["content"].as_str().unwrap_or("").chars().take(600).collect::<String>()))
        .collect();
    if lines.is_empty() { String::new() } else { format!("Recent conversation, oldest first:\n{}", lines.join("\n")) }
}

/// Remembers a turn that happened in a realtime session (so a later hold can follow up on it).
pub fn remember_turn(app: &AppHandle, user: &str, assistant: &str) {
    remember(app, user, assistant);
}

/// How long since the last question (None when there was none this session).
pub fn since_last_turn(app: &AppHandle) -> Option<u128> {
    state(app).turns.lock().unwrap().last_turn.map(|l| l.elapsed().as_millis())
}

fn remember(app: &AppHandle, user: &str, assistant: &str) {
    let st = state(app);
    let mut t = st.turns.lock().unwrap();
    t.history.push((user.into(), assistant.into()));
    if t.history.len() > HISTORY_TURNS {
        t.history.remove(0);
    }
    t.last_turn = Some(Instant::now());
}

fn llm_shots(shots: &[Shot]) -> Vec<LlmShot> {
    shots.iter().map(|s| LlmShot { bounds: Rect { x: s.bounds.x, y: s.bounds.y, width: s.bounds.width, height: s.bounds.height }, width: s.width as f64, height: s.height as f64 }).collect()
}

fn is_action(t: &Tag) -> bool {
    matches!(t, Tag::Delegate { .. } | Tag::Do { .. } | Tag::Focus { .. } | Tag::Drop { .. } | Tag::Timer { .. } | Tag::Remind { .. } | Tag::Media { .. })
}

fn tag_screen(t: &Tag) -> usize {
    serde_json::to_value(t).ok().and_then(|v| v["screen"].as_u64()).unwrap_or(1).max(1) as usize
}

// One step: wait for its audio (bounded), then show its visuals and subtitle as the voice starts.
// Returns when both the speech and the drawings are done (or the turn was cancelled).
fn play_step(app: &AppHandle, step: Step, audio: Option<Receiver<Result<Clip, String>>>, shots: &[Shot], turn_gen: u64) {
    let clip = audio.and_then(|rx| rx.recv_timeout(AUDIO_WAIT).ok()).and_then(|r| r.map_err(|e| eprintln!("[voice] {e}")).ok());
    if GEN.load(Ordering::SeqCst) != turn_gen {
        return;
    }
    let llm = llm_shots(shots);
    let visuals: Vec<(String, Value)> = step
        .tags
        .iter()
        .filter(|t| !is_action(t))
        .filter_map(|t| {
            let label = shots.get(tag_screen(t) - 1).or(shots.first())?.display_id.clone();
            Some((label, to_global(t, &llm)?))
        })
        .collect();
    let drawing = step.tags.iter().any(|t| t.is_drawing());
    let (id, drawn) = {
        let st = state(app);
        let mut t = st.turns.lock().unwrap();
        t.step_seq += 1;
        let id = t.step_seq;
        let drawn = drawing.then(|| {
            let (tx, rx) = channel();
            t.waiters.insert(id, tx);
            rx
        });
        (id, drawn)
    };
    let text = step.text.clone();
    let a = app.clone();
    let show = move |ms: u128| {
        set_phase(&a, "speaking");
        windows::broadcast(&a, "voice", true);
        for (label, v) in &visuals {
            let mut v = v.clone();
            v["stepId"] = json!(id);
            windows::emit_overlay(&a, label, "visual", v);
        }
        if !text.is_empty() {
            windows::broadcast(&a, "text", json!({ "text": text, "ms": ms }));
        }
    };
    match clip {
        Some(clip) if !step.text.is_empty() => {
            // No sound device: the text still types out at reading pace.
            let est = step.text.chars().count() as u128 * 40;
            state(app).player.play(clip, move |d| show(if d.is_zero() { est } else { d.as_millis() }));
        }
        _ => {
            let n = step.text.chars().count() as u64;
            show(n as u128 * 40);
            sleep(Duration::from_millis(n * 55));
        }
    }
    windows::broadcast(app, "voice", false);
    if let Some(rx) = drawn {
        let until = Instant::now() + DRAWING_WAIT;
        while GEN.load(Ordering::SeqCst) == turn_gen && Instant::now() < until {
            if rx.recv_timeout(Duration::from_millis(100)).is_ok() {
                break;
            }
        }
    }
}

/// Plays streamed text step by step: each sentence's voice is requested as soon as the sentence is
/// known, and steps play in order with their visuals. Used for Claude's replies and for the realtime
/// model's text when Badi speaks it with its own voice.
// A step and, when it is spoken, its audio on the way.
type Queued = (Step, Option<Receiver<Result<Clip, String>>>);

pub struct Speaker {
    app: AppHandle,
    tx: Option<Sender<Queued>>,
    player: Option<std::thread::JoinHandle<()>>,
    splitter: StepSplitter,
    voice: Option<badi_voice::tts::Voice>,
}

impl Speaker {
    pub fn new(app: &AppHandle, shots: Vec<Shot>, turn_gen: u64) -> Speaker {
        let (tx, rx) = channel::<Queued>();
        let a = app.clone();
        let player = std::thread::spawn(move || {
            for (step, audio) in rx {
                if GEN.load(Ordering::SeqCst) != turn_gen {
                    break;
                }
                play_step(&a, step, audio, &shots, turn_gen);
            }
        });
        Speaker { app: app.clone(), tx: Some(tx), player: Some(player), splitter: StepSplitter::new(), voice: talk::voice(app) }
    }

    fn enqueue(&self, steps: Vec<Step>) {
        let Some(tx) = &self.tx else { return };
        for step in steps {
            let audio = match (&self.voice, step.text.is_empty()) {
                (Some(v), false) => {
                    let (atx, arx) = channel();
                    let (a, v, text) = (self.app.clone(), v.clone(), step.text.clone());
                    std::thread::spawn(move || {
                        let _ = atx.send(talk::synthesize(&a, &text, &v));
                    });
                    Some(arx)
                }
                _ => None,
            };
            let _ = tx.send((step, audio));
        }
    }

    pub fn push(&mut self, delta: &str) {
        let steps = self.splitter.push(delta);
        self.enqueue(steps);
    }

    /// The text is complete: plays what is left and returns once everything was said (or cancelled).
    pub fn finish(mut self) {
        let steps = self.splitter.finish();
        self.enqueue(steps);
        self.tx = None;
        if let Some(p) = self.player.take() {
            let _ = p.join();
        }
    }
}

/// Streams a reply and plays it step by step. Returns the full reply's tags and text, or None when
/// cancelled.
fn stream_reply(app: &AppHandle, messages: Value, shots: Vec<Shot>, turn_gen: u64) -> Result<Option<(String, Vec<Tag>)>, String> {
    if GEN.load(Ordering::SeqCst) != turn_gen {
        return Ok(None);
    }
    set_phase(app, "thinking");
    let started = Instant::now();
    let mut speaker = Speaker::new(app, shots, turn_gen);
    let m = Model::of(app);
    let mut first_token: Option<u128> = None;
    let result = claude::stream(&m.request(messages), &|| GEN.load(Ordering::SeqCst) != turn_gen, &mut |delta| {
        first_token.get_or_insert(started.elapsed().as_millis());
        speaker.push(delta);
    });
    let (text, usage) = match result {
        Ok(r) => r,
        Err(_) if GEN.load(Ordering::SeqCst) != turn_gen => return Ok(None),
        Err(e) => return Err(e),
    };
    speaker.finish();
    meter(&m.model, usage, first_token, started);
    if GEN.load(Ordering::SeqCst) != turn_gen {
        return Ok(None);
    }
    Ok(Some((text.clone(), extract_tags(&text).0)))
}

// Every turn is metered (tokens and the estimated cost), as v1's usage.js did for the Brain.
fn meter(model: &str, u: Usage, first_token: Option<u128>, started: Instant) {
    if std::env::var_os("BADI_LOG").is_some() || std::env::var_os("BADI_TIMING").is_some() {
        eprintln!(
            "[turn] {model}: first token {} ms, done {} ms; in {} + cache read {} + cache write {}, out {}; ≈ ${:.4} (Rp {:.0})",
            first_token.unwrap_or(0),
            started.elapsed().as_millis(),
            u.input,
            u.cache_read,
            u.cache_write,
            u.output,
            u.usd(model),
            u.usd(model) * 18_000.0
        );
    }
}

/// A question about the screen (v1 runCompanion + askCompanion).
pub fn run_companion(app: &AppHandle, transcript: &str, shots: Vec<Shot>, turn_gen: u64) {
    let (mut messages, since) = history(app);
    let question = if shots.iter().any(|s| s.mark.is_some()) { format!("(Look at what I marked on screen and answer about it.) {transcript}") } else { transcript.to_string() };
    let mut content = screenshot_blocks(&shots);
    content.push(json!({ "type": "text", "text": format!("(context) {}", prompts::moment(since)) }));
    // Passages of the user's knowledge base that match the question (none when nothing matches).
    if let Some(kb) = crate::knowledge::context_for(app, transcript) {
        content.push(json!({ "type": "text", "text": kb }));
    }
    content.push(json!({ "type": "text", "text": question }));
    messages.push(json!({ "role": "user", "content": content }));
    let reply = match stream_reply(app, Value::Array(messages), shots.clone(), turn_gen) {
        Ok(Some(r)) => r,
        Ok(None) => return,
        Err(e) => {
            crate::cloud::turn_error(app, e);
            return set_phase(app, "idle");
        }
    };
    remember(app, transcript, &reply.0);
    finish(app, transcript, &reply.1, &shots);
}

// After a reply: the action tags run, a TARGET/HOVER starts (or continues) a walkthrough.
fn finish(app: &AppHandle, task: &str, tags: &[Tag], shots: &[Shot]) {
    let drew = tags.iter().any(|t| t.is_drawing());
    let mut guiding = false;
    let mut acting = false;
    for tag in tags {
        match tag {
            Tag::Target { .. } | Tag::Hover { .. } if !guiding => {
                if let Some(v) = to_global(tag, &llm_shots(shots)) {
                    begin_guide(app, task, v);
                    guiding = true;
                }
            }
            Tag::Timer { off: Some(true), .. } => {
                state(app).timers.lock().unwrap().stop_timer();
            }
            Tag::Timer { pomodoro: Some(true), label, .. } => state(app).timers.lock().unwrap().start_pomodoro(now_ms(), label.as_deref().unwrap_or("")),
            Tag::Timer { ms: Some(ms), label, .. } => state(app).timers.lock().unwrap().set_timer(now_ms(), *ms, label.as_deref().unwrap_or("")),
            Tag::Remind { at, text } => {
                state(app).timers.lock().unwrap().add_reminder(*at, text);
                crate::reminders::changed(app);
            }
            Tag::Drop { purpose } => notch::request_drop(app, purpose),
            Tag::Do { task } if !acting => {
                acting = true;
                crate::brain::start(app, task);
            }
            Tag::Delegate { task } => crate::agents::delegate(app, task),
            Tag::Focus { folder } if !acting => {
                acting = true;
                crate::agents::apply_focus(app, folder.clone());
            }
            Tag::Media { action } => {
                let (a, action) = (app.clone(), action.clone());
                std::thread::spawn(move || crate::apps::media_control(&a, &action));
            }
            _ => {}
        }
    }
    notch::send_status(app);
    if !guiding && !acting {
        settle(app, drew);
    }
}

// The answer stays up a while (longer when it drew something), and Badi is free again.
fn settle(app: &AppHandle, drew: bool) {
    windows::broadcast(app, "linger", if drew { 15_000 } else { 9000 });
    set_phase(app, "idle");
}

// ---------- guided mode ----------

fn begin_guide(app: &AppHandle, task: &str, visual: Value) {
    let id = {
        let st = state(app);
        let mut t = st.turns.lock().unwrap();
        t.guide_seq += 1;
        let id = t.guide_seq;
        let prev = t.guide.take().map(|g| (g.steps, g.rounds)).unwrap_or_default();
        t.guide = Some(Guide { id, task: task.into(), steps: prev.0, visual, rounds: prev.1, hover_since: None });
        id
    };
    arm_guide(app, id);
}

fn arm_guide(app: &AppHandle, id: u64) {
    set_phase(app, "guiding");
    // A HOVER step counts once the cursor rests on it, so it is checked on a clock, not only on moves.
    let a = app.clone();
    std::thread::spawn(move || {
        loop {
            sleep(Duration::from_millis(100));
            let live = state(&a).turns.lock().unwrap().guide.as_ref().is_some_and(|g| g.id == id);
            if !live || phase(&a) != "guiding" {
                return;
            }
            if let Some((_, x, y)) = badi_input::hook::last_cursor().and_then(|(x, y)| talk::to_overlay(&a, x, y)) {
                on_hover(&a, x, y);
            }
        }
    });
    // Idle timeout: quietly drop back to idle.
    let a = app.clone();
    std::thread::spawn(move || {
        sleep(GUIDE_IDLE);
        let still = state(&a).turns.lock().unwrap().guide.as_ref().is_some_and(|g| g.id == id);
        if still && phase(&a) == "guiding" {
            end_guide(&a);
            windows::broadcast(&a, "clear", Value::Null);
        }
    });
}

pub fn end_guide(app: &AppHandle) {
    let had = state(app).turns.lock().unwrap().guide.take().is_some();
    if had && phase(app) == "guiding" {
        set_phase(app, "idle");
    }
}

/// A click anywhere (physical px, from the mouse hook): inside the target advances the walkthrough,
/// outside a TARGET ends it without nagging.
pub fn on_mouse_down(app: &AppHandle, x: f64, y: f64) {
    if phase(app) != "guiding" {
        return;
    }
    let hit = {
        let st = state(app);
        let t = st.turns.lock().unwrap();
        let Some(g) = &t.guide else { return };
        let v = &g.visual;
        let (vx, vy, r) = (v["x"].as_f64().unwrap_or(0.0), v["y"].as_f64().unwrap_or(0.0), v["r"].as_f64().unwrap_or(14.0));
        ((x - vx).hypot(y - vy) <= r + 12.0, v["type"] == "hover", v["label"].as_str().unwrap_or("").to_string())
    };
    match hit {
        (true, hover, label) => {
            let label = if label.is_empty() { "the target".to_string() } else { label };
            let (done, verb) = if hover { ("Hovered", "hovered") } else { ("Clicked", "clicked") };
            if let Some(g) = state(app).turns.lock().unwrap().guide.as_mut() {
                g.steps.push(format!("{done} \"{label}\""));
            }
            let a = app.clone();
            std::thread::spawn(move || advance_guide(&a, &format!("The user {verb} the target \"{label}\".")));
        }
        (false, false, _) => {
            end_guide(app);
            windows::broadcast(app, "clear", Value::Null);
        }
        _ => {}
    }
}

/// The cursor (overlay space) while guiding a HOVER step: resting on it for 0.8 s counts.
pub fn on_hover(app: &AppHandle, x: f64, y: f64) {
    if phase(app) != "guiding" {
        return;
    }
    let fire = {
        let st = state(app);
        let mut t = st.turns.lock().unwrap();
        let Some(g) = t.guide.as_mut() else { return };
        if g.visual["type"] != "hover" {
            return;
        }
        let (vx, vy, r) = (g.visual["x"].as_f64().unwrap_or(0.0), g.visual["y"].as_f64().unwrap_or(0.0), g.visual["r"].as_f64().unwrap_or(14.0));
        if (x - vx).hypot(y - vy) > r + 8.0 {
            g.hover_since = None;
            return;
        }
        let since = *g.hover_since.get_or_insert_with(Instant::now);
        since.elapsed() >= Duration::from_millis(800)
    };
    if fire {
        on_mouse_down(app, x, y);
    }
}

fn advance_guide(app: &AppHandle, event: &str) {
    let (task, steps, rounds_left) = {
        let st = state(app);
        let mut t = st.turns.lock().unwrap();
        let Some(g) = t.guide.as_mut() else { return };
        g.rounds += 1;
        g.hover_since = None;
        let max = st.store.get("maxGuidedRounds").as_u64().unwrap_or(8) as u32;
        (g.task.clone(), g.steps.clone(), max.saturating_sub(g.rounds))
    };
    cancel(app);
    let turn_gen = generation();
    windows::broadcast(app, "clear", Value::Null);
    chime(app, "step");
    // Out of "guiding" at once, so the same click or hover can't count twice.
    set_phase(app, "thinking");
    // Give the app a moment to react to the click before recapturing.
    sleep(Duration::from_millis(700));
    begin_turn(app, turn_gen, false, false);
    let cursor = badi_input::hook::last_cursor().and_then(|(x, y)| talk::to_overlay(app, x, y)).map(|(_, x, y)| (x, y)).unwrap_or((f64::NAN, f64::NAN));
    let shots = match capture_all(&talk::displays(app), cursor, None, false) {
        Ok(s) => s,
        Err(e) => {
            windows::broadcast(app, "error", e);
            return end_guide(app);
        }
    };
    let mut content = screenshot_blocks(&shots);
    content.push(json!({ "type": "text", "text": prompts::follow_up_prompt(&task, &steps, rounds_left, event) }));
    match stream_reply(app, json!([{ "role": "user", "content": content }]), shots.clone(), turn_gen) {
        Ok(Some((text, tags))) => {
            remember(app, &format!("(guided step) {event}"), &text);
            let next = (rounds_left > 0).then(|| tags.iter().find(|t| matches!(t, Tag::Target { .. } | Tag::Hover { .. }))).flatten();
            match next.and_then(|t| to_global(t, &llm_shots(&shots))) {
                Some(v) => {
                    let id = {
                        let st = state(app);
                        let mut t = st.turns.lock().unwrap();
                        t.guide.as_mut().map(|g| {
                            g.visual = v;
                            g.id
                        })
                    };
                    if let Some(id) = id {
                        arm_guide(app, id);
                    }
                }
                // The walkthrough is over (done, or out of rounds). A follow-up reply never starts a
                // new walkthrough or runs action tags, as in v1.
                None => {
                    end_guide(app);
                    settle(app, tags.iter().any(|t| t.is_drawing()));
                }
            }
        }
        Ok(None) => {}
        Err(e) => {
            crate::cloud::turn_error(app, e);
            end_guide(app);
        }
    }
}
