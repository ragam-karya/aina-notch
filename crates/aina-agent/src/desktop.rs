// Executor for the computer toolset (computer_toolset_20260801) on one real Windows display. Claude's
// coordinates are in the pixel space of the screenshots returned here. Port of src/brain/desktop.js.

use std::sync::Mutex;
use std::thread::sleep;
use std::time::Duration;

use aina_input::hook;
use aina_input::keys::{parse_combo, parse_modifiers};
use aina_input::send::{self, Button};
use aina_screen::Display;
use aina_screen::capture::jpeg;
use aina_screen::coords::{Point, SHOT_MAX_EDGE, Size, check_point, dip_to_shot, shot_size, zoom_rect};
use aina_screen::grab::grab;
use image::imageops::{self, FilterType};
use image::{ImageBuffer, RgbaImage};
use serde_json::{Value, json};

pub const MEMBERS: [&str; 17] = [
    "screenshot", "zoom", "left_click", "right_click", "middle_click", "double_click", "triple_click", "left_click_drag", "mouse_move", "left_mouse_down",
    "left_mouse_up", "cursor_position", "scroll", "type", "key", "hold_key", "wait",
];

/// Shows where aina is about to act: a physical desktop point and a kind (click, drag, move, scroll, type).
pub type OnAction = Box<dyn Fn(i32, i32, &str) + Send + Sync>;

pub struct Desktop {
    display: Display,
    shot: Size,
    full: Mutex<Option<RgbaImage>>, // last full-resolution capture, for zoom
    many: bool,
    on_action: OnAction,
}

fn ok() -> Vec<Value> {
    vec![json!({ "type": "text", "text": "OK" })]
}

fn image_block(img: &RgbaImage) -> Result<Value, String> {
    Ok(json!({ "type": "image", "source": { "type": "base64", "media_type": "image/jpeg", "data": jpeg(img, 82)? } }))
}

fn coord(input: &Value, key: &str) -> Option<Vec<f64>> {
    input.get(key)?.as_array().map(|a| a.iter().map(|v| v.as_f64().unwrap_or(f64::NAN)).collect())
}

impl Desktop {
    /// `many`: the PC has more than one display (Claude is told it only sees this one).
    pub fn new(display: Display, many: bool, on_action: OnAction) -> Desktop {
        let shot = shot_size(&display.bounds, SHOT_MAX_EDGE);
        Desktop { display, shot, full: Mutex::new(None), many, on_action }
    }

    pub fn describe(&self) -> String {
        let other = if self.many { " the user was looking at (other monitors are not visible to you)" } else { "" };
        format!("{}x{} screenshot of the display{other}", self.shot.width, self.shot.height)
    }

    fn capture(&self) -> Result<RgbaImage, String> {
        let (x, y, w, h) = self.display.phys;
        let px = grab(x, y, w as i32, h as i32).map_err(|_| "Failed to capture screenshot. The display may be locked or unavailable.".to_string())?;
        let img: RgbaImage = ImageBuffer::from_raw(w, h, px).ok_or("Failed to capture screenshot.")?;
        *self.full.lock().unwrap() = Some(img.clone());
        Ok(img)
    }

    pub fn screenshot_block(&self) -> Result<Value, String> {
        let full = self.capture()?;
        image_block(&imageops::resize(&full, self.shot.width as u32, self.shot.height as u32, FilterType::Triangle))
    }

    // Screenshot px -> physical desktop px.
    fn physical(&self, c: Option<Vec<f64>>, what: &str) -> Result<(i32, i32), String> {
        let p = check_point(&c.unwrap_or_default(), self.shot, what)?;
        let (px, py, pw, ph) = self.display.phys;
        Ok(((px as f64 + p.x * pw as f64 / self.shot.width).round() as i32, (py as f64 + p.y * ph as f64 / self.shot.height).round() as i32))
    }

    // aina's cursor flies to the spot just before acting there.
    fn announce(&self, at: (i32, i32), kind: &str) {
        (self.on_action)(at.0, at.1, kind);
        sleep(Duration::from_millis(220));
    }

