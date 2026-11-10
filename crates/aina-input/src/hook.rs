// The global keyboard and mouse, through low-level hooks (WH_KEYBOARD_LL, WH_MOUSE_LL) on their own
// thread, like v1's uiohook. Event-driven: nothing runs while the user is idle, which keeps aina at
// 0 % CPU. The callbacks only queue what happened; consumers run on their own threads. Hooks never
// swallow input.
//
// Two filters v1 did not have:
//   - Input aina injects itself (send.rs) carries AINA_TAG in dwExtraInfo and is skipped, so an
//     injected Escape can't stop the task that typed it (v1 muted everything for a while instead;
//     mute() is kept for parity).
//   - AltGr sends a fake left Ctrl (scan code with 0x200) before right Alt; it is dropped, so typing
//     €, ą or @ on European layouts never looks like Ctrl+Alt.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, SyncSender, channel, sync_channel};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, KBDLLHOOKSTRUCT, MSG, MSLLHOOKSTRUCT, SetWindowsHookExW, TranslateMessage, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN, WM_KEYUP,
    WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_MOUSEMOVE, WM_RBUTTONDOWN, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

/// Marks input aina injects (SendInput dwExtraInfo), so the hooks can tell it from the user's.
pub const AINA_TAG: usize = 0x00BA_D1AD;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Input {
    Key { vk: u16, down: bool },
    MouseDown { x: i32, y: i32, button: u8 }, // 1 left, 2 right, 3 middle (uiohook's numbering)
}

// The latest cursor position, physical desktop px, packed as (x << 32) | y.
static LAST: AtomicU64 = AtomicU64::new(u64::MAX);
static MOVED: OnceLock<SyncSender<()>> = OnceLock::new();
static EVENTS: OnceLock<std::sync::Mutex<Sender<Input>>> = OnceLock::new();
static MUTED_UNTIL: AtomicI64 = AtomicI64::new(0);
static STARTED: OnceLock<()> = OnceLock::new();

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// Ignores the user's input for a while (v1 hotkeys.mute).
pub fn mute(ms: u64) {
    MUTED_UNTIL.fetch_max(now_ms() + ms as i64, Ordering::Relaxed);
}

pub fn muted() -> bool {
    now_ms() < MUTED_UNTIL.load(Ordering::Relaxed)
}

fn pack(x: i32, y: i32) -> u64 {
    ((x as u32 as u64) << 32) | (y as u32 as u64)
}

fn unpack(v: u64) -> (i32, i32) {
    ((v >> 32) as u32 as i32, v as u32 as i32)
}

/// The cursor as the hook last saw it (physical px), if it has moved since start.
pub fn last_cursor() -> Option<(i32, i32)> {
    let v = LAST.load(Ordering::Relaxed);
    (v != u64::MAX).then(|| unpack(v))
}

fn queue(input: Input) {
    if let Some(tx) = EVENTS.get() {
        let _ = tx.lock().unwrap().send(input);
    }
}

unsafe extern "system" fn on_mouse(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let info = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
        if info.dwExtraInfo != AINA_TAG {
            let (x, y) = (info.pt.x, info.pt.y);
            match wparam.0 as u32 {
                WM_MOUSEMOVE => {
                    LAST.store(pack(x, y), Ordering::Relaxed);
                    if let Some(tx) = MOVED.get() {
                        // A wake-up already queued is enough: the consumer reads the latest position.
                        let _ = tx.try_send(());
                    }
                }
                WM_LBUTTONDOWN if !muted() => queue(Input::MouseDown { x, y, button: 1 }),
                WM_RBUTTONDOWN if !muted() => queue(Input::MouseDown { x, y, button: 2 }),
                WM_MBUTTONDOWN if !muted() => queue(Input::MouseDown { x, y, button: 3 }),
                _ => {}
            }
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

unsafe extern "system" fn on_key(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let altgr_ctrl = info.vkCode == 0xa2 && info.scanCode & 0x200 != 0;
        if info.dwExtraInfo != AINA_TAG && !altgr_ctrl {
            let vk = info.vkCode as u16;
            match wparam.0 as u32 {
                // Key-ups always pass, so a key released while muted is never stuck as held.
                WM_KEYDOWN | WM_SYSKEYDOWN if !muted() => queue(Input::Key { vk, down: true }),
                WM_KEYUP | WM_SYSKEYUP => queue(Input::Key { vk, down: false }),
                _ => {}
            }
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// Installs both hooks once. Returns the queue of key and click events to the first caller that asks
/// for it; watch_mouse installs the hooks without taking it, so the hotkeys still get it after.
pub fn start() -> Option<Receiver<Input>> {
    install();
    RX.lock().unwrap().take()
}

static RX: std::sync::Mutex<Option<Receiver<Input>>> = std::sync::Mutex::new(None);

fn install() {
    STARTED.get_or_init(|| {
        let (tx, rx) = channel();
        let _ = EVENTS.set(std::sync::Mutex::new(tx));
        *RX.lock().unwrap() = Some(rx);
        std::thread::spawn(|| unsafe {
            // LL hooks are called on the installing thread, which must pump messages.
            let mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(on_mouse), None, 0);
            let keys = SetWindowsHookExW(WH_KEYBOARD_LL, Some(on_key), None, 0);
            if mouse.is_err() && keys.is_err() {
                return;
            }
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        });
    });
}

/// Calls `on_move(x, y)` with the cursor in physical desktop px, at most once per `frame` while the
/// mouse moves, never while it rests. Starts the hooks if needed.
pub fn watch_mouse(frame: Duration, on_move: impl Fn(i32, i32) + Send + 'static) {
    let (tx, rx): (SyncSender<()>, Receiver<()>) = sync_channel(1);
    if MOVED.set(tx).is_err() {
        return; // already watched
    }
    install();
    std::thread::spawn(move || {
        let mut sent = u64::MAX;
        while rx.recv().is_ok() {
            let now = LAST.load(Ordering::Relaxed);
            if now != sent {
                sent = now;
                let (x, y) = unpack(now);
                on_move(x, y);
            }
            std::thread::sleep(frame);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_pack_including_negative_monitors() {
        for (x, y) in [(0, 0), (1919, 1079), (-1920, -200), (5120, -1440)] {
            assert_eq!(unpack(pack(x, y)), (x, y));
        }
    }

    #[test]
    fn mute_lasts_its_time() {
        mute(200);
        assert!(muted());
        MUTED_UNTIL.store(0, Ordering::Relaxed);
        assert!(!muted());
    }
}
