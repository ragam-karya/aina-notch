// The realtime voice engine (Settings → Suara → Mesin suara: realtime): OpenAI Realtime over WebRTC.
// Port of v1's conversation.js and its glue in main.js. One engine, three ways in:
//   - push-to-talk: hold the talk keys; the mic streams only while held (no turn detection, a manual commit)
//   - a session: tap the keys or say "Hey aina"; server VAD takes turns and lets the user cut in
//   - while the connection is still opening, the first turn runs on the classic engine
// The WebRTC part is v1's worker.js in a hidden window (made only while realtime is used); it gets a
// 60-second client secret from aina Cloud, which holds the OpenAI key. The realtime model never sees the screen: it calls
// look_at_screen, which runs the classic Claude turn (screenshots, pointing, drawing, aina's voice).
// With a voice of aina's own, the model answers in text and aina speaks it, so only one voice is heard.

use std::sync::mpsc::{Sender, channel};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use aina_llm::prompts::{self, TONE};
use aina_llm::tags::{Tag, duration_ms, parse_tag};
use reqwest::blocking::Client;
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, Manager};

use crate::{AppState, talk, turn, windows};

const TAP_MS: u128 = 350;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const WARM: Duration = Duration::from_secs(2 * 60);
const WORKER: &str = "worker";

#[derive(Clone, Copy, PartialEq, Debug)]
enum Status {
    Closed,
    Connecting,
    Open,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Mode {
    Ptt,
    Session,
}

type After = Box<dyn FnOnce(&AppHandle) + Send>;

struct Rt {
    status: Status,
    mode: Option<Mode>,
    mic_on: bool,
    speaking: bool,
    responding: bool,
    awaiting: bool,
    tools_running: u32,
    user_talking: bool,
    text_voice: bool,
    transcript: String,
    after: Vec<After>,
    speech: Option<turn::Speaker>,
    looked: bool,
    timer_gen: u64, // bumped to cancel the idle and warm timers
    open_waiters: Vec<Sender<Result<(), String>>>,
    worker_ready: bool,
    pending_connect: Option<String>, // a secret waiting for the worker page to load
    // The key press being handled.
    press_in_session: bool,
    rt_turn: bool,
}

static RT: LazyLock<Mutex<Rt>> = LazyLock::new(|| {
    Mutex::new(Rt {
        status: Status::Closed,
        mode: None,
        mic_on: false,
        speaking: false,
        responding: false,
        awaiting: false,
        tools_running: 0,
        user_talking: false,
        text_voice: false,
        transcript: String::new(),
        after: Vec::new(),
        speech: None,
        looked: false,
        timer_gen: 0,
        open_waiters: Vec::new(),
        worker_ready: false,
        pending_connect: None,
        press_in_session: false,
        rt_turn: false,
    })
});

fn state(app: &AppHandle) -> tauri::State<'_, AppState> {
    app.state::<AppState>()
}

fn rt() -> std::sync::MutexGuard<'static, Rt> {
    RT.lock().unwrap_or_else(|e| e.into_inner())
}

/// Realtime is chosen in Settings and aina Cloud is signed in (it opens the sessions).
pub fn enabled(app: &AppHandle) -> bool {
    state(app).store.get("voiceEngine").as_str() == Some("realtime") && crate::cloud::signed_in(app) && std::env::var_os("AINA_DEMO").is_none()
}

pub fn mic_on() -> bool {
    rt().mic_on
}

fn in_session() -> bool {
    rt().mode == Some(Mode::Session)
}

fn is_open() -> bool {
    rt().status == Status::Open
}

// ---------- the worker window ----------

fn send_worker<S: serde::Serialize + Clone>(app: &AppHandle, ch: &str, payload: S) {
    let _ = app.emit_to(WORKER, ch, payload);
}

fn ensure_worker(app: &AppHandle) {
    if app.get_webview_window(WORKER).is_some() {
        return;
    }
    rt().worker_ready = false;
    let a = app.clone();
    let _ = app.run_on_main_thread(move || {
        let built = windows::builder(&a, WORKER, windows::page("worker.html"))
            .title("aina")
            .visible(false)
            .skip_taskbar(true)
            .focused(false)
            .inner_size(200.0, 100.0)
            // The worker streams the microphone to OpenAI during a realtime turn; nothing else is allowed.
            .on_permission_request(|_, kind| {
                if matches!(kind, tauri::webview::PermissionKind::Microphone) { tauri::webview::PermissionResponse::Allow } else { tauri::webview::PermissionResponse::Deny }
            })
            .initialization_script(windows::init_script(""))
            .build();
        if let Err(e) = built {
            eprintln!("[realtime] worker window: {e}");
        }
    });
}

/// The worker page registered its listeners (aina_ready): a waiting connection goes out now.
pub fn worker_ready(app: &AppHandle) {
    let secret = {
        let mut r = rt();
        r.worker_ready = true;
        r.pending_connect.take()
    };
    if let Some(secret) = secret {
        send_worker(app, "rt:connect", json!({ "secret": secret }));
    }
}