    pub fn run(&self, name: &str, input: &Value) -> Result<Vec<Value>, String> {
        if !MEMBERS.contains(&name) {
            return Err(format!("Unknown computer action \"{name}\"."));
        }
        let text = input["text"].as_str();
        // Only injected key presses (and modifier-held clicks) can look like aina's own hotkeys; typed
        // text goes in as Unicode events that never match, so the user's Esc keeps working while typing.
        if name == "key" || (text.is_some() && name != "type") {
            hook::mute(1500);
        }
        let mods = || parse_modifiers(if name == "type" || name == "key" || name == "hold_key" { None } else { text });
        match name {
            "screenshot" => Ok(vec![self.screenshot_block()?]),
            "zoom" => {
                let full = match self.full.lock().unwrap().clone() {
                    Some(f) => f,
                    None => self.capture()?,
                };
                let size = Size { width: full.width() as f64, height: full.height() as f64 };
                let (x, y, w, h) = zoom_rect(&coord(input, "region").unwrap_or_default(), self.shot, size)?;
                let crop = imageops::crop_imm(&full, x, y, w, h).to_image();
                let scale = (self.shot.width / w as f64).min(self.shot.height / h as f64);
                let img = imageops::resize(&crop, (w as f64 * scale).round() as u32, (h as f64 * scale).round() as u32, FilterType::CatmullRom);
                Ok(vec![image_block(&img)?])
            }
            "left_click" | "right_click" | "middle_click" | "double_click" | "triple_click" => {
                let (button, count) = match name {
                    "right_click" => (Button::Right, 1),
                    "middle_click" => (Button::Middle, 1),
                    "double_click" => (Button::Left, 2),
                    "triple_click" => (Button::Left, 3),
                    _ => (Button::Left, 1),
                };
                let mods = mods()?;
                let target = match coord(input, "coordinate") {
                    Some(c) => {
                        let at = self.physical(Some(c), "coordinate")?;
                        self.announce(at, "click");
                        Some(at)
                    }
                    None => None,
                };
                send::click(target, button, count, &mods)?;
                Ok(ok())
            }
            "left_click_drag" => {
                let a = self.physical(coord(input, "start_coordinate"), "start_coordinate")?;
                let b = self.physical(coord(input, "coordinate"), "coordinate")?;
                let mods = mods()?;
                self.announce(a, "drag");
                send::drag(a, b, &mods)?;
                Ok(ok())
            }
            "mouse_move" => {
                let at = self.physical(coord(input, "coordinate"), "coordinate")?;
                (self.on_action)(at.0, at.1, "move");
                send::glide_to(at.0, at.1, 12)?;
                Ok(ok())
            }
            "left_mouse_down" => send::button_down(Button::Left).map(|_| ok()),
            "left_mouse_up" => send::button_up(Button::Left).map(|_| ok()),
            "cursor_position" => {
                let (cx, cy) = send::cursor();
                let (px, py, pw, ph) = self.display.phys;
                let b = self.display.bounds;
                // Physical -> the overlay space -> screenshot px.
                let dip = Point { x: b.x + (cx - px) as f64 * b.width / pw as f64, y: b.y + (cy - py) as f64 * b.height / ph as f64 };
                let p = dip_to_shot(dip, self.shot, &b);
                Ok(vec![json!({ "type": "text", "text": format!("X={}, Y={}", p.x, p.y) })])
            }
            "scroll" => {
                let dir = input["scroll_direction"].as_str().unwrap_or("");
                if !["up", "down", "left", "right"].contains(&dir) {
                    return Err("scroll_direction must be up, down, left or right.".into());
                }
                let mods = mods()?;
                let target = match coord(input, "coordinate") {
                    Some(c) => {
                        let at = self.physical(Some(c), "coordinate")?;
                        (self.on_action)(at.0, at.1, "scroll");
                        Some(at)
                    }
                    None => None,
                };
                let amount = input["scroll_amount"].as_u64().filter(|&n| n > 0).unwrap_or(3) as u32;
                send::scroll(target, dir, amount, &mods)?;
                Ok(ok())
            }
            "type" => {
                let t = text.ok_or("type: \"text\" is required.")?;
                send::type_text(t)?;
                Ok(ok())
            }
            "key" => {
                let repeat = input["repeat"].as_u64().unwrap_or(1).clamp(1, 100) as u32;
                send::press_combo(&parse_combo(text.unwrap_or(""))?, repeat)?;
                Ok(ok())
            }
            "hold_key" => {
                let seconds = input["duration"].as_f64().filter(|&s| s > 0.0).unwrap_or(1.0);
                hook::mute((seconds.min(300.0) * 1000.0) as u64 + 1500);
                send::hold_key(&parse_combo(text.unwrap_or(""))?, seconds)?;
                Ok(ok())
            }
            _ => {
                // wait
                let seconds = input["duration"].as_f64().unwrap_or(1.0).clamp(0.0, 300.0);
                sleep(Duration::from_secs_f64(seconds));
                Ok(ok())
            }
        }
    }
}
