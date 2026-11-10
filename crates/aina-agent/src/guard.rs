// What aina may change on the user's PC. Their own folders and data drives: yes. Off limits: Windows,
// program folders, AppData (apps' data, aina's own config and keys), dot-folders in the home folder
// (.ssh, .claude…) and system folders on any drive. The home, Users and known folders themselves
// (Desktop, Documents…) and drive roots can't be deleted or moved, only their contents.
// Port of real/inside/protectedReason/fixedReason/expandPath in src/agentTools.js.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

fn env(k: &str) -> String {
    std::env::var(k).unwrap_or_default()
}

pub fn home() -> PathBuf {
    std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("C:\\"))
}

// Path::canonicalize answers with \\?\C:\…; people and the model write C:\….
fn plain(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    match s.strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => p,
    }
}

/// Absolute, with `.` and `..` folded (like Node's path.resolve), without touching the disk.
pub fn resolve(p: &Path) -> PathBuf {
    let abs = if p.is_absolute() { p.to_path_buf() } else { std::env::current_dir().unwrap_or_default().join(p) };
    let mut out = PathBuf::new();
    for c in abs.components() {
        match c {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            c => out.push(c.as_os_str()),
        }
    }
    out
}

/// Real, long-form path (8.3 names like SHARIN~1 and links resolved) for the part that exists.
pub fn real(p: &Path) -> PathBuf {
    let full = resolve(p);
    let mut head = full.clone();
    let mut rest = Vec::new();
    while !head.exists() {
        let Some(up) = head.parent().map(Path::to_path_buf) else { return full };
        if let Some(name) = head.file_name() {
            rest.insert(0, name.to_os_string());
        }
        head = up;
    }
    match head.canonicalize() {
        Ok(r) => rest.iter().fold(plain(r), |acc, n| acc.join(n)),
        Err(_) => full,
    }
}

fn norm(p: &Path) -> String {
    real(p).to_string_lossy().to_lowercase()
}

pub fn inside(file: &Path, dir: &Path) -> bool {
    let f = norm(&if file.is_absolute() { file.to_path_buf() } else { dir.join(file) });
    let d = norm(dir);
    f == d || f.starts_with(&format!("{d}\\"))
}

/// The focus lock: while the person is focused on a folder with the lock on, aina's AI changes files
/// only inside it. Why a change touching `paths` is refused, or None when it stays inside. A change
/// whose places can't be seen (no paths) is let through to the usual checks.
pub fn outside_focus(paths: &[PathBuf], focus: &Path) -> Option<String> {
    let out: Vec<String> = paths.iter().filter(|p| !inside(p, focus)).map(|p| p.display().to_string()).collect();
    (!out.is_empty()).then(|| {
        format!(
            "Not allowed: the person is focused on {} and asked aina to change files only inside it (outside: {}). Reading elsewhere is fine; make the change inside the focus folder, or tell them it needs a place outside it.",
            focus.display(),
            out.join(", ")
        )
    })
}

pub fn lower(p: &Path) -> String {
    resolve(p).to_string_lossy().to_lowercase()
}

pub fn protected_roots() -> Vec<String> {
    let sys = Some(env("SystemDrive")).filter(|s| !s.is_empty()).unwrap_or_else(|| "C:".into());
    let or = |k: &str, d: String| Some(env(k)).filter(|s| !s.is_empty()).unwrap_or(d);
    [
        or("SystemRoot", format!("{sys}\\Windows")),
        or("ProgramFiles", format!("{sys}\\Program Files")),
        or("ProgramFiles(x86)", format!("{sys}\\Program Files (x86)")),
        env("ProgramW6432"),
        or("ProgramData", format!("{sys}\\ProgramData")),
        format!("{sys}\\Recovery"),
        format!("{sys}\\Boot"),
        home().join("AppData").to_string_lossy().into_owned(),
    ]
    .into_iter()
    .filter(|p| !p.is_empty())
    .map(|p| lower(Path::new(&p)))
    .collect()
}

const KNOWN: [&str; 12] = ["Desktop", "Documents", "Downloads", "Pictures", "Music", "Videos", "OneDrive", "Favorites", "Contacts", "Links", "Saved Games", "Searches"];

pub fn fixed_folders() -> Vec<String> {
    let h = home();
    let mut out = vec![lower(&h), h.parent().map(lower).unwrap_or_default()];
    out.extend(KNOWN.iter().map(|k| lower(&h.join(k))));
    out
}

static SYSTEM_ON_ANY_DRIVE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z]:\\(\$recycle\.bin|system volume information|pagefile\.sys|hiberfil\.sys|swapfile\.sys)(\\|$)").unwrap());

