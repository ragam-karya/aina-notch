// A window that takes the whole screen: while the window in front is maximized, or full screen (a
// game, a video, a presentation: covering its whole monitor without a title bar), that display's notch
// hides and comes back only when the cursor rests at the very top for a second (island.js). Normal
// windows leave it as it is. Checked once a second; each overlay is told only when its own state changes.

use std::time::Duration;

use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};

use crate::AppState;

/// The monitor (physical px: x, y, width, height) the window in front fills (maximized or full
/// screen), if it fills one.
#[cfg(windows)]
fn covered_monitor() -> Option<(i32, i32, i32, i32)> {
    use ::windows::Win32::Foundation::RECT;
    use ::windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow};
    use ::windows::Win32::UI::WindowsAndMessaging::{GWL_STYLE, GetClassNameW, GetForegroundWindow, GetWindowLongW, GetWindowRect, GetWindowThreadProcessId, IsIconic, IsZoomed, WS_CAPTION};
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() || IsIconic(hwnd).as_bool() {
            return None;
        }
        let maximized = IsZoomed(hwnd).as_bool();
        let titled = GetWindowLongW(hwnd, GWL_STYLE) as u32 & WS_CAPTION.0 == WS_CAPTION.0;
        // aina's own windows, and the desktop and taskbar, never count.
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == std::process::id() {
            return None;
        }
        let mut class = [0u16; 64];
        let n = GetClassNameW(hwnd, &mut class) as usize;
        let class = String::from_utf16_lossy(&class[..n.min(class.len())]);
        if ["Progman", "WorkerW", "Shell_TrayWnd", "Shell_SecondaryTrayWnd"].contains(&class.as_str()) {
            return None;
        }
        let mut r = RECT::default();
        GetWindowRect(hwnd, &mut r).ok()?;
        let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(mon, &mut info).as_bool() {
            return None;
        }
        let m = info.rcMonitor;
        // Full screen: the whole monitor, no title bar (a normal window stretched that far still has one).
        let full = r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom && !titled;
        (maximized || full).then_some((m.left, m.top, m.right - m.left, m.bottom - m.top))
    }
}

#[cfg(not(windows))]
fn covered_monitor() -> Option<(i32, i32, i32, i32)> {
    None
}

/// Watches the window in front once a second and tells each overlay whether its display is covered.
pub fn start(app: AppHandle) {
    // A demo drives the notch itself (AINA_DEMO=fullscreen tells it directly); the real window in front
    // would only get in its way.
    if std::env::var_os("AINA_DEMO").is_some() {
        return;
    }
    std::thread::spawn(move || {
        let mut last: Vec<(String, bool)> = Vec::new();
        loop {
            std::thread::sleep(Duration::from_secs(1));
            let covered = covered_monitor();
            let now: Vec<(String, bool)> = {
                let st = app.state::<AppState>();
                let overlays = st.overlays.lock().unwrap();
                overlays
                    .iter()
                    .map(|o| {
                        let on = covered.is_some_and(|(x, y, w, h)| x == o.pos.x && y == o.pos.y && w == o.size.width as i32 && h == o.size.height as i32);
                        (o.label.clone(), on)
                    })
                    .collect()
            };
            for (label, on) in &now {
                if !last.iter().any(|(l, v)| l == label && v == on) {
                    if std::env::var_os("AINA_LOG").is_some() {
                        eprintln!("[fullscreen] {label}: {on}");
                    }
                    let _ = app.emit_to(label.as_str(), "fullscreen", json!(on));
                }
            }
            last = now;
        }
    });
}
