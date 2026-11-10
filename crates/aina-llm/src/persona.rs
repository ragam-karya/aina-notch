// aina's small talk (port of v1's persona.js): the startup greeting, the short reply when woken with
// "Hey aina", and the filler said while an answer takes a moment. Local templates, so they are
// instant, free and work offline. They follow the time of day, how long since aina was last used, how
// new the user is and their nickname, and never repeat the line said last time.

use std::collections::HashMap;
use std::sync::Mutex;

const HOUR: i64 = 60 * 60 * 1000;
const DAY: i64 = 24 * HOUR;

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

fn greetings(en: bool, kind: &str, n: &str, wake: bool) -> Vec<String> {
    let lines: Vec<String> = match (en, kind) {
        (false, "first") => vec![format!("Hai{n}, aku aina. Aku di sini kalau kamu butuh{}.", if wake { ", panggil aja \"Hey aina\"" } else { "" })],
        (false, "early") => vec![format!("Hai lagi{n}! Aku di sini kalau kamu butuh."), format!("Halo{n}, aku standby ya.")],
        (false, "again") => vec!["Aku di sini lagi.".into(), format!("Hai lagi{n}."), "Siap kalau kamu butuh.".into()],
        (false, "away") => vec![format!("Hai{n}, lama nggak ketemu. Aku di sini kalau perlu."), format!("Eh, kamu balik{n}! Aku standby ya.")],
        (false, "pagi") => vec![format!("Pagi{n}! Aku siap kalau kamu butuh."), format!("Selamat pagi{n}. Aku standby ya."), format!("Pagi{n}. Semoga harinya lancar.")],
        (false, "siang") => vec![format!("Siang{n}! Aku standby."), format!("Hai{n}, aku di sini kalau perlu bantuan."), format!("Siang{n}. Panggil aja kalau butuh.")],
        (false, "sore") => vec![format!("Sore{n}! Aku di sini kalau perlu."), format!("Hai{n}, sore ini aku standby ya."), format!("Sore{n}. Panggil aja kalau butuh.")],
        (false, "malam") => vec![format!("Malam{n}. Aku di sini kalau butuh."), format!("Hai{n}, aku standby ya."), format!("Malam{n}! Panggil aja kalau perlu.")],
        (false, _) => vec![format!("Masih melek{n}? Aku standby kok."), format!("Udah larut nih{n}. Aku di sini kalau perlu."), format!("Hai{n}, aku masih standby kok.")],
        (true, "first") => vec![format!("Hi{n}, I'm aina. I'm here whenever you need me{}.", if wake { ", just say \"Hey aina\"" } else { "" })],
        (true, "early") => vec![format!("Hi again{n}! I'm here whenever you need me."), format!("Hey{n}, I'm around.")],
        (true, "again") => vec!["I'm back.".into(), format!("Hi again{n}."), "Here if you need me.".into()],
        (true, "away") => vec![format!("Hey{n}, it's been a while. I'm here if you need me."), format!("Welcome back{n}! I'm around.")],
        (true, "pagi") => vec![format!("Morning{n}! Here when you need me."), format!("Good morning{n}. I'm around."), format!("Morning{n}. Hope today goes smoothly.")],
        (true, "siang") => vec![format!("Hey{n}! I'm around."), format!("Hi{n}, here if you need a hand."), format!("Hey{n}. Just call me if you need anything.")],
        (true, "sore") => vec![format!("Good afternoon{n}! I'm around."), format!("Hey{n}, I'm around this afternoon."), format!("Afternoon{n}. Just call me if you need anything.")],
        (true, "malam") => vec![format!("Evening{n}. Here if you need me."), format!("Hey{n}, I'm around tonight."), format!("Evening{n}! Just call me if you need anything.")],
        (true, _) => vec![format!("Still up{n}? I'm around."), format!("It's late{n}. I'm here if you need me."), format!("Hey{n}, I'm still around.")],
    };
    lines
}

