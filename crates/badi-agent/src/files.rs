// High-level file tools: find, list, move, copy, rename, create and delete (to the Recycle Bin) as
// single, checked operations instead of driving File Explorer, plus read/edit/write for text files.
// Every change passes the guard, asks the person through approve() unless they allowed it in Settings,
// never overwrites, and is verified on disk. Port of src/fileOps.js and save_file's src/brain/files.js.

use std::collections::{BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use regex::Regex;
use serde_json::{Value, json};

use crate::Need;
use crate::guard::{expand_path, fixed_reason, home, protected_reason, real};

const MAX_FOUND: usize = 50;
const MAX_LISTED: usize = 200;
const MAX_READ: usize = 120_000; // characters
const FIND_DEPTH: usize = 8;
const FIND_TIME: Duration = Duration::from_secs(30);
const USER_FOLDERS: [&str; 7] = ["Desktop", "Documents", "Downloads", "Pictures", "Music", "Videos", "OneDrive"];

pub fn file_tools() -> Vec<Value> {
    vec![
        json!({
            "name": "find_files",
            "description": "Find files and folders by name on the person's PC: a glob like \"*.pdf\" or \"laporan*\", or part of a name. Searches their folders (Desktop, Documents, Downloads, Pictures, Music, Videos, OneDrive) unless you give a folder. Returns full paths with size and date. Use it instead of browsing in File Explorer.",
            "input_schema": { "type": "object", "properties": { "pattern": { "type": "string" }, "folder": { "type": "string", "description": "Folder to search in, e.g. \"Documents/aina\" or a full path" } }, "required": ["pattern"] }
        }),
        json!({
            "name": "list_folder",
            "description": "List what is in a folder: names, file or folder, size and date. Paths may be full, or relative to the home folder like \"Documents/aina\".",
            "input_schema": { "type": "object", "properties": { "path": { "type": "string" } }, "required": ["path"] }
        }),
        json!({
            "name": "move_paths",
            "description": "Move files or folders into a folder (created if missing). Never overwrites: a name clash is reported with a free name to use instead. aina asks the person first when needed.",
            "input_schema": { "type": "object", "properties": { "paths": { "type": "array", "items": { "type": "string" } }, "to": { "type": "string", "description": "Destination folder" } }, "required": ["paths", "to"] }
        }),
        json!({
            "name": "copy_paths",
            "description": "Copy files or folders into a folder (created if missing). Never overwrites.",
            "input_schema": { "type": "object", "properties": { "paths": { "type": "array", "items": { "type": "string" } }, "to": { "type": "string", "description": "Destination folder" } }, "required": ["paths", "to"] }
        }),
        json!({
            "name": "rename_path",
            "description": "Rename a file or folder in place. new_name is just the name, not a path.",
            "input_schema": { "type": "object", "properties": { "path": { "type": "string" }, "new_name": { "type": "string" } }, "required": ["path", "new_name"] }
        }),
        json!({
            "name": "create_folder",
            "description": "Create a folder (and any missing parents).",
            "input_schema": { "type": "object", "properties": { "path": { "type": "string" } }, "required": ["path"] }
        }),
        json!({
            "name": "delete_paths",
            "description": "Delete files or folders by sending them to the Recycle Bin, so the person can undo it. aina asks the person first when needed; do not ask them yourself as well.",
            "input_schema": { "type": "object", "properties": { "paths": { "type": "array", "items": { "type": "string" } } }, "required": ["paths"] }
        }),
    ]
}

/// Reading and changing a file's text (the Brain's way to edit an existing file, never an app on screen).
pub fn edit_tools() -> Vec<Value> {
    vec![
        json!({
            "name": "read_file",
            "description": "Read a text file (code, HTML, notes…) with line numbers. Use it before editing a file. Long files: give start_line/end_line.",
            "input_schema": { "type": "object", "properties": { "path": { "type": "string" }, "start_line": { "type": "integer" }, "end_line": { "type": "integer" } }, "required": ["path"] }
        }),
        json!({
            "name": "edit_file",
            "description": "Change part of a text file: old_text must appear exactly once (copy it from read_file, without line numbers) and is replaced by new_text. Fails clearly if old_text is missing or ambiguous, so include enough surrounding lines.",
            "input_schema": { "type": "object", "properties": { "path": { "type": "string" }, "old_text": { "type": "string" }, "new_text": { "type": "string" } }, "required": ["path", "old_text", "new_text"] }
        }),
        json!({
            "name": "write_file",
            "description": "Create a text file anywhere the user keeps files, with its whole content. An existing file is only replaced with overwrite: true (read it first and keep what the user wrote); for small changes use edit_file. Returns the real path written.",
            "input_schema": { "type": "object", "properties": { "path": { "type": "string" }, "content": { "type": "string" }, "overwrite": { "type": "boolean" } }, "required": ["path", "content"] }
        }),
    ]
}

fn size(n: u64) -> String {
    match n {
        n if n < 1024 => format!("{n} B"),
        n if n < 1_048_576 => format!("{:.0} KB", n as f64 / 1024.0),
        n => format!("{:.1} MB", n as f64 / 1_048_576.0),
    }
}

fn day(t: SystemTime) -> String {
    let secs = t.duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    chrono_day(secs)
}

// yyyy-mm-dd (UTC) from Unix seconds, as Date.toISOString().slice(0, 10).
fn chrono_day(secs: i64) -> String {
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

fn describe(p: &Path) -> String {
    match fs::metadata(p) {
        Ok(m) if m.is_dir() => format!("{}\\ (folder, {})", p.display(), m.modified().map(day).unwrap_or_default()),
        Ok(m) => format!("{} ({}, {})", p.display(), size(m.len()), m.modified().map(day).unwrap_or_default()),
        Err(_) => p.display().to_string(),
    }
}

fn base_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string())
}