fn close_worker(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(WORKER) {
        let _ = w.destroy();
    }
    rt().worker_ready = false;
}

// ---------- connection ----------

fn turn_detection(level: &str) -> Value {
    json!({
        "type": "server_vad",
        "threshold": if level == "low" { 0.8 } else { 0.6 },
        "prefix_padding_ms": 300,
        "silence_duration_ms": 700,
        "create_response": true,
        "interrupt_response": level != "off",
    })
}

fn tools() -> Value {
    json!([
        { "type": "function", "name": "look_at_screen", "description": "Answer anything about the user's screen with aina's vision model, which also points, highlights, guides clicks and draws on screen. It speaks the answer to the user itself, so after it returns you say nothing more. Call it for anything about what is on screen: what something is, where a button is, how to do something in an app, reading or explaining what they see, or something they marked. Before calling, say only the short filler \"Bentar, aku lihat dulu ya.\" / \"One sec, let me check.\" in the user's language.",
          "parameters": { "type": "object", "properties": { "question": { "type": "string", "description": "The user's request, self-contained, in their words and language, with any details from the conversation." } }, "required": ["question"] } },
        { "type": "function", "name": "do_on_computer", "description": "Do something for the user on their computer: open apps or links, click, type, play music, change a setting, browse or research and save a report. Use it when they ask you to do it, not when they ask how to do it themselves.",
          "parameters": { "type": "object", "properties": { "task": { "type": "string", "description": "Self-contained instruction with every detail they gave." } }, "required": ["task"] } },
        { "type": "function", "name": "delegate_task", "description": "Spawn a Sprite (a specialist helper of yours) for a long job (research, writing a file, code work, a recurring task such as \"every morning…\") that keeps working in the background while they do other things. Call it once per separate job; several Sprites can work at once. Say \"Sprite\", never \"agent\".",
          "parameters": { "type": "object", "properties": {
            "sprite": { "type": "string", "enum": ["ciro", "mino", "routine", "filo", "nilo", "daro"], "description": "ciro: coding, debugging, testing. mino: research, documentation, finding information. routine: reminders, scheduling, recurring jobs. filo: files and documents. nilo: email and communication. daro: data and analysis." },
            "task": { "type": "string", "description": "Self-contained instruction." } }, "required": ["sprite", "task"] } },
        { "type": "function", "name": "set_focus", "description": "The user wants you to focus on or work in a folder for now (\"fokus di folder X\", \"kerjain di folder Development dulu\"), or to stop (\"selesai fokus\"). aina finds the folder; tell them the result it returns.",
          "parameters": { "type": "object", "properties": { "folder": { "type": "string", "description": "The folder as they said it, with the folder above it if named (Development/weather-invitation), a full path, or \"off\"." } }, "required": ["folder"] } },
        { "type": "function", "name": "request_drop", "description": "Open a drop zone in the notch at the top of the screen. Use it when they want you to focus on a folder but point at it instead of naming it (\"fokus di folder ini\"), or want to upload files to Google Drive without naming them (\"upload file ini ke Drive\").",
          "parameters": { "type": "object", "properties": { "purpose": { "type": "string", "enum": ["focus", "upload"] } }, "required": ["purpose"] } },
        { "type": "function", "name": "set_timer", "description": "Start a countdown shown in the notch (\"timer 10 menit\"), a pomodoro, or stop the timer.",
          "parameters": { "type": "object", "properties": { "duration": { "type": "string", "description": "Like 10m, 90s or 1h30m." }, "label": { "type": "string", "description": "Optional short label, in their words." }, "pomodoro": { "type": "boolean", "description": "True for a pomodoro (25 minutes focus, 5 minutes break)." }, "off": { "type": "boolean", "description": "True to stop the timer." } } } },
        { "type": "function", "name": "set_reminder", "description": "Remind them of something at a local date and time (\"ingetin aku jam 3 meeting sama Sari\"), worked out from the local time and date you were given.",
          "parameters": { "type": "object", "properties": { "at": { "type": "string", "description": "Local time as YYYY-MM-DDTHH:MM." }, "text": { "type": "string", "description": "What to remind them of, short." } }, "required": ["at", "text"] } },
        { "type": "function", "name": "media_control", "description": "Play or pause, skip, or go back in the music or video that is playing.",
          "parameters": { "type": "object", "properties": { "action": { "type": "string", "enum": ["play_pause", "next", "previous"] } }, "required": ["action"] } },
        { "type": "function", "name": "search_knowledge", "description": "Look something up in the user's knowledge base (their own files dropped on the notch: documents, notes, PDFs). Use it for questions about their own documents, policies, projects or notes. It returns matching passages with file names; answer from them in a sentence or two.",
          "parameters": { "type": "object", "properties": { "query": { "type": "string", "description": "What to look for, with the key words from their question." } }, "required": ["query"] } },
        { "type": "function", "name": "end_conversation", "description": "The user is done talking for now (\"udah dulu\", \"makasih ya\", \"bye\"). Say a short warm goodbye first, then call this.",
          "parameters": { "type": "object", "properties": {} } }
    ])
}

