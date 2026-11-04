// What the user's mouse movement means while they talk (no click needed): a circle around something,
// a line under it, or pointing at it by holding still. Works on the cursor trail in overlay coordinates,
// [{ x, y, t }] with t in ms. Pure. Port of src/gesture.js, same thresholds and same output shapes.

use std::f64::consts::PI;

use serde::Serialize;

pub const DWELL_MS: f64 = 600.0; // holding still this long counts as pointing
const DWELL_RADIUS: f64 = 6.0; // px the cursor may drift while "still"
const MIN_CIRCLE: f64 = 24.0; // px: smaller loops are jitter
const CLOSE_RATIO: f64 = 0.35; // a loop must end within this share of its size from where it started
const MIN_SWEEP: f64 = (300.0 / 360.0) * 2.0 * PI; // and turn at least 300 degrees around its center

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrailPoint {
    pub x: f64,
    pub y: f64,
    pub t: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Box2 {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
}

/// A mark, serialized as v1's objects ({ type: 'circle', x1, … }, { type: 'point', x, y, heldMs }).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Mark {
    Circle {
        #[serde(flatten)]
        b: Box2,
    },
    Underline {
        #[serde(flatten)]
        b: Box2,
    },
    Point {
        x: f64,
        y: f64,
        #[serde(rename = "heldMs")]
        held_ms: f64,
    },
}

pub fn bbox(points: &[TrailPoint]) -> Box2 {
    points.iter().fold(Box2 { x1: f64::INFINITY, y1: f64::INFINITY, x2: f64::NEG_INFINITY, y2: f64::NEG_INFINITY }, |b, p| Box2 {
        x1: b.x1.min(p.x),
        y1: b.y1.min(p.y),
        x2: b.x2.max(p.x),
        y2: b.y2.max(p.y),
    })
}

// Total angle the path turns around its own center (radians, absolute).
fn sweep(points: &[TrailPoint], cx: f64, cy: f64) -> f64 {
    let mut total = 0.0;
    let mut prev = (points[0].y - cy).atan2(points[0].x - cx);
    for p in &points[1..] {
        let a = (p.y - cy).atan2(p.x - cx);
        let mut d = a - prev;
        if d > PI {
            d -= 2.0 * PI;
        }
        if d < -PI {
            d += 2.0 * PI;
        }
        total += d;
        prev = a;
    }
    total.abs()
}

/// A closed loop around something.
pub fn circle_of(points: &[TrailPoint]) -> Option<Mark> {
    if points.len() < 8 {
        return None;
    }
    let b = bbox(points);
    let (w, h) = (b.x2 - b.x1, b.y2 - b.y1);
    let size = w.max(h);
    if size < MIN_CIRCLE || w.min(h) < size * 0.25 {
        return None; // too small or too flat
    }
    let (first, last) = (points[0], points[points.len() - 1]);
    if (last.x - first.x).hypot(last.y - first.y) > size * CLOSE_RATIO {
        return None;
    }
    if sweep(points, (b.x1 + b.x2) / 2.0, (b.y1 + b.y2) / 2.0) < MIN_SWEEP {
        return None;
    }
    Some(Mark::Circle { b })
}

/// A mostly straight, flat stroke under or through something.
pub fn underline_of(points: &[TrailPoint]) -> Option<Mark> {
    if points.len() < 4 {
        return None;
    }
    let b = bbox(points);
    let (w, h) = (b.x2 - b.x1, b.y2 - b.y1);
    if w < 40.0 || h > 18f64.max(w * 0.15) {
        return None;
    }
    // It should travel mostly one way, not scribble back and forth.
    let path: f64 = points.windows(2).map(|p| (p[1].x - p[0].x).hypot(p[1].y - p[0].y)).sum();
    if path > w * 1.6 {
        return None;
    }
    Some(Mark::Underline { b })
}