/// A free name next to an existing one: "laporan (2).pdf".
pub fn free_name(target: &Path) -> PathBuf {
    let dir = target.parent().unwrap_or(Path::new(""));
    let ext = target.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let stem = target.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    (2..).map(|i| dir.join(format!("{stem} ({i}){ext}"))).find(|p| !p.exists()).unwrap()
}

fn check_change(p: &Path, moving: bool) -> Result<(), String> {
    match protected_reason(p, None).or_else(|| if moving { fixed_reason(p) } else { None }) {
        Some(why) => Err(format!("Not allowed: {why}.")),
        None => Ok(()),
    }
}

/// A path as the person or the model wrote it: ~, $env:X and %X% expanded; relative ones are under
/// base (the focus folder, when one is set) or else the home folder.
pub fn resolve_user_path(p: &str, base: Option<&Path>) -> Result<PathBuf, String> {
    let t = p.trim().trim_matches(|c| c == '"' || c == '\'');
    if t.is_empty() {
        return Err("A path is required.".into());
    }
    let raw = PathBuf::from(expand_path(t).replace('/', "\\"));
    Ok(real(&if raw.is_absolute() { raw } else { base.map(Path::to_path_buf).unwrap_or_else(home).join(raw) }))
}

/// Sends paths to the Recycle Bin in one PowerShell process; the paths go in through the environment,
/// so nothing needs quoting.
pub fn recycle(paths: &[PathBuf]) -> Result<(), String> {
    let script = "$ErrorActionPreference = 'Stop'; Add-Type -AssemblyName Microsoft.VisualBasic; \
        foreach ($p in ($env:BADI_PATHS | ConvertFrom-Json)) { \
        if (Test-Path -LiteralPath $p -PathType Container) { [Microsoft.VisualBasic.FileIO.FileSystem]::DeleteDirectory($p, 'OnlyErrorDialogs', 'SendToRecycleBin') } \
        else { [Microsoft.VisualBasic.FileIO.FileSystem]::DeleteFile($p, 'OnlyErrorDialogs', 'SendToRecycleBin') } }";
    let list: Vec<String> = paths.iter().map(|p| p.display().to_string()).collect();
    let out = hidden("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script])
        .env("BADI_PATHS", serde_json::to_string(&list).unwrap())
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().lines().next().unwrap_or("Recycle Bin failed").to_string())
    }
}