fn instructions(app: &AppHandle) -> String {
    let st = state(app);
    let name = st.store.get("userName").as_str().unwrap_or("").trim().to_string();
    let focus = crate::agents::current_focus(app).map(|f| f.display().to_string());
    let mut lines = vec![
        "You are aina, the user's assistant on their Windows PC. You are talking out loud with them, in real time.".to_string(),
        String::new(),
        "How you talk:".into(),
    ];
    lines.extend(TONE.iter().map(|l| format!("- {l}")));
    lines.extend(
        [
            "- Start with the answer; no openers like \"Tentu!\", \"Baik,\" or \"Sure!\".",
            "- Be brief: every word is spoken and makes them wait. One short sentence by default (about 20 words at most), two only when needed, longer only when they ask for detail. No extra tips or alternatives unless asked.",
            "- Fit their tone: casual gets casual, careful gets a bit neater, frustrated gets one calm beat first. Light humor is fine when they are playful. No lists, no markdown.",
            "- In Indonesian use everyday casual Indonesian with \"aku\" and \"kamu\", never \"saya\" or \"Anda\".",
            "- Never name keyboard keys or shortcuts.",
            "- If they interrupt you, stop and follow what they say now.",
            "",
            "Tools:",
            "- You cannot see the screen yourself. For anything about the screen, call look_at_screen after the short filler \"Bentar, aku lihat dulu ya.\" / \"One sec, let me check.\". It speaks the answer to the user itself; when it returns, stay silent until they talk again.",
            "- Doing something on their computer: do_on_computer. Long or scheduled work, and code work (git diff, code review, bugs, security, fixes): delegate_task with the Sprite that fits. In both cases say in one short sentence that you are on it (for a Sprite, by its name).",
            "- Focusing on a folder (\"fokus di folder X\", \"selesai fokus\"): set_focus. When they point at a folder instead of naming it (\"fokus di folder ini\"), or want to upload files to Google Drive without naming them: request_drop, and tell them to drop it on the notch at the top of the screen.",
            "- Timers and pomodoros: set_timer. Reminders at a time: set_reminder. Pausing or skipping music: media_control. Confirm in a few words.",
            "- When they say goodbye or that they are done, say a short, casual goodbye (\"Oke, sampai nanti.\") and call end_conversation.",
            "- Answer general questions (facts, ideas, chit-chat) yourself without tools.",
        ]
        .map(String::from),
    );
    if !name.is_empty() {
        lines.push(format!("\nThe user likes to be called {name}. Use it rarely."));
    }
    lines.push(format!("\n{}", prompts::moment(turn::since_last_turn(app))));
    if let Some(f) = focus {
        lines.push(format!("\nFocus folder: {f}. The user asked you to work there for now."));
    }
    let brief = crate::focus::brief(app);
    if !brief.is_empty() {
        lines.push(format!("\n{brief}"));
    }
    let kb = crate::knowledge::summary_line(app);
    if !kb.is_empty() {
        lines.push(format!("\n{kb} Use search_knowledge to read from it."));
    }
    let lang = prompts::language_instruction(st.store.get("language").as_str().unwrap_or("auto"));
    if !lang.is_empty() {
        lines.push(format!("\n{lang}"));
    }
    lines.join("\n")
}

// A 60-second client secret for the worker, minted by aina Cloud (it checks the plan and the limits).
fn mint_secret(app: &AppHandle, text_voice: bool) -> Result<String, String> {
    let st = state(app);
    let token = st.store.secret("openaiApiKey").ok_or("Masuk ke aina Cloud dulu (Settings → Akun).")?;
    let get = |k: &str, d: &str| st.store.get(k).as_str().filter(|s| !s.is_empty()).unwrap_or(d).to_string();
    let mut session = json!({
        "type": "realtime",
        "model": get("realtimeModel", "gpt-realtime"),
        "instructions": instructions(app),
        "output_modalities": if text_voice { json!(["text"]) } else { json!(["audio"]) },
        "max_output_tokens": st.store.get("realtimeMaxTokens").as_u64().unwrap_or(800),
        "audio": { "input": { "noise_reduction": { "type": "near_field" }, "turn_detection": null, "transcription": null } },
        "tools": tools(),
        "tool_choice": "auto",
    });
    if !text_voice {
        session["audio"]["output"] = json!({ "voice": get("realtimeVoice", "marin") });
    }
    if let Some(effort) = st.store.get("realtimeReasoning").as_str().filter(|s| !s.is_empty()) {
        session["reasoning"] = json!({ "effort": effort });
    }
    let res = Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?
        .post(format!("{}/v1/realtime/client_secrets", crate::cloud::base(app)))
        .header("x-api-key", token)
        .json(&json!({ "session": session }))
        .send()
        .map_err(|_| "aina Cloud belum bisa dihubungi. Cek koneksi internetmu.".to_string())?;
    let status = res.status();
    let body: Value = res.json().unwrap_or(Value::Null);
    if !status.is_success() {
        return Err(body["error"]["message"].as_str().map(String::from).unwrap_or_else(|| format!("Realtime {status}")));
    }
    body["value"].as_str().map(String::from).ok_or_else(|| "Realtime: no client secret".into())
}

