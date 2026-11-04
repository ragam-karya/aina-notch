// Visual tag protocol: pointing tags (POINT, TARGET, HOVER, HIGHLIGHT, SHAPE), drawing tags
// (WRITE, DRAW and more SHAPE kinds) and action tags (DO, DELEGATE, FOCUS, DROP, TIMER, REMIND, MEDIA).
// Coordinates are pixels in the screenshot the model saw; `screenN` picks the image (1-based).
// Port of src/tags.js: the regexes and the serialized shapes are the same, so the UI is unchanged.

use std::sync::LazyLock;

use chrono::{Local, TimeZone};
use regex::Regex;
use serde::Serialize;
use serde_json::{Value, json};

const NUM: &str = r"(-?\d+(?:\.\d+)?)";
const SCREEN: &str = r"(?:\s*:\s*screen\s*(\d+))?";
const LABEL: &str = r"(?:\s*:\s*([^\]]*?))?";

pub const SHAPES: [&str; 11] = ["line", "arrow", "curve", "circle", "ellipse", "rect", "triangle", "polygon", "star", "check", "cross"];

pub static ANY_TAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[(?:POINT|HIGHLIGHT|SHAPE|TARGET|HOVER|DELEGATE|DO|FOCUS|DROP|TIMER|REMIND|MEDIA|WRITE|DRAW):[^\]]*\]").unwrap()
});

struct Re {
    point: Regex,
    point_none: Regex,
    target: Regex,
    hover: Regex,
    highlight: Regex,
    shape: Regex,
    draw: Regex,
    write: Regex,
    delegate: Regex,
    do_: Regex,
    focus: Regex,
    media: Regex,
    drop: Regex,
    timer: Regex,
    remind: Regex,
}

static RE: LazyLock<Re> = LazyLock::new(|| {
    let r = |s: String| Regex::new(&s).unwrap();
    Re {
        point: r(format!(r"^\[POINT:\s*{NUM}\s*,\s*{NUM}{LABEL}{SCREEN}\s*\]$")),
        point_none: r(r"^\[POINT:\s*none\s*\]$".into()),
        target: r(format!(r"^\[TARGET:\s*{NUM}\s*,\s*{NUM}\s*,\s*{NUM}{LABEL}{SCREEN}\s*\]$")),
        hover: r(format!(r"^\[HOVER:\s*{NUM}\s*,\s*{NUM}\s*,\s*{NUM}{LABEL}{SCREEN}\s*\]$")),
        highlight: r(format!(r"^\[HIGHLIGHT:\s*{NUM}\s*,\s*{NUM}\s*,\s*{NUM}\s*,\s*{NUM}{LABEL}{SCREEN}\s*\]$")),
        shape: r(format!(r"^\[SHAPE:\s*({})\s*:\s*([-\d.,;\s]+?)\s*(?::([^\]]*))?\]$", SHAPES.join("|"))),
        draw: r(r"^\[DRAW:\s*([-\d.,;\s]+?)\s*(?::([^\]]*))?\]$".into()),
        write: r(format!(r"^\[WRITE:\s*{NUM}\s*,\s*{NUM}\s*(?:,\s*{NUM})?\s*:([^\]]*)\]$")),
        delegate: r(r"^\[DELEGATE:\s*([^\]]+?)\s*\]$".into()),
        do_: r(r"^\[DO:\s*([^\]]+?)\s*\]$".into()),
        focus: r(r"^\[FOCUS:\s*([^\]]+?)\s*\]$".into()),
        media: r(r"(?i)^\[MEDIA:\s*(play_pause|next|previous)\s*\]$".into()),
        drop: r(r"(?i)^\[DROP:\s*(focus|upload)\s*\]$".into()),
        timer: r(r"^\[TIMER:\s*([^\]:]+?)\s*(?::\s*([^\]]*?))?\s*\]$".into()),
        remind: r(r"^\[REMIND:\s*(\d{4}-\d{2}-\d{2})[T ](\d{1,2}):(\d{2})\s*(?::\s*([^\]]*?))?\s*\]$".into()),
    }
});