/// A child process without a console window.
pub fn hidden(exe: &str) -> std::process::Command {
    let mut c = std::process::Command::new(exe);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    c
}

fn copy_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    if src.is_dir() {
        fs::create_dir(dst)?;
        for e in fs::read_dir(src)? {
            let e = e?;
            copy_all(&e.path(), &dst.join(e.file_name()))?;
        }
        Ok(())
    } else {
        if dst.exists() {
            return Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, format!("{} already exists", dst.display())));
        }
        fs::copy(src, dst).map(|_| ())
    }
}

pub fn glob_regex(glob: &str) -> Regex {
    let mut re = String::from("(?i)^");
    for c in glob.chars() {
        match c {
            '*' => re.push_str(".*"),
            '?' => re.push('.'),
            c => re.push_str(&regex::escape(&c.to_string())),
        }
    }
    re.push('$');
    Regex::new(&re).unwrap_or_else(|_| Regex::new("^$").unwrap())
}

fn arg<'a>(input: &'a Value, k: &str) -> &'a str {
    input[k].as_str().unwrap_or("")
}

/// The file tools for one task. `approve(need)` returns why the person said no, or None.
pub struct FileTools<'a> {
    pub base: Option<PathBuf>,
    pub approve: &'a (dyn Fn(&Need) -> Option<String> + Sync),
}