/// Opens the connection (blocking; run off the main thread). Ok once the data channel is open.
pub fn connect(app: &AppHandle) -> Result<(), String> {
    let (tx, rx) = channel();
    let first = {
        let mut r = rt();
        if r.status == Status::Open {
            return Ok(());
        }
        r.open_waiters.push(tx);
        let first = r.status == Status::Closed;
        if first {
            r.status = Status::Connecting;
            // One voice everywhere: with a voice of aina's own, the model answers in text.
            r.text_voice = talk::voice(app).is_some();
        }
        first
    };
    if first {
        let text_voice = rt().text_voice;
        match mint_secret(app, text_voice) {
            Ok(secret) => {
                ensure_worker(app);
                let ready = rt().worker_ready;
                if ready {
                    send_worker(app, "rt:connect", json!({ "secret": secret }));
                } else {
                    rt().pending_connect = Some(secret);
                }
            }
            Err(e) => fail_waiters(app, e),
        }
    }
    let r = rx.recv_timeout(CONNECT_TIMEOUT).unwrap_or_else(|_| Err("Realtime: koneksi terlalu lama.".into()));
    if r.is_err() && rt().status == Status::Connecting {
        fail_waiters(app, "Realtime: koneksi terlalu lama.".into());
    }
    r
}

fn fail_waiters(app: &AppHandle, e: String) {
    let waiters = {
        let mut r = rt();
        r.status = Status::Closed;
        r.pending_connect = None;
        std::mem::take(&mut r.open_waiters)
    };
    for w in waiters {
        let _ = w.send(Err(e.clone()));
    }
    send_worker(app, "rt:close", Value::Null);
}

/// From the worker: { state: "open" | "closed" | "error", detail }.
pub fn on_state(app: &AppHandle, s: &Value) {
    match s["state"].as_str() {
        Some("open") => {
            let waiters = {
                let mut r = rt();
                r.status = Status::Open;
                std::mem::take(&mut r.open_waiters)
            };
            let history = turn::history_text(app);
            if !history.is_empty() {
                send(app, json!({ "type": "conversation.item.create", "item": { "type": "message", "role": "system", "content": [{ "type": "input_text", "text": history }] } }));
            }
            for w in waiters {
                let _ = w.send(Ok(()));
            }
        }
        _ => {
            let detail = s["detail"].as_str().unwrap_or("Realtime: koneksi gagal.").to_string();
            let (was_open, was_session) = {
                let r = rt();
                (r.status == Status::Open, r.mode == Some(Mode::Session))
            };
            if !was_open {
                return fail_waiters(app, detail);
            }
            reset();
            if was_session {
                ended(app, "disconnected");
            }
        }
    }
}

fn send(app: &AppHandle, event: Value) {
    send_worker(app, "rt:send", event);
}

fn set_mic(app: &AppHandle, on: bool) {
    rt().mic_on = on;
    send_worker(app, "rt:mic", on);
}

fn set_turn_detection(app: &AppHandle, vad: bool) {
    let level = state(app).store.get("bargeIn").as_str().unwrap_or("low").to_string();
    send(app, json!({ "type": "session.update", "session": { "type": "realtime", "audio": { "input": { "turn_detection": if vad { turn_detection(&level) } else { Value::Null } } } } }));
}

/// Stops aina mid-sentence (barge-in by key, or Esc).
pub fn cancel_speech(app: &AppHandle) {
    if !is_open() {
        return;
    }
    send(app, json!({ "type": "response.cancel" }));
    send(app, json!({ "type": "output_audio_buffer.clear" }));
    rt().after.clear();
    turn::cancel(app);
}

/// Esc: stop talking and close any open turn or session.
pub fn stop(app: &AppHandle) {
    if rt().status == Status::Closed {
        return;
    }
    cancel_speech(app);
    if in_session() {
        return end_session(app, "esc");
    }
    if rt().mode == Some(Mode::Ptt) {
        rt().mode = None;
        if is_open() {
            set_mic(app, false);
        }
        arm_warm(app);
    }
}

fn begin_ptt_turn(app: &AppHandle) {
    rt().timer_gen += 1;
    cancel_speech(app);
    rt().mode = Some(Mode::Ptt);
    set_turn_detection(app, false);
    send(app, json!({ "type": "input_audio_buffer.clear" }));
    set_mic(app, true);
}

