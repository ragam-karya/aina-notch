// The tray icon and its menu, same items as v1.

use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use crate::windows;

// aina's logo mark (icons/64x64.png, made by `npm run icon`); Windows scales it to the tray's size.
fn logo_icon() -> Image<'static> {
    Image::from_bytes(include_bytes!("../icons/64x64.png")).expect("icons/64x64.png is a PNG")
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let menu = Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, "ask", "Ketik pertanyaan…", true, None::<&str>)?,
            &MenuItem::with_id(app, "agents", "Sprites…", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?,
            &MenuItem::with_id(app, "stop", "Hentikan jawaban (Esc)", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            // For beta testers: what to send when something goes wrong, and where.
            &MenuItem::with_id(app, "feedback", "Kirim masukan…", true, None::<&str>)?,
            &MenuItem::with_id(app, "log", "Buka log", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "quit", "Keluar", true, None::<&str>)?,
        ],
    )?;
    TrayIconBuilder::with_id("badi")
        .icon(logo_icon())
        .tooltip("aina")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "ask" => windows::open_ask(app),
            "agents" => windows::open_agents(app, None),
            "settings" => windows::open_settings(app),
            // The same as Esc: the task, the voice, a walkthrough and what is on screen all stop.
            "stop" => {
                crate::brain::stop(app);
                crate::notch::answer(app, crate::notch::Answer { approved: false, text: String::new() });
                crate::talk::cancel(app);
            }
            "feedback" => {
                let url = app.state::<crate::AppState>().store.get("feedbackUrl").as_str().unwrap_or("").trim().to_string();
                // From Settings' config, else the link the beta build was made with (BADI_FEEDBACK_URL).
                let url = if url.starts_with("https://") { url } else { option_env!("BADI_FEEDBACK_URL").unwrap_or("").to_string() };
                if url.starts_with("https://") {
                    let _ = badi_agent::apps::shell_open(&url);
                } else {
                    crate::notch::show_notice(app, "info", "Link masukan belum diatur di build ini.", &[], vec![], 5000);
                }
            }
            "log" => {
                let path = crate::settings::log_path();
                if !path.exists() {
                    let _ = std::fs::write(&path, "");
                }
                let _ = std::process::Command::new("notepad.exe").arg(path).spawn();
            }
            "quit" => {
                crate::settings::on_quit(app);
                app.exit(0)
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                windows::open_ask(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}
