// Real mouse and keyboard input on Windows (SendInput / SetCursorPos). Badi is per-monitor DPI aware,
// so every coordinate here is in physical screen pixels. Everything sent carries hook::BADI_TAG, so
// Badi's own input never triggers its hotkeys. Blocking (small sleeps between steps): call it from a
// worker thread. Port of src/brain/input.js.

use std::thread::sleep;
use std::time::Duration;

use windows::Win32::Foundation::POINT;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MOUSE_EVENT_FLAGS, MOUSEEVENTF_HWHEEL,
    MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL, MOUSEINPUT, SendInput,
    VIRTUAL_KEY, VkKeyScanW,
};
use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, SetCursorPos};

use crate::hook::BADI_TAG;
use crate::keys::{Combo, Key, is_extended};

const WHEEL_DELTA: i32 = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
    Middle,
}

impl Button {
    pub fn parse(s: &str) -> Button {
        match s {
            "right" => Button::Right,
            "middle" => Button::Middle,
            _ => Button::Left,
        }
    }

    fn flags(self) -> (MOUSE_EVENT_FLAGS, MOUSE_EVENT_FLAGS) {
        match self {
            Button::Left => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
            Button::Right => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
            Button::Middle => (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP),
        }
    }
}

fn send(inputs: &[INPUT]) -> Result<(), String> {
    let sent = unsafe { SendInput(inputs, size_of::<INPUT>() as i32) };
    // Windows silently drops input aimed at a window of higher integrity (e.g. an admin app).
    if sent as usize != inputs.len() {
        return Err("Windows blocked the input (the app may be running as administrator).".into());
    }
    Ok(())
}

fn mouse(flags: MOUSE_EVENT_FLAGS, data: i32) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 { mi: MOUSEINPUT { dx: 0, dy: 0, mouseData: data as u32, dwFlags: flags, time: 0, dwExtraInfo: BADI_TAG } },
    }
}

fn key(vk: u16, up: bool) -> INPUT {
    let mut flags = KEYBD_EVENT_FLAGS(0);
    if up {
        flags |= KEYEVENTF_KEYUP;
    }
    if is_extended(vk) {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VIRTUAL_KEY(vk), wScan: 0, dwFlags: flags, time: 0, dwExtraInfo: BADI_TAG } } }
}

fn unicode(unit: u16, up: bool) -> INPUT {
    let flags = if up { KEYEVENTF_UNICODE | KEYEVENTF_KEYUP } else { KEYEVENTF_UNICODE };
    INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VIRTUAL_KEY(0), wScan: unit, dwFlags: flags, time: 0, dwExtraInfo: BADI_TAG } } }
}

/// The left mouse button is held right now (a drag in progress, in any app).
pub fn left_button_down() -> bool {
    unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(windows::Win32::UI::Input::KeyboardAndMouse::VK_LBUTTON.0 as i32) < 0 }
}

pub fn cursor() -> (i32, i32) {
    let mut p = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut p);
    }
    (p.x, p.y)
}

pub fn move_to(x: i32, y: i32) -> Result<(), String> {
    unsafe { SetCursorPos(x, y) }.map_err(|_| "Could not move the mouse.".to_string())
}

/// Moves in small steps so apps that track hover and drags see the motion.
pub fn glide_to(x: i32, y: i32, steps: u32) -> Result<(), String> {
    let (fx, fy) = cursor();
    for i in 1..=steps.max(1) {
        let t = i as f64 / steps.max(1) as f64;
        move_to((fx as f64 + (x - fx) as f64 * t).round() as i32, (fy as f64 + (y - fy) as f64 * t).round() as i32)?;
        sleep(Duration::from_millis(8));
    }
    Ok(())
}

// Runs `f` with modifiers held down, releasing them even when it fails.
fn hold<T>(mods: &[u16], f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    if !mods.is_empty() {
        send(&mods.iter().map(|m| key(*m, false)).collect::<Vec<_>>())?;
    }
    let out = f();
    if !mods.is_empty() {
        send(&mods.iter().rev().map(|m| key(*m, true)).collect::<Vec<_>>())?;
    }
    out
}

