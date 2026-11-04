// Cursor moves for the overlays. They are click-through, so their pages get no mouse events of their
// own; v1 forwarded them (setIgnoreMouseEvents forward). Here the global mouse hook (badi-input)
// reports moves, and the overlay under the cursor gets the position in its CSS px; shim.js turns it
// into the mousemove events the notch's hover code already listens to. Nothing runs while the mouse
// rests, and a page that said where it wants the cursor (cursor:zone: the notch, the Sprite cards)
// gets only the moves there, plus one move away when the cursor leaves, so the overlays stay asleep
// while the mouse works elsewhere.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use serde_json::json;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition};

use crate::AppState;

/// A demo drives the overlays' cursor itself (BADI_DEMO=sprites); the real mouse is not forwarded then.
pub static DEMO_HOLDS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Per overlay: each page part's rects (x, y, w, h in its CSS px), and whether the last move was in one.
struct Zones {
    parts: HashMap<String, Vec<[f64; 4]>>,
    inside: bool,
}

static ZONES: Mutex<Option<HashMap<String, Zones>>> = Mutex::new(None);

/// A page part (`{ part, rects }`) says where it wants the cursor; no rects: nowhere for now.
pub fn set_zone(label: &str, payload: &serde_json::Value) {
    let part = payload["part"].as_str().unwrap_or("").to_string();
    let rects: Vec<[f64; 4]> = payload["rects"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| {
            let r = r.as_array()?;
            Some([r.first()?.as_f64()?, r.get(1)?.as_f64()?, r.get(2)?.as_f64()?, r.get(3)?.as_f64()?])
        })
        .collect();
    let mut all = ZONES.lock().unwrap();
    let z = all.get_or_insert_with(HashMap::new).entry(label.to_string()).or_insert_with(|| Zones { parts: HashMap::new(), inside: true });
    z.parts.insert(part, rects);
}

/// Whether this move goes to the page: inside a zone, or the first move after leaving one (the page
/// closes its hover). A page that never said where it wants the cursor gets every move.
fn wanted(label: &str, x: f64, y: f64) -> bool {
    let mut all = ZONES.lock().unwrap();
    let Some(z) = all.as_mut().and_then(|m| m.get_mut(label)) else { return true };
    let inside = z.parts.values().flatten().any(|r| x >= r[0] && y >= r[1] && x <= r[0] + r[2] && y <= r[1] + r[3]);
    let send = inside || z.inside;
    z.inside = inside;
    send
}

/// The cursor went to another display: the overlay it left gets one move far away (hover off).
fn left(app: &AppHandle, label: &str) {
    let was = ZONES.lock().unwrap().as_mut().and_then(|m| m.get_mut(label)).map(|z| std::mem::replace(&mut z.inside, false));
    if was != Some(false) {
        let _ = app.emit_to(label, "badi:cursor", json!({ "x": -10000.0, "y": -10000.0 }));
    }
}

pub fn start(app: AppHandle) {
    let last: Mutex<Option<String>> = Mutex::new(None);
    badi_input::hook::watch_mouse(Duration::from_millis(16), move |x, y| {
        let p = PhysicalPosition::new(x as f64, y as f64);
        let state = app.state::<AppState>();
        let target = state.overlays.lock().unwrap().iter().find(|o| o.contains(p)).map(|o| (o.label.clone(), o.local(p)));
        if let Some((label, (x, y))) = target.filter(|_| !DEMO_HOLDS.load(std::sync::atomic::Ordering::Relaxed)) {
            let prev = last.lock().unwrap().replace(label.clone());
            if let Some(prev) = prev.filter(|p| *p != label) {
                left(&app, &prev);
            }
            if wanted(&label, x, y) {
                let _ = app.emit_to(label.as_str(), "badi:cursor", json!({ "x": x, "y": y }));
            }
        }
        // While the talk keys are held the trail is also ink.
        crate::talk::on_move(&app, p.x as i32, p.y as i32);
        // Files dragged toward the notch: it opens to take them.
        crate::notch::watch_drag(&app, p.x as i32, p.y as i32);
    });
}
