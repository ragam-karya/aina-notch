// "Hey aina" (off by default): v1's offline helper AinaWake.exe (Windows' own speech recognizer, a
// grammar that only knows the wake phrase; audio never leaves the PC) runs while the setting is on.
// It is paused unless aina is idle and quiet, and while a video or song plays, since speech in videos
// woke aina by mistake. On "Hey aina", aina answers briefly and listens hands-free: the recording ends
// by itself once the user pauses (aina-voice vad). Port of v1's wake.js, updateWake and
// watchMediaForWake.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant};

use aina_voice::vad::AutoStop;
use tauri::{AppHandle, Manager};

use crate::{AppState, talk, turn, windows};

const MAX_RESTARTS: u32 = 3;

static ENABLED: AtomicBool = AtomicBool::new(false);
static PAUSED: AtomicBool = AtomicBool::new(false);
static RESTARTS: AtomicU32 = AtomicU32::new(0);
static CHILD: Mutex<Option<(Child, ChildStdin)>> = Mutex::new(None);
static WATCHING: AtomicBool = AtomicBool::new(false);
// The hands-free recording after a wake, ended by the voice activity check.
static AUTO: Mutex<Option<(AutoStop, Instant)>> = Mutex::new(None);

fn state(app: &AppHandle) -> tauri::State<'_, AppState> {
    app.state::<AppState>()
}

fn exe(app: &AppHandle) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(r) = app.path().resource_dir() {
        candidates.push(r.join("wake").join("AinaWake.exe"));
    }
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../native/wake/AinaWake.exe"));
    candidates.into_iter().find(|p| p.is_file())
}

/// Turns the listener on or off to match the setting (at start and when Settings changes it).
pub fn update(app: &AppHandle) {
    let on = state(app).store.get("wakeWord").as_bool() == Some(true);
    if on == ENABLED.swap(on, Ordering::SeqCst) {
        return;
    }
    RESTARTS.store(0, Ordering::SeqCst);
    if on {
        spawn(app);
        watch(app);
    } else {
        stop();
    }
}

/// Stops the helper (setting off, or quitting).
pub fn stop() {
    if let Some((mut child, mut stdin)) = CHILD.lock().unwrap().take() {
        let _ = stdin.write_all(b"quit\n");
        drop(stdin);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(2));
            let _ = child.kill();
        });
    }
}

fn write(cmd: &str) {
    if let Some((_, stdin)) = CHILD.lock().unwrap().as_mut() {
        let _ = stdin.write_all(format!("{cmd}\n").as_bytes());
        let _ = stdin.flush();
    }
}

fn spawn(app: &AppHandle) {
    if !ENABLED.load(Ordering::SeqCst) || CHILD.lock().unwrap().is_some() {
        return;
    }
    let Some(exe) = exe(app) else {
        return windows::broadcast(app, "error", "Komponen \"Hey aina\" tidak ditemukan.");
    };
    #[cfg(windows)]
    let mut cmd = {
        use std::os::windows::process::CommandExt;
        let mut c = Command::new(exe);
        c.creation_flags(0x0800_0000); // no console window
        c
    };
    #[cfg(not(windows))]
    let mut cmd = Command::new(exe);
    let child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn();
    let Ok(mut child) = child else {
        return windows::broadcast(app, "error", "Pendengar \"Hey aina\" tidak bisa dijalankan.");
    };
    let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else { return };
    let pid = child.id();
    *CHILD.lock().unwrap() = Some((child, stdin));
    PAUSED.store(false, Ordering::SeqCst);
    let a = app.clone();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let line = line.trim();
            if line == "ready" {
                RESTARTS.store(0, Ordering::SeqCst);
                if PAUSED.load(Ordering::SeqCst) {
                    write("pause");
                }
            } else if line.starts_with("wake") && !PAUSED.load(Ordering::SeqCst) {
                on_wake(&a);
            } else if let Some(msg) = line.strip_prefix("error ") {
                windows::broadcast(&a, "error", msg.to_string());
            }
        }
        // The helper ended. While still on (e.g. the mic was unplugged) it is restarted a few times.
        let ours = CHILD.lock().unwrap().as_ref().is_some_and(|(c, _)| c.id() == pid);
        if !ours {
            return;
        }
        *CHILD.lock().unwrap() = None;
        if ENABLED.load(Ordering::SeqCst) {
            if RESTARTS.fetch_add(1, Ordering::SeqCst) < MAX_RESTARTS {
                std::thread::sleep(Duration::from_secs(3));
                spawn(&a);
            } else {
                windows::broadcast(&a, "error", "Pendengar \"Hey aina\" berhenti. Matikan lalu nyalakan lagi di Settings.");
            }
        }
    });
}

