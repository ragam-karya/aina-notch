// aina: a Windows tray app. One process: the Rust core plus WebView2 pages that are v1's UI,
// unchanged. Windows are opened from here (overlays per display, Settings, Agents, Ask, drop).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod agents;
mod apps;
mod brain;
mod cloud;
mod cursor;
mod demo;
mod dictation;
mod focus;
mod fullscreen;
mod i18n;
mod ipc;
mod knowledge;
mod notch;
mod notes;
mod package;
mod realtime;
mod reminders;
mod settings;
mod snap;
mod talk;
mod tray;
mod turn;
mod wake;
mod weather;
mod windows;

use std::sync::Mutex;

use aina_core::timers::Timers;
use aina_store::Store;
use aina_uia::Uia;
use tauri::Manager;

pub struct AppState {
    pub store: Store,
    pub overlays: Mutex<Vec<windows::Overlay>>,
    pub notch: Mutex<notch::Notch>,
    pub timers: Mutex<Timers>,
    pub talk: Mutex<talk::Talk>,
    pub turns: Mutex<turn::Turns>,
    pub uia: Uia,
    pub brain: Mutex<Option<std::sync::Arc<std::sync::atomic::AtomicBool>>>,
    pub bridge: aina_agent::bridge::Bridge,
    pub agents: Mutex<agents::Agents>,
    pub mic: aina_voice::mic::Mic,
    pub tts: aina_voice::tts::Tts,
    pub player: aina_voice::player::Player,
}

/// An installed aina has no console: its error lines go to aina.log next to config.json, so a beta tester
/// can send it (tray → Buka log). Only what the code prints without AINA_LOG lands there: errors, never
/// what was said. Started with a console or a redirect, stderr stays as it is.
#[cfg(windows)]
fn log_to_file() {
    use std::os::windows::io::IntoRawHandle;
    use ::windows::Win32::Foundation::HANDLE;
    use ::windows::Win32::System::Console::{GetStdHandle, STD_ERROR_HANDLE, SetStdHandle};
    if unsafe { GetStdHandle(STD_ERROR_HANDLE) }.is_ok_and(|h| !h.is_invalid() && !h.0.is_null()) {
        return;
    }
    let path = settings::log_path();
    let _ = std::fs::create_dir_all(path.parent().unwrap_or(std::path::Path::new(".")));
    // Kept small: a log over 1 MB starts over (the previous one stays as aina.old.log).
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 1_000_000) {
        let _ = std::fs::rename(&path, path.with_file_name("aina.old.log"));
    }
    let Ok(file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) else { return };
    let handle = HANDLE(file.into_raw_handle());
    let _ = unsafe { SetStdHandle(STD_ERROR_HANDLE, handle) };
    eprintln!("---- aina {} started {}", env!("CARGO_PKG_VERSION"), chrono::Local::now().format("%Y-%m-%d %H:%M:%S"));
}

#[cfg(not(windows))]
fn log_to_file() {}

fn main() {
    // Helper modes Claude Code starts during an agent task: its tool guard and aina's tools (MCP).
    match std::env::args().nth(1).as_deref() {
        Some("--agent-hook") => return aina_agent::engines::hook_main(),
        Some("--agent-mcp") => return aina_agent::engines::mcp_main(),
        _ => {}
    }
    log_to_file();
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        // A second launch opens Settings, as in v1; one opened by an aina:// link hands the link over,
        // and the installer's --setup-extension opens the extension section.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| match argv.iter().find(|a| a.starts_with(cloud::SCHEME)) {
            Some(link) => {
                let (a, link) = (app.clone(), link.clone());
                std::thread::spawn(move || cloud::handle_link(&a, &link));
            }
            None if argv.iter().any(|a| a == "--setup-extension") => settings::setup_extension(app),
            None => windows::open_settings(app),
        }))
        .invoke_handler(tauri::generate_handler![ipc::aina_invoke, ipc::aina_send, ipc::aina_ready])
        .setup(|app| {
            // The installer's shortcuts carry the identifier as their AppUserModelID; the process
            // declares the same one before its first window, so the taskbar groups the running aina
            // with its pinned shortcut under one aina button.
            // As an MSIX package the identity is the package's own.
            if !package::packaged() {
                let aumid = ::windows::core::HSTRING::from(app.config().identifier.as_str());
                let _ = unsafe { ::windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID(&aumid) };
            }
            let store = Store::open(aina_store::data_dir());
            // Provider keys live on aina Cloud only; an earlier build may have saved some on this PC.
            store.forget_provider_keys();
            let mut timers = Timers::new(&store.get("reminders"));
            notch::load_pomodoro(&mut timers, &store);
            // The notch's wave follows the microphone level while recording.
            let level_app = app.handle().clone();
            let mic = aina_voice::mic::Mic::new(move |level| {
                windows::broadcast(&level_app, "level", level.max(0.0));
                wake::on_level(&level_app, level);
            });
            // Nothing a page stored in an earlier session is kept (v1 privacy.js).
            windows::wipe_webview_data();
            app.manage(AppState {
                mic,
                tts: aina_voice::tts::Tts::default(),
                player: aina_voice::player::Player::default(),
                store,
                overlays: Mutex::new(Vec::new()),
                notch: Mutex::default(),
                timers: Mutex::new(timers),
                talk: Mutex::default(),
                turns: Mutex::default(),
                uia: Uia::new(),
                brain: Mutex::default(),
                bridge: brain::new_bridge(),
                agents: Mutex::default(),
            });
            let handle = app.handle().clone();
            windows::create_overlays(&handle)?;
            tray::create(&handle)?;
            cursor::start(handle.clone());
            talk::start(handle.clone());
            windows::watch_displays(handle.clone());
            notch::start_timers(handle.clone());
            reminders::start(handle.clone());
            weather::start(handle.clone());
            fullscreen::start(handle.clone());
            apps::start_media_watch(handle.clone());
            brain::start_bridge(&handle);
            agents::start_routines(handle.clone());
            cloud::start(&handle);
            #[cfg(windows)]
            cloud::register_scheme();
            // Started by an aina:// link (aina was not running yet).
            if let Some(link) = std::env::args().find(|a| a.starts_with(cloud::SCHEME)) {
                let a = handle.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    cloud::handle_link(&a, &link);
                });
            }
            wake::update(&handle);
            if let Ok(kind) = std::env::var("AINA_DEMO") {
                demo::start(handle, kind);
            } else {
                settings::on_start(&handle);
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building aina")
        .run(|_app, event| match event {
            // A tray app: closing Settings or Agents must not quit it.
            tauri::RunEvent::ExitRequested { api, code: None, .. } => api.prevent_exit(),
            tauri::RunEvent::Exit => {
                wake::stop();
                windows::wipe_webview_data();
            }
            _ => {}
        });
}