/// Named colors the model may use, in English and Indonesian.
pub fn color(name: &str) -> Option<&'static str> {
    Some(match name {
        "blue" | "biru" => "#2b8aff",
        "red" | "merah" => "#ff3b47",
        "green" | "hijau" => "#1fbf63",
        "yellow" | "kuning" => "#ffd21f",
        "orange" | "oranye" => "#ff8a1f",
        "purple" | "ungu" => "#9b5cff",
        "pink" => "#ff5cb1",
        "black" | "hitam" => "#151a22",
        "white" | "putih" => "#ffffff",
        "gray" | "grey" => "#8a94a3",
        _ => return None,
    })
}

/// One parsed tag. Serializes to the same objects src/tags.js produced (`{ type: 'point', … }`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Tag {
    Point { x: f64, y: f64, label: String, screen: u32 },
    Target { x: f64, y: f64, r: f64, label: String, screen: u32 },
    Hover { x: f64, y: f64, r: f64, label: String, screen: u32 },
    Highlight { x1: f64, y1: f64, x2: f64, y2: f64, label: String, screen: u32 },
    Shape { shape: String, points: Vec<[f64; 2]>, label: String, screen: u32, color: Option<String>, fill: bool, width: Option<f64> },
    Draw { points: Vec<[f64; 2]>, label: String, screen: u32, color: Option<String>, width: Option<f64> },
    Write { x: f64, y: f64, size: f64, text: String, screen: u32, color: Option<String> },
    Delegate { task: String },
    Do { task: String },
    Focus { folder: Option<String> },
    Media { action: String },
    Drop { purpose: String },
    Timer {
        #[serde(skip_serializing_if = "Option::is_none")]
        off: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pomodoro: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        ms: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        label: Option<String>,
    },
    Remind { at: i64, text: String },
}

impl Tag {
    /// Shapes, freehand and handwriting: drawn stroke by stroke rather than shown at once.
    pub fn is_drawing(&self) -> bool {
        matches!(self, Tag::Shape { .. } | Tag::Draw { .. } | Tag::Write { .. })
    }
}

fn num(s: &str) -> f64 {
    s.parse().unwrap_or(f64::NAN)
}

// The label group is lazy and can swallow a trailing ":screenN"; split it back out.
fn split_screen(label: Option<&str>, screen: Option<&str>) -> (String, u32) {
    static TAIL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(.*?)\s*:\s*screen\s*(\d+)$").unwrap());
    static ONLY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^screen\s*\d+$").unwrap());
    let screen_n = |s: &str| s.chars().filter(char::is_ascii_digit).collect::<String>().parse().unwrap_or(1);
    if let (Some(l), None) = (label, screen) {
        if let Some(m) = TAIL.captures(l) {
            return (m[1].to_string(), screen_n(&m[2]));
        }
        if ONLY.is_match(l) {
            return (String::new(), screen_n(l));
        }
    }
    (label.unwrap_or("").to_string(), screen.map(screen_n).unwrap_or(1))
}

struct Options {
    text: String,
    screen: u32,
    color: Option<String>,
    fill: bool,
    width: Option<f64>,
}