pub fn click(at: Option<(i32, i32)>, button: Button, count: u32, mods: &[u16]) -> Result<(), String> {
    let (down, up) = button.flags();
    hold(mods, || {
        if let Some((x, y)) = at {
            glide_to(x, y, 8)?;
        }
        for i in 0..count.max(1) {
            send(&[mouse(down, 0), mouse(up, 0)])?;
            if i + 1 < count {
                sleep(Duration::from_millis(40));
            }
        }
        Ok(())
    })
}

pub fn button_down(button: Button) -> Result<(), String> {
    send(&[mouse(button.flags().0, 0)])
}

pub fn button_up(button: Button) -> Result<(), String> {
    send(&[mouse(button.flags().1, 0)])
}

pub fn drag(from: (i32, i32), to: (i32, i32), mods: &[u16]) -> Result<(), String> {
    hold(mods, || {
        glide_to(from.0, from.1, 8)?;
        button_down(Button::Left)?;
        sleep(Duration::from_millis(60));
        glide_to(to.0, to.1, 20)?;
        sleep(Duration::from_millis(60));
        button_up(Button::Left)
    })
}

pub fn scroll(at: Option<(i32, i32)>, direction: &str, amount: u32, mods: &[u16]) -> Result<(), String> {
    hold(mods, || {
        if let Some((x, y)) = at {
            glide_to(x, y, 4)?;
        }
        let horizontal = matches!(direction, "left" | "right");
        let sign = if matches!(direction, "up" | "right") { 1 } else { -1 };
        for _ in 0..amount.clamp(1, 50) {
            send(&[mouse(if horizontal { MOUSEEVENTF_HWHEEL } else { MOUSEEVENTF_WHEEL }, sign * WHEEL_DELTA)])?;
            sleep(Duration::from_millis(15));
        }
        Ok(())
    })
}

/// Presses a parsed combo `repeat` times.
pub fn press_combo(combo: &Combo, repeat: u32) -> Result<(), String> {
    for _ in 0..repeat.max(1) {
        let (vk, extra) = match &combo.key {
            Key::Vk(vk) => (*vk, vec![]),
            Key::Char(ch) if combo.mods.is_empty() => {
                type_text(ch)?;
                continue;
            }
            Key::Char(ch) => {
                // A symbol in a shortcut (ctrl+/): find the key that produces it on this keyboard layout.
                let unit = ch.encode_utf16().next().unwrap_or(0);
                let scan = unsafe { VkKeyScanW(unit) };
                if scan == -1 {
                    return Err(format!("key \"{ch}\" is not on this keyboard layout."));
                }
                (scan as u16 & 0xff, if scan as u16 & 0x100 != 0 { vec![0x10] } else { vec![] })
            }
        };
        let all: Vec<u16> = combo.mods.iter().chain(extra.iter()).copied().collect();
        let mut seq: Vec<INPUT> = all.iter().map(|m| key(*m, false)).collect();
        seq.push(key(vk, false));
        seq.push(key(vk, true));
        seq.extend(all.iter().rev().map(|m| key(*m, true)));
        send(&seq)?;
        if repeat > 1 {
            sleep(Duration::from_millis(25));
        }
    }
    Ok(())
}

pub fn hold_key(combo: &Combo, seconds: f64) -> Result<(), String> {
    let mut vks = combo.mods.clone();
    if let Key::Vk(vk) = combo.key {
        vks.push(vk);
    }
    send(&vks.iter().map(|v| key(*v, false)).collect::<Vec<_>>())?;
    sleep(Duration::from_secs_f64(seconds.clamp(0.0, 300.0)));
    send(&vks.iter().rev().map(|v| key(*v, true)).collect::<Vec<_>>())
}

/// Types literal text as Unicode keystrokes, so Indonesian text and emoji work without the clipboard.
pub fn type_text(text: &str) -> Result<(), String> {
    for ch in text.chars() {
        match ch {
            '\r' => continue,
            '\n' => send(&[key(0x0d, false), key(0x0d, true)])?,
            '\t' => send(&[key(0x09, false), key(0x09, true)])?,
            _ => {
                let mut buf = [0u16; 2];
                let units = ch.encode_utf16(&mut buf);
                let mut seq: Vec<INPUT> = units.iter().map(|u| unicode(*u, false)).collect();
                seq.extend(units.iter().map(|u| unicode(*u, true)));
                send(&seq)?;
            }
        }
        sleep(Duration::from_millis(4));
    }
    Ok(())
}
