// BADI_DEMO: scripted runs without Claude or a microphone, to see the v1 UI driven by the v2 core.
//   BADI_DEMO=1 (or draw): one turn through the real StepSplitter and to_global: notch, pointer,
//     handwriting, a shape, spoken with the configured voice (text only when the voice is off).
//   BADI_DEMO=notch: the same walk through the smart notch as v1's runNotchDemo (timer, drop zones,
//     upload, notices, a question with buttons, the hover peek), so the two can be compared.
//   BADI_DEMO_OUT=<folder>: saves the top of the primary overlay as <step>.png at each notch step.
//   BADI_DEMO=sprites: Sprite cards spawning, working, asking, failing and finishing in the panel at the
//     top right (no task runs), a hovered card's detail, and 30 cards to show the columns; BADI_DEMO_OUT
//     saves that corner (and the whole display for the columns) at each step.
//   BADI_DEMO=tasks: the notch's Tasks page with Sprite cards (no task runs): running, needing the
//     person, Stop asked about and confirmed, a failure first, then the history; BADI_DEMO_OUT saves it.
//   BADI_DEMO=fullscreen: the notch over a full-screen app: hidden, called up by resting the cursor at
//     the very top for a second, the peek, then hidden again once the cursor leaves.
//   BADI_DEMO=mini: the compact pill with a song: the line being sung beside Badi's eyes (no cover),
//     then the cover; then a pomodoro over the song.
//   BADI_DEMO=reminders: the reminder page (Agenda, Timeline, Calendar, prayer times) with local
//     reminders and Google events given as if fetched; use a scratch BADI_PROFILE.
//   BADI_DEMO=mascot: the notch's performance scenarios (scripts/perf-mascot.ps1 runs it): idle30,
//     anim10, toggle100, expr, hideshow. BADI_DEMO_MASCOT=<names, comma-separated> picks some,
//     BADI_PERF_SCALE=0.05 shortens every duration for a smoke run. The overlays carry perf_probe.js;
//     its summaries go to BADI_DEMO_OUT/renderer.csv and the scenario times to markers.csv.
// Demos turn capture protection off (windows.rs), as in v1.

use std::path::PathBuf;
use std::thread::sleep;
use std::time::{Duration, Instant};

use badi_llm::tags::{Rect, Shot};
use badi_llm::{StepSplitter, to_global};
use serde_json::{Map, Value, json};
use tauri::{AppHandle, Manager};

use crate::i18n::{self, t};
use crate::notch::{self, Action, now_ms};
use crate::windows::{self, Overlay};
use crate::{AppState, snap};

const REPLY: &str = "[POINT:1180,40:Simpan] Tombol simpannya ada di kanan atas. \
[WRITE:420,300,44:Ctrl + S:red] Atau tekan Ctrl S. \
[SHAPE:circle:640,420;720,420:green] Gampang kan!";

pub fn start(app: AppHandle, kind: String) {
    std::thread::spawn(move || {
        sleep(Duration::from_millis(2500));
        let Some(primary) = primary(&app) else { return };
        match kind.as_str() {
            "notch" => run_notch(&app, &primary),
            "talk" => run_talk(&app, &primary),
            "ask" => run_ask(&app, &primary),
            "brain" => run_brain(&app),
            "agent" => run_agent(&app),
            "sprites" => run_sprites(&app, &primary),
            "tasks" => run_tasks(&app, &primary),
            "reminders" => run_reminders(&app, &primary),
            "fullscreen" => run_fullscreen(&app, &primary),
            "mini" => run_mini(&app, &primary),
            "signin" => run_signin(&app, &primary),
            "credits" => run_credits(&app, &primary),
            "mascot" => run_mascot(&app, &primary),
            _ => run_turn(&app, &primary),
        }
    });
}

fn primary(app: &AppHandle) -> Option<Overlay> {
    let state = app.state::<AppState>();
    let overlays = state.overlays.lock().unwrap();
    overlays.iter().find(|o| o.primary).or(overlays.first()).cloned()
}

fn run_turn(app: &AppHandle, o: &Overlay) {
    let (bx, by, bw, bh) = o.bounds();
    // The "screenshot" the model saw: 1280Ã—720 of the primary display.
    let shots = [Shot { bounds: Rect { x: bx, y: by, width: bw, height: bh }, width: 1280.0, height: 720.0 }];
    let say = |ch: &str, p: Value| windows::broadcast(app, ch, p);

    say("subtitleDisplay", json!({ "x": bx, "y": by }));
    say("mode", json!("companion"));
    say("phase", json!("listening"));
    for i in 0..30 {
        say("level", json!(0.25 + 0.2 * ((i as f64) * 0.7).sin().abs()));
        sleep(Duration::from_millis(60));
    }
    say("transcript", json!("aina, tombol simpannya di mana ya?"));
    say("phase", json!("thinking"));
    sleep(Duration::from_millis(1200));

    // Stream the reply in small chunks, as Claude would.
    let mut splitter = StepSplitter::new();
    let chars: Vec<char> = REPLY.chars().collect();
    let mut steps = Vec::new();
    for piece in chars.chunks(7) {
        steps.extend(splitter.push(&piece.iter().collect::<String>()));
    }
    steps.extend(splitter.finish());

    say("phase", json!("speaking"));
    say("voice", json!(true));
    for (id, step) in steps.iter().enumerate() {
        for tag in &step.tags {
            if let Some(mut v) = to_global(tag, &shots) {
                v["stepId"] = json!(id + 1);
                // A position: only the overlay of that screenshot's display gets it.
                windows::emit_overlay(app, &o.label, "visual", v);
            }
        }
        let ms = (step.text.chars().count() as u64 * 55).max(900);
        if !step.text.is_empty() {
            say("text", json!({ "text": step.text, "ms": ms }));
        }
        sleep(Duration::from_millis(ms + 500));
    }
    say("voice", json!(false));
    say("linger", json!(9000));
    say("phase", json!("idle"));
}

