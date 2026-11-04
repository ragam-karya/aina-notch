// Pure helpers for the notch: what a drop means, and the one-line agent status it shows.
// Port of src/notch.js.

use serde_json::{Value, json};

pub const MAX_UPLOAD: u64 = 100 * 1024 * 1024; // per file

pub struct Entry {
    pub path: String,
    pub dir: bool,
    pub size: u64,
}

/// What a drop on the notch means for its purpose ("focus" | "upload"):
/// { focus } | { upload, skipped } | { problem, … } for Badi to explain. Same JSON as v1's classifyDrop.
pub fn classify_drop(entries: &[Entry], purpose: &str) -> Value {
    let dirs: Vec<&Entry> = entries.iter().filter(|e| e.dir).collect();
    let files: Vec<&Entry> = entries.iter().filter(|e| !e.dir).collect();
    if entries.is_empty() {
        return json!({ "problem": "empty" });
    }
    match purpose {
        "focus" => {
            if dirs.len() == 1 && files.is_empty() {
                json!({ "focus": dirs[0].path })
            } else if dirs.is_empty() {
                json!({ "problem": "needFolder" })
            } else {
                json!({ "problem": "oneFolder" })
            }
        }
        "upload" => {
            if files.is_empty() {
                return json!({ "problem": "needFiles" });
            }
            if let Some(big) = files.iter().find(|f| f.size > MAX_UPLOAD) {
                return json!({ "problem": "tooBig", "path": big.path });
            }
            json!({ "upload": files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(), "skipped": dirs.len() })
        }
        _ => json!({ "problem": "unknown" }),
    }
}

/// An agent's newest log chunk as a short status line: first non-empty line, no markdown or tool noise.
pub fn agent_line(chunk: &str) -> String {
    let line = chunk
        .lines()
        .map(|l| {
            let l = l.trim_start_matches(|c: char| c.is_whitespace() || ">*#`•-–".contains(c));
            l.chars().filter(|c| !"`*_".contains(*c)).collect::<String>().trim().to_string()
        })
        .find(|l| !l.is_empty() && !l.starts_with(['[', '{', '(']));
    match line {
        Some(l) if l.chars().count() > 60 => format!("{}…", l.chars().take(59).collect::<String>().trim_end()),
        Some(l) => l,
        None => String::new(),
    }
}

/// A Sprite's state as the notch's Tasks page names it: "needs" (waits for the person), "queued"
/// (not started yet), "running", "failed", "completed" or "cancelled".
pub fn task_status(state: &str) -> &'static str {
    match state {
        "waiting" | "needs_approval" => "needs",
        "starting" => "queued",
        "error" => "failed",
        "success" => "completed",
        "cancelled" => "cancelled",
        _ => "running",
    }
}

/// Where a task goes on the Tasks page, smaller first: what needs the person, then failures, then
/// work in progress (the longest-running first, since that is the one that may be stuck), then the
/// finished (the latest first). Creation order alone never decides.
pub fn task_order(status: &str, started: i64, ended: Option<i64>) -> (u8, i64) {
    let rank = match status {
        "needs" => 0,
        "failed" => 1,
        "running" => 2,
        "queued" => 3,
        "completed" => 4,
        _ => 5,
    };
    (rank, if rank <= 3 && status != "failed" { started } else { -ended.unwrap_or(started) })
}

// Same cases as the drop and agent-line tests in test/notch.test.js.
#[cfg(test)]
mod tests {
    use super::*;

    fn dir(p: &str) -> Entry {
        Entry { path: p.into(), dir: true, size: 0 }
    }
    fn file(p: &str, size: u64) -> Entry {
        Entry { path: p.into(), dir: false, size }
    }

    #[test]
    fn drops_a_folder_for_focus_files_for_upload_problems_otherwise() {
        assert_eq!(classify_drop(&[dir("C:\\proj")], "focus"), json!({ "focus": "C:\\proj" }));
        assert_eq!(classify_drop(&[file("C:\\a.pdf", 1000)], "focus"), json!({ "problem": "needFolder" }));
        assert_eq!(classify_drop(&[dir("C:\\proj"), dir("C:\\b")], "focus"), json!({ "problem": "oneFolder" }));
        assert_eq!(classify_drop(&[file("C:\\a.pdf", 1000), dir("C:\\proj")], "upload"), json!({ "upload": ["C:\\a.pdf"], "skipped": 1 }));
        assert_eq!(classify_drop(&[dir("C:\\proj")], "upload"), json!({ "problem": "needFiles" }));
        assert_eq!(classify_drop(&[file("C:\\a.pdf", MAX_UPLOAD + 1)], "upload"), json!({ "problem": "tooBig", "path": "C:\\a.pdf" }));
        assert_eq!(classify_drop(&[], "upload"), json!({ "problem": "empty" }));
    }

    #[test]
    fn agent_lines_the_first_readable_line_short() {
        assert_eq!(agent_line("\n> **Membaca** `laporan.md`\n"), "Membaca laporan.md");
        assert_eq!(agent_line("{\"tool\":\"x\"}\n"), "");
        let long = agent_line(&"x".repeat(100));
        assert!(long.ends_with('…'));
        assert_eq!(long.chars().count(), 60);
    }

    #[test]
    fn tasks_are_ordered_by_what_needs_attention() {
        assert_eq!(task_status("needs_approval"), "needs");
        assert_eq!(task_status("starting"), "queued");
        assert_eq!(task_status("working"), "running");
        assert_eq!(task_status("error"), "failed");
        let mut list = vec![
            ("completed", 10, Some(50)),
            ("running", 30, None),
            ("cancelled", 5, Some(60)),
            ("failed", 1, Some(20)),
            ("running", 20, None),
            ("needs", 40, None),
            ("queued", 45, None),
            ("failed", 2, Some(40)),
            ("completed", 12, Some(70)),
        ];
        list.sort_by_key(|(s, a, e)| task_order(s, *a, *e));
        let got: Vec<(&str, i64)> = list.iter().map(|(s, a, _)| (*s, *a)).collect();
        assert_eq!(got, vec![("needs", 40), ("failed", 2), ("failed", 1), ("running", 20), ("running", 30), ("queued", 45), ("completed", 12), ("completed", 10), ("cancelled", 5)]);
    }
}