/// Why a path may not be changed (None if it may). `dir`: a working folder that is always allowed.
pub fn protected_reason(p: &Path, dir: Option<&Path>) -> Option<String> {
    if dir.is_some_and(|d| inside(p, d)) {
        return None;
    }
    let f = norm(p);
    let under = |root: &String| f == *root || f.starts_with(&format!("{root}\\"));
    let shown = p.display();
    if protected_roots().iter().any(under) {
        return Some(format!("{shown} is a system, program or app-data location"));
    }
    let h = lower(&home());
    if let Some(rel) = f.strip_prefix(&format!("{h}\\"))
        && rel.starts_with('.')
    {
        return Some(format!("{shown} is a hidden settings folder"));
    }
    if SYSTEM_ON_ANY_DRIVE.is_match(&f) {
        return Some(format!("{shown} is a system location"));
    }
    None
}

static DRIVE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-z]:\\?$|^\\\\[^\\]+\\[^\\]+\\?$").unwrap());

/// Folders that may hold changes but must never be deleted, moved or renamed themselves.
pub fn fixed_reason(p: &Path) -> Option<String> {
    let f = norm(p);
    if DRIVE.is_match(&f) {
        return Some(format!("{} is a whole drive", p.display()));
    }
    if fixed_folders().contains(&f.trim_end_matches('\\').to_string()) {
        return Some(format!("{} is one of the person's main folders", p.display()));
    }
    None
}

static ENV_PS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\$\{?env:([a-z0-9_()]+)\}?").unwrap());
static ENV_CMD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)%([a-z0-9_()]+)%").unwrap());
static HOME_PS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^\$home\b").unwrap());

fn env_ci(k: &str) -> Option<String> {
    std::env::vars().find(|(name, _)| name.eq_ignore_ascii_case(k)).map(|(_, v)| v).filter(|v| !v.is_empty())
}

/// ~, $HOME, $env:NAME, ${env:NAME} and %NAME% in a path, expanded.
pub fn expand_path(p: &str) -> String {
    let h = home().to_string_lossy().into_owned();
    let mut s = p.to_string();
    if s == "~" || s.starts_with("~\\") || s.starts_with("~/") {
        s = format!("{h}{}", &s[1..]);
    }
    s = HOME_PS.replace(&s, regex::NoExpand(&h)).into_owned();
    s = ENV_PS.replace_all(&s, |c: &regex::Captures| env_ci(&c[1]).unwrap_or_else(|| c[0].to_string())).into_owned();
    ENV_CMD.replace_all(&s, |c: &regex::Captures| env_ci(&c[1]).unwrap_or_else(|| c[0].to_string())).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_focus_lock_keeps_changes_inside_the_folder() {
        let f = Path::new("D:\\Projects\\aina");
        assert_eq!(outside_focus(&[PathBuf::from("D:\\Projects\\aina\\src\\main.rs"), PathBuf::from("D:\\PROJECTS\\Aina\\x")], f), None);
        let why = outside_focus(&[PathBuf::from("D:\\Projects\\aina\\a.rs"), PathBuf::from("D:\\Projects\\other\\b.rs")], f).unwrap();
        assert!(why.contains("D:\\Projects\\other\\b.rs") && !why.contains("a.rs,"));
        assert_eq!(outside_focus(&[PathBuf::from("D:\\Projects\\aina-old\\x")], f).is_some(), true); // a sibling with the same prefix is outside
        assert_eq!(outside_focus(&[], f), None);
    }

    #[test]
    fn system_and_app_data_are_protected() {
        let h = home();
        assert!(protected_reason(Path::new(r"C:\Windows\System32\x.dll"), None).is_some());
        assert!(protected_reason(&h.join(r"AppData\Roaming\aina\config.json"), None).is_some());
        assert!(protected_reason(&h.join(r".ssh\id_rsa"), None).is_some());
        assert!(protected_reason(Path::new(r"D:\$Recycle.Bin\x"), None).is_some());
        assert!(protected_reason(&h.join(r"Documents\aina\a.md"), None).is_none());
        // A working folder is always allowed, even under AppData.
        let work = h.join(r"AppData\Roaming\aina\agents\general");
        assert!(protected_reason(&work.join("out.md"), Some(&work)).is_none());
    }

    #[test]
    fn drives_and_main_folders_stay_put() {
        let h = home();
        assert!(fixed_reason(Path::new(r"D:\")).is_some());
        assert!(fixed_reason(&h.join("Documents")).is_some());
        assert!(fixed_reason(&h).is_some());
        assert!(fixed_reason(&h.join(r"Documents\aina")).is_none());
    }

    #[test]
    fn paths_expand() {
        let h = home().to_string_lossy().into_owned();
        assert_eq!(expand_path(r"~\Documents"), format!(r"{h}\Documents"));
        assert_eq!(expand_path(r"$HOME\x"), format!(r"{h}\x"));
        assert_eq!(expand_path(r"%USERPROFILE%\x"), format!(r"{h}\x"));
        assert_eq!(expand_path(r"$env:USERPROFILE\x"), format!(r"{h}\x"));
        assert_eq!(expand_path(r"%NOPE_NOT_SET%\x"), r"%NOPE_NOT_SET%\x");
        assert_eq!(resolve(Path::new(r"C:\a\..\b\.\c")), PathBuf::from(r"C:\b\c"));
    }
}