fn commit_ptt_turn(app: &AppHandle) {
    set_mic(app, false);
    send(app, json!({ "type": "input_audio_buffer.commit" }));
    request_response(app);
    turn::set_phase(app, "thinking");
}

fn start_session(app: &AppHandle) -> Result<(), String> {
    connect(app)?;
    {
        let mut r = rt();
        r.timer_gen += 1;
        r.mode = Some(Mode::Session);
    }
    set_turn_detection(app, true);
    send(app, json!({ "type": "input_audio_buffer.clear" }));
    set_mic(app, true);
    turn::set_phase(app, "conversing");
    arm_idle(app);
    Ok(())
}

fn end_session(app: &AppHandle, reason: &str) {
    {
        let mut r = rt();
        r.timer_gen += 1;
        if r.mode != Some(Mode::Session) {
            return;
        }
        r.mode = None;
    }
    if is_open() {
        set_mic(app, false);
        set_turn_detection(app, false);
    }
    ended(app, reason);
    arm_warm(app);
}

// The session is over: the notch goes back to rest, unless a walkthrough or a task carries on.
fn ended(app: &AppHandle, _reason: &str) {
    let phase = turn::phase(app);
    if phase != "idle" && phase != "acting" && phase != "guiding" {
        windows::broadcast(app, "linger", 6000);
        turn::set_phase(app, "idle");
    }
}

// Ends a quiet session after realtimeIdleSec without speech.
fn arm_idle(app: &AppHandle) {
    let ticket = {
        let mut r = rt();
        r.timer_gen += 1;
        r.timer_gen
    };
    if !in_session() {
        return;
    }
    let sec = state(app).store.get("realtimeIdleSec").as_u64().unwrap_or(30).max(5);
    let a = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(sec));
        let (same, speaking) = {
            let r = rt();
            (r.timer_gen == ticket, r.speaking)
        };
        if same && !speaking {
            end_session(&a, "idle");
        }
    });
}

// Closes the connection (and the mic) a while after the last use.
fn arm_warm(app: &AppHandle) {
    let ticket = {
        let mut r = rt();
        r.timer_gen += 1;
        r.timer_gen
    };
    let a = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(WARM);
        if rt().timer_gen == ticket {
            close(&a);
        }
    });
}

pub fn close(app: &AppHandle) {
    let was = rt().status;
    if was != Status::Closed {
        send_worker(app, "rt:close", Value::Null);
    }
    reset();
    close_worker(app);
}

fn reset() {
    let mut r = rt();
    r.timer_gen += 1;
    r.status = Status::Closed;
    r.mode = None;
    r.mic_on = false;
    r.speaking = false;
    r.after.clear();
    r.responding = false;
    r.awaiting = false;
    r.tools_running = 0;
    r.user_talking = false;
    r.speech = None;
}

fn request_response(app: &AppHandle) {
    rt().awaiting = true;
    send(app, json!({ "type": "response.create" }));
}

// ---------- events from the model ----------

pub fn on_event(app: &AppHandle, raw: &Value) {
    let e: Value = match raw {
        Value::String(s) => serde_json::from_str(s).unwrap_or(Value::Null),
        v => v.clone(),
    };
    match e["type"].as_str().unwrap_or("") {
        "input_audio_buffer.speech_started" => {
            {
                let mut r = rt();
                r.timer_gen += 1; // the idle timer
                r.user_talking = true;
                r.after.clear();
            }
            // Cutting in also stops an answer spoken by the classic playback (look_at_screen).
            turn::cancel(app);
            turn::end_guide(app);
            windows::broadcast(app, "clear", Value::Null);
            turn::set_phase(app, "listening");
        }
        "input_audio_buffer.speech_stopped" => {
            {
                let mut r = rt();
                r.user_talking = false;
                if r.mode == Some(Mode::Session) {
                    r.awaiting = true;
                }
            }
            turn::set_phase(app, "thinking");
            arm_idle(app);
        }
        "response.created" => {
            let mut r = rt();
            r.transcript.clear();
            r.responding = true;
            r.awaiting = false;
        }
        "output_audio_buffer.started" => {
            rt().speaking = true;
            windows::broadcast(app, "voice", true);
            turn::set_phase(app, "speaking");
        }
        // Text mode: aina speaks the reply with its own voice as it streams in.
        "response.output_text.delta" => {
            let delta = e["delta"].as_str().unwrap_or("").to_string();
            let mut r = rt();
            r.transcript.push_str(&delta);
            if r.speech.is_none() {
                r.speaking = true;
                r.speech = Some(turn::Speaker::new(app, Vec::new(), turn::generation()));
            }
            if let Some(s) = r.speech.as_mut() {
                s.push(&delta);
            }
        }
        "response.output_text.done" => text_done(app),
        "response.output_audio_transcript.delta" => {
            let text = {
                let mut r = rt();
                r.transcript.push_str(e["delta"].as_str().unwrap_or(""));
                r.transcript.clone()
            };
            windows::broadcast(app, "text", json!({ "text": text, "ms": text.chars().count() * 62, "extend": true }));
        }
        "output_audio_buffer.stopped" | "output_audio_buffer.cleared" => {
            let was = std::mem::take(&mut rt().speaking);
            if was {
                windows::broadcast(app, "voice", false);
            }
            finish_if_quiet(app);
        }
        "response.done" => {
            {
                rt().responding = false;
            }
            if rt().text_voice {
                text_done(app); // also closes a reply that was cut off
            }
            let response = e["response"].clone();
            let a = app.clone();
            std::thread::spawn(move || {
                on_response_done(&a, &response);
                finish_if_quiet(&a);
            });
        }
        "error" => {
            let msg = e["error"]["message"].as_str().unwrap_or("Realtime error").to_string();
            // Cancelling when nothing is playing is harmless; everything else is worth showing.
            if !msg.to_lowercase().contains("cancel") && !msg.to_lowercase().contains("no active response") {
                windows::broadcast(app, "error", msg);
            }
        }
        _ => {}
    }
}

