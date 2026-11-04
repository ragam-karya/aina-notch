// UI strings from v1's i18n module (ui-dist/i18n.json, written by sync-ui). Strings that were
// functions in v1 (counts, names) are not in the JSON; the few the core needs so far are below.

use serde_json::Value;
use tauri::{AppHandle, Manager};

use crate::AppState;

pub fn asset_json(app: &AppHandle, path: &str) -> Value {
    app.asset_resolver()
        .get(path.to_string())
        .and_then(|a| serde_json::from_slice(&a.bytes).ok())
        .unwrap_or(Value::Null)
}

/// The UI language: the setting; with "auto", the language of the last question (Indonesian until
/// one is heard), as v1's uiLang().
pub fn lang(app: &AppHandle) -> &'static str {
    match app.state::<AppState>().store.get("language").as_str() {
        Some("en") => "en",
        Some("id") => "id",
        _ => match *SPOKEN.lock().unwrap() {
            Some("en") => "en",
            _ => "id",
        },
    }
}

static SPOKEN: std::sync::Mutex<Option<&'static str>> = std::sync::Mutex::new(None);

const ID_WORDS: &str = "yang ini itu apa aku kamu gimana bagaimana di ke dari dan tidak nggak gak ga ada bisa tolong buka mana kenapa berapa sih dong ya juga untuk dengan sudah udah lagi jam saya mau bikin cara tombol jelasin coba kok deh nih tuh";
const EN_WORDS: &str = "the is are what how where why this that i you can please open my me to of and in on it do does a an with for button time which show tell explain help could would should";

/// v1 detectLang: counts common words of each language; None when it can't tell.
pub fn detect_lang(text: &str) -> Option<&'static str> {
    let (mut id, mut en) = (0, 0);
    let lower = text.to_lowercase();
    for w in lower.split(|c: char| !(c.is_ascii_lowercase() || c == '\'')).filter(|w| !w.is_empty()) {
        id += ID_WORDS.split(' ').any(|x| x == w) as i32;
        en += EN_WORDS.split(' ').any(|x| x == w) as i32;
    }
    (id != en).then_some(if id > en { "id" } else { "en" })
}

/// A question was heard or typed: with "auto", Badi follows its language (island labels too).
pub fn note_spoken(app: &AppHandle, text: &str) {
    let before = lang(app);
    if let Some(l) = detect_lang(text) {
        *SPOKEN.lock().unwrap() = Some(l);
    }
    if lang(app) != before {
        crate::ipc::send_island_labels(app, None);
    }
}

pub fn strings(app: &AppHandle) -> Value {
    asset_json(app, "i18n.json")[lang(app)].clone()
}

/// A plain string by key, or the key itself when missing (easy to spot on screen).
pub fn t(app: &AppHandle, key: &str) -> String {
    strings(app)[key].as_str().map(String::from).unwrap_or_else(|| key.to_string())
}

/// v1's `timerDone(label)`.
pub fn timer_done(app: &AppHandle, label: &str) -> String {
    match (lang(app), label.is_empty()) {
        ("en", true) => "Time's up.".into(),
        ("en", false) => format!("Time's up: {label}."),
        (_, true) => "Waktunya habis.".into(),
        (_, false) => format!("Waktunya habis: {label}."),
    }
}

/// v1's `pomodoroBreak(min)`.
pub fn pomodoro_break(app: &AppHandle, min: u64) -> String {
    match lang(app) {
        "en" => format!("That's one focus session done. Take a {min}-minute break."),
        _ => format!("Satu sesi fokus selesai. Istirahat {min} menit dulu ya."),
    }
}

/// The notice when a focus session ends: "Fokus selesai: badi · 1 j 12 mnt · 2 pomodoro".
pub fn focus_ended(app: &AppHandle, name: &str, ms: u64, pomodoros: u32) -> String {
    let min = ms / 60_000;
    let en = lang(app) == "en";
    let time = match (min / 60, min % 60, en) {
        (0, m, true) => format!("{m} min"),
        (h, m, true) => format!("{h} h {m} min"),
        (0, m, false) => format!("{m} mnt"),
        (h, m, false) => format!("{h} j {m} mnt"),
    };
    let pomo = if pomodoros > 0 { format!(" · {pomodoros} pomodoro") } else { String::new() };
    if en { format!("Focus ended: {name} · {time}{pomo}") } else { format!("Fokus selesai: {name} · {time}{pomo}") }
}

