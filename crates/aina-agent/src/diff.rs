// Code diffs for what a Sprite changed, the way Coucou shows them: each edit becomes "main.rs +3 −1",
// and opens to its lines (added, removed, 3 lines of context around each change). Port of Coucou's
// DiffEngine.swift (LCS line diff, the same size limits). Also the whole task as a git diff: a
// snapshot of the repo when the task starts (git stash create, which changes nothing), compared with
// the working tree when it ends.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::files::hidden;

pub const MAX_BYTES: usize = 200 * 1024;
pub const MAX_LINES: usize = 4000;
/// Diffs kept per task.
pub const MAX_DIFFS: usize = 50;
const CONTEXT: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Context,
    Added,
    Removed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub kind: Kind,
    pub text: String,
    /// 1-based; 0 for an added line.
    pub old_no: usize,
    /// 1-based; 0 for a removed line.
    pub new_no: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Hunk {
    pub old_start: usize,
    pub new_start: usize,
    pub lines: Vec<Line>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FileDiff {
    pub path: String,
    pub added: usize,
    pub removed: usize,
    pub hunks: Vec<Hunk>,
    pub too_large: bool,
    pub new_file: bool,
}

impl FileDiff {
    pub fn name(&self) -> String {
        Path::new(&self.path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| self.path.clone())
    }

    /// "main.rs +3 −1", for a Sprite's activity line.
    pub fn line(&self) -> String {
        format!("✎ {} +{} −{}", self.name(), self.added, self.removed)
    }

    pub fn summary(&self, i: usize) -> Value {
        json!({ "i": i, "name": self.name(), "path": self.path, "added": self.added, "removed": self.removed, "newFile": self.new_file })
    }

    pub fn to_json(&self) -> Value {
        let kind = |k: Kind| match k {
            Kind::Context => " ",
            Kind::Added => "+",
            Kind::Removed => "-",
        };
        let hunks: Vec<Value> = self
            .hunks
            .iter()
            .map(|h| {
                let lines: Vec<Value> = h.lines.iter().map(|l| json!([kind(l.kind), l.old_no, l.new_no, l.text])).collect();
                json!({ "oldStart": h.old_start, "newStart": h.new_start, "lines": lines })
            })
            .collect();
        json!({ "name": self.name(), "path": self.path, "added": self.added, "removed": self.removed, "newFile": self.new_file, "tooLarge": self.too_large, "hunks": hunks })
    }

    /// The first changed line in the new file, to open the editor there.
    pub fn first_line(&self) -> usize {
        self.hunks.iter().flat_map(|h| &h.lines).find(|l| l.kind == Kind::Added).map(|l| l.new_no).unwrap_or(1)
    }
}

fn split_lines(text: &str) -> Vec<&str> {
    let mut v: Vec<&str> = text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect();
    if v.last() == Some(&"") {
        v.pop();
    }
    v
}

/// The diff between two versions of a file.
pub fn from_edit(old: &str, new: &str, path: &str) -> FileDiff {
    if old.len() + new.len() > MAX_BYTES {
        return count_fallback(old, new, path);
    }
    let (a, b) = (split_lines(old), split_lines(new));
    // LCS is O(m·n): stop before it blows up.
    if a.len() + b.len() > MAX_LINES || a.len() * b.len() > 1_000_000 {
        return count_fallback(old, new, path);
    }
    let flat = diff_lines(&a, &b);
    let added = flat.iter().filter(|l| l.kind == Kind::Added).count();
    let removed = flat.iter().filter(|l| l.kind == Kind::Removed).count();
    FileDiff { path: path.into(), added, removed, hunks: hunks(&flat), too_large: false, new_file: false }
}

/// A file written from nothing: every line is added.
pub fn from_new(content: &str, path: &str) -> FileDiff {
    let lines = split_lines(content);
    if content.len() > MAX_BYTES || lines.len() > MAX_LINES {
        return FileDiff { path: path.into(), added: lines.len(), removed: 0, hunks: vec![], too_large: true, new_file: true };
    }
    let lines: Vec<Line> = lines.iter().enumerate().map(|(i, t)| Line { kind: Kind::Added, text: t.to_string(), old_no: 0, new_no: i + 1 }).collect();
    let added = lines.len();
    let hunks = if lines.is_empty() { vec![] } else { vec![Hunk { old_start: 0, new_start: 1, lines }] };
    FileDiff { path: path.into(), added, removed: 0, hunks, too_large: false, new_file: true }
}

fn diff_lines(a: &[&str], b: &[&str]) -> Vec<Line> {
    let (m, n) = (a.len(), b.len());
    // dp[i][j]: LCS length of a[i..] and b[j..] (suffixes, so the walk below goes forward).
    let mut dp = vec![vec![0u32; n + 1]; m + 1];
    for i in (0..m).rev() {
        for j in (0..n).rev() {
            dp[i][j] = if a[i] == b[j] { dp[i + 1][j + 1] + 1 } else { dp[i + 1][j].max(dp[i][j + 1]) };
        }
    }
    let mut out = Vec::with_capacity(m + n);
    let (mut i, mut j) = (0, 0);
    while i < m || j < n {
        if i < m && j < n && a[i] == b[j] {
            out.push(Line { kind: Kind::Context, text: a[i].into(), old_no: i + 1, new_no: j + 1 });
            i += 1;
            j += 1;
        } else if j < n && (i == m || dp[i][j + 1] > dp[i + 1][j]) {
            out.push(Line { kind: Kind::Added, text: b[j].into(), old_no: 0, new_no: j + 1 });
            j += 1;
        } else {
            out.push(Line { kind: Kind::Removed, text: a[i].into(), old_no: i + 1, new_no: 0 });
            i += 1;
        }
    }
    out
}

fn hunks(lines: &[Line]) -> Vec<Hunk> {
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for (i, _) in lines.iter().enumerate().filter(|(_, l)| l.kind != Kind::Context) {
        let r = (i.saturating_sub(CONTEXT), (i + CONTEXT).min(lines.len() - 1));
        match ranges.last_mut() {
            Some(last) if r.0 <= last.1 + 1 => last.1 = last.1.max(r.1),
            _ => ranges.push(r),
        }
    }
    ranges
        .into_iter()
        .map(|(s, e)| {
            let part = lines[s..=e].to_vec();
            let old_start = part.iter().find(|l| l.old_no > 0).map(|l| l.old_no).unwrap_or(1);
            let new_start = part.iter().find(|l| l.new_no > 0).map(|l| l.new_no).unwrap_or(1);
            Hunk { old_start, new_start, lines: part }
        })
        .collect()
}

// Too large to show: only the counts, roughly (lines on one side and not the other).
fn count_fallback(old: &str, new: &str, path: &str) -> FileDiff {
    let (a, b) = (split_lines(old), split_lines(new));
    let (sa, sb): (HashSet<&str>, HashSet<&str>) = (a.iter().copied().collect(), b.iter().copied().collect());
    let added = b.iter().filter(|l| !l.is_empty() && !sa.contains(*l)).count();
    let removed = a.iter().filter(|l| !l.is_empty() && !sb.contains(*l)).count();
    FileDiff { path: path.into(), added, removed, hunks: vec![], too_large: true, new_file: false }
}

// ---------- the edits a tool is about to make ----------

/// What an edit tool call will do to a file, worked out before it runs (Claude Code's Edit,
/// MultiEdit and Write, from the PreToolUse hook). None when it changes nothing or can't apply.
pub fn for_tool(name: &str, input: &Value, dir: &Path) -> Option<FileDiff> {
    let p = input["file_path"].as_str()?;
    let file = if Path::new(p).is_absolute() { PathBuf::from(p) } else { dir.join(p) };
    let shown = file.display().to_string();
    let before = fs::read_to_string(&file).ok();
    let after = match name {
        "Write" => input["content"].as_str()?.to_string(),
        "Edit" => apply(before.as_deref()?, input)?,
        "MultiEdit" => {
            let mut text = before.clone()?;
            for e in input["edits"].as_array()? {
                text = apply(&text, e)?;
            }
            text
        }
        _ => return None,
    };
    let d = match before {
        Some(b) => from_edit(&b, &after, &shown),
        None => from_new(&after, &shown),
    };
    (d.added + d.removed > 0 || d.new_file).then_some(d)
}

fn apply(text: &str, e: &Value) -> Option<String> {
    let old = e["old_string"].as_str().filter(|o| !o.is_empty())?;
    let new = e["new_string"].as_str().unwrap_or("");
    if !text.contains(old) {
        return None;
    }
    Some(if e["replace_all"].as_bool() == Some(true) { text.replace(old, new) } else { text.replacen(old, new, 1) })
}

// ---------- the whole task as a git diff ----------

/// The repo's state when a task starts.
#[derive(Debug, Clone)]
pub struct GitSnap {
    root: PathBuf,
    base: String,
    untracked: HashSet<String>,
}

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = hidden("git").arg("-C").arg(dir).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

fn untracked(root: &Path) -> HashSet<String> {
    git(root, &["ls-files", "--others", "--exclude-standard", "-z"]).map(|s| s.split('\0').filter(|p| !p.is_empty()).map(String::from).collect()).unwrap_or_default()
}

/// A snapshot of the repo `dir` is in, without touching it: `git stash create` records the working
/// tree's tracked changes as a commit object and leaves everything as it was; with nothing to stash it
/// prints nothing and HEAD is the base. None outside a repo (or without git).
pub fn git_snapshot(dir: &Path) -> Option<GitSnap> {
    let root = PathBuf::from(git(dir, &["rev-parse", "--show-toplevel"])?.trim());
    let base = match git(&root, &["stash", "create"]).map(|s| s.trim().to_string()) {
        Some(s) if !s.is_empty() => s,
        _ => git(&root, &["rev-parse", "HEAD"])?.trim().to_string(),
    };
    Some(GitSnap { untracked: untracked(&root), root, base })
}

/// Everything the task changed in the repo: tracked files against the snapshot, and files that are new
/// since it (untracked then and now). At most MAX_DIFFS files.
pub fn git_changes(snap: &GitSnap) -> Vec<FileDiff> {
    let mut out = Vec::new();
    let changed = git(&snap.root, &["diff", "--name-only", "-z", &snap.base, "--"]).unwrap_or_default();
    for rel in changed.split('\0').filter(|p| !p.is_empty()).take(MAX_DIFFS) {
        let file = snap.root.join(rel);
        let shown = file.display().to_string();
        let old = git(&snap.root, &["show", &format!("{}:{rel}", snap.base)]);
        let new = fs::read_to_string(&file).unwrap_or_default();
        out.push(match old {
            Some(o) => from_edit(&o, &new, &shown),
            None => from_new(&new, &shown),
        });
    }
    let mut fresh: Vec<String> = untracked(&snap.root).into_iter().filter(|p| !snap.untracked.contains(p)).collect();
    fresh.sort();
    for rel in fresh {
        if out.len() >= MAX_DIFFS {
            break;
        }
        let file = snap.root.join(&rel);
        if let Ok(text) = fs::read_to_string(&file) {
            out.push(from_new(&text, &file.display().to_string()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // The cases from Coucou's DiffEngineTests.swift.
    #[test]
    fn additions_removals_and_replacements() {
        let d = from_edit("", "hello\nworld\n", "/a/b.swift");
        assert_eq!((d.added, d.removed, d.too_large, d.hunks.len()), (2, 0, false, 1));
        assert_eq!(d.name(), "b.swift");
        let d = from_edit("hello\nworld\n", "", "/x.py");
        assert_eq!((d.added, d.removed, d.hunks.len()), (0, 2, 1));
        let d = from_edit("foo\nbar\nbaz\n", "foo\nqux\nbaz\n", "/f.ts");
        assert_eq!((d.added, d.removed), (1, 1));
        assert!(d.hunks[0].lines.iter().any(|l| l.kind == Kind::Context));
        assert_eq!(d.line(), "✎ f.ts +1 −1");
    }

    #[test]
    fn new_files_are_all_added_and_big_ones_only_counted() {
        let d = from_new("line1\nline2\nline3\n", "/new.rs");
        assert_eq!((d.added, d.removed, d.new_file), (3, 0, true));
        assert!(d.hunks.iter().flat_map(|h| &h.lines).all(|l| l.kind == Kind::Added));
        let d = from_edit(&"x".repeat(150 * 1024), &"y".repeat(60 * 1024), "/big.swift");
        assert!(d.too_large && d.hunks.is_empty());
    }

    #[test]
    fn hunks_keep_three_lines_of_context_and_real_line_numbers() {
        let old: String = (1..=20).map(|i| format!("l{i}\n")).collect();
        let new = old.replace("l5\n", "five\n").replace("l18\n", "");
        let d = from_edit(&old, &new.replace('\n', "\r\n"), "/n.txt"); // CRLF on one side only
        assert_eq!((d.added, d.removed), (1, 2));
        assert_eq!(d.hunks.len(), 2, "changes 13 lines apart are separate hunks");
        let h = &d.hunks[0];
        assert_eq!((h.old_start, h.new_start), (2, 2));
        assert_eq!(h.lines.first().unwrap().text, "l2");
        let five = h.lines.iter().find(|l| l.text == "five").unwrap();
        assert_eq!((five.old_no, five.new_no), (0, 5));
        assert_eq!(d.first_line(), 5);
        assert_eq!(from_edit("same\n", "same\n", "/s").hunks.len(), 0);
    }

    #[test]
    fn a_tool_call_is_diffed_against_the_whole_file() {
        let dir = std::env::temp_dir().join(format!("aina-diff-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("a.rs"), "fn a() {}\nfn b() {}\nfn c() {}\n").unwrap();
        let d = for_tool("Edit", &json!({ "file_path": "a.rs", "old_string": "fn b() {}", "new_string": "fn b() { todo!() }" }), &dir).unwrap();
        assert_eq!((d.added, d.removed), (1, 1));
        assert_eq!(d.hunks[0].lines.iter().find(|l| l.kind == Kind::Added).unwrap().new_no, 2);
        let d = for_tool("MultiEdit", &json!({ "file_path": "a.rs", "edits": [{ "old_string": "fn a", "new_string": "fn x" }, { "old_string": "fn c", "new_string": "fn y" }] }), &dir).unwrap();
        assert_eq!((d.added, d.removed), (2, 2));
        let d = for_tool("Write", &json!({ "file_path": "new.md", "content": "# Hi\n" }), &dir).unwrap();
        assert!(d.new_file && d.added == 1);
        assert!(for_tool("Edit", &json!({ "file_path": "a.rs", "old_string": "nope", "new_string": "x" }), &dir).is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_git_task_diff_covers_edits_and_new_files_and_leaves_the_repo_alone() {
        let dir = std::env::temp_dir().join(format!("aina-gitdiff-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let run = |args: &[&str]| git(&dir, args).is_some();
        if !run(&["init", "-q"]) {
            return; // no git on this machine
        }
        run(&["config", "user.email", "t@t"]);
        run(&["config", "user.name", "t"]);
        fs::write(dir.join("readme.md"), "# Old title\ntext\n").unwrap();
        fs::write(dir.join("notes.txt"), "mine, uncommitted\n").unwrap();
        run(&["add", "readme.md"]);
        run(&["commit", "-qm", "init"]);
        fs::write(dir.join("readme.md"), "# Old title\ntext\nlocal edit\n").unwrap(); // uncommitted before the task
        let snap = git_snapshot(&dir).unwrap();
        assert!(git(&dir, &["status", "--porcelain"]).unwrap().contains("readme.md"), "the snapshot changes nothing");
        // The task.
        fs::write(dir.join("readme.md"), "# New title\ntext\nlocal edit\n").unwrap();
        fs::write(dir.join("todo.md"), "- a\n- b\n").unwrap();
        let ds = git_changes(&snap);
        let names: Vec<String> = ds.iter().map(|d| d.name()).collect();
        assert_eq!(names, ["readme.md", "todo.md"], "notes.txt was there before the task");
        assert_eq!((ds[0].added, ds[0].removed), (1, 1), "only the task's change, not the earlier local edit");
        assert!(ds[1].new_file && ds[1].added == 2);
        let _ = fs::remove_dir_all(&dir);
    }
}