// The text reply is complete: the rest is spoken, and the turn ends once aina is quiet.
fn text_done(app: &AppHandle) {
    let Some(speech) = rt().speech.take() else { return };
    let a = app.clone();
    std::thread::spawn(move || {
        speech.finish();
        rt().speaking = false;
        windows::broadcast(&a, "voice", false);
        finish_if_quiet(&a);
    });
}

// A turn is over only when aina is silent, no response is coming and no tool is still working: a
// reply with a tool call speaks twice (a filler, then the answer).
fn finish_if_quiet(app: &AppHandle) {
    let (after, mode) = {
        let mut r = rt();
        if r.speaking || r.responding || r.awaiting || r.tools_running > 0 || r.user_talking || r.speech.is_some() {
            return;
        }
        (std::mem::take(&mut r.after), r.mode)
    };
    for f in after {
        f(app);
    }
    // A walkthrough started by the answer keeps its own phase, so clicks advance it.
    if turn::phase(app) == "guiding" {
        return;
    }
    match mode {
        Some(Mode::Session) => {
            turn::set_phase(app, "conversing");
            arm_idle(app);
        }
        Some(Mode::Ptt) => {
            rt().mode = None;
            if turn::phase(app) != "acting" {
                windows::broadcast(app, "linger", 9000);
                turn::set_phase(app, "idle");
            }
            arm_warm(app);
        }
        None => {}
    }
}

fn output(app: &AppHandle, call_id: &str, out: String) {
    send(app, json!({ "type": "conversation.item.create", "item": { "type": "function_call_output", "call_id": call_id, "output": out } }));
}

// The audio goes straight to OpenAI, so aina Cloud bills each reply from the usage it carried.
fn report_usage(app: &AppHandle, usage: &Value) {
    let Some(token) = state(app).store.secret("openaiApiKey") else { return };
    let model = state(app).store.get("realtimeModel").as_str().filter(|s| !s.is_empty()).unwrap_or("gpt-realtime").to_string();
    let (url, body) = (format!("{}/v1/realtime/usage", crate::cloud::base(app)), json!({ "model": model, "usage": usage }));
    std::thread::spawn(move || {
        if let Ok(c) = Client::builder().timeout(Duration::from_secs(15)).build() {
            let _ = c.post(url).header("x-api-key", token).json(&body).send();
        }
    });
}

fn on_response_done(app: &AppHandle, response: &Value) {
    if response["usage"].is_object() {
        report_usage(app, &response["usage"]);
    }
    let outputs = response["output"].as_array().cloned().unwrap_or_default();
    let text: String = outputs.iter().flat_map(|o| o["content"].as_array().cloned().unwrap_or_default()).map(|c| c["transcript"].as_str().or(c["text"].as_str()).unwrap_or("").to_string()).collect::<Vec<_>>().join(" ").trim().to_string();
    if !text.is_empty() {
        let looked = std::mem::take(&mut rt().looked);
        if !looked {
            turn::remember_turn(app, "(spoken question)", &text);
        }
    }
    let calls: Vec<Value> = outputs.into_iter().filter(|o| o["type"] == "function_call").collect();
    if calls.is_empty() {
        return;
    }
    rt().tools_running += 1;
    let (mut ending, mut spoken) = (false, false);
    for call in &calls {
        let args: Value = serde_json::from_str(call["arguments"].as_str().unwrap_or("{}")).unwrap_or(json!({}));
        let id = call["call_id"].as_str().unwrap_or("").to_string();
        let name = call["name"].as_str().unwrap_or("");
        if name == "end_conversation" {
            ending = true;
            output(app, &id, "ok".into());
            continue;
        }
        let out = match run_tool(app, name, &args) {
            Ok(ToolOut::Spoken) => {
                spoken = true;
                json!({ "status": "answered", "note": "The answer was already spoken to the user, with pointers on screen. Do not repeat, rephrase or add to it; stay silent until they speak." })
            }
            Ok(ToolOut::Tell(t)) => json!({ "answer": t, "note": "Tell the user this in one short sentence, in your own voice." }),
            Ok(ToolOut::Started(after)) => {
                if let Some(f) = after {
                    rt().after.push(f);
                }
                json!({ "status": "started", "note": "Not finished yet; it is being done now. Say only that you are on it (\"Oke, aku kerjain.\" / \"On it.\"), never that it is done." })
            }
            Err(e) => json!({ "error": e }),
        };
        if !is_open() {
            rt().tools_running = 0;
            return;
        }
        output(app, &id, out.to_string());
    }
    rt().tools_running -= 1;
    // Goodbye: let it finish speaking, then close the session.
    if ending {
        rt().after.push(Box::new(|a: &AppHandle| end_session(a, "goodbye")));
    } else if !spoken {
        request_response(app);
    }
}