// Trailing ":"-separated fields: options (screenN, a color, fill, thick/thin) are peeled off the end;
// whatever remains is the label/text, so text may itself contain ":" (e.g. "10:30").
fn split_options(rest: Option<&str>) -> Options {
    static SCREEN_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^screen\s*(\d+)$").unwrap());
    static HEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(?:color=)?(#[0-9a-f]{3,8})$").unwrap());
    static NAMED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^color=(\w+)$").unwrap());
    let mut segs: Vec<&str> = rest.map(|r| r.split(':').collect()).unwrap_or_default();
    let mut o = Options { text: String::new(), screen: 1, color: None, fill: false, width: None };
    while let Some(last) = segs.last() {
        let s = last.trim().to_lowercase();
        if let Some(m) = SCREEN_RE.captures(&s) {
            o.screen = m[1].parse().unwrap_or(1);
        } else if let Some(c) = color(&s) {
            o.color = Some(c.into());
        } else if let Some(m) = HEX.captures(&s) {
            o.color = Some(m[1].into());
        } else if let Some(c) = NAMED.captures(&s).and_then(|m| color(&m[1])) {
            o.color = Some(c.into());
        } else if s == "fill" || s == "filled" {
            o.fill = true;
        } else if s == "thick" || s == "tebal" {
            o.width = Some(6.0);
        } else if s == "thin" || s == "tipis" {
            o.width = Some(2.0);
        } else {
            break;
        }
        segs.pop();
    }
    o.text = segs.join(":").trim().to_string();
    o
}

fn parse_points(s: &str) -> Vec<[f64; 2]> {
    s.split(';')
        .filter_map(|p| {
            let v: Vec<f64> = p.split(',').map(|n| n.trim().parse::<f64>().unwrap_or(f64::NAN)).collect();
            (v.len() == 2 && v.iter().all(|n| n.is_finite())).then(|| [v[0], v[1]])
        })
        .collect()
}

/// "25m", "90s", "1h30m", "1.5h" → milliseconds (0 when it isn't a duration).
pub fn duration_ms(s: &str) -> u64 {
    static DUR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(?:(\d+(?:\.\d+)?)h)?(?:(\d+(?:\.\d+)?)m)?(?:(\d+)s)?$").unwrap());
    let s: String = s.to_lowercase().split_whitespace().collect();
    let Some(m) = DUR.captures(&s) else { return 0 };
    if m.get(1).is_none() && m.get(2).is_none() && m.get(3).is_none() {
        return 0;
    }
    let g = |i| m.get(i).map(|x| num(x.as_str())).unwrap_or(0.0);
    ((g(1) * 3600.0 + g(2) * 60.0 + g(3)) * 1000.0).round() as u64
}

fn opt<'t>(m: &regex::Captures<'t>, i: usize) -> Option<&'t str> {
    m.get(i).map(|x| x.as_str())
}

