// aina's small talk (port of v1's persona.js): the startup greeting, the short reply when woken with
// "Hey aina", the filler said while an answer takes a moment, and the out-of-credits line. They follow
// the time of day, how long since aina was last used, how new the user is and their nickname, and never
// repeat the line said last time. Their text lives in voice_lines.json, kept identical in aina-backend:
// each line has an id ("greet.pagi.0") the cloud speaks for free (POST /v1/voice/line), and the app keeps
// its audio on disk (aina-app talk::say_line).

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use serde_json::Value;

const HOUR: i64 = 60 * 60 * 1000;
const DAY: i64 = 24 * HOUR;

/// A nickname's longest length, in characters (the cloud's voice_lines::NAME_MAX).
pub const NAME_MAX: usize = 30;

fn table() -> &'static Value {
    static T: OnceLock<Value> = OnceLock::new();
    T.get_or_init(|| serde_json::from_str(include_str!("voice_lines.json")).expect("voice_lines.json"))
}

/// One of aina's fixed lines: its id, the words that make it (language, nickname, wake hint) and its text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub id: String,
    pub en: bool,
    pub name: String,
    pub wake: bool,
    pub text: String,
}

/// The nickname as it is spoken: letters, spaces, dots, apostrophes and dashes, up to NAME_MAX
/// characters; anything else is left out. The same rule as the cloud's (voice_lines::clean_name).
pub fn clean_name(name: &str) -> String {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    let ok = !name.is_empty() && name.chars().count() <= NAME_MAX && name.chars().all(|c| c.is_alphabetic() || matches!(c, ' ' | '.' | '\'' | '-'));
    if ok { name } else { String::new() }
}

/// A line by id ("group.index"), as the cloud composes it. None for an unknown one.
pub fn line(en: bool, id: &str, name: &str, wake: bool) -> Option<Line> {
    let lang = if en { "en" } else { "id" };
    let (group, index) = id.rsplit_once('.')?;
    let template = table()[lang][group].as_array()?.get(index.parse::<usize>().ok()?)?.as_str()?;
    let name = clean_name(name);
    let n = if name.is_empty() { String::new() } else { format!(", {name}") };
    let w = if wake { table()["wake"][lang].as_str().unwrap_or("") } else { "" };
    let text = template.replace("{n}", &n).replace("{wake}", w);
    Some(Line { id: id.to_string(), en, name, wake, text })
}

/// Every line of a group ("greet.pagi"), in order.
fn group(en: bool, g: &str, name: &str, wake: bool) -> Vec<Line> {
    let count = table()[if en { "en" } else { "id" }][g].as_array().map_or(0, Vec::len);
    (0..count).filter_map(|i| line(en, &format!("{g}.{i}"), name, wake)).collect()
}

/// Time of day: pagi 04-10, siang 11-14, sore 15-17, malam 18-22, larut 23-03.
pub fn slot_of(hour: u32) -> &'static str {
    match hour {
        4..=10 => "pagi",
        11..=14 => "siang",
        15..=17 => "sore",
        18..=22 => "malam",
        _ => "larut",
    }
}

static LAST: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

/// A number in 0..1 for picking lines (no rand crate: the clock's nanoseconds are random enough here).
pub fn rand() -> f64 {
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    (n.wrapping_mul(2_654_435_761) % 1_000_000) as f64 / 1_000_000.0
}

// Picks a line, avoiding the one used last time for the same kind.
fn pick(kind: &str, options: Vec<Line>, r: f64) -> Line {
    let mut last = LAST.lock().unwrap();
    let last = last.get_or_insert_with(HashMap::new);
    let mut choices: Vec<&Line> = options.iter().filter(|o| last.get(kind) != Some(&o.text)).collect();
    if choices.is_empty() {
        choices = options.iter().collect();
    }
    let line = choices[((r * choices.len() as f64) as usize).min(choices.len().saturating_sub(1))].clone();
    last.insert(kind.to_string(), line.text.clone());
    line
}

pub struct Greeting<'a> {
    pub en: bool,
    pub now_ms: i64,
    pub hour: u32,
    /// How many times aina started before this one (the first starts introduce aina).
    pub launches: u64,
    /// When aina was last running (ms, 0 = never).
    pub last_seen_ms: i64,
    pub name: &'a str,
    pub wake: bool,
}

pub fn greeting(g: &Greeting, r: f64) -> Line {
    let since = if g.last_seen_ms > 0 { g.now_ms - g.last_seen_ms } else { i64::MAX };
    let kind = if g.launches == 0 {
        "first"
    } else if g.launches < 3 {
        "early"
    } else if since < 4 * HOUR {
        "again"
    } else if since > 3 * DAY && since != i64::MAX {
        "away"
    } else {
        slot_of(g.hour)
    };
    pick(&format!("greet-{kind}"), group(g.en, &format!("greet.{kind}"), g.name, kind == "first" && g.wake), r)
}