impl FileTools<'_> {
    fn path(&self, p: &str) -> Result<PathBuf, String> {
        resolve_user_path(p, self.base.as_deref())
    }

    fn ask(&self, op: &str, targets: &[PathBuf], place: Option<&Path>, new_name: Option<&str>) -> Result<(), String> {
        let names = targets.iter().map(|p| base_name(p)).collect();
        let list = targets.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ");
        let detail = match (new_name, place) {
            (Some(n), _) => format!("{op}: {list} → {n}"),
            (None, Some(p)) => format!("{op}: {list} → {}", p.display()),
            _ => format!("{op}: {list}"),
        };
        let mut paths = targets.to_vec();
        if let Some(p) = place.filter(|_| op != "delete") {
            paths.push(p.to_path_buf());
        }
        let need = Need { kind: "files".into(), op: op.into(), names, place: place.map(base_name), new_name: new_name.map(String::from), paths, detail, ..Need::default() };
        match (self.approve)(&need) {
            Some(no) => Err(no),
            None => Ok(()),
        }
    }

    fn existing(&self, list: &Value) -> Result<Vec<PathBuf>, String> {
        let items = list.as_array().filter(|a| !a.is_empty()).ok_or("\"paths\" must be a non-empty list.")?;
        items
            .iter()
            .map(|p| {
                let full = self.path(p.as_str().unwrap_or(""))?;
                if !full.exists() {
                    return Err(format!("{} does not exist. Use find_files to locate it.", full.display()));
                }
                Ok(full)
            })
            .collect()
    }

    fn dest_folder(&self, to: &str, sources: &[PathBuf], copy: bool) -> Result<PathBuf, String> {
        let dest = self.path(to)?;
        check_change(&dest, false)?;
        if dest.exists() && !dest.is_dir() {
            return Err(format!("{} is a file, not a folder.", dest.display()));
        }
        let low = |p: &Path| p.to_string_lossy().to_lowercase();
        for src in sources {
            check_change(src, !copy)?;
            let target = dest.join(src.file_name().unwrap_or_default());
            if low(&target) == low(src) {
                return Err(format!("{} is already in {}.", src.display(), dest.display()));
            }
            if format!("{}\\", low(&dest)).starts_with(&format!("{}\\", low(src))) {
                return Err(format!("Can't put {} inside itself.", src.display()));
            }
            if target.exists() {
                return Err(format!(
                    "{} already exists; nothing was changed. Rename one of them first, e.g. to \"{}\".",
                    target.display(),
                    base_name(&free_name(&target))
                ));
            }
        }
        Ok(dest)
    }

    /// Runs one file tool. None when `name` isn't one of them.
    pub fn run(&self, name: &str, input: &Value) -> Option<Result<String, String>> {
        Some(match name {
            "find_files" => self.find(arg(input, "pattern"), input["folder"].as_str()),
            "list_folder" => self.list(arg(input, "path")),
            "move_paths" => self.move_paths(&input["paths"], arg(input, "to")),
            "copy_paths" => self.copy_paths(&input["paths"], arg(input, "to")),
            "rename_path" => self.rename(arg(input, "path"), arg(input, "new_name")),
            "create_folder" => self.create_folder(arg(input, "path")),
            "delete_paths" => self.delete(&input["paths"]),
            "read_file" => self.read(arg(input, "path"), input["start_line"].as_u64(), input["end_line"].as_u64()),
            "edit_file" => self.edit(arg(input, "path"), arg(input, "old_text"), arg(input, "new_text")),
            "write_file" => self.write(arg(input, "path"), input["content"].as_str(), input["overwrite"].as_bool().unwrap_or(false)),
            _ => return None,
        })
    }

    fn find(&self, pattern: &str, folder: Option<&str>) -> Result<String, String> {
        if pattern.trim().is_empty() {
            return Err("\"pattern\" is required.".into());
        }
        // In scope: the folder given, else the focus folder, else the person's own folders.
        let roots: Vec<PathBuf> = match (folder, &self.base) {
            (Some(f), _) => vec![self.path(f)?],
            (None, Some(b)) => vec![b.clone()],
            _ => USER_FOLDERS.iter().map(|f| home().join(f)).filter(|p| p.exists()).collect(),
        };
        for r in &roots {
            if !r.exists() {
                return Err(format!("{} does not exist. Find the folder first (find_files with its name), or ask the user where it is.", r.display()));
            }
        }
        let glob = if pattern.contains(['*', '?', '[']) { pattern.to_string() } else { format!("*{pattern}*") };
        let re = glob_regex(&glob);
        let mut found = BTreeSet::new();
        let started = Instant::now();
        // Breadth first, so near matches come before deep ones. A matching folder is reported once,
        // not every file in it.
        let mut queue: VecDeque<(PathBuf, usize)> = roots.iter().map(|r| (r.clone(), 0)).collect();
        'walk: while let Some((dir, depth)) = queue.pop_front() {
            let Ok(entries) = fs::read_dir(&dir) else { continue };
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                if re.is_match(&name) {
                    found.insert(e.path());
                    if found.len() >= MAX_FOUND || started.elapsed() > FIND_TIME {
                        break 'walk;
                    }
                } else if is_dir && depth + 1 < FIND_DEPTH {
                    queue.push_back((e.path(), depth + 1));
                }
            }
        }
        let shown: Vec<String> = roots.iter().map(|r| r.display().to_string()).collect();
        if found.is_empty() {
            return Ok(format!("Nothing named like \"{pattern}\" in {}. Ask the user before searching anywhere wider.", shown.join(", ")));
        }
        let more = if found.len() >= MAX_FOUND { "\n(more results not shown; narrow the search)" } else { "" };
        Ok(found.iter().map(|p| describe(p)).collect::<Vec<_>>().join("\n") + more)
    }

    fn list(&self, p: &str) -> Result<String, String> {
        let dir = self.path(p)?;
        if !dir.exists() {
            return Err(format!("{} does not exist.", dir.display()));
        }
        let entries: Vec<_> = fs::read_dir(&dir).map_err(|e| e.to_string())?.flatten().collect();
        let prefix = format!("{}\\", dir.display());
        let lines: Vec<String> = entries.iter().take(MAX_LISTED).map(|e| describe(&e.path()).replacen(&prefix, "", 1)).collect();
        let more = if entries.len() > MAX_LISTED { format!("\n… {} more", entries.len() - MAX_LISTED) } else { String::new() };
        Ok(format!("{}\n{}{more}", dir.display(), if lines.is_empty() { "(empty)".into() } else { lines.join("\n") }))
    }

    fn move_paths(&self, paths: &Value, to: &str) -> Result<String, String> {
        let sources = self.existing(paths)?;
        let dest = self.dest_folder(to, &sources, false)?;
        self.ask("move", &sources, Some(&dest), None)?;
        fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
        let mut done = Vec::new();
        for src in &sources {
            let target = dest.join(src.file_name().unwrap_or_default());
            if fs::rename(src, &target).is_err() {
                // Across drives: copy, then remove the original.
                copy_all(src, &target).map_err(|e| e.to_string())?;
                if src.is_dir() { fs::remove_dir_all(src) } else { fs::remove_file(src) }.map_err(|e| e.to_string())?;
            }
            if !target.exists() || src.exists() {
                return Err(format!("Moving {} did not complete.", src.display()));
            }
            done.push(target.display().to_string());
        }
        Ok(format!("Moved {} item(s) to {}:\n{}", done.len(), dest.display(), done.join("\n")))
    }

    fn copy_paths(&self, paths: &Value, to: &str) -> Result<String, String> {
        let sources = self.existing(paths)?;
        let dest = self.dest_folder(to, &sources, true)?;
        self.ask("copy", &sources, Some(&dest), None)?;
        fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
        for src in &sources {
            copy_all(src, &dest.join(src.file_name().unwrap_or_default())).map_err(|e| e.to_string())?;
        }
        Ok(format!("Copied {} item(s) to {}.", sources.len(), dest.display()))
    }

    fn rename(&self, p: &str, new_name: &str) -> Result<String, String> {
        let src = self.existing(&json!([p]))?.remove(0);
        let new_name = new_name.trim();
        if new_name.is_empty() || new_name.contains(['\\', '/', ':', '*', '?', '"', '<', '>', '|']) {
            return Err("\"new_name\" must be a plain file or folder name without \\ / : * ? \" < > |.".into());
        }
        check_change(&src, true)?;
        let parent = src.parent().unwrap_or(Path::new("")).to_path_buf();
        let target = parent.join(new_name);
        if target.exists() && target.to_string_lossy().to_lowercase() != src.to_string_lossy().to_lowercase() {
            return Err(format!("{} already exists; pick another name, e.g. \"{}\".", target.display(), base_name(&free_name(&target))));
        }
        self.ask("rename", std::slice::from_ref(&src), Some(&parent), Some(new_name))?;
        fs::rename(&src, &target).map_err(|e| e.to_string())?;
        if !target.exists() {
            return Err(format!("Renaming {} did not complete.", src.display()));
        }
        Ok(format!("Renamed to {}.", target.display()))
    }

    fn create_folder(&self, p: &str) -> Result<String, String> {
        let dir = self.path(p)?;
        check_change(&dir, false)?;
        if dir.exists() {
            return Ok(format!("{} already exists.", dir.display()));
        }
        self.ask("create", std::slice::from_ref(&dir), dir.parent(), None)?;
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        Ok(format!("Created {}.", dir.display()))
    }

    fn read(&self, p: &str, start: Option<u64>, end: Option<u64>) -> Result<String, String> {
        let file = self.path(p)?;
        if !file.exists() {
            return Err(format!("{} does not exist. Use find_files to locate it.", file.display()));
        }
        if file.is_dir() {
            return Err(format!("{} is a folder; use list_folder.", file.display()));
        }
        let buf = fs::read(&file).map_err(|e| e.to_string())?;
        if buf[..buf.len().min(8000)].contains(&0) {
            return Err(format!("{} is not a text file.", file.display()));
        }
        let text = String::from_utf8_lossy(&buf);
        let lines: Vec<&str> = text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect();
        let from = start.unwrap_or(1).max(1) as usize;
        let to = end.map(|e| e as usize).filter(|&e| e > 0).unwrap_or(lines.len()).min(lines.len());
        let mut out = String::new();
        let mut i = from;
        while i <= to && out.len() < MAX_READ {
            out.push_str(&format!("{i}\t{}\n", lines[i - 1]));
            i += 1;
        }
        let more = if i <= to { format!("(stopped at line {} of {}; read on with start_line {i})", i - 1, lines.len()) } else { format!("({} lines)", lines.len()) };
        Ok(format!("{}\n{out}{more}", file.display()))
    }

    fn edit(&self, p: &str, old_text: &str, new_text: &str) -> Result<String, String> {
        let file = self.path(p)?;
        if !file.exists() {
            return Err(format!("{} does not exist.", file.display()));
        }
        check_change(&file, false)?;
        let text = fs::read_to_string(&file).map_err(|e| e.to_string())?;
        // Files saved with Windows line ends still match text copied with plain ones.
        let crlf = text.contains("\r\n");
        let fix = |s: &str| if crlf { s.replace("\r\n", "\n").replace('\n', "\r\n") } else { s.to_string() };
        let (want, put) = (fix(old_text), fix(new_text));
        let count = if want.is_empty() { 0 } else { text.matches(&want).count() };
        if count == 0 {
            return Err(format!("old_text was not found in {}; nothing changed. read_file it again and copy the text exactly.", file.display()));
        }
        if count > 1 {
            return Err(format!("old_text appears {count} times in {}; nothing changed. Include more surrounding lines so it is unique.", file.display()));
        }
        self.ask("edit", std::slice::from_ref(&file), file.parent(), None)?;
        fs::write(&file, text.replacen(&want, &put, 1)).map_err(|e| e.to_string())?;
        let at = text[..text.find(&want).unwrap_or(0)].matches('\n').count() + 1;
        Ok(format!("Edited {} at line {at}.", file.display()))
    }

    fn write(&self, p: &str, content: Option<&str>, overwrite: bool) -> Result<String, String> {
        let file = self.path(p)?;
        check_change(&file, false)?;
        let content = content.ok_or("\"content\" must be text.")?;
        let exists = file.exists();
        if exists && file.is_dir() {
            return Err(format!("{} is a folder.", file.display()));
        }
        if exists && !overwrite {
            return Err(format!(
                "{} already exists; nothing changed. read_file it and use edit_file, or write it again with overwrite: true (keep what the user wrote), or pick a free name such as \"{}\".",
                file.display(),
                base_name(&free_name(&file))
            ));
        }
        self.ask(if exists { "overwrite" } else { "write" }, std::slice::from_ref(&file), file.parent(), None)?;
        if let Some(dir) = file.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        fs::write(&file, content).map_err(|e| e.to_string())?;
        Ok(format!("{} {} ({}).", if exists { "Replaced" } else { "Created" }, file.display(), size(content.len() as u64)))
    }

    fn delete(&self, paths: &Value) -> Result<String, String> {
        let targets = self.existing(paths)?;
        for t in &targets {
            check_change(t, true)?;
        }
        let parents: BTreeSet<PathBuf> = targets.iter().filter_map(|t| t.parent().map(Path::to_path_buf)).collect();
        let place = if parents.len() == 1 { parents.into_iter().next() } else { None };
        self.ask("delete", &targets, place.as_deref(), None)?;
        recycle(&targets)?;
        let left: Vec<String> = targets.iter().filter(|t| t.exists()).map(|t| t.display().to_string()).collect();
        if !left.is_empty() {
            return Err(format!("Could not delete: {}.", left.join(", ")));
        }
        Ok(format!("Sent {} item(s) to the Recycle Bin:\n{}", targets.len(), targets.iter().map(|t| t.display().to_string()).collect::<Vec<_>>().join("\n")))
    }
}

