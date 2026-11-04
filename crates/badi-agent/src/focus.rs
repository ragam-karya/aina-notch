// Focus mode: "fokus di folder weather-invitation" makes that folder where Badi works until the user
// says otherwise. This finds the folder from what the user said. Port of src/focus.js.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use crate::guard::{home, real};

const SKIP: [&str; 17] = [
    "node_modules", ".git", "appdata", "$recycle.bin", "system volume information", "windows", "program files", "program files (x86)", "programdata", "recovery", ".venv",
    "venv", "__pycache__", "dist", "build", ".next", "vendor",
];
const MAX_DEPTH: usize = 5;
const MAX_DIRS: usize = 40_000; // a ceiling on how much one lookup walks

fn norm(s: &str) -> String {
    s.to_lowercase().replace(['_', '-'], " ").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Where people keep their projects: their own folders first, then the other drives.
pub fn default_roots() -> Vec<PathBuf> {
    let h = home();
    let sys = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into()).to_uppercase();
    let mut roots: Vec<PathBuf> = ["Desktop", "Documents", "Downloads", "OneDrive", "source", "Projects", "Development", "dev", "code"].iter().map(|f| h.join(f)).collect();
    roots.push(h);
    roots.extend("DEFGHIJ".chars().map(|d| format!("{d}:")).filter(|d| *d != sys).map(|d| PathBuf::from(format!("{d}\\"))));
    roots.into_iter().filter(|p| p.exists()).collect()
}

/// Folders whose name matches the last part of what was said ("Development/weather invitation" also
/// needs a "Development" folder somewhere above it). Exact names come first, then shallower ones.
pub fn find_folders(phrase: &str, roots: &[PathBuf], limit: usize) -> Vec<PathBuf> {
    let parts: Vec<&str> = phrase.split(['\\', '/']).map(str::trim).filter(|p| !p.is_empty()).collect();
    let Some(last) = parts.last() else { return vec![] };
    // A full path that exists needs no search.
    let p = Path::new(phrase);
    if p.is_absolute() && p.is_dir() {
        return vec![real(p)];
    }
    let want = norm(last);
    let parents: Vec<String> = parts[..parts.len() - 1].iter().map(|p| norm(p)).collect();
    let mut found: HashMap<String, (PathBuf, bool, usize)> = HashMap::new();
    let mut seen = HashSet::new();
    let mut queue: VecDeque<(PathBuf, usize)> = roots.iter().map(|r| (r.clone(), 0)).collect();
    let mut walked = 0;
    while let Some((dir, depth)) = queue.pop_front() {
        if walked >= MAX_DIRS {
            break;
        }
        if !seen.insert(dir.to_string_lossy().to_lowercase()) {
            continue;
        }
        walked += 1;
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let Ok(ft) = e.file_type() else { continue };
            if !ft.is_dir() || ft.is_symlink() {
                continue;
            }
            let raw = e.file_name().to_string_lossy().into_owned();
            let name = norm(&raw);
            let full = e.path();
            if name == want || name.contains(&want) {
                let shown = full.to_string_lossy().into_owned();
                let above: Vec<String> = shown.split('\\').map(norm).collect();
                let above = &above[..above.len() - 1];
                if parents.iter().all(|p| above.iter().any(|a| a == p || a.contains(p.as_str()))) {
                    let depth_n = shown.split('\\').count();
                    found.insert(shown.to_lowercase(), (full.clone(), name == want, depth_n));
                }
            }
            if depth + 1 < MAX_DEPTH && !SKIP.contains(&raw.to_lowercase().as_str()) && !raw.starts_with('.') {
                queue.push_back((full, depth + 1));
            }
        }
    }
    let mut list: Vec<_> = found.into_values().collect();
    list.sort_by(|a, b| b.1.cmp(&a.1).then(a.2.cmp(&b.2)).then(a.0.cmp(&b.0)));
    list.into_iter().take(limit).map(|f| f.0).collect()
}

pub fn find(phrase: &str) -> Vec<PathBuf> {
    find_folders(phrase, &default_roots(), 8)
}

/// How to tell candidates apart out loud: the folder above each one ("di Development").
pub fn where_label(p: &Path) -> String {
    let parent = p.parent().unwrap_or(p);
    parent.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| parent.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_folders_by_spoken_name_scoped_by_the_parent() {
        let root = std::env::temp_dir().join(format!("badi-focus-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for d in [r"Development\weather-invitation", r"Downloads\weather_invitation", r"Development\node_modules\weather-invitation"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        let roots = [root.clone()];
        let all = find_folders("weather invitation", &roots, 8);
        assert_eq!(all.len(), 2, "{all:?}"); // node_modules is skipped
        let scoped = find_folders("Development/weather invitation", &roots, 8);
        assert_eq!(scoped.len(), 1);
        assert_eq!(where_label(&scoped[0]), "Development");
        assert_eq!(find_folders(&root.display().to_string(), &roots, 8).len(), 1); // a full path needs no search
        assert!(find_folders("tidak-ada-folder-ini", &roots, 8).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }
}