pub fn parse_tag(tag: &str) -> Option<Tag> {
    let re = &*RE;
    if re.point_none.is_match(tag) {
        return None;
    }
    if let Some(m) = re.point.captures(tag) {
        let (label, screen) = split_screen(opt(&m, 3), opt(&m, 4));
        return Some(Tag::Point { x: num(&m[1]), y: num(&m[2]), label, screen });
    }
    if let Some(m) = re.target.captures(tag).or_else(|| re.hover.captures(tag)) {
        let (label, screen) = split_screen(opt(&m, 4), opt(&m, 5));
        let (x, y, r) = (num(&m[1]), num(&m[2]), num(&m[3]));
        return Some(if tag.starts_with("[TARGET") {
            Tag::Target { x, y, r, label, screen }
        } else {
            Tag::Hover { x, y, r, label, screen }
        });
    }
    if let Some(m) = re.highlight.captures(tag) {
        let (label, screen) = split_screen(opt(&m, 5), opt(&m, 6));
        return Some(Tag::Highlight { x1: num(&m[1]), y1: num(&m[2]), x2: num(&m[3]), y2: num(&m[4]), label, screen });
    }
    if let Some(m) = re.shape.captures(tag) {
        let points = parse_points(&m[2]);
        if points.is_empty() {
            return None;
        }
        let o = split_options(opt(&m, 3));
        return Some(Tag::Shape { shape: m[1].into(), points, label: o.text, screen: o.screen, color: o.color, fill: o.fill, width: o.width });
    }
    if let Some(m) = re.draw.captures(tag) {
        let points = parse_points(&m[1]);
        if points.len() < 2 {
            return None;
        }
        let o = split_options(opt(&m, 2));
        return Some(Tag::Draw { points, label: o.text, screen: o.screen, color: o.color, width: o.width });
    }
    if let Some(m) = re.write.captures(tag) {
        let o = split_options(opt(&m, 4));
        if o.text.is_empty() {
            return None;
        }
        let size = opt(&m, 3).map(num).unwrap_or(32.0);
        return Some(Tag::Write { x: num(&m[1]), y: num(&m[2]), size, text: o.text, screen: o.screen, color: o.color });
    }
    if let Some(m) = re.delegate.captures(tag) {
        return Some(Tag::Delegate { task: m[1].into() });
    }
    if let Some(m) = re.do_.captures(tag) {
        return Some(Tag::Do { task: m[1].into() });
    }
    if let Some(m) = re.focus.captures(tag) {
        let off = matches!(m[1].to_lowercase().as_str(), "off" | "none" | "selesai" | "stop");
        return Some(Tag::Focus { folder: (!off).then(|| m[1].to_string()) });
    }
    if let Some(m) = re.media.captures(tag) {
        return Some(Tag::Media { action: m[1].to_lowercase() });
    }
    if let Some(m) = re.drop.captures(tag) {
        return Some(Tag::Drop { purpose: m[1].to_lowercase() });
    }
    if let Some(m) = re.timer.captures(tag) {
        let what = m[1].to_lowercase();
        let label = Some(opt(&m, 2).unwrap_or("").to_string());
        if matches!(what.as_str(), "off" | "none" | "stop" | "selesai") {
            return Some(Tag::Timer { off: Some(true), pomodoro: None, ms: None, label: None });
        }
        if what == "pomodoro" {
            return Some(Tag::Timer { off: None, pomodoro: Some(true), ms: None, label });
        }
        let ms = duration_ms(&m[1]);
        return (ms > 0).then_some(Tag::Timer { off: None, pomodoro: None, ms: Some(ms), label });
    }
    if let Some(m) = re.remind.captures(tag) {
        let text = opt(&m, 4).filter(|t| !t.is_empty())?;
        let ymd: Vec<i32> = m[1].split('-').map(|p| p.parse().unwrap_or(0)).collect();
        // The user's local time.
        let at = Local
            .with_ymd_and_hms(ymd[0], ymd[1] as u32, ymd[2] as u32, m[2].parse().ok()?, m[3].parse().ok()?, 0)
            .earliest()?
            .timestamp_millis();
        return Some(Tag::Remind { at, text: text.into() });
    }
    None
}

/// The tags in a full reply, and the reply with the tags taken out (what is spoken).
pub fn extract_tags(text: &str) -> (Vec<Tag>, String) {
    static TRAILING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[ \t]+\n").unwrap());
    static BLANKS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n{2,}").unwrap());
    let tags = ANY_TAG.find_iter(text).filter_map(|m| parse_tag(m.as_str())).collect();
    let spoken = ANY_TAG.replace_all(text, "");
    let spoken = TRAILING.replace_all(&spoken, "\n");
    let spoken = BLANKS.replace_all(&spoken, "\n");
    (tags, spoken.trim().to_string())
}