/// Pointing: the cursor stayed within DWELL_RADIUS for DWELL_MS up to `now`. The trail may end before
/// `now` (no moves while still).
pub fn dwell_of(points: &[TrailPoint], now: f64) -> Option<Mark> {
    let last = *points.last()?;
    let mut since = last.t;
    for p in points.iter().rev() {
        if (p.x - last.x).hypot(p.y - last.y) > DWELL_RADIUS {
            break;
        }
        since = p.t;
    }
    let held_ms = now - since;
    (held_ms >= DWELL_MS).then_some(Mark::Point { x: last.x, y: last.y, held_ms })
}

/// The region a mark covers, for the crop: circles and underlines as drawn (an underline gets some room
/// above it, where the text it underlines is), a point as a box around it.
pub fn region_of(mark: &Mark) -> Box2 {
    match *mark {
        Mark::Point { x, y, .. } => Box2 { x1: x - 80.0, y1: y - 50.0, x2: x + 80.0, y2: y + 50.0 },
        Mark::Underline { b } => Box2 { x1: b.x1, y1: b.y1 - 40.0, x2: b.x2, y2: b.y2 + 8.0 },
        Mark::Circle { b } => b,
    }
}

// JS Math.round: halves go up.
fn js_round(v: f64) -> f64 {
    (v + 0.5).floor()
}

/// One line for the model, in screenshot pixels: `b` is the mark mapped onto screenshot `screen`.
pub fn describe(mark: &Mark, b: Box2, screen: u32) -> String {
    match *mark {
        Mark::Point { held_ms, .. } => {
            let (x, y) = (js_round((b.x1 + b.x2) / 2.0), js_round((b.y1 + b.y2) / 2.0));
            format!("The user pointed at {x},{y} on screen{screen} (held still {:.1} s).", held_ms / 1000.0)
        }
        Mark::Underline { .. } => format!("The user underlined {},{} to {},{} on screen{screen}.", b.x1, b.y1, b.x2, b.y2),
        Mark::Circle { .. } => format!("The user circled {},{} to {},{} on screen{screen}.", b.x1, b.y1, b.x2, b.y2),
    }
}

// Same cases as test/gesture.test.js.
#[cfg(test)]
mod tests {
    use super::*;

    fn trail(f: impl Fn(f64) -> (f64, f64)) -> Vec<TrailPoint> {
        let n = 40;
        (0..=n)
            .map(|i| {
                let (x, y) = f(i as f64 / n as f64);
                TrailPoint { x, y, t: i as f64 * 16.0 }
            })
            .collect()
    }

    fn lp(cx: f64, cy: f64, rx: f64, ry: f64, turns: f64) -> Vec<TrailPoint> {
        trail(|u| (cx + rx * (u * turns * 2.0 * PI).cos(), cy + ry * (u * turns * 2.0 * PI).sin()))
    }

    fn near(a: Box2, b: Box2) -> bool {
        [(a.x1, b.x1), (a.y1, b.y1), (a.x2, b.x2), (a.y2, b.y2)].iter().all(|(p, q)| (p - q).abs() < 1e-9)
    }

    #[test]
    fn a_closed_loop_is_a_circle_an_open_arc_is_not() {
        let Some(Mark::Circle { b }) = circle_of(&lp(300.0, 200.0, 80.0, 50.0, 1.0)) else { panic!() };
        assert!(near(b, Box2 { x1: 220.0, y1: 150.0, x2: 380.0, y2: 250.0 }));
        assert_eq!(circle_of(&lp(300.0, 200.0, 80.0, 50.0, 0.6)), None); // ~216 degrees, not closed
        assert_eq!(circle_of(&lp(300.0, 200.0, 6.0, 6.0, 1.0)), None); // jitter-sized
    }