// ---------- save_file ----------

/// The user's Documents folder (it may live in OneDrive).
pub fn documents() -> PathBuf {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::UI::Shell::{FOLDERID_Documents, KF_FLAG_DEFAULT, SHGetKnownFolderPath};
        if let Ok(p) = SHGetKnownFolderPath(&FOLDERID_Documents, KF_FLAG_DEFAULT, None) {
            let s = p.to_string().ok();
            windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as *const _));
            if let Some(s) = s {
                return PathBuf::from(s);
            }
        }
    }
    home().join("Documents")
}

/// The app's own folder in Documents (reports, dictations, the extension): Documents\aina. A PC that
/// used the app before the rename keeps its Documents\Badi, so earlier notes stay where they are.
pub fn app_documents() -> PathBuf {
    app_documents_in(&documents())
}

/// app_documents() inside the given Documents folder.
pub fn app_documents_in(docs: &Path) -> PathBuf {
    pick_app_documents(docs, &|p: &Path| p.is_dir())
}

fn pick_app_documents(docs: &Path, is_dir: &dyn Fn(&Path) -> bool) -> PathBuf {
    let (new, old) = (docs.join("aina"), docs.join("Badi"));
    if !is_dir(&new) && is_dir(&old) { old } else { new }
}

const TEXT_EXT: [&str; 4] = [".md", ".txt", ".csv", ".json"];

/// A safe file name inside the Badi folder (no paths, no reserved characters, a known extension).
/// exists(fullPath) lets the caller avoid overwriting.
pub fn target_path(dir: &Path, name: &str, exists: &dyn Fn(&Path) -> bool) -> PathBuf {
    // Reserved characters go first so "a:b.txt" isn't read as a drive letter, then any folder part.
    let flat: String = name.chars().map(|c| if "<>:\"|?*".contains(c) || (c as u32) < 0x20 { '-' } else if c == '\\' { '/' } else { c }).collect();
    let mut base = flat.rsplit('/').next().unwrap_or("").trim_start_matches('.').trim().to_string();
    if base.is_empty() {
        base = "aina".into();
    }
    let mut ext = base.rfind('.').map(|i| base[i..].to_lowercase()).unwrap_or_default();
    if !TEXT_EXT.contains(&ext.as_str()) && ext != ".html" {
        base.push_str(".md");
        ext = ".md".into();
    }
    let stem: String = base[..base.len() - ext.len()].chars().take(120).collect();
    let stem = if stem.is_empty() { "aina".to_string() } else { stem };
    let mut file = dir.join(format!("{stem}{ext}"));
    let mut i = 2;
    while exists(&file) {
        file = dir.join(format!("{stem}-{i}{ext}"));
        i += 1;
    }
    file
}

/// Saves a report into `dir` and opens it: text in Notepad (Windows has no default app for .md),
/// HTML in the browser. Returns the file written.
pub fn save_and_open(dir: &Path, name: &str, content: &str) -> Result<PathBuf, String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let file = target_path(dir, name, &|p| p.exists());
    fs::write(&file, content).map_err(|e| e.to_string())?;
    let ext = file.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    if TEXT_EXT.contains(&ext.as_str()) {
        let _ = std::process::Command::new("notepad.exe").arg(&file).spawn();
    } else {
        crate::apps::shell_open(&file.display().to_string())?;
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_app_folder_is_aina_unless_only_the_old_badi_folder_exists() {
        let docs = Path::new(r"C:\Users\x\Documents");
        assert_eq!(pick_app_documents(docs, &|_| false), docs.join("aina"));
        assert_eq!(pick_app_documents(docs, &|p| p.ends_with("Badi")), docs.join("Badi"));
        assert_eq!(pick_app_documents(docs, &|_| true), docs.join("aina"));
    }

    #[test]
    fn save_file_names_stay_inside_the_app_folder() {
        let dir = Path::new(r"C:\Users\x\Documents\aina");
        let no = |_: &Path| false;
        assert_eq!(target_path(dir, "berita.md", &no), dir.join("berita.md"));
        assert_eq!(target_path(dir, r"..\..\Windows\evil.md", &no), dir.join("evil.md"));
        assert_eq!(target_path(dir, "/etc/passwd", &no), dir.join("passwd.md"));
        assert_eq!(target_path(dir, "run.bat", &no), dir.join("run.bat.md"));
        assert_eq!(target_path(dir, "a:b?.txt", &no), dir.join("a-b-.txt"));
        assert_eq!(target_path(dir, "", &no), dir.join("aina.md"));
        let taken = [dir.join("r.md"), dir.join("r-2.md")];
        assert_eq!(target_path(dir, "r.md", &|p| taken.iter().any(|t| t == p)), dir.join("r-3.md"));
    }

    #[test]
    fn days_match_iso_dates() {
        assert_eq!(chrono_day(0), "1970-01-01");
        assert_eq!(chrono_day(1_792_368_000), "2026-10-19");
    }

    // A temp folder under the home folder (the guard refuses AppData, where %TEMP% lives).
    fn scratch(name: &str) -> PathBuf {
        let d = home().join("Documents").join(format!("badi-v2-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn edit_write_move_rename_and_find_work_and_ask_first() {
        let d = scratch("ops");
        let asked = std::sync::Mutex::new(Vec::new());
        let approve = |n: &Need| {
            asked.lock().unwrap().push(format!("{} {}", n.op, n.names.join(",")));
            None
        };
        let ft = FileTools { base: Some(d.clone()), approve: &approve };
        let ok = |name: &str, input: Value| ft.run(name, &input).unwrap();

        assert!(ok("write_file", json!({ "path": "a.html", "content": "<p>\r\nhalo\r\n</p>" })).unwrap().starts_with("Created"));
        assert!(ok("write_file", json!({ "path": "a.html", "content": "x" })).unwrap_err().contains("already exists"));
        // Text copied with plain line ends still matches a CRLF file.
        assert!(ok("edit_file", json!({ "path": "a.html", "old_text": "<p>\nhalo", "new_text": "<p>\nhai" })).unwrap().contains("line 1"));
        assert_eq!(fs::read_to_string(d.join("a.html")).unwrap(), "<p>\r\nhai\r\n</p>");
        assert!(ok("read_file", json!({ "path": "a.html" })).unwrap().contains("2\thai\n"));
        assert!(ok("move_paths", json!({ "paths": ["a.html"], "to": "sub" })).unwrap().contains("Moved 1"));
        assert!(ok("rename_path", json!({ "path": "sub/a.html", "new_name": "b.html" })).unwrap().contains("b.html"));
        assert!(ok("find_files", json!({ "pattern": "b.htm*" })).unwrap().contains("b.html"));
        assert!(ok("rename_path", json!({ "path": "sub/b.html", "new_name": "../x" })).is_err());
        assert_eq!(*asked.lock().unwrap(), ["write a.html", "edit a.html", "move a.html", "rename a.html"]);

        // A "no" stops the change.
        let deny = |_: &Need| Some("The user declined.".to_string());
        let ft = FileTools { base: Some(d.clone()), approve: &deny };
        assert_eq!(ft.run("create_folder", &json!({ "path": "nope" })).unwrap().unwrap_err(), "The user declined.");
        assert!(!d.join("nope").exists());
        // Protected and fixed places are refused before anyone is asked.
        assert!(ft.run("write_file", &json!({ "path": r"C:\Windows\x.txt", "content": "" })).unwrap().unwrap_err().starts_with("Not allowed"));
        assert!(ft.run("delete_paths", &json!({ "paths": ["~/Documents"] })).unwrap().unwrap_err().contains("main folders"));
        let _ = fs::remove_dir_all(&d);
    }
}