/// A screenshot the model saw: its pixel size and where it sits on the desktop.
#[derive(Debug, Clone, Copy)]
pub struct Shot {
    pub bounds: Rect,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Maps screenshot pixels to desktop coordinates using the capture metadata. Returns the tag as JSON
/// with the mapped fields, exactly as toGlobal did, ready to send to the overlay.
pub fn to_global(tag: &Tag, shots: &[Shot]) -> Option<Value> {
    let screen = match tag {
        Tag::Point { screen, .. }
        | Tag::Target { screen, .. }
        | Tag::Hover { screen, .. }
        | Tag::Highlight { screen, .. }
        | Tag::Shape { screen, .. }
        | Tag::Draw { screen, .. }
        | Tag::Write { screen, .. } => *screen,
        _ => 1,
    };
    let shot = shots.get(screen.max(1) as usize - 1).or_else(|| shots.first())?;
    let sx = shot.bounds.width / shot.width;
    let sy = shot.bounds.height / shot.height;
    let pt = |x: f64, y: f64| (shot.bounds.x + x * sx, shot.bounds.y + y * sy);
    let mut v = serde_json::to_value(tag).ok()?;
    let o = v.as_object_mut()?;
    match tag {
        Tag::Point { x, y, .. } => {
            let (gx, gy) = pt(*x, *y);
            o.insert("x".into(), json!(gx));
            o.insert("y".into(), json!(gy));
        }
        Tag::Target { x, y, r, .. } | Tag::Hover { x, y, r, .. } => {
            let (gx, gy) = pt(*x, *y);
            o.insert("x".into(), json!(gx));
            o.insert("y".into(), json!(gy));
            o.insert("r".into(), json!((r * sx).max(14.0)));
        }
        Tag::Highlight { x1, y1, x2, y2, .. } => {
            let a = pt(x1.min(*x2), y1.min(*y2));
            let b = pt(x1.max(*x2), y1.max(*y2));
            o.insert("x".into(), json!(a.0));
            o.insert("y".into(), json!(a.1));
            o.insert("w".into(), json!(b.0 - a.0));
            o.insert("h".into(), json!(b.1 - a.1));
        }
        Tag::Shape { points, .. } | Tag::Draw { points, .. } => {
            let mapped: Vec<Value> = points.iter().map(|p| pt(p[0], p[1])).map(|(x, y)| json!({ "x": x, "y": y })).collect();
            o.insert("points".into(), Value::Array(mapped));
        }
        Tag::Write { x, y, size, .. } => {
            let (gx, gy) = pt(*x, *y);
            o.insert("x".into(), json!(gx));
            o.insert("y".into(), json!(gy));
            o.insert("size".into(), json!((size * sy).max(12.0)));
        }
        _ => {}
    }
    Some(v)
}

// Same cases as test/parsers.test.js.
#[cfg(test)]
mod tests {
    use super::*;

    fn shots(x: f64, w: f64, h: f64) -> Vec<Shot> {
        vec![Shot { bounds: Rect { x, y: 0.0, width: w, height: h }, width: 1280.0, height: 720.0 }]
    }

    #[test]
    fn point_with_label_and_screen() {
        assert_eq!(parse_tag("[POINT:120,210:save button:screen2]"), Some(Tag::Point { x: 120.0, y: 210.0, label: "save button".into(), screen: 2 }));
        assert_eq!(parse_tag("[POINT:5,6]"), Some(Tag::Point { x: 5.0, y: 6.0, label: "".into(), screen: 1 }));
        assert_eq!(parse_tag("[POINT:none]"), None);
    }

    #[test]
    fn target_hover_highlight_shape() {
        assert_eq!(parse_tag("[TARGET:120,210,40:add modifier]"), Some(Tag::Target { x: 120.0, y: 210.0, r: 40.0, label: "add modifier".into(), screen: 1 }));
        assert!(matches!(parse_tag("[HOVER:1,2,3:menu:screen3]"), Some(Tag::Hover { screen: 3, .. })));
        assert_eq!(
            parse_tag("[HIGHLIGHT:10,20,110,220:work area]"),
            Some(Tag::Highlight { x1: 10.0, y1: 20.0, x2: 110.0, y2: 220.0, label: "work area".into(), screen: 1 })
        );
        assert_eq!(
            parse_tag("[SHAPE:arrow:10,10;50,60:drag here:screen2]"),
            Some(Tag::Shape { shape: "arrow".into(), points: vec![[10.0, 10.0], [50.0, 60.0]], label: "drag here".into(), screen: 2, color: None, fill: false, width: None })
        );
    }

    #[test]
    fn extract_tags_strips_tags_from_spoken_text() {
        let (tags, spoken) = extract_tags("[TARGET:120,210,40:add modifier] Click Add Modifier to open the list.");
        assert_eq!(tags.len(), 1);
        assert_eq!(spoken, "Click Add Modifier to open the list.");
        let (d, _) = extract_tags("[DELEGATE:research the top 5 note apps] On it.");
        assert_eq!(d[0], Tag::Delegate { task: "research the top 5 note apps".into() });
    }