    // The loop-closing scan the app runs on every move (v1 onInkMove), on a drawn ellipse.
    #[test]
    fn a_circle_drawn_point_by_point_closes() {
        let mut seg: Vec<TrailPoint> = Vec::new();
        let mut found = None;
        for i in 0..=48 {
            let a = i as f64 / 48.0 * 2.0 * PI * 1.05;
            seg.push(TrailPoint { x: 960.0 + 120.0 * a.cos(), y: 540.0 + 84.0 * a.sin(), t: i as f64 * 16.0 });
            let last = *seg.last().unwrap();
            if seg.len() >= 9 && found.is_none() {
                for j in (0..=seg.len() - 9).rev() {
                    if (seg[j].x - last.x).hypot(seg[j].y - last.y) > 24.0 {
                        continue;
                    }
                    if let Some(c) = circle_of(&seg[j..]) {
                        found = Some((i, c));
                        break;
                    }
                }
            }
        }
        assert!(found.is_some(), "the loop closed");
    }

    #[test]
    fn a_flat_straight_stroke_is_an_underline_a_scribble_is_not() {
        assert!(matches!(underline_of(&trail(|u| (100.0 + 300.0 * u, 400.0 + 3.0 * (u * 9.0).sin()))), Some(Mark::Underline { .. })));
        assert_eq!(underline_of(&trail(|u| (100.0 + 300.0 * (u * 12.0).sin().abs(), 400.0))), None);
        assert_eq!(underline_of(&lp(300.0, 200.0, 80.0, 50.0, 1.0)), None);
    }

    #[test]
    fn holding_still_is_pointing_even_with_tiny_drift() {
        let p = |x, y, t| TrailPoint { x, y, t };
        let pts = vec![p(10.0, 10.0, 0.0), p(400.0, 300.0, 100.0), p(402.0, 301.0, 200.0), p(401.0, 299.0, 300.0)];
        assert_eq!(dwell_of(&pts, 500.0), None);
        let Some(Mark::Point { x, y, held_ms }) = dwell_of(&pts, 800.0) else { panic!() };
        assert_eq!((x, y), (401.0, 299.0));
        assert!(held_ms >= 600.0);
        let mut moved = pts.clone();
        moved.push(p(600.0, 300.0, 790.0)); // moved away just now
        assert_eq!(dwell_of(&moved, 800.0), None);
    }

    #[test]
    fn regions_and_descriptions() {
        assert_eq!(region_of(&Mark::Point { x: 400.0, y: 300.0, held_ms: 0.0 }), Box2 { x1: 320.0, y1: 250.0, x2: 480.0, y2: 350.0 });
        assert_eq!(region_of(&Mark::Underline { b: Box2 { x1: 100.0, y1: 400.0, x2: 400.0, y2: 404.0 } }).y1, 360.0);
        let circle = Mark::Circle { b: Box2 { x1: 0.0, y1: 0.0, x2: 0.0, y2: 0.0 } };
        assert_eq!(describe(&circle, Box2 { x1: 1.0, y1: 2.0, x2: 3.0, y2: 4.0 }, 1), "The user circled 1,2 to 3,4 on screen1.");
        let point = Mark::Point { x: 0.0, y: 0.0, held_ms: 800.0 };
        assert_eq!(describe(&point, Box2 { x1: 10.0, y1: 10.0, x2: 30.0, y2: 30.0 }, 2), "The user pointed at 20,20 on screen2 (held still 0.8 s).");
    }

    #[test]
    fn marks_serialize_like_v1() {
        let point = Mark::Point { x: 1.0, y: 2.0, held_ms: 700.0 };
        assert_eq!(serde_json::to_string(&point).unwrap(), r#"{"type":"point","x":1.0,"y":2.0,"heldMs":700.0}"#);
        let circle = Mark::Circle { b: Box2 { x1: 1.0, y1: 2.0, x2: 3.0, y2: 4.0 } };
        assert_eq!(serde_json::to_string(&circle).unwrap(), r#"{"type":"circle","x1":1.0,"y1":2.0,"x2":3.0,"y2":4.0}"#);
    }
}