enum ToolOut {
    Spoken,
    Tell(String),
    Started(Option<After>),
}

fn run_tool(app: &AppHandle, name: &str, args: &Value) -> Result<ToolOut, String> {
    let s = |k: &str| args[k].as_str().unwrap_or("").to_string();
    match name {
        "look_at_screen" => {
            look_at_screen(app, &s("question"));
            Ok(ToolOut::Spoken)
        }
        "do_on_computer" => {
            let task = s("task");
            Ok(ToolOut::Started(Some(Box::new(move |a: &AppHandle| {
                end_session(a, "brain");
                crate::brain::start(a, &task);
            }))))
        }
        "search_knowledge" => Ok(ToolOut::Tell(crate::knowledge::context_for(app, &s("query")).unwrap_or_else(|| "Nothing in the knowledge base matches that.".into()))),
        "delegate_task" => {
            let sprite = s("sprite");
            crate::agents::delegate(app, &if sprite.is_empty() { s("task") } else { format!("{sprite}|{}", s("task")) });
            Ok(ToolOut::Started(None))
        }
        "set_focus" => {
            let folder = s("folder");
            let off = folder.is_empty() || matches!(folder.to_lowercase().as_str(), "off" | "none");
            Ok(ToolOut::Tell(crate::agents::focus_folder(app, (!off).then_some(folder.as_str()))))
        }
        "request_drop" => {
            let purpose = if s("purpose") == "upload" { "upload" } else { "focus" };
            crate::notch::request_drop(app, purpose);
            Ok(ToolOut::Tell(if purpose == "focus" {
                "A drop zone is open in the notch at the top of the screen: they can drop the folder there, or say its name so aina finds it.".into()
            } else {
                "A drop zone is open in the notch at the top of the screen: they can drop the files there to upload them to Google Drive.".into()
            }))
        }
        "set_timer" => {
            let label = s("label");
            let st = state(app);
            let mut timers = st.timers.lock().unwrap();
            let tell = if args["off"] == true {
                timers.stop_timer();
                "The timer is off.".to_string()
            } else if args["pomodoro"] == true {
                timers.start_pomodoro(crate::notch::now_ms(), &label);
                let c = &timers.config;
                format!("A pomodoro is running: {} minutes of focus, then a {}-minute break.", c.focus_min, c.short_min)
            } else {
                let ms = duration_ms(&s("duration"));
                if ms == 0 {
                    return Err(format!("\"{}\" is not a duration like 10m, 90s or 1h30m.", s("duration")));
                }
                timers.set_timer(crate::notch::now_ms(), ms, &label);
                format!("A {}-minute timer is running in the notch.", (ms / 60_000).max(1))
            };
            drop(timers);
            crate::notch::send_status(app);
            Ok(ToolOut::Tell(tell))
        }
        "set_reminder" => {
            let Some(Tag::Remind { at, text }) = parse_tag(&format!("[REMIND:{}:{}]", s("at"), s("text"))) else {
                return Err("Give the time as YYYY-MM-DDTHH:MM in local time, and a short text.".into());
            };
            state(app).timers.lock().unwrap().add_reminder(at, &text);
            crate::reminders::changed(app);
            let when = chrono::DateTime::from_timestamp_millis(at).map(|d| d.with_timezone(&chrono::Local).format("%A %H:%M").to_string()).unwrap_or_default();
            Ok(ToolOut::Tell(format!("Reminder set for {when}: {text}.")))
        }
        "media_control" => {
            crate::apps::media_control(app, &s("action"));
            let now = aina_apps::media::LAST.lock().unwrap().clone();
            Ok(ToolOut::Tell(match now["title"].as_str() {
                Some(t) => format!("Done. Now: {t} ({}).", if now["playing"] == true { "playing" } else { "paused" }),
                None => "Done.".into(),
            }))
        }
        other => Err(format!("Unknown tool {other}")),
    }
}