    #[test]
    fn to_global_maps_screenshot_pixels_to_display() {
        let g = to_global(&Tag::Target { x: 640.0, y: 360.0, r: 20.0, label: "".into(), screen: 1 }, &shots(1920.0, 1920.0, 1080.0)).unwrap();
        assert_eq!(g["x"], json!(1920.0 + 960.0));
        assert_eq!(g["y"], json!(540.0));
        assert_eq!(g["r"], json!(30.0));
    }

    #[test]
    fn drawing_tags_shapes_freehand_handwriting() {
        let Some(Tag::Shape { shape, points, color, fill, label, .. }) = parse_tag("[SHAPE:triangle:200,120;300,300;100,300:green:fill]") else { panic!() };
        assert_eq!((shape.as_str(), points.len(), color.as_deref(), fill, label.as_str()), ("triangle", 3, Some("#1fbf63"), true, ""));
        let Some(Tag::Shape { shape, label, color, width, screen, .. }) = parse_tag("[SHAPE:star:400,300;400,200:my star:#ff00aa:thick:screen2]") else { panic!() };
        assert_eq!((shape.as_str(), label.as_str(), color.as_deref(), width, screen), ("star", "my star", Some("#ff00aa"), Some(6.0), 2));
        for k in ["rect", "ellipse", "polygon", "check", "cross", "curve", "line"] {
            assert!(matches!(parse_tag(&format!("[SHAPE:{k}:1,2;3,4;5,6]")), Some(Tag::Shape { shape, .. }) if shape == k));
        }
        let d = parse_tag("[DRAW:10,10;20,15;30,10;40,15:red]").unwrap();
        assert!(matches!(&d, Tag::Draw { points, color, .. } if points.len() == 4 && color.as_deref() == Some("#ff3b47")));
        assert_eq!(parse_tag("[DRAW:10,10]"), None);
        let w = parse_tag("[WRITE:80,60,40:Meeting at 10:30!:red]").unwrap();
        assert_eq!(w, Tag::Write { x: 80.0, y: 60.0, size: 40.0, text: "Meeting at 10:30!".into(), screen: 1, color: Some("#ff3b47".into()) });
        assert!(matches!(parse_tag("[WRITE:5,6:hi]"), Some(Tag::Write { size, .. }) if size == 32.0));
        let (tags, spoken) = extract_tags("[WRITE:1,2,30:Hi][SHAPE:rect:1,1;9,9:blue] Done!");
        assert_eq!(tags.len(), 2);
        assert_eq!(spoken, "Done!");
        let s = shots(0.0, 1920.0, 1080.0);
        assert_eq!(to_global(&w, &s).unwrap()["size"], json!(60.0));
        assert_eq!(to_global(&d, &s).unwrap()["points"][0], json!({ "x": 15.0, "y": 15.0 }));
    }

    #[test]
    fn action_tags() {
        assert_eq!(parse_tag("[FOCUS:selesai]"), Some(Tag::Focus { folder: None }));
        assert_eq!(parse_tag("[MEDIA:Next]"), Some(Tag::Media { action: "next".into() }));
        assert_eq!(parse_tag("[TIMER:1h30m:rapat]"), Some(Tag::Timer { off: None, pomodoro: None, ms: Some(5_400_000), label: Some("rapat".into()) }));
        assert_eq!(parse_tag("[TIMER:soon]"), None);
        assert!(matches!(parse_tag("[REMIND:2026-10-20 15:00:angkat jemuran]"), Some(Tag::Remind { text, .. }) if text == "angkat jemuran"));
        // Same JSON as the JS objects the overlay already reads.
        assert_eq!(serde_json::to_value(parse_tag("[POINT:5,6]").unwrap()).unwrap(), json!({ "type": "point", "x": 5.0, "y": 6.0, "label": "", "screen": 1 }));
    }
}