/// A focus session ended and auto-start is off: the break waits in the notch.
pub fn pomodoro_break_ready(app: &AppHandle, min: u64) -> String {
    match lang(app) {
        "en" => format!("Focus session done. Your {min}-minute break is ready, start it from the notch."),
        _ => format!("Sesi fokus selesai. Istirahat {min} menit sudah siap, mulai dari notch ya."),
    }
}

/// A break ended and auto-start is off: the next focus session waits in the notch.
pub fn pomodoro_work_ready(app: &AppHandle) -> String {
    match lang(app) {
        "en" => "Break's over. The next focus session is ready in the notch.".into(),
        _ => "Istirahat selesai. Sesi fokus berikutnya sudah siap di notch.".into(),
    }
}

/// The whole pomodoro cycle is over.
pub fn pomodoro_done(app: &AppHandle, today: u32) -> String {
    match lang(app) {
        "en" => format!("That's a full pomodoro cycle. {today} focus sessions today, nice work."),
        _ => format!("Satu siklus pomodoro beres. Hari ini sudah {today} sesi fokus, mantap."),
    }
}

/// v1's `brainFailed(m)`.
pub fn brain_failed(app: &AppHandle, m: &str) -> String {
    match lang(app) {
        "en" => format!("I couldn't finish it: {m}"),
        _ => format!("Aku nggak berhasil ngerjainnya: {m}"),
    }
}

/// v1's `brainUnavailable(m)`.
pub fn brain_unavailable(app: &AppHandle, m: &str) -> String {
    match lang(app) {
        "en" => format!("Computer control isn't available: {m}"),
        _ => format!("Kontrol komputer belum bisa dipakai: {m}"),
    }
}

/// A file change in Badi's words (v1 fileQuestion): who is None for the Brain ("Aku mau…"), an
/// agent's name otherwise. "Aku mau hapus kalkulator.html dari folder Badi ke Recycle Bin. Boleh?"
pub fn file_question(app: &AppHandle, who: Option<&str>, op: &str, names: &[String], place: Option<&str>, new_name: Option<&str>) -> String {
    let en = lang(app) == "en";
    let and = if en { "and" } else { "dan" };
    let what = match names {
        [one] => one.clone(),
        [a, b] => format!("{a} {and} {b}"),
        _ if en => format!("{} items", names.len()),
        _ => format!("{} item", names.len()),
    };
    let who = match (who, en) {
        (Some(n), true) => format!("{n} wants to"),
        (Some(n), false) => format!("{n} mau"),
        (None, true) => "I want to".into(),
        (None, false) => "Aku mau".into(),
    };
    let place = place.unwrap_or("");
    let opt = |pre: &str| if place.is_empty() { String::new() } else { format!(" {pre} {place}") };
    let name = new_name.unwrap_or("");
    if en {
        match op {
            "delete" => format!("{who} delete {what}{} to the Recycle Bin. Okay?", opt("from")),
            "move" => format!("{who} move {what} to {place}. Okay?"),
            "copy" => format!("{who} copy {what} to {place}. Okay?"),
            "rename" => format!("{who} rename {what} to {name}. Okay?"),
            "create" => format!("{who} create the folder {what}{}. Okay?", opt("in")),
            "edit" => format!("{who} edit {what}. Okay?"),
            "write" => format!("{who} create {what}{}. Okay?", opt("in")),
            _ => format!("{who} rewrite {what}. Okay?"),
        }
    } else {
        match op {
            "delete" => format!("{who} hapus {what}{} ke Recycle Bin. Boleh?", opt("dari folder")),
            "move" => format!("{who} pindahin {what} ke folder {place}. Boleh?"),
            "copy" => format!("{who} salin {what} ke folder {place}. Boleh?"),
            "rename" => format!("{who} ganti nama {what} jadi {name}. Boleh?"),
            "create" => format!("{who} bikin folder {what}{}. Boleh?", opt("di")),
            "edit" => format!("{who} ngedit {what}. Boleh?"),
            "write" => format!("{who} bikin file {what}{}. Boleh?", opt("di")),
            _ => format!("{who} nulis ulang {what}. Boleh?"),
        }
    }
}