// Listens only while aina is idle and nothing is playing, never while it talks or records.
fn watch(app: &AppHandle) {
    if WATCHING.swap(true, Ordering::SeqCst) {
        return;
    }
    let a = app.clone();
    std::thread::spawn(move || {
        let mut media_playing = false;
        let mut last_media = Instant::now() - Duration::from_secs(10);
        while ENABLED.load(Ordering::SeqCst) {
            if last_media.elapsed() > Duration::from_secs(3) {
                last_media = Instant::now();
                media_playing = aina_apps::media::now()["playing"] == true;
            }
            let busy = turn::phase(&a) != "idle" || talk::listening(&a) || waiting() || crate::realtime::mic_on();
            let pause = busy || media_playing;
            if pause != PAUSED.swap(pause, Ordering::SeqCst) {
                write(if pause { "pause" } else { "resume" });
            }
            std::thread::sleep(Duration::from_millis(400));
        }
        WATCHING.store(false, Ordering::SeqCst);
    });
}

// "Hey aina": a realtime session when that engine is on, else a short reply and a hands-free question.
fn on_wake(app: &AppHandle) {
    if turn::phase(app) != "idle" || talk::listening(app) || !crate::cloud::require(app) {
        return;
    }
    PAUSED.store(true, Ordering::SeqCst);
    write("pause");
    crate::notch::pin_display(app);
    if crate::realtime::enabled(app) {
        return crate::realtime::start_conversation(app, "wake");
    }
    let st = state(app);
    let en = crate::i18n::lang(app) == "en";
    let hour = chrono::Timelike::hour(&chrono::Local::now());
    let line = aina_llm::persona::wake_ack(en, hour, turn::since_last_turn(app), st.store.get("userName").as_str().unwrap_or(""), aina_llm::persona::rand());
    talk::say_line(app, &line);
    hands_free(app);
}

/// A question with no keys held: the recording ends by itself once the user pauses.
pub fn hands_free(app: &AppHandle) {
    *AUTO.lock().unwrap() = Some((AutoStop::new(0.0), Instant::now()));
    talk::on_talk_down(app);
}

/// A hands-free recording is still waiting for the user to finish.
pub fn waiting() -> bool {
    AUTO.lock().unwrap().is_some()
}

/// The microphone level (about 30 times a second while recording): ends a hands-free recording once
/// the user pauses.
pub fn on_level(app: &AppHandle, level: f32) {
    let verdict = {
        let mut auto = AUTO.lock().unwrap();
        let Some((vad, t0)) = auto.as_mut() else { return };
        let v = vad.feed(level.max(0.0), t0.elapsed().as_secs_f64() * 1000.0);
        if v.is_some() {
            *auto = None;
        }
        v
    };
    match verdict {
        Some(true) => {
            let a = app.clone();
            std::thread::spawn(move || talk::on_talk_up(&a));
        }
        // Nothing was said after the wake: drop it quietly.
        Some(false) => talk::cancel(app),
        None => {}
    }
}

/// A hold or Esc ends a hands-free recording that is still waiting.
pub fn forget() {
    *AUTO.lock().unwrap() = None;
}