fn acks(en: bool, kind: &str, n: &str) -> Vec<String> {
    let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    match (en, kind) {
        (false, "recent") => v(&["Ya?", "Hm?", "Iya?", "Lanjut?"]),
        (false, "normal") => vec!["Ya, kenapa?".into(), format!("Ya{n}?"), "Ya, aku di sini.".into(), "Ada apa?".into(), "Kenapa?".into()],
        (false, "pagi") => v(&["Pagi! Ada apa?"]),
        (false, "larut") => v(&["Iya, masih di sini."]),
        (true, "recent") => v(&["Yes?", "Hm?", "Yeah?", "Go on?"]),
        (true, "normal") => vec!["Yeah, what's up?".into(), format!("Yeah{n}?"), "Yeah, I'm here.".into(), "What's up?".into(), "What is it?".into()],
        (true, "pagi") => v(&["Morning! What's up?"]),
        (true, "larut") => v(&["Yeah, still here."]),
        _ => vec![],
    }
}

static LAST: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

/// A number in 0..1 for picking lines (no rand crate: the clock's nanoseconds are random enough here).
pub fn rand() -> f64 {
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    (n.wrapping_mul(2_654_435_761) % 1_000_000) as f64 / 1_000_000.0
}

// Picks a line, avoiding the one used last time for the same kind.
fn pick(kind: &str, options: Vec<String>, r: f64) -> String {
    let mut last = LAST.lock().unwrap();
    let last = last.get_or_insert_with(HashMap::new);
    let mut choices: Vec<&String> = options.iter().filter(|o| last.get(kind) != Some(*o)).collect();
    if choices.is_empty() {
        choices = options.iter().collect();
    }
    let line = choices.get(((r * choices.len() as f64) as usize).min(choices.len().saturating_sub(1))).map(|s| s.to_string()).unwrap_or_default();
    last.insert(kind.to_string(), line.clone());
    line
}

fn name_suffix(name: &str) -> String {
    if name.trim().is_empty() { String::new() } else { format!(", {}", name.trim()) }
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

pub fn greeting(g: &Greeting, r: f64) -> String {
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
    pick(&format!("greet-{kind}"), greetings(g.en, kind, &name_suffix(g.name), g.wake), r)
}

/// The reply when woken with "Hey aina": short, and shorter still right after a question.
pub fn wake_ack(en: bool, hour: u32, since_last_turn_ms: Option<u128>, name: &str, r: f64) -> String {
    let n = name_suffix(name);
    if since_last_turn_ms.is_some_and(|ms| ms < 3 * 60 * 1000) {
        return pick("ack-recent", acks(en, "recent", &n), r);
    }
    let mut options = acks(en, "normal", &n);
    options.extend(acks(en, slot_of(hour), &n));
    pick("ack", options, r)
}

/// Said when an answer takes a moment (the user's own wording).
pub fn filler(en: bool) -> &'static str {
    if en { "One sec, let me check." } else { "Bentar, aku lihat dulu ya." }
}

/// Every line wake_ack can give, plus the filler, so their audio can be fetched ahead of time.
pub fn ack_lines(en: bool, name: &str) -> Vec<String> {
    let n = name_suffix(name);
    let mut out: Vec<String> = Vec::new();
    for kind in ["recent", "normal", "pagi", "larut"] {
        for l in acks(en, kind, &n) {
            if !out.contains(&l) {
                out.push(l);
            }
        }
    }
    out.push(filler(en).to_string());
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
        assert!(greeting(&g(0, 0, 9), 0.1).starts_with("Hai, aku aina"));
        assert!(greeting(&Greeting { wake: true, ..g(0, 0, 9) }, 0.1).contains("Hey aina"));
        assert!(greeting(&g(1, 0, 9), 0.1).contains("lagi") || greeting(&g(1, 0, 9), 0.1).contains("standby"));
        let away = greeting(&g(9, 10 * DAY - 5 * DAY, 9), 0.0);
        assert!(away.contains("lama") || away.contains("balik"), "{away}");
        let morning = greeting(&g(9, 10 * DAY - 10 * HOUR, 8), 0.0);
        assert!(morning.to_lowercase().contains("pagi"), "{morning}");
    }

    #[test]
    fn a_line_is_not_repeated_back_to_back() {
        let a = wake_ack(false, 12, Some(1000), "", 0.0);
        let b = wake_ack(false, 12, Some(1000), "", 0.0);
        assert_ne!(a, b);
    }

    #[test]
    fn names_and_lists() {
        assert!(wake_ack(true, 12, None, "Faisal", 0.99).len() > 2);
        let lines = ack_lines(false, "");
        assert!(lines.contains(&"Bentar, aku lihat dulu ya.".to_string()));
        assert!(lines.contains(&"Ya, kenapa?".to_string()));
        assert_eq!(slot_of(3), "larut");
        assert_eq!(slot_of(16), "sore");
    }
}