/// v1's `agentDone(name)`.
pub fn agent_done(app: &AppHandle, name: &str) -> String {
    match lang(app) {
        "en" => format!("{name} is done"),
        _ => format!("{name} selesai"),
    }
}

/// A Sprite card's short status for a state other than working (that one is the kind's own verb).
pub fn sprite_state(app: &AppHandle, state: &str) -> String {
    let en = lang(app) == "en";
    match state {
        "starting" => if en { "Starting" } else { "Bersiap" },
        "waiting" => if en { "Waiting" } else { "Menunggu" },
        "needs_approval" => if en { "Needs your OK" } else { "Butuh izinmu" },
        "success" => if en { "Done" } else { "Selesai" },
        "error" => if en { "Failed" } else { "Gagal" },
        "cancelled" => if en { "Stopped" } else { "Dihentikan" },
        _ => if en { "Working" } else { "Bekerja" },
    }
    .into()
}

pub fn sprite_failed(app: &AppHandle, name: &str) -> String {
    match lang(app) {
        "en" => format!("{name} couldn't finish"),
        _ => format!("{name} nggak berhasil selesai"),
    }
}

pub fn sprites_all_done(app: &AppHandle) -> String {
    if lang(app) == "en" { "All Sprites are done".into() } else { "Semua Sprite selesai".into() }
}

pub fn sprites_busy(app: &AppHandle, max: usize) -> String {
    match lang(app) {
        "en" => format!("{max} Sprites are already working. I'll leave this one until one finishes."),
        _ => format!("Sudah ada {max} Sprite yang bekerja. Yang ini aku tahan dulu sampai ada yang selesai ya."),
    }
}

/// v1's `focusOn(name, where)`.
pub fn focus_on(app: &AppHandle, name: &str, place: &str) -> String {
    match (lang(app), place.is_empty()) {
        ("en", true) => format!("Okay, I'm focused on {name}."),
        ("en", false) => format!("Okay, I'm focused on {name} in {place}."),
        (_, true) => format!("Oke, aku fokus di folder {name}."),
        (_, false) => format!("Oke, aku fokus di folder {name} yang di {place}."),
    }
}

/// v1's `focusNotFound(name)`.
pub fn focus_not_found(app: &AppHandle, name: &str) -> String {
    match lang(app) {
        "en" => format!("I couldn't find a folder called {name}. Tell me the folder above it, like \"{name} in Documents\"."),
        _ => format!("Aku nggak nemu folder {name}. Sebutin folder di atasnya, misalnya \"{name} di Documents\"."),
    }
}

/// v1's `focusMany(n, name, list)`.
pub fn focus_many(app: &AppHandle, n: usize, name: &str, list: &str) -> String {
    match lang(app) {
        "en" => format!("There are {n} folders called {name}: {list}. Which one?"),
        _ => format!("Ada {n} folder {name}: {list}. Yang mana?"),
    }
}

/// v1's `focusIn(where)`.
pub fn focus_in(app: &AppHandle, place: &str) -> String {
    match lang(app) {
        "en" => format!("in {place}"),
        _ => format!("di {place}"),
    }
}

/// v1's `uploaded(n)`.
pub fn uploaded(app: &AppHandle, n: usize) -> String {
    match (lang(app), n) {
        ("en", 1) => "It's uploaded to Google Drive.".into(),
        ("en", n) => format!("{n} files are uploaded to Google Drive."),
        (_, 1) => "Sudah aku upload ke Google Drive.".into(),
        (_, n) => format!("{n} file sudah aku upload ke Google Drive."),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn detects_the_question_language() {
        assert_eq!(super::detect_lang("tombol save di mana ya?"), Some("id"));
        assert_eq!(super::detect_lang("where is the save button?"), Some("en"));
        assert_eq!(super::detect_lang("Photoshop"), None);
    }
}