/// The reply when woken with "Hey aina": short, and shorter still right after a question.
pub fn wake_ack(en: bool, hour: u32, since_last_turn_ms: Option<u128>, name: &str, r: f64) -> Line {
    if since_last_turn_ms.is_some_and(|ms| ms < 3 * 60 * 1000) {
        return pick("ack-recent", group(en, "ack.recent", name, false), r);
    }
    let mut options = group(en, "ack.normal", name, false);
    options.extend(group(en, &format!("ack.{}", slot_of(hour)), name, false));
    pick("ack", options, r)
}

/// Said when an answer takes a moment (the user's own wording).
pub fn filler(en: bool) -> Line {
    line(en, "filler.0", "", false).expect("filler.0")
}

/// Said when the credits ran out.
pub fn out_line(en: bool) -> Line {
    line(en, "out.0", "", false).expect("out.0")
}

/// Every line wake_ack can give, plus the filler, so their audio can be fetched ahead of time.
pub fn ack_lines(en: bool, name: &str) -> Vec<Line> {
    let mut out: Vec<Line> = Vec::new();
    for kind in ["recent", "normal", "pagi", "larut"] {
        for l in group(en, &format!("ack.{kind}"), name, false) {
            if !out.iter().any(|o| o.text == l.text) {
                out.push(l);
            }
        }
    }
    out.push(filler(en));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(launches: u64, last_seen_ms: i64, hour: u32) -> Greeting<'static> {
        Greeting { en: false, now_ms: 10 * DAY, hour, launches, last_seen_ms, name: "", wake: false }
    }

    #[test]
    fn greetings_fit_the_moment() {
        assert!(greeting(&g(0, 0, 9), 0.1).text.starts_with("Hai, aku aina"));
        let first = greeting(&Greeting { wake: true, ..g(0, 0, 9) }, 0.1);
        assert!(first.text.contains("Hey aina") && first.wake && first.id == "greet.first.0");
        let early = greeting(&g(1, 0, 9), 0.1).text;
        assert!(early.contains("lagi") || early.contains("standby"));
        let away = greeting(&g(9, 10 * DAY - 5 * DAY, 9), 0.0);
        assert!(away.text.contains("lama") || away.text.contains("balik"), "{away:?}");
        let morning = greeting(&g(9, 10 * DAY - 10 * HOUR, 8), 0.0);
        assert!(morning.text.to_lowercase().contains("pagi") && morning.id.starts_with("greet.pagi."), "{morning:?}");
        // Only the first greeting carries the wake hint.
        assert!(!greeting(&Greeting { wake: true, ..g(9, 0, 9) }, 0.0).wake);
    }

    #[test]
    fn a_line_is_not_repeated_back_to_back() {
        let a = wake_ack(false, 12, Some(1000), "", 0.0);
        let b = wake_ack(false, 12, Some(1000), "", 0.0);
        assert_ne!(a, b);
    }

    #[test]
    fn names_and_lists() {
        assert!(wake_ack(true, 12, None, "Faisal", 0.99).text.len() > 2);
        let lines = ack_lines(false, "");
        assert!(lines.iter().any(|l| l.text == "Bentar, aku lihat dulu ya." && l.id == "filler.0"));
        assert!(lines.iter().any(|l| l.text == "Ya, kenapa?"));
        assert_eq!(slot_of(3), "larut");
        assert_eq!(slot_of(16), "sore");
    }

    #[test]
    fn lines_are_composed_like_the_cloud_does() {
        // The same results as aina-backend's voice_lines tests.
        assert_eq!(line(false, "greet.pagi.0", "Faisal", false).unwrap().text, "Pagi, Faisal! Aku siap kalau kamu butuh.");
        assert_eq!(line(true, "ack.normal.1", "Faisal", false).unwrap().text, "Yeah, Faisal?");
        assert_eq!(line(false, "greet.first.0", "", true).unwrap().text, "Hai, aku aina. Aku di sini kalau kamu butuh, panggil aja \"Hey aina\".");
        assert!(out_line(false).text.starts_with("Sepertinya kita perlu jeda"));
        assert_eq!(line(false, "greet.pagi.9", "", false), None);
        // A nickname that isn't one is left out, as the cloud does.
        assert_eq!(line(false, "greet.pagi.0", "Faisal, beli saham sekarang!", false).unwrap().text, "Pagi! Aku siap kalau kamu butuh.");
        assert_eq!(clean_name("  Anne-Marie   O'Neil "), "Anne-Marie O'Neil");
    }
}