fn run_notch(app: &AppHandle, o: &Overlay) {
    let started = Instant::now();
    let log = |m: &str| println!("[notch] +{}ms {m}", started.elapsed().as_millis());
    let out = std::env::var_os("BADI_DEMO_OUT").map(PathBuf::from);
    let snap = |name: &str| {
        if let Some(dir) = &out {
            match snap_top(o, dir, name) {
                Ok(()) => log(&format!("saved {name}.png")),
                Err(e) => log(&format!("snap {name} failed: {e}")),
            }
        }
    };
    let ms = |n| sleep(Duration::from_millis(n));

    log("timer");
    app.state::<AppState>().timers.lock().unwrap().set_timer(now_ms(), 25 * 60_000, "nulis laporan");
    notch::send_status(app);
    ms(4000);

    log("drop");
    notch::request_drop(app, "focus");
    ms(1200);
    snap("0-drop-folder-quiet"); // a folder drop waits quietly: nothing shows until something is dragged over
    notch::drop_hover(app, true);
    ms(1200);
    snap("1-drop-folder");
    notch::drop_hover(app, false);
    ms(3800);
    notch::close_drop(app);

    // Upload: the drop zone with something dragged over it, then a pretend 3 MB upload.
    // A folder dropped for focus mode: the notice, then the pill that keeps showing the folder.
    log("focus");
    let folder = badi_agent::guard::home().join("Documents");
    crate::agents::set_focus(app, &folder.display().to_string());
    crate::agents::show_focus(app, &folder);
    ms(1000);
    snap("1b-focus-notice");
    notch::clear_panel_kind(app, "focus");
    ms(1500);
    snap("1c-focus-pill");
    crate::agents::set_focus(app, "");
    ms(500);

    log("drop upload");
    notch::request_drop(app, "upload");
    ms(1500);
    snap("2-drop-files");
    windows::broadcast(app, "island:dropHover", true);
    log("drag over");
    ms(700);
    snap("3-drag-over");
    ms(800);
    windows::broadcast(app, "island:dropHover", false);
    notch::close_drop(app);

    log("upload send");
    let upload = |stage: &str| {
        let mut extra = Map::new();
        extra.insert("progress".into(), json!({ "i": 0, "n": 1, "size": 3e6, "stage": stage, "at": now_ms() }));
        notch::show_panel(app, "upload", "", &[], vec![], 0, extra);
    };
    upload("send");
    ms(1200);
    snap("4-upload-send");
    ms(1300);
    log("upload save");
    upload("save");
    ms(1500);
    snap("5-upload-save");
    ms(500);
    upload("done");
    ms(450);
    notch::clear_panel_kind(app, "upload");
    log("uploaded");
    let open = Action::new("openUpload", t(app, "notchOpen"), "", Some(std::sync::Arc::new(|_: &AppHandle| {})));
    notch::show_notice(app, "uploaded", &format!("âœ“ {}", i18n::uploaded(app, 1)), &["laporan-q3.pdf".into()], vec![open], 4000);
    ms(800);
    snap("6-uploaded");
    ms(3200);

    log("confirm");
    // The question blocks this thread until answered, so its picture is taken from another one.
    if let Some(dir) = out.clone() {
        let o = o.clone();
        std::thread::spawn(move || {
            sleep(Duration::from_millis(2500));
            if snap_top(&o, &dir, "7-confirm").is_ok() {
                println!("[notch] saved 7-confirm.png");
            }
        });
    }
    let a = notch::wait_for_user(app, "confirm", "Aku mau hapus kalkulator.html dari folder aina ke Recycle Bin. Boleh?", true, 8000);
    log(&format!("answer approved={} text={:?}", a.approved, a.text));
    let open = Action::new("openAgent", t(app, "notchOpen"), "", Some(std::sync::Arc::new(|_: &AppHandle| {})));
    notch::show_notice(app, "agentDone", "✓ Filo selesai", &["Folder Unduhan sudah rapi: 42 file dipindah ke 5 folder.".into()], vec![open], 5000);
    log("notice");
    ms(6000);

    // A long dictation's note: the summary above the text, scrolling inside the notch.
    log("dictation");
    let mut extra = Map::new();
    let text = "Besok rapat Q4 jam sepuluh di ruang Merapi. Target penjualan kita naik dua puluh persen dibanding kuartal lalu, jadi tiap tim perlu rencana yang jelas.

Sari pegang laporan keuangan, Budi siapkan slide, dan Rina kirim undangan ke klien hari ini. Tenggat semua bahan hari Jumat jam tiga sore.

Jangan lupa minta data penjualan per kota dari tim gudang, dan cek lagi anggaran iklan bulan depan.".repeat(2);
    extra.insert("note".into(), json!({ "summary": "Persiapan rapat Q4.
- Target penjualan naik 20%
- Sari laporan, Budi slide, Rina undangan
- Tenggat Jumat 15.00", "summaryLabel": "Ringkasan", "text": text }));
    let noop = || Some(std::sync::Arc::new(|_: &AppHandle| {}) as notch::Run);
    notch::show_panel(app, "dictation", "âœ“ Dikte tersimpan", &["2026-10-19 14.32.05.md".into()], vec![Action::new("dictation:copy", "Salin", "", noop()), Action::new("dictation:open", "Buka file", "", noop()), Action::new("dictation:close", "Tutup", "", noop())], 0, extra);
    ms(1500);
    snap("8-dictation");
    ms(2500);
    notch::clear_panel_kind(app, "dictation");

    // The notes list (in a folder of the demo profile), one note opened from it, and the knowledge base.
    log("notes");
    let notes_dir = badi_store::data_dir().join("demo-notes");
    let _ = std::fs::remove_dir_all(&notes_dir);
    for (i, (summary, text)) in [(Some("Persiapan rapat Q4.\n- Target naik 20%"), "Besok rapat Q4 jam sepuluh."), (None, "Beli kopi, telepon klinik, kirim invoice ke PT Maju."), (None, "Ide konten: tutorial singkat fitur notch aina.")].into_iter().enumerate() {
        let created = chrono::Local::now() - chrono::Duration::hours(i as i64 * 5);
        let _ = crate::notes::save(&notes_dir, &crate::notes::Note { created, duration_s: 40, lang: "id", summary, text });
    }
    let mut patch = Map::new();
    patch.insert("dictationFolder".into(), json!(notes_dir.display().to_string()));
    let _ = app.state::<crate::AppState>().store.remember(&patch);
    notch::show_notes(app);
    ms(1200);
    snap("9-notes");
    notch::act(app, "notes:open:0");
    ms(1200);
    snap("9b-note");
    crate::knowledge::add(app, std::slice::from_ref(&notes_dir));
    notch::show_knowledge(app);
    ms(1200);
    snap("9c-knowledge");
    notch::clear_panel_kind(app, "knowledge");

    // The hover peek, then the timer's Ã— clicked inside the page, without touching the user's cursor:
    // cursor moves are sent as the shim's badi:cursor events, the button is clicked in the page.
    log("peek");
    let w = o.size.width as f64 / o.scale;
    for y in [4.0, 20.0, 60.0] {
        windows::emit_overlay(app, &o.label, "badi:cursor", json!({ "x": w / 2.0, "y": y }));
        ms(150);
    }
    ms(900);
    snap("8-peek");
    if let Some(win) = app.get_webview_window(&o.label) {
        let _ = win.eval("document.querySelector('[data-act=\"timerOff\"]')?.click()");
    }
    ms(400);
    let running = app.state::<AppState>().timers.lock().unwrap().timer.is_some();
    log(&format!("after click: timer {}", if running { "still running" } else { "stopped" }));
    windows::emit_overlay(app, &o.label, "badi:cursor", json!({ "x": w / 2.0, "y": 600.0 }));
    ms(600);

    // The pomodoro page: started from the notch's idle page, then paused, skipped, reset and
    // stopped by its own buttons (clicked inside the page), checking the timers after each.
    let click = |sel: &str| {
        if let Some(win) = app.get_webview_window(&o.label) {
            let _ = win.eval(format!("document.querySelector('{sel}')?.click()"));
        }
        ms(450);
    };
    let pomo = || {
        let st = app.state::<AppState>();
        let t = st.timers.lock().unwrap();
        t.timer.as_ref().map(|x| format!("{} round {} paused={}", x.pomodoro.as_ref().map(|p| p.phase).unwrap_or("timer"), x.pomodoro.as_ref().map(|p| p.round).unwrap_or(0), x.paused_left.is_some())).unwrap_or_else(|| "none".into())
    };
    let hover = || {
        for y in [4.0, 20.0, 60.0] {
            windows::emit_overlay(app, &o.label, "badi:cursor", json!({ "x": w / 2.0, "y": y }));
            ms(150);
        }
        ms(900);
    };
    log("pomodoro");
    hover();
    snap("10-pomodoro-idle");
    click("[data-act=\"timer:start\"]");
    log(&format!("start -> {}", pomo()));
    ms(1500);
    snap("10b-pomodoro-running");
    click("[data-act=\"timer:pause\"]");
    log(&format!("pause -> {}", pomo()));
    ms(500);
    snap("10c-pomodoro-paused");
    click("[data-act=\"timer:skip\"]");
    log(&format!("skip -> {}", pomo()));
    ms(500);
    snap("10d-pomodoro-break");
    click("[data-act=\"timer:reset\"]");
    log(&format!("reset -> {} left {:?}", pomo(), app.state::<AppState>().timers.lock().unwrap().left(now_ms())));
    click("[data-act=\"timerOff\"]");
    log(&format!("stop -> {}", pomo()));
    windows::emit_overlay(app, &o.label, "badi:cursor", json!({ "x": w / 2.0, "y": 600.0 }));
    ms(800);

    // Music: the page opens from its tab even with nothing playing (when nothing plays on this PC),
    // then a new song opens the track card by itself, then the hover peek's music page.
    log("music");
    // A question's voice from earlier may still be going: the notch back to rest first.
    crate::turn::set_phase(app, "idle");
    windows::broadcast(app, "clear", Value::Null);
    ms(400);
    hover();
    click(".pdot.k-media");
    snap("11a-music-tab");
    windows::emit_overlay(app, &o.label, "badi:cursor", json!({ "x": w / 2.0, "y": 600.0 }));
    ms(600);
    let key = "Hati-Hati di Jalan\u{1}Tulus";
    let caps = json!({ "playPause": true, "next": true, "previous": false, "seek": true, "shuffle": true, "repeat": true });
    let song = json!({ "title": "Hati-Hati di Jalan", "artist": "Tulus", "album": "Manusia", "app": "Spotify", "source": "Spotify.exe", "playing": true, "key": key, "pos": 72_000, "dur": 242_000, "at": now_ms(), "volume": 0.42, "muted": false, "caps": caps, "shuffle": true, "repeat": "track" });
    let lines = json!([[60_000, "Perjalanan membawamu"], [70_000, "Bertemu denganku"], [76_000, "Ku bertemu kamu"], [84_000, "Sepertimu yang kucari"]]);
    crate::apps::demo_media(app, song, Value::Null, lines);
    ms(900);
    snap("11-track");
    ms(6500); // the card closes after ~4.5 s, then the notch settles into the pill
    snap("11b-compact-music");
    hover();
    snap("11c-music-page");
    click(".art[data-local]");
    hover(); // the real cursor's position may have come in meanwhile
    snap("11d-lyrics");
    ms(1200);
    hover();
    snap("11e-lyrics-later");
    windows::emit_overlay(app, &o.label, "badi:cursor", json!({ "x": w / 2.0, "y": 600.0 }));
    ms(600);

    // Focus: the page from its tab with nothing focused, then a project focus an hour in (as the voice
    // or a dropped folder starts one) with a pomodoro for it; the AI edits lock and brief are checked;
    // then the Focus window for notes, ending with a summary, and the history.
    log("focus");
    let snap_window = |label: &str, name: &str| {
        if let (Some(dir), Some(win)) = (&out, app.get_webview_window(label))
            && let (Ok(p), Ok(s)) = (win.outer_position(), win.outer_size())
        {
            match crate::snap::region_png(p.x, p.y, s.width as i32, s.height as i32, &dir.join(format!("{name}.png"))) {
                Ok(()) => log(&format!("saved {name}.png")),
                Err(e) => log(&format!("snap {name} failed: {e}")),
            }
        }
    };
    crate::focus::end(app, Default::default());
    hover();
    click(".pdot.k-focus");
    snap("12-focus-idle");
    let project = badi_store::data_dir().join("demo-project");
    let _ = std::fs::create_dir_all(&project);
    let mut s = badi_core::focus::Session::for_folder(&project.display().to_string(), now_ms() - 72 * 60_000);
    s.about = "Aplikasi Tauri untuk asisten aina".into();
    s.goal = "Selesaikan halaman fokus di notch".into();
    s.pomodoros = 2;
    s.pomodoro_ms = 50 * 60_000;
    let name = s.name.clone();
    crate::focus::start(app, s);
    app.state::<AppState>().timers.lock().unwrap().start_pomodoro(now_ms(), &name);
    notch::send_status(app);
    let outside = badi_agent::guard::home().join("Desktop").join("catatan.txt");
    let lock_in = badi_agent::guard::outside_focus(&[project.join("src").join("main.rs")], &project).is_none();
    let lock_out = badi_agent::guard::outside_focus(&[outside], &project).is_some();
    log(&format!("lock: inside allowed={lock_in} outside refused={lock_out} locked_folder={:?}", crate::focus::locked_folder(app)));
    log(&format!("brief: {}", crate::focus::brief(app)));
    hover();
    click(".pdot.k-focus");
    snap("12b-focus-active");
    windows::emit_overlay(app, &o.label, "badi:cursor", json!({ "x": w / 2.0, "y": 600.0 }));
    crate::focus::update(app, &json!({ "note": "Halaman fokus sudah jadi, tinggal uji di Tauri." }));
    crate::focus::open_window(app, "note");
    ms(3000);
    snap_window("focus", "12c-focus-notes");
    let mut patch = Map::new();
    patch.insert("focusSummary".into(), json!(true));
    let _ = app.state::<AppState>().store.remember(&patch);
    crate::focus::act(app, "end");
    ms(3000);
    snap_window("focus", "12d-focus-end");
    let r = crate::focus::invoke(app, "focus:end", &json!({ "done": "Halaman fokus di notch", "pending": "Uji dengan project asli", "next": "Commit" }));
    log(&format!("ended: {}", r.map(|v| format!("{} ms={} pomodoros={}", v["name"], v["ms"], v["pomodoros"])).unwrap_or_else(|e| e)));
    app.state::<AppState>().timers.lock().unwrap().stop_timer();
    ms(600);
    crate::focus::open_window(app, "history");
    ms(3000);
    snap_window("focus", "12e-focus-history");
    if let Some(win) = app.get_webview_window("focus") {
        let _ = win.destroy();
    }
    windows::emit_overlay(app, &o.label, "badi:cursor", json!({ "x": w / 2.0, "y": 600.0 }));
    ms(300);
    log("done");
    app.exit(0);
}

// BADI_DEMO=talk: a hold of the talk keys with a circle drawn around the middle of the primary display,
// fed through the same path as the mouse hook (talk::on_move), without touching the real mouse or keys.
// Saves the whole display while the circle is on screen and the notch's answer afterwards.
fn run_talk(app: &AppHandle, o: &Overlay) {
    let started = Instant::now();
    let log = |m: &str| println!("[talk] +{}ms {m}", started.elapsed().as_millis());
    let out = std::env::var_os("BADI_DEMO_OUT").map(PathBuf::from);
    let (cx, cy) = (o.pos.x + o.size.width as i32 / 2, o.pos.y + o.size.height as i32 / 2);
    let r = 120.0 * o.scale;

    log("talk keys down");
    crate::talk::on_talk_down(app);
    sleep(Duration::from_millis(500));
    log("circle");
    for i in 0..=48 {
        let a = i as f64 / 48.0 * std::f64::consts::TAU * 1.05;
        crate::talk::on_move(app, cx + (r * a.cos()) as i32, cy + (r * 0.7 * a.sin()) as i32);
        sleep(Duration::from_millis(16));
    }
    // Released before 0.6 s of stillness: holding still longer would point (the newest mark wins, as in
    // v1). The picture is taken after the release, while Badi looks (the circle is still drawn).
    sleep(Duration::from_millis(150));
    log("talk keys up");
    let up = {
        let app = app.clone();
        std::thread::spawn(move || crate::talk::on_talk_up(&app))
    };
    sleep(Duration::from_millis(100));
    if let Some(dir) = &out {
        let _ = snap::region_png(o.pos.x, o.pos.y, o.size.width as i32, o.size.height as i32, &dir.join("talk-1-circle.png"));
        log("saved talk-1-circle.png");
    }
    let _ = up.join();
    sleep(Duration::from_millis(600));
    if let Some(dir) = &out {
        let _ = snap_top(o, dir, "talk-2-answer");
        log("saved talk-2-answer.png");
    }
    sleep(Duration::from_millis(1500));
    log("done");
    app.exit(0);
}

// BADI_DEMO=ask: a real turn without a microphone. The question is BADI_DEMO_Q (a typed question); the
// screen, Claude, the voice and the visuals are real. Saves the display while Badi answers.
fn run_ask(app: &AppHandle, o: &Overlay) {
    let q = std::env::var("BADI_DEMO_Q").unwrap_or_else(|_| "Tombol untuk menutup jendela yang paling depan ada di mana? Tunjukin ya.".into());
    let out = std::env::var_os("BADI_DEMO_OUT").map(PathBuf::from);
    crate::turn::prewarm(app);
    let shots = match badi_screen::capture::capture_all(&crate::talk::displays(app), (f64::NAN, f64::NAN), None, true) {
        Ok(s) => s,
        Err(e) => return eprintln!("[ask] capture failed: {e}"),
    };
    if let Some(dir) = out {
        let o = o.clone();
        std::thread::spawn(move || {
            sleep(Duration::from_millis(5500));
            let _ = snap::region_png(o.pos.x, o.pos.y, o.size.width as i32, o.size.height as i32, &dir.join("ask.png"));
        });
    }
    windows::broadcast(app, "transcript", &q);
    crate::turn::run_companion(app, &q, shots, crate::turn::generation());
    sleep(Duration::from_millis(1500));
    app.exit(0);
}

// BADI_DEMO=brain: a real "Kerjakan untukku" task (BADI_DEMO_Q) on the real screen, as if a reply
// ended with [DO:task]. Consent is skipped in demos; questions still wait in the notch. Run with
// BADI_TIMING=1 for the per-call log and the cost line.
fn run_brain(app: &AppHandle) {
    let task = std::env::var("BADI_DEMO_Q").unwrap_or_else(|_| "Buka Calculator, hitung 12 kali 3 pakai tombolnya, lalu bilang hasilnya.".into());
    crate::brain::start(app, &task);
    sleep(Duration::from_secs(2));
    while crate::brain::running(app) || crate::turn::phase(app) != "idle" {
        sleep(Duration::from_millis(500));
    }
    sleep(Duration::from_millis(1500));
    app.exit(0);
}

fn run_sprites(app: &AppHandle, o: &Overlay) {
    use crate::agents::{demo_end, demo_spawn, demo_state, demo_step};
    let started = Instant::now();
    let log = |m: &str| println!("[sprites] +{}ms {m}", started.elapsed().as_millis());
    let out = std::env::var_os("BADI_DEMO_OUT").map(PathBuf::from);
    // The top-right corner of the primary display, where the panel is.
    let snap = |name: &str| {
        if let Some(dir) = &out {
            let (w, h) = ((900.0 * o.scale) as i32, (o.size.height as f64 * 0.9) as i32);
            let x = o.pos.x + o.size.width as i32 - w;
            match snap::region_png(x, o.pos.y, w, h, &dir.join(format!("{name}.png"))) {
                Ok(()) => log(&format!("saved {name}.png")),
                Err(e) => log(&format!("snap {name} failed: {e}")),
            }
        }
    };
    let ms = |n| sleep(Duration::from_millis(n));

    log("spawn Ciro");
    let ciro = demo_spawn(app, "ciro", "Perbaiki capture protection di overlay Windows");
    ms(900);
    snap("01-spawn");
    demo_step(app, ciro, "Memeriksa file main.rs");
    ms(700);
    demo_step(app, ciro, "Menganalisis error pada Windows API");
    let mino = demo_spawn(app, "mino", "Bandingkan 5 aplikasi catatan terpopuler");
    ms(600);
    demo_step(app, mino, "Membuka beberapa sumber");
    let ciro2 = demo_spawn(app, "ciro", "Review git diff di folder weather-invitation");
    let routine = demo_spawn(app, "routine", "Setiap pagi kirim ringkasan berita keuangan");
    let filo = demo_spawn(app, "filo", "Rapikan folder Unduhan");
    let nilo = demo_spawn(app, "nilo", "Balas email Sari soal jadwal meeting");
    let daro = demo_spawn(app, "daro", "Buat grafik penjualan Q3 dari sales.csv");
    ms(500);
    demo_step(app, ciro2, "Membaca git diff (12 file)");
    demo_step(app, filo, "Mencari file di Unduhan");
    demo_step(app, nilo, "Membaca email terakhir dari Sari");
    demo_step(app, daro, "Membaca sales.csv (4.210 baris)");
    demo_step(app, mino, "Membandingkan informasi");
    demo_step(app, ciro, "Memperbaiki implementasi capture protection");
    // Ciro's edits, as the hook would report them.
    let old = "use windows::Win32::UI::WindowsAndMessaging::*;\n\npub fn protect(w: &Window) {\n    let h = w.hwnd();\n    unsafe { SetWindowDisplayAffinity(h, WDA_MONITOR) };\n}\n\npub fn show(w: &Window) {\n    w.show();\n}\n";
    let new = "use windows::Win32::UI::WindowsAndMessaging::*;\n\n/// Keeps the overlay out of screen captures (Windows 10 2004+), else the older monitor-only mode.\npub fn protect(w: &Window) -> bool {\n    let h = w.hwnd();\n    unsafe { SetWindowDisplayAffinity(h, WDA_EXCLUDEFROMCAPTURE) }\n        .or_else(|_| unsafe { SetWindowDisplayAffinity(h, WDA_MONITOR) })\n        .is_ok()\n}\n\npub fn show(w: &Window) {\n    w.show();\n}\n";
    crate::agents::demo_diff(app, ciro, badi_agent::diff::from_edit(old, new, r"C:\proyek\badi\src\capture.rs"));
    crate::agents::demo_diff(app, ciro, badi_agent::diff::from_edit("#[test]\nfn hides() {}\n", "#[test]\nfn hides() {}\n\n#[test]\nfn falls_back_to_monitor() {\n    assert!(protect(&fake(false)));\n}\n", r"C:\proyek\badi\src\capture_test.rs"));
    crate::agents::demo_diff(app, ciro, badi_agent::diff::from_new("# Capture\n\nOverlay disembunyikan dari tangkapan layar.\n", r"C:\proyek\badi\docs\capture.md"));
    demo_state(app, routine, "waiting");
    demo_state(app, filo, "needs_approval");
    ms(1200);
    snap("02-seven");
    log(&format!("seven cards, {} px tall display", o.size.height));

    // The cursor as the mouse hook reports it (injected moves are ignored by the hook): over Ciro 1,
    // the oldest card, for its detail.
    let (_, _, bw, _) = o.bounds();
    let hover = |x: f64, y: f64| windows::emit_overlay(app, &o.label, "badi:cursor", json!({ "x": x, "y": y }));
    crate::cursor::DEMO_HOLDS.store(true, std::sync::atomic::Ordering::Relaxed);
    // Square tiles of 76 px, 8 px apart, 12 px from the edge: Ciro 1 is the seventh in the first column.
    let (kx, ky) = (bw - 12.0 - 38.0, 12.0 + 6.0 * 84.0 + 38.0);
    hover(kx, ky);
    ms(3000);
    snap("03-hover");
    // Open capture.rs's diff (the list shows the latest edit first, so it is the third row).
    windows::emit_overlay(app, &o.label, "badi:cursor", json!({ "x": kx, "y": ky, "click": ".sp-detail .dv-file:nth-child(3)" }));
    ms(3000);
    snap("03-diff");
    hover(bw - 600.0, 600.0);
    ms(700);
    snap("03a-left");
    crate::cursor::DEMO_HOLDS.store(false, std::sync::atomic::Ordering::Relaxed);

    // More cards than a column holds (only the panel's layout; no state behind them).
    let fake: Vec<Value> = (0..30)
        .map(|i| {
            let k = &badi_agent::sprites::KINDS[i % 6];
            json!({ "id": 10_000 + i, "kind": k.id, "name": format!("{} {}", k.name, i / 6 + 1), "icon": k.icon, "role": k.role[1], "state": "working", "status": k.working[1], "task": "", "steps": [], "result": "", "started": now_ms(), "ended": null, "live": true })
        })
        .collect();
    windows::emit_overlay(app, &o.label, "sprites:status", json!({ "sprites": fake, "now": now_ms() }));
    ms(900);
    if let Some(dir) = &out {
        let _ = snap::region_png(o.pos.x, o.pos.y, o.size.width as i32, o.size.height as i32, &dir.join("03b-columns.png"));
    }
    crate::agents::send_sprites(app);
    ms(600);

    demo_end(app, ciro, "success", "Capture protection sudah benar lagi; build dan tes lolos.");
    demo_end(app, daro, "error", "Couldn't finish this: sales.csv tidak ditemukan.");
    demo_end(app, nilo, "cancelled", "");
    ms(900);
    snap("04-ended");
    for id in [mino, ciro2, routine, filo] {
        demo_end(app, id, "success", "Beres.");
    }
    ms(25_000);
    snap("05-gone");
    app.exit(0);
}

fn run_tasks(app: &AppHandle, o: &Overlay) {
    use crate::agents::{demo_end, demo_spawn, demo_state, demo_step};
    let started = Instant::now();
    let log = |m: &str| println!("[tasks] +{}ms {m}", started.elapsed().as_millis());
    let out = std::env::var_os("BADI_DEMO_OUT").map(PathBuf::from);
    let ms = |n| sleep(Duration::from_millis(n));
    let snap = |name: &str| {
        if let Some(dir) = &out {
            let (w, h) = ((760.0 * o.scale) as i32, (300.0 * o.scale) as i32);
            let x = o.pos.x + (o.size.width as i32 - w) / 2;
            match snap::region_png(x, o.pos.y, w, h, &dir.join(format!("{name}.png"))) {
                Ok(()) => log(&format!("saved {name}.png")),
                Err(e) => log(&format!("snap {name} failed: {e}")),
            }
        }
    };
    let w = o.size.width as f64 / o.scale;
    let hover = || {
        for y in [4.0, 20.0, 60.0] {
            windows::emit_overlay(app, &o.label, "badi:cursor", json!({ "x": w / 2.0, "y": y }));
            ms(150);
        }
        ms(900);
    };
    let leave = || {
        windows::emit_overlay(app, &o.label, "badi:cursor", json!({ "x": w / 2.0, "y": 600.0 }));
        ms(600);
    };
    let click = |sel: &str| {
        if let Some(win) = app.get_webview_window(&o.label) {
            let _ = win.eval(format!("document.querySelector('{sel}')?.click()"));
        }
        ms(450);
    };
    crate::cursor::DEMO_HOLDS.store(true, std::sync::atomic::Ordering::Relaxed);

    let ciro = demo_spawn(app, "ciro", "Build API dan jalankan tesnya");
    let mino = demo_spawn(app, "mino", "Bandingkan 5 aplikasi catatan terpopuler");
    let filo = demo_spawn(app, "filo", "Rapikan folder Unduhan");
    ms(300);
    demo_step(app, ciro, "Menjalankan cargo test (42 tes)");
    demo_step(app, mino, "Membaca ulasan di beberapa situs");
    demo_state(app, filo, "needs_approval");
    ms(2500);
    hover();
    click(".pdot.k-tasks");
    snap("t1-active");

    // Stop asks first; only the answer stops the Sprite.
    click("#island .peek .task:not(.s-needs) [data-local=\"taskStop\"]");
    snap("t2-stop-ask");
    click("#island .peek [data-act^=\"sprite:stop:\"]");
    log(&format!("after confirm: {} running", crate::agents::notch_tasks(app)["list"].as_array().map(|l| l.iter().filter(|t| t["status"] == "running").count()).unwrap_or(0)));
    snap("t3-stopped");
    leave();

    // A failure: the page comes first, with its short error and Retry.
    demo_end(app, mino, "error", "Couldn't finish this: situs pembanding menolak akses (HTTP 403).");
    ms(400);
    hover();
    snap("t4-failed");
    leave();

    // Once the cards have left: the history.
    demo_end(app, filo, "success", "Unduhan sudah rapi: 48 file dipindah ke 6 folder.");
    ms(21_000);
    hover();
    click(".pdot.k-tasks");
    click("#island .peek [data-local=\"taskView:history\"]");
    snap("t5-history");
    log(&format!("history: {}", crate::agents::notch_tasks(app)["history"].as_array().map(|h| h.len()).unwrap_or(0)));
    leave();
    crate::cursor::DEMO_HOLDS.store(false, std::sync::atomic::Ordering::Relaxed);
    log("done");
    app.exit(0);
}

// BADI_DEMO=reminders (run with BADI_PROFILE set to a scratch folder: it writes reminders and prayer
// times into that config): local reminders, Google events given as if fetched (no request), today's
// prayer times around now, then the notch's three views, the prayer list, a sync failure, the compact
// pill and the Reminder window.
fn run_reminders(app: &AppHandle, o: &Overlay) {
    use badi_core::calendar::CalendarInfo;
    use badi_core::timers::Reminder;
    use chrono::{Local, TimeZone, Timelike};
    let started = Instant::now();
    let log = |m: &str| println!("[reminders] +{}ms {m}", started.elapsed().as_millis());
    let out = std::env::var_os("BADI_DEMO_OUT").map(PathBuf::from);
    let ms = |n| sleep(Duration::from_millis(n));
    let snap = |name: &str, h: f64| {
        if let Some(dir) = &out {
            let (w, hh) = ((820.0 * o.scale) as i32, (h * o.scale) as i32);
            let x = o.pos.x + (o.size.width as i32 - w) / 2;
            match snap::region_png(x, o.pos.y, w, hh, &dir.join(format!("{name}.png"))) {
                Ok(()) => log(&format!("saved {name}.png")),
                Err(e) => log(&format!("snap {name} failed: {e}")),
            }
        }
    };
    let w = o.size.width as f64 / o.scale;
    let hover = || {
        for y in [4.0, 20.0, 60.0] {
            windows::emit_overlay(app, &o.label, "badi:cursor", json!({ "x": w / 2.0, "y": y }));
            ms(150);
        }
        ms(900);
    };
    let leave = || {
        windows::emit_overlay(app, &o.label, "badi:cursor", json!({ "x": w / 2.0, "y": 700.0 }));
        ms(700);
    };
    let click = |sel: &str| {
        if let Some(win) = app.get_webview_window(&o.label) {
            let _ = win.eval(format!("document.querySelector('{sel}')?.click()"));
        }
        ms(500);
    };
    crate::cursor::DEMO_HOLDS.store(true, std::sync::atomic::Ordering::Relaxed);
    let now = now_ms();
    let min = 60_000;
    let today = Local::now().date_naive();

    // Local: one due in 12 minutes, one overdue, a daily repeat, one done, one tomorrow.
    {
        let st = app.state::<AppState>();
        let mut t = st.timers.lock().unwrap();
        t.reminders.clear();
        t.put(Reminder::new(String::new(), now + 12 * min, "Update progress ke tim".into()), Some(now));
        let mut late = Reminder::new(String::new(), now - 40 * min, "Bayar tagihan listrik".into());
        late.fired = true;
        t.put(late, Some(now));
        let mut daily = Reminder::new(String::new(), now + 3 * 60 * min, "Minum obat".into());
        daily.repeat = Some("daily".into());
        t.put(daily, Some(now));
        let mut done = Reminder::new(String::new(), now - 3 * 60 * min, "Kirim invoice".into());
        done.done = true;
        done.done_at = Some(now - 2 * 60 * min);
        t.put(done, Some(now));
        t.put(Reminder::new(String::new(), now + 26 * 60 * min, "Review kontrak".into()), Some(now));
    }
    crate::reminders::changed(app);

    // Google: a work calendar (writable) and a holidays calendar (read-only).
    let cals = vec![
        CalendarInfo { id: "me@gmail.com".into(), name: "Kerja".into(), color: Some("#4285f4".into()), access: "owner".into(), primary: true, selected: true },
        CalendarInfo { id: "id.indonesian#holiday".into(), name: "Hari libur".into(), color: Some("#0b8043".into()), access: "reader".into(), primary: false, selected: true },
    ];
    let iso = |ms: i64| Local.timestamp_millis_opt(ms).unwrap().to_rfc3339();
    let ev = |id: &str, title: &str, s: i64, e: i64| json!({ "id": id, "summary": title, "start": { "dateTime": iso(s) }, "end": { "dateTime": iso(e) }, "htmlLink": "https://calendar.google.com/calendar/event?eid=demo" });
    let tomorrow = today.succ_opt().unwrap();
    let raw = vec![
        (0, ev("e1", "Review PR", now + 35 * min, now + 65 * min)),
        (0, ev("e2", "Daily meeting", now + 95 * min, now + 125 * min)),
        (0, ev("e3", "Sync desain", now - 20 * min, now + 25 * min)),
        (0, ev("e4", "1:1 dengan Sari", now + 25 * 60 * min, now + 26 * 60 * min)),
        (1, json!({ "id": "h1", "summary": "Hari Sumpah Pemuda", "start": { "date": tomorrow.format("%Y-%m-%d").to_string() }, "end": { "date": tomorrow.succ_opt().unwrap().format("%Y-%m-%d").to_string() } })),
    ];
    crate::reminders::demo_remote(app, cals, raw);

    // No city yet: the page's guide line, and a click on it opens Settings at the city field.
    {
        let mut patch = Map::new();
        patch.insert("prayerEnabled".into(), json!(false));
        patch.insert("prayerCity".into(), Value::Null);
        patch.insert("prayerGuideDismissed".into(), json!(false));
        let _ = app.state::<AppState>().store.remember(&patch);
        notch::clear_panel_kind(app, "remind");
        crate::reminders::publish(app);
    }
    ms(800);
    hover();
    click(".pdot.k-later");
    snap("r0b-guide", 300.0);
    click("#island .peek button.cpgo");
    ms(3200);
    if let (Some(dir), Some(win)) = (&out, app.get_webview_window("settings"))
        && let (Ok(p), Ok(s)) = (win.outer_position(), win.outer_size())
    {
        let _ = snap::region_png(p.x, p.y, s.width as i32, s.height as i32, &dir.join("r0c-guide-settings.png"));
        log("saved r0c-guide-settings.png");
    }
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.destroy();
    }
    leave();

    // Prayer: today's eight times laid out around now, so the next one is ~12 minutes away.
    let base = Local.timestamp_millis_opt(now + 12 * min).unwrap();
    let next_hm = (base.hour(), base.minute());
    let shift = |h: i32, m: i32| {
        let t = Local.from_local_datetime(&today.and_hms_opt(next_hm.0, next_hm.1, 0).unwrap()).earliest().unwrap() + chrono::Duration::minutes((h * 60 + m) as i64);
        if t.date_naive() == today { t.format("%H:%M").to_string() } else if h < 0 { "00:01".into() } else { "23:58".into() }
    };
    let times = vec![shift(-9, -55), shift(-9, -45), shift(-8, -30), shift(-8, -5), shift(-3, -45), shift(0, 0), shift(2, 50), shift(4, 0)];
    let mut days = std::collections::BTreeMap::new();
    days.insert(today.format("%Y-%m-%d").to_string(), times.clone());
    days.insert(tomorrow.format("%Y-%m-%d").to_string(), times);
    crate::reminders::demo_prayer(app, ("1221", "KOTA BEKASI"), days);
    // A reminder left from an earlier run in this profile may have just gone off: its notice would
    // cover the peek.
    notch::clear_panel_kind(app, "remind");
    notch::send_status(app);
    ms(1500);

    leave();
    snap("r0-compact", 120.0);
    // Home, the page the peek opens on, with the weather on the left (real when the laptop's location
    // answers in time, else the demo's).
    if crate::weather::status()["temp"].is_null() {
        crate::weather::demo(app, "Bekasi", 30, 61);
    }
    hover();
    snap("r0d-home", 300.0);
    click(".pdot.k-later");
    snap("r1-agenda", 420.0);
    click("#island .peek [data-local=\"cal:prayer\"]");
    snap("r2-prayer-open", 520.0);
    click("#island .peek [data-local=\"cal:prayer\"]");
    click("#island .peek [data-local=\"cal:view:timeline\"]");
    hover();
    snap("r3-timeline", 420.0);
    // The calendar grid (comfortable, as Settings would set it).
    crate::reminders::act(app, "density:comfortable");
    click("#island .peek [data-local=\"cal:view:calendar\"]");
    hover();
    snap("r4-calendar", 480.0);
    crate::reminders::act(app, "density:compact");
    // A row asking before it deletes (the overdue local reminder).
    click("#island .peek [data-local=\"cal:view:agenda\"]");
    hover();
    click("#island .peek .crow.st-overdue [data-local^=\"cal:ask:delete\"]");
    snap("r5-delete-ask", 480.0);
    click("#island .peek [data-local=\"cal:askoff\"]");
    crate::reminders::demo_sync(app, "offline", "error sending request");
    ms(500);
    hover();
    snap("r6-sync-offline", 480.0);
    let view = app.state::<AppState>().store.get("reminderView");
    log(&format!("view saved: {view}, density: {}", app.state::<AppState>().store.get("reminderDensity")));
    // Another page at the usual size (160 px), reached with the indicator's arrow.
    click("#island .peek [data-pagestep=\"1\"]");
    ms(400);
    snap("r6b-next-page", 300.0);
    leave();
    crate::cursor::DEMO_HOLDS.store(false, std::sync::atomic::Ordering::Relaxed);
    crate::reminders::act(app, "add:0");
    ms(3000);
    if let (Some(dir), Some(win)) = (&out, app.get_webview_window("reminder"))
        && let (Ok(p), Ok(s)) = (win.outer_position(), win.outer_size())
    {
        let _ = snap::region_png(p.x, p.y, s.width as i32, s.height as i32, &dir.join("r7-window.png"));
        log("saved r7-window.png");
    }
    if let Some(w) = app.get_webview_window("reminder") {
        let _ = w.destroy();
    }
    notch::open_settings_at(app, "remindersCard");
    ms(3500);
    if let (Some(dir), Some(win)) = (&out, app.get_webview_window("settings"))
        && let (Ok(p), Ok(s)) = (win.outer_position(), win.outer_size())
    {
        let _ = snap::region_png(p.x, p.y, s.width as i32, s.height as i32, &dir.join("r8-settings.png"));
        log("saved r8-settings.png");
    }
    log("done");
    app.exit(0);
}

fn run_fullscreen(app: &AppHandle, o: &Overlay) {
    let started = Instant::now();
    let log = |m: &str| println!("[fullscreen] +{}ms {m}", started.elapsed().as_millis());
    let out = std::env::var_os("BADI_DEMO_OUT").map(PathBuf::from);
    let ms = |n| sleep(Duration::from_millis(n));
    let snap = |name: &str| {
        if let Some(dir) = &out {
            let _ = snap_top(o, dir, name);
            log(&format!("saved {name}.png"));
        }
    };
    let w = o.size.width as f64 / o.scale;
    let cursor = |x: f64, y: f64| windows::emit_overlay(app, &o.label, "badi:cursor", json!({ "x": x, "y": y }));
    crate::cursor::DEMO_HOLDS.store(true, std::sync::atomic::Ordering::Relaxed);
    // A timer, so the notch would show a pill if nothing hid it.
    app.state::<AppState>().timers.lock().unwrap().set_timer(now_ms(), 25 * 60_000, "Fokus");
    notch::send_status(app);
    ms(800);
    snap("f0-normal");
    windows::emit_overlay(app, &o.label, "fullscreen", json!(true));
    cursor(w / 2.0, 400.0);
    ms(700);
    snap("f1-hidden");
    // At the very top for less than a second: still hidden.
    cursor(w / 2.0, 1.0);
    ms(500);
    snap("f2-top-half-second");
    ms(900);
    snap("f3-revealed");
    for y in [6.0, 20.0, 40.0] {
        cursor(w / 2.0, y);
        ms(150);
    }
    ms(900);
    snap("f4-peek");
    cursor(w / 2.0, 500.0);
    ms(1200);
    snap("f5-hidden-again");
    windows::emit_overlay(app, &o.label, "fullscreen", json!(false));
    ms(700);
    snap("f6-normal-again");
    app.state::<AppState>().timers.lock().unwrap().stop_timer();
    crate::cursor::DEMO_HOLDS.store(false, std::sync::atomic::Ordering::Relaxed);
    log("done");
    app.exit(0);
}

fn run_mini(app: &AppHandle, o: &Overlay) {
    let started = Instant::now();
    let log = |m: &str| println!("[mini] +{}ms {m}", started.elapsed().as_millis());
    let out = std::env::var_os("BADI_DEMO_OUT").map(PathBuf::from);
    let ms = |n| sleep(Duration::from_millis(n));
    let snap = |name: &str| {
        if let Some(dir) = &out {
            let _ = snap_top(o, dir, name);
            log(&format!("saved {name}.png"));
        }
    };
    let w = o.size.width as f64 / o.scale;
    crate::cursor::DEMO_HOLDS.store(true, std::sync::atomic::Ordering::Relaxed);
    windows::emit_overlay(app, &o.label, "badi:cursor", json!({ "x": w / 2.0, "y": 600.0 }));
    // Nothing else running (no timer, no reminder from an earlier run): the song has the pill.
    {
        let st = app.state::<AppState>();
        let mut t = st.timers.lock().unwrap();
        t.stop_timer();
        t.reminders.clear();
    }
    crate::reminders::changed(app);
    notch::clear_panel_kind(app, "remind");
    let song = |key: &str| json!({ "title": "Hati-Hati di Jalan", "artist": "Tulus", "album": "Manusia", "app": "Spotify", "playing": true, "key": key, "pos": 72_000, "dur": 242_000, "at": now_ms(), "volume": 0.4, "muted": false });
    let lines = json!([[60_000, "Perjalanan membawamu"], [70_000, "Bertemu denganku"], [76_000, "Ku bertemu kamu"], [84_000, "Sepertimu yang kucari"]]);
    crate::apps::demo_media(app, song("demo-a"), Value::Null, lines.clone());
    ms(6000); // the track card closes, the pill takes over
    snap("m1-lyric-eyes");
    ms(4500);
    snap("m2-next-line");
    // A cover (a small gradient) instead of the eyes, and no lyrics: the title.
    let cover = "data:image/svg+xml;base64,PHN2ZyB4bWxucz0naHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmcnIHdpZHRoPSc0MCcgaGVpZ2h0PSc0MCc+PGRlZnM+PGxpbmVhckdyYWRpZW50IGlkPSdnJz48c3RvcCBvZmZzZXQ9JzAnIHN0b3AtY29sb3I9JyNmZjZiOWEnLz48c3RvcCBvZmZzZXQ9JzEnIHN0b3AtY29sb3I9JyMyYjhhZmYnLz48L2xpbmVhckdyYWRpZW50PjwvZGVmcz48cmVjdCB3aWR0aD0nNDAnIGhlaWdodD0nNDAnIGZpbGw9J3VybCgjZyknLz48L3N2Zz4=";
    crate::apps::demo_media(app, song("demo-b"), json!(cover), Value::Null);
    ms(6000);
    snap("m3-cover-title");
    // A pomodoro over the song: the timer has the pill, the equalizer says a song plays.
    app.state::<AppState>().timers.lock().unwrap().start_pomodoro(now_ms(), "");
    notch::send_status(app);
    ms(1200);
    snap("m4-timer-over-song");
    app.state::<AppState>().timers.lock().unwrap().stop_timer();
    crate::cursor::DEMO_HOLDS.store(false, std::sync::atomic::Ordering::Relaxed);
    log("done");
    app.exit(0);
}

// BADI_DEMO=agent: a [DELEGATE:task] (BADI_DEMO_Q) on the configured engine; BADI_LOG=1 prints the log.
fn run_agent(app: &AppHandle) {
    let task = std::env::var("BADI_DEMO_Q").unwrap_or_else(|_| "Tulis file output/halo.md berisi satu kalimat sapaan, lalu sebutkan isi folder Documents/aina pakai list_folder.".into());
    crate::agents::delegate(app, &task);
    sleep(Duration::from_secs(1));
    while crate::agents::any_running(app) {
        sleep(Duration::from_millis(500));
    }
    let last = crate::agents::agents_store().get("mino").and_then(|m| m["messages"].as_array()?.last().cloned());
    println!("[agent] reply: {}", last.map(|m| m.to_string()).unwrap_or_default());
    sleep(Duration::from_millis(1500));
    app.exit(0);
}

// The top of an overlay: 700Ã—230 around the notch, as v1 saved it.
// BADI_DEMO=signin: the panel asking Badi shows while signed out; BADI_DEMO_OUT saves it.
fn run_signin(app: &AppHandle, o: &Overlay) {
    let out = std::env::var_os("BADI_DEMO_OUT").map(PathBuf::from);
    // The panel itself: a signed-in PC (the token lives in the Credential Manager) would skip require.
    crate::cloud::show_signin(app);
    sleep(Duration::from_millis(1500));
    if let Some(dir) = &out {
        match snap_top(o, dir, "signin") {
            Ok(()) => println!("[signin] saved signin.png"),
            Err(e) => println!("[signin] snap failed: {e}"),
        }
    }
}

// BADI_DEMO=credits: what asking Badi does with no credits left (no paid call): the line in Windows'
// voice or a fetched clip, then the Beli kredit panel; BADI_DEMO_OUT saves both.
fn run_credits(app: &AppHandle, o: &Overlay) {
    let out = std::env::var_os("BADI_DEMO_OUT").map(PathBuf::from);
    let started = Instant::now();
    crate::cloud::out_of_credits(app);
    for (at, name) in [(2500, "credits-line"), (16000, "credits-panel")] {
        sleep(Duration::from_millis(at).saturating_sub(started.elapsed()));
        if let Some(dir) = &out {
            match snap_top(o, dir, name) {
                Ok(()) => println!("[credits] +{}ms saved {name}.png", started.elapsed().as_millis()),
                Err(e) => println!("[credits] snap failed: {e}"),
            }
        }
    }
}

fn snap_top(o: &Overlay, dir: &std::path::Path, name: &str) -> Result<(), String> {
    let (w, h) = ((700.0 * o.scale) as i32, (230.0 * o.scale) as i32);
    let x = o.pos.x + (o.size.width as i32 - w) / 2;
    snap::region_png(x, o.pos.y, w, h, &dir.join(format!("{name}.png")))
}

// ---------- BADI_DEMO=mascot: performance scenarios ----------

struct PerfRec {
    dir: PathBuf,
    scenario: String,
    keys: Vec<String>,
}

static PERF: std::sync::Mutex<Option<PerfRec>> = std::sync::Mutex::new(None);

/// One summary from an overlay's probe (perf_probe.js) → a row of renderer.csv, tagged with the scenario.
pub fn perf_sample(label: &str, p: &Value) {
    use std::io::Write;
    let mut g = PERF.lock().unwrap();
    let (Some(rec), Some(obj)) = (g.as_mut(), p.as_object()) else { return };
    let path = rec.dir.join("renderer.csv");
    let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) else { return };
    if rec.keys.is_empty() {
        rec.keys = obj.keys().cloned().collect();
        let _ = writeln!(f, "scenario,overlay,{}", rec.keys.join(","));
    }
    let cell = |v: Option<&Value>| match v {
        Some(Value::String(s)) => s.replace(',', ";"),
        Some(Value::Null) | None => String::new(),
        Some(v) => v.to_string(),
    };
    let row: Vec<String> = rec.keys.iter().map(|k| cell(obj.get(k))).collect();
    let _ = writeln!(f, "{},{label},{}", rec.scenario, row.join(","));
}

fn run_mascot(app: &AppHandle, o: &Overlay) {
    use std::io::Write;
    let dir = std::env::var_os("BADI_DEMO_OUT").map(PathBuf::from).unwrap_or_else(|| std::env::temp_dir().join("badi-perf"));
    let _ = std::fs::create_dir_all(&dir);
    let scale: f64 = std::env::var("BADI_PERF_SCALE").ok().and_then(|s| s.parse().ok()).unwrap_or(1.0);
    let pick = std::env::var("BADI_DEMO_MASCOT").unwrap_or_else(|_| "all".into());
    let all = ["idle30", "anim10", "toggle100", "expr", "hideshow"];
    let chosen: Vec<&str> = all.iter().copied().filter(|n| pick == "all" || pick.split(',').any(|p| p.trim() == *n)).collect();
    let secs = |s: f64| Duration::from_secs_f64((s * scale).max(0.2));
    let times = |n: f64| ((n * scale).round() as usize).max(2);
    let ms = |n| sleep(Duration::from_millis(n));
    let started = Instant::now();
    let log = |m: &str| println!("[mascot] +{}s {m}", started.elapsed().as_secs());
    let set_scenario = |name: &str| {
        if let Some(rec) = PERF.lock().unwrap().as_mut() {
            rec.scenario = name.to_string();
        }
    };
    *PERF.lock().unwrap() = Some(PerfRec { dir: dir.clone(), scenario: "warmup".into(), keys: vec![] });
    let _ = std::fs::write(dir.join("markers.csv"), "scenario,start_ms,end_ms\n");
    let mark = |name: &str, start: i64| {
        if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(dir.join("markers.csv")) {
            let _ = writeln!(f, "{name},{start},{}", now_ms());
        }
    };

    let say = |ch: &str, p: Value| windows::broadcast(app, ch, p);
    let w = o.size.width as f64 / o.scale;
    let cursor = |y: f64| windows::emit_overlay(app, &o.label, "badi:cursor", json!({ "x": w / 2.0, "y": y }));
    crate::cursor::DEMO_HOLDS.store(true, std::sync::atomic::Ordering::Relaxed);
    cursor(600.0);
    // The notch at rest: no timer, reminder or panel of an earlier run.
    {
        let st = app.state::<AppState>();
        let mut t = st.timers.lock().unwrap();
        t.stop_timer();
        t.reminders.clear();
    }
    crate::reminders::changed(app);
    notch::send_status(app);
    let (bx, by, _, _) = o.bounds();
    say("subtitleDisplay", json!({ "x": bx, "y": by }));
    say("mode", json!("companion"));
    say("phase", json!("idle"));
    let start = now_ms();
    log("warmup");
    sleep(secs(20.0).max(Duration::from_secs(6)));
    mark("warmup", start);

    for name in chosen {
        set_scenario(name);
        let start = now_ms();
        log(name);
        match name {
            // The notch at rest, cursor away, nothing happening.
            "idle30" => sleep(secs(1800.0)),
            // Turns back to back: listening (with a mic level), thinking, speaking.
            "anim10" => {
                let end = Instant::now() + secs(600.0);
                while Instant::now() < end {
                    say("phase", json!("listening"));
                    for i in 0..30 {
                        say("level", json!(0.25 + 0.2 * ((i as f64) * 0.7).sin().abs()));
                        ms(100);
                    }
                    say("transcript", json!("aina, ini apa ya?"));
                    say("phase", json!("thinking"));
                    ms(3000);
                    say("phase", json!("speaking"));
                    say("voice", json!(true));
                    say("text", json!({ "text": "Itu tombol simpan, ada di kanan atas.", "ms": 3500 }));
                    ms(4000);
                    say("voice", json!(false));
                }
            }
            // Compact ↔ expanded: the hover peek opened and closed by the cursor.
            "toggle100" => {
                for _ in 0..times(100.0) {
                    for y in [4.0, 20.0, 60.0] {
                        cursor(y);
                        ms(120);
                    }
                    ms(1200);
                    cursor(600.0);
                    ms(1200);
                }
            }
            // Expression changes every 150 ms.
            "expr" => {
                let phases = ["listening", "thinking", "speaking", "idle", "guiding", "error"];
                for i in 0..times(1000.0) {
                    match phases[i % phases.len()] {
                        "error" => say("error", json!("Ups, ada yang salah.")),
                        "speaking" => {
                            say("phase", json!("speaking"));
                            say("voice", json!(true));
                        }
                        p => {
                            say("voice", json!(false));
                            say("clear", Value::Null);
                            say("phase", json!(p));
                        }
                    }
                    ms(150);
                }
                say("voice", json!(false));
                say("clear", Value::Null);
            }
            // The overlay window hidden and shown again while Badi talks.
            "hideshow" => {
                say("phase", json!("speaking"));
                say("voice", json!(true));
                say("text", json!({ "text": "Aku masih ngomong, coba sembunyikan aku.", "ms": 200_000 }));
                for _ in 0..times(100.0) {
                    windows::set_overlay_visible(app, &o.label, false);
                    ms(1000);
                    windows::set_overlay_visible(app, &o.label, true);
                    ms(1000);
                }
                say("voice", json!(false));
            }
            _ => {}
        }
        mark(name, start);
        // Back to rest between scenarios; the cool-down is recorded on its own.
        say("phase", json!("idle"));
        say("clear", Value::Null);
        cursor(600.0);
        let cool = format!("cool-{name}");
        set_scenario(&cool);
        let start = now_ms();
        sleep(secs(15.0).max(Duration::from_secs(3)));
        mark(&cool, start);
    }
    *PERF.lock().unwrap() = None;
    crate::cursor::DEMO_HOLDS.store(false, std::sync::atomic::Ordering::Relaxed);
    log("done");
    app.exit(0);
}