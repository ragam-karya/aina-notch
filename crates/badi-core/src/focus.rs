// Focus: what the person is working on right now (a project folder, or a context with just a name),
// shared by the notch, the pomodoro and Badi's AI. Pomodoro sets the time; focus sets the work. A
// session runs from start to Selesai (or to another project); ending it keeps a record, with an
// optional short summary, for the history. Kept by the caller in config.json (focusSession,
// focusHistory). Pure apart from the clock passed in.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Records kept in the history, newest first.
pub const HISTORY_MAX: usize = 50;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Session {
    pub name: String,
    /// The project folder, or "" for a context without one.
    pub folder: String,
    /// A line on what the work is about.
    pub about: String,
    /// What this session should get done ("finish the login PR").
    pub goal: String,
    pub started_at: i64,
    /// Badi's AI changes files only inside `folder` (reading elsewhere is fine).
    pub lock: bool,
    /// Pomodoro focus sessions finished during this focus, and their time.
    pub pomodoros: u32,
    pub pomodoro_ms: u64,
    /// Notes written while working.
    pub note: String,
}

impl Session {
    /// A session on a folder, named after it.
    pub fn for_folder(folder: &str, now: i64) -> Session {
        let name = Path::new(folder).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| folder.to_string());
        Session { name, folder: folder.into(), started_at: now, lock: true, ..Default::default() }
    }

    pub fn elapsed(&self, now: i64) -> u64 {
        (now - self.started_at).max(0) as u64
    }

    /// The edits lock applies: a folder to keep them in, and the lock on.
    pub fn locked_folder(&self) -> Option<&str> {
        (self.lock && !self.folder.is_empty()).then_some(self.folder.as_str())
    }

    /// What Badi's AI is told about the work, in one short paragraph.
    pub fn brief(&self) -> String {
        let mut s = format!("The person is focused on \"{}\"", self.name);
        if !self.folder.is_empty() {
            s.push_str(&format!(" (project folder: {})", self.folder));
        }
        s.push('.');
        if !self.about.trim().is_empty() {
            s.push_str(&format!(" About it: {}.", self.about.trim().trim_end_matches('.')));
        }
        if !self.goal.trim().is_empty() {
            s.push_str(&format!(" Goal for this session: {}.", self.goal.trim().trim_end_matches('.')));
        }
        if let Some(f) = self.locked_folder() {
            s.push_str(&format!(" Keep every file change inside {f}: creating, editing, moving or deleting anything outside it is refused (reading elsewhere is fine)."));
        }
        s.push_str(" Keep answers and work tied to this unless they ask about something else.");
        s
    }
}

/// An ended session, with the optional summary written at the end.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Record {
    pub name: String,
    pub folder: String,
    pub goal: String,
    pub started_at: i64,
    pub ended_at: i64,
    /// How long the focus ran.
    pub ms: u64,
    pub pomodoros: u32,
    /// The focus time actually spent in pomodoro focus phases.
    pub pomodoro_ms: u64,
    pub note: String,
    pub done: String,
    pub pending: String,
    pub next: String,
}

/// The end of a session's summary: what got done, what is still open, the next step. All optional.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Summary {
    pub done: String,
    pub pending: String,
    pub next: String,
}

/// Ends `s` at `now` and adds its record to the front of `history` (sessions shorter than a minute
/// with nothing in them are not kept). Returns the record.
pub fn end(s: &Session, now: i64, summary: Summary, history: &mut Vec<Record>) -> Record {
    let r = Record {
        name: s.name.clone(),
        folder: s.folder.clone(),
        goal: s.goal.clone(),
        started_at: s.started_at,
        ended_at: now,
        ms: s.elapsed(now),
        pomodoros: s.pomodoros,
        pomodoro_ms: s.pomodoro_ms,
        note: s.note.trim().into(),
        done: summary.done.trim().into(),
        pending: summary.pending.trim().into(),
        next: summary.next.trim().into(),
    };
    let empty = r.ms < 60_000 && r.pomodoros == 0 && r.note.is_empty() && r.done.is_empty() && r.pending.is_empty() && r.next.is_empty();
    if !empty {
        history.insert(0, r.clone());
        history.truncate(HISTORY_MAX);
    }
    r
}