// look_at_screen: the question goes to Claude with the screen, and the answer is spoken sentence by
// sentence with its pointers and drawings (the classic playback), instead of the realtime voice
// repeating it. A walkthrough or a task it starts carries on after the session's turn.
fn look_at_screen(app: &AppHandle, question: &str) {
    rt().looked = true;
    crate::i18n::note_spoken(app, question);
    // Let the realtime filler ("Bentar, aku lihat dulu ya.") finish first.
    for _ in 0..50 {
        if !rt().speaking {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let shots = match talk::capture_now(app) {
        Ok(s) => s,
        Err(e) => return windows::broadcast(app, "error", e),
    };
    let ticket = turn::generation();
    turn::begin_turn(app, ticket, false, false);
    turn::run_companion(app, question, shots, ticket);
    // "Kerjakan untukku" took over: the session ends so the task has the screen.
    if crate::brain::running(app) {
        end_session(app, "brain");
    }
}

// ---------- the talk keys and "Hey aina" ----------

/// Talk keys pressed. Holding is push-to-talk; a short tap opens (or closes) a session on key-up.
pub fn talk_down(app: &AppHandle) {
    if !enabled(app) || crate::notch::pending(app) {
        return talk::on_talk_down(app);
    }
    {
        let mut r = rt();
        r.press_in_session = false;
        r.rt_turn = false;
    }
    if in_session() {
        // Mid-session the keys cut aina off; a tap ends the session on key-up.
        rt().press_in_session = true;
        cancel_speech(app);
        return;
    }
    let open = is_open();
    rt().rt_turn = open;
    talk::on_talk_down(app); // records locally too: the fallback if realtime isn't ready
    if open {
        begin_ptt_turn(app);
    } else {
        let a = app.clone();
        std::thread::spawn(move || match connect(&a) {
            Ok(()) if rt().mode.is_none() => arm_warm(&a),
            Ok(()) => {}
            Err(e) => eprintln!("[realtime] {e}"),
        });
    }
}

pub fn talk_up(app: &AppHandle) {
    let held = talk::held_ms(app);
    if !enabled(app) {
        return talk::on_talk_up(app);
    }
    let (press_in_session, rt_turn) = {
        let r = rt();
        (r.press_in_session, r.rt_turn)
    };
    if press_in_session {
        if held < TAP_MS {
            end_session(app, "user");
        }
        return;
    }
    if held < TAP_MS {
        if rt().mode == Some(Mode::Ptt) {
            stop(app);
        }
        talk::discard(app);
        return start_conversation(app, "tap");
    }
    if rt_turn && is_open() && rt().mode == Some(Mode::Ptt) {
        talk::discard(app);
        return commit_ptt_turn(app);
    }
    talk::on_talk_up(app);
}

/// A session, opened by a tap or "Hey aina". Opening a connection takes several seconds, so when it
/// isn't open yet the first turn runs hands-free on the classic engine while it opens in the
/// background; the session then carries on in realtime.
pub fn start_conversation(app: &AppHandle, kind: &str) {
    crate::brain::stop(app);
    turn::cancel(app);
    turn::end_guide(app);
    windows::broadcast(app, "clear", Value::Null);
    turn::set_phase(app, "listening");
    let token = turn::generation();
    let en = crate::i18n::lang(app) == "en";
    let hour = chrono::Timelike::hour(&chrono::Local::now());
    let line = aina_llm::persona::wake_ack(en, hour, turn::since_last_turn(app), state(app).store.get("userName").as_str().unwrap_or(""), aina_llm::persona::rand());
    let open = is_open();
    let (a, kind) = (app.clone(), kind.to_string());
    std::thread::spawn(move || {
        if talk::voice(&a).is_some() {
            talk::say(&a, &line);
        } else {
            turn::chime(&a, "start");
        }
        if turn::generation() != token {
            return;
        }
        if open {
            match start_session(&a) {
                Ok(()) => return,
                Err(e) => eprintln!("[realtime] {e}"),
            }
        }
        // The first turn on the classic engine, hands-free; then the session continues in realtime.
        if std::env::var_os("AINA_WAKE_DEBUG").is_some() {
            eprintln!("[talk] first turn on the classic engine while realtime connects ({kind})");
        }
        let connecting = {
            let a = a.clone();
            std::thread::spawn(move || connect(&a).is_ok())
        };
        crate::wake::hands_free(&a);
        let connected = connecting.join().unwrap_or(false);
        // Wait for that first turn to be answered, then go on in realtime.
        for _ in 0..1200 {
            if turn::phase(&a) == "idle" && !talk::listening(&a) && !crate::wake::waiting() {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        if connected && turn::phase(&a) == "idle" && enabled(&a) {
            let _ = start_session(&a);
        }
    });
}
