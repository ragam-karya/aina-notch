// Dictations, kept local-first as Markdown files in Documents\Badi\Dikte (or the `dictationFolder`
// setting): one file per dictation, named by its time, with a small front matter (when, how long, the
// language, the word count) and, for a long one, a summary above the text. They're the user's own
// files: open them in any editor, search them, or have an agent read them.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Datelike, Local, Timelike};

/// Above this many words a dictation also gets a summary.
pub const SUMMARY_WORDS: usize = 120;
const MONTHS_ID: [&str; 12] = ["Januari", "Februari", "Maret", "April", "Mei", "Juni", "Juli", "Agustus", "September", "Oktober", "November", "Desember"];
const MONTHS_EN: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

pub struct Note<'a> {
    pub created: DateTime<Local>,
    pub duration_s: u64,
    pub lang: &'a str,
    pub summary: Option<&'a str>,
    pub text: &'a str,
}

pub fn words(text: &str) -> usize {
    text.split_whitespace().count()
}

pub fn needs_summary(text: &str) -> bool {
    words(text) > SUMMARY_WORDS
}

/// "2026-10-19 14.32.05.md": sorts by time and is a valid Windows name.
pub fn file_name(t: &DateTime<Local>) -> String {
    t.format("%Y-%m-%d %H.%M.%S.md").to_string()
}

pub fn render(n: &Note) -> String {
    let t = &n.created;
    let en = n.lang == "en";
    let month = if en { MONTHS_EN } else { MONTHS_ID }[t.month0() as usize];
    let title = if en { "Dictation" } else { "Dikte" };
    let mut out = format!(
        "---\ncreated: {}\nduration: {}\nlang: {}\nwords: {}\n---\n\n# {title} · {} {month} {} {:02}.{:02}\n\n",
        t.to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
        n.duration_s,
        n.lang,
        words(n.text),
        t.day(),
        t.year(),
        t.hour(),
        t.minute()
    );
    if let Some(s) = n.summary.map(str::trim).filter(|s| !s.is_empty()) {
        out.push_str(&format!("## {}\n\n{s}\n\n", if en { "Summary" } else { "Ringkasan" }));
    }
    out.push_str(&format!("## {}\n\n{}\n", if en { "Text" } else { "Isi" }, n.text.trim()));
    out
}

/// Saves a dictation into `dir` (made when missing) and returns its file.
pub fn save(dir: &Path, n: &Note) -> Result<PathBuf, String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let file = badi_agent::files::target_path(dir, &file_name(&n.created), &|p| p.exists());
    fs::write(&file, render(n)).map_err(|e| e.to_string())?;
    Ok(file)
}

/// A note read back: when (ms), its summary and its text.
#[derive(Debug, PartialEq)]
pub struct Read {
    pub created_ms: i64,
    pub summary: String,
    pub text: String,
}

/// Reads a note file. Only files with Badi's front matter count; the sections may have been edited by
/// hand, so a body without them is taken as the text.
pub fn parse(content: &str) -> Option<Read> {
    let content = content.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let rest = content.strip_prefix("---\n")?;
    let end = rest.find("\n---")?;
    let (front, body) = (&rest[..end], &rest[end + 4..]);
    let created = front.lines().find_map(|l| l.strip_prefix("created:")).map(str::trim)?;
    let created_ms = DateTime::parse_from_rfc3339(created).ok()?.timestamp_millis();
    // [summary, text]; before any section heading, lines are text.
    let mut parts = [String::new(), String::new()];
    let mut current = Some(1);
    for line in body.lines() {
        if let Some(h) = line.strip_prefix("## ") {
            current = match h.trim().to_lowercase().as_str() {
                "ringkasan" | "summary" => Some(0),
                "isi" | "text" => Some(1),
                _ => None,
            };
            continue;
        }
        if line.starts_with("# ") {
            continue;
        }
        if let Some(i) = current {
            parts[i].push_str(line);
            parts[i].push('\n');
        }
    }
    let [summary, text] = parts;
    Some(Read { created_ms, summary: summary.trim().to_string(), text: text.trim().to_string() })
}

/// The notes in `dir`, newest first, at most `limit`: (file, note).
pub fn list(dir: &Path, limit: usize) -> Vec<(PathBuf, Read)> {
    let mut out: Vec<(PathBuf, Read)> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "md"))
        .filter_map(|p| fs::read_to_string(&p).ok().and_then(|c| parse(&c)).map(|r| (p, r)))
        .collect();
    out.sort_by_key(|(_, r)| std::cmp::Reverse(r.created_ms));
    out.truncate(limit);
    out
}

/// How many note files there are (for the notch; no file is read).
pub fn count(dir: &Path) -> usize {
    fs::read_dir(dir).into_iter().flatten().flatten().filter(|e| e.path().extension().is_some_and(|x| x == "md")).count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at() -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 10, 19, 14, 32, 5).unwrap()
    }

    #[test]
    fn a_dictation_file_has_its_facts_a_summary_and_the_text() {
        let n = Note { created: at(), duration_s: 48, lang: "id", summary: Some("Rapat Q4."), text: "Besok rapat jam sepuluh.

Bawa laporan." };
        let file = render(&n);
        assert!(file.starts_with("---
created: 2026-10-19T14:32:05"));
        assert!(file.contains("duration: 48
lang: id
words: 6
---"));
        assert!(file.contains("# Dikte · 19 Oktober 2026 14.32

## Ringkasan

Rapat Q4.

## Isi

Besok rapat jam sepuluh.

Bawa laporan.
"));
        assert!(!render(&Note { summary: None, ..n }).contains("Ringkasan"));
        assert_eq!(file_name(&at()), "2026-10-19 14.32.05.md");
    }

    #[test]
    fn a_note_reads_back_even_after_hand_edits() {
        let n = Note { created: at(), duration_s: 48, lang: "id", summary: Some("Rapat Q4."), text: "Besok rapat.\n\nBawa laporan." };
        let r = parse(&render(&n).replace('\n', "\r\n")).unwrap();
        assert_eq!(r, Read { created_ms: at().timestamp_millis(), summary: "Rapat Q4.".into(), text: "Besok rapat.\n\nBawa laporan.".into() });
        let edited = parse("---\ncreated: 2026-10-19T14:32:05+07:00\n---\njust words\n").unwrap();
        assert_eq!((edited.text.as_str(), edited.summary.as_str()), ("just words", ""));
        assert!(parse("# notes\nhello").is_none());
    }

    #[test]
    fn only_long_dictations_get_a_summary() {
        assert!(!needs_summary(&"kata ".repeat(SUMMARY_WORDS)));
        assert!(needs_summary(&"kata ".repeat(SUMMARY_WORDS + 1)));
    }
}