/// The last few distinct contexts to start again with one click (by folder, else by name), leaving
/// out the current one.
pub fn recent(history: &[Record], current: Option<&Session>, n: usize) -> Vec<(String, String)> {
    let key = |name: &str, folder: &str| if folder.is_empty() { format!("n:{}", name.to_lowercase()) } else { format!("f:{}", folder.to_lowercase()) };
    let mut seen: Vec<String> = current.map(|c| key(&c.name, &c.folder)).into_iter().collect();
    let mut out = Vec::new();
    for r in history {
        let k = key(&r.name, &r.folder);
        if !seen.contains(&k) {
            seen.push(k);
            out.push((r.name.clone(), r.folder.clone()));
            if out.len() == n {
                break;
            }
        }
    }
    out
}

/// Time per project since `since` (ms), most first: (name, total ms, sessions).
pub fn totals(history: &[Record], since: i64) -> Vec<(String, u64, u32)> {
    let mut out: Vec<(String, u64, u32)> = Vec::new();
    for r in history.iter().filter(|r| r.ended_at >= since) {
        match out.iter_mut().find(|t| t.0.eq_ignore_ascii_case(&r.name)) {
            Some(t) => {
                t.1 += r.ms;
                t.2 += 1;
            }
            None => out.push((r.name.clone(), r.ms, 1)),
        }
    }
    out.sort_by(|a, b| b.1.cmp(&a.1));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: i64 = 60_000;

    #[test]
    fn a_folder_session_is_named_after_it_and_locked() {
        let s = Session::for_folder("D:\\Projects\\weather-invitation", 1_000);
        assert_eq!((s.name.as_str(), s.lock), ("weather-invitation", true));
        assert_eq!(s.locked_folder(), Some("D:\\Projects\\weather-invitation"));
        let ctx = Session { name: "Laporan Q4".into(), lock: true, ..Default::default() };
        assert_eq!(ctx.locked_folder(), None); // no folder, nothing to lock to
    }

    #[test]
    fn the_brief_says_what_the_work_is_and_keeps_edits_in_the_folder() {
        let s = Session { name: "badi".into(), folder: "D:\\badi".into(), about: "Tauri app.".into(), goal: "finish the notch PR".into(), lock: true, ..Default::default() };
        let b = s.brief();
        assert!(b.contains("focused on \"badi\" (project folder: D:\\badi)"));
        assert!(b.contains("About it: Tauri app.") && b.contains("Goal for this session: finish the notch PR."));
        assert!(b.contains("Keep every file change inside D:\\badi"));
        assert!(!Session { lock: false, ..s }.brief().contains("Keep every file change"));
    }

    #[test]
    fn ending_keeps_a_record_with_its_summary_unless_it_was_empty() {
        let mut h = Vec::new();
        let s = Session { name: "badi".into(), started_at: 0, pomodoros: 2, pomodoro_ms: 50 * MIN as u64, ..Default::default() };
        let r = end(&s, 75 * MIN, Summary { done: " notch pages ".into(), pending: "".into(), next: "release".into() }, &mut h);
        assert_eq!((r.ms, r.pomodoros, r.done.as_str(), r.next.as_str()), (75 * MIN as u64, 2, "notch pages", "release"));
        assert_eq!(h.len(), 1);
        end(&Session { name: "oops".into(), started_at: 0, ..Default::default() }, 20_000, Summary::default(), &mut h);
        assert_eq!(h.len(), 1); // 20 seconds with nothing in them: not kept
    }

    #[test]
    fn recent_contexts_are_distinct_and_skip_the_current_one() {
        let rec = |name: &str, folder: &str| Record { name: name.into(), folder: folder.into(), ..Default::default() };
        let h = vec![rec("badi", "D:\\badi"), rec("Laporan", ""), rec("badi", "D:\\BADI"), rec("site", "D:\\site"), rec("x", "D:\\x")];
        let now = Session::for_folder("D:\\site", 0);
        assert_eq!(recent(&h, Some(&now), 3), vec![("badi".into(), "D:\\badi".into()), ("Laporan".into(), "".into()), ("x".into(), "D:\\x".into())]);
    }

    #[test]
    fn totals_add_up_per_project_since_a_time() {
        let rec = |name: &str, end: i64, ms: u64| Record { name: name.into(), ended_at: end, ms, ..Default::default() };
        let h = vec![rec("badi", 10, 30), rec("site", 9, 50), rec("BADI", 8, 40), rec("old", 1, 999)];
        assert_eq!(totals(&h, 5), vec![("badi".into(), 70, 2), ("site".into(), 50, 1)]);
    }
}
