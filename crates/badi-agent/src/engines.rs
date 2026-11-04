// The engines that run a background agent's task:
// - Badi's own: the Brain loop without the computer, with Anthropic's text editor tool, PowerShell
//   (a fresh process per call, 120 s timeout, output clipped), file search and web search/fetch.
// - Claude Code the user installed (not bundled): `claude -p --output-format stream-json` with a
//   throwaway config folder. Its PreToolUse hook and Badi's tools (an MCP server) are this same exe in
//   a helper mode (`--agent-hook`, `--agent-mcp`) that talks back to the app over a local link with a
//   per-task token, so every tool call passes Badi's guard and approvals, as v1's SDK hook did.
// - The Codex CLI, only when picked.
// Port of src/agentLocal.js, the engine parts of src/agents.js and findClaudeCode in agentTools.js.

use std::collections::VecDeque;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use regex::Regex;
use serde_json::{Value, json};

use crate::commands::{approval_for, describe_tool, guard_tool, prepare_command};
use crate::diff::FileDiff;
use crate::files::{glob_regex, hidden};
use crate::{Host, Need};

pub const MAX_OUTPUT: usize = 30_000; // characters of tool output sent back to Claude
pub const DESKTOP_TOOLS: [&str; 7] = ["list_windows", "get_window_state", "click_element", "set_value", "focus_window", "open_app", "open_url"];
const FILE_TOOLS: [&str; 7] = ["find_files", "list_folder", "move_paths", "copy_paths", "rename_path", "create_folder", "delete_paths"];
/// Claude Code's built-in tools an agent gets; the rest (subagents, cron, worktrees, skills…) stays off.
pub const BUILTIN_TOOLS: [&str; 10] = ["Read", "Write", "Edit", "Glob", "Grep", "PowerShell", "Bash", "WebSearch", "WebFetch", "TodoWrite"];

/// Keeps the start and the end of long output.
pub fn clip(text: &str, max: usize) -> String {
    let n = text.chars().count();
    if n <= max {
        return text.to_string();
    }
    let half = max / 2;
    let head: String = text.chars().take(half).collect();
    let tail: String = text.chars().skip(n - half).collect();
    format!("{head}\n… ({} characters cut) …\n{tail}", n - max)
}

// ---------- Badi's own engine ----------

fn local_tool_defs() -> Vec<Value> {
    vec![
        json!({ "name": "powershell", "description": "Run a PowerShell command on the Windows PC and get its output (stdout and stderr). Each call starts fresh in your working folder, so variables and cd do not carry over; chain steps in one command with \";\". Use it for files, folders, scripts (python, node) and system info. Nothing is interactive: never run commands that wait for input.",
            "input_schema": { "type": "object", "properties": { "command": { "type": "string" }, "timeout_sec": { "type": "integer", "description": "Default 120, at most 600" } }, "required": ["command"] } }),
        json!({ "name": "search_files", "description": "Search file contents with ripgrep (regex). Returns matching lines as path:line:text. Faster than reading files one by one.",
            "input_schema": { "type": "object", "properties": {
                "pattern": { "type": "string", "description": "Regular expression" },
                "path": { "type": "string", "description": "Folder or file to search; default your working folder" },
                "glob": { "type": "string", "description": "Only files matching this glob, e.g. \"*.md\"" },
                "ignore_case": { "type": "boolean" },
                "files_only": { "type": "boolean", "description": "List only the files that match" } }, "required": ["pattern"] } }),
        json!({ "name": "glob_files", "description": "Find files under a folder with a glob, e.g. \"**/*.xlsx\". Returns paths. Defaults to your working folder.",
            "input_schema": { "type": "object", "properties": { "glob": { "type": "string" }, "path": { "type": "string", "description": "Folder to look in; default your working folder" } }, "required": ["glob"] } }),
    ]
}

/// The tools of Badi's own engine. `desktop`: the person allows Badi to use their apps.
pub fn agent_tools(desktop: bool, extra: Vec<Value>) -> Vec<Value> {
    let mut tools = vec![json!({ "type": "text_editor_20250728", "name": "str_replace_based_edit_tool", "max_characters": MAX_OUTPUT })];
    tools.extend(local_tool_defs());
    tools.push(json!({ "type": "web_search_20260209", "name": "web_search", "max_uses": 8 }));
    tools.push(json!({ "type": "web_fetch_20260209", "name": "web_fetch", "max_uses": 8 }));
    tools.extend(crate::brain::own_tools().into_iter().filter(|t| {
        let n = t["name"].as_str().unwrap_or("");
        (desktop && DESKTOP_TOOLS.contains(&n)) || FILE_TOOLS.contains(&n)
    }));
    tools.extend(extra);
    for t in &mut tools {
        if t.get("input_schema").is_some() {
            t["eager_input_streaming"] = json!(true);
        }
    }
    if let Some(last) = tools.last_mut() {
        last["cache_control"] = json!({ "type": "ephemeral" });
    }
    tools
}

/// Runs a process, collecting output; stopping kills its whole tree.
fn run_process(mut cmd: Command, timeout: Duration, stop: &AtomicBool) -> (Option<i32>, String, String) {
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return (Some(-1), e.to_string(), String::new()),
    };
    let out = Arc::new(Mutex::new(Vec::new()));
    let readers: Vec<_> = [child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>), child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>)]
        .into_iter()
        .flatten()
        .map(|mut r| {
            let out = out.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 8192];
                while let Ok(n) = r.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    let mut o = out.lock().unwrap();
                    if o.len() < MAX_OUTPUT * 8 {
                        o.extend_from_slice(&buf[..n]);
                    }
                }
            })
        })
        .collect();
    let started = Instant::now();
    let mut why = String::new();
    let code = loop {
        if let Ok(Some(s)) = child.try_wait() {
            break s.code();
        }
        if stop.load(Ordering::SeqCst) {
            why = "stopped".into();
        } else if started.elapsed() > timeout {
            why = format!("timed out after {} s", timeout.as_secs());
        }
        if !why.is_empty() {
            kill_tree(&mut child);
            break None;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    for r in readers {
        let _ = r.join();
    }
    let text = String::from_utf8_lossy(&out.lock().unwrap()).into_owned();
    (code, text, why)
}

pub fn kill_tree(child: &mut Child) {
    let _ = hidden("taskkill").args(["/T", "/F", "/PID", &child.id().to_string()]).output();
    let _ = child.kill();
}

const SKIP_DIRS: [&str; 4] = ["node_modules", ".git", "target", "$recycle.bin"];

// Files under `root` (or root itself), skipping hidden and dependency folders, as ripgrep does.
fn walk(root: &Path, limit: usize) -> Vec<PathBuf> {
    if root.is_file() {
        return vec![root.to_path_buf()];
    }
    let mut out = Vec::new();
    let mut queue = VecDeque::from([root.to_path_buf()]);
    while let Some(dir) = queue.pop_front() {
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || SKIP_DIRS.contains(&name.to_lowercase().as_str()) {
                continue;
            }
            match e.file_type() {
                Ok(t) if t.is_dir() => queue.push_back(e.path()),
                Ok(t) if t.is_file() => {
                    out.push(e.path());
                    if out.len() >= limit {
                        return out;
                    }
                }
                _ => {}
            }
        }
    }
    out
}

/// A ripgrep-style glob: without a slash it matches the file name; with one, the path under the root.
fn path_glob(glob: &str) -> (Regex, bool) {
    let g = glob.replace('\\', "/");
    if !g.contains('/') {
        return (glob_regex(&g), false);
    }
    let mut re = String::from("(?i)^");
    let chars: Vec<char> = g.trim_start_matches("./").chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '*' if chars.get(i + 1) == Some(&'*') => {
                if chars.get(i + 2) == Some(&'/') {
                    re.push_str("(.*/)?");
                    i += 3;
                } else {
                    re.push_str(".*");
                    i += 2;
                }
                continue;
            }
            '*' => re.push_str("[^/]*"),
            '?' => re.push_str("[^/]"),
            c => re.push_str(&regex::escape(&c.to_string())),
        }
        i += 1;
    }
    re.push('$');
    (Regex::new(&re).unwrap_or_else(|_| Regex::new("^$").unwrap()), true)
}

fn matches_glob(root: &Path, file: &Path, glob: &(Regex, bool)) -> bool {
    if glob.1 {
        let rel = file.strip_prefix(root).unwrap_or(file).to_string_lossy().replace('\\', "/");
        glob.0.is_match(&rel)
    } else {
        file.file_name().is_some_and(|n| glob.0.is_match(&n.to_string_lossy()))
    }
}

/// The handlers of Badi's own engine, bound to one working folder.
pub struct LocalTools<'a> {
    pub dir: PathBuf,
    pub focus: bool,
    pub stop: Arc<AtomicBool>,
    pub approve: &'a (dyn Fn(&Need) -> Option<String> + Sync),
    /// Told about every file the editor changed, as a diff.
    pub on_diff: Option<&'a (dyn Fn(FileDiff) + Sync)>,
}

impl LocalTools<'_> {
    // Writes a file and reports the change as a diff (from what was there before, if anything).
    fn write_diffed(&self, file: &Path, before: Option<&str>, after: &str) -> Result<(), String> {
        fs::write(file, after).map_err(|e| e.to_string())?;
        if let Some(f) = self.on_diff {
            let shown = file.display().to_string();
            f(match before {
                Some(b) => crate::diff::from_edit(b, after, &shown),
                None => crate::diff::from_new(after, &shown),
            });
        }
        Ok(())
    }

    fn abs(&self, p: Option<&str>) -> PathBuf {
        let p = Path::new(p.unwrap_or("."));
        crate::guard::resolve(&if p.is_absolute() { p.to_path_buf() } else { self.dir.join(p) })
    }

    fn rel(&self, p: &Path) -> String {
        p.strip_prefix(&self.dir).map(|r| r.display().to_string()).unwrap_or_else(|_| p.display().to_string())
    }

    fn guarded(&self, name: &str, input: &Value) -> Result<(), String> {
        if let Some(why) = guard_tool(name, input, &self.dir, self.focus) {
            return Err(why);
        }
        if let Some(need) = approval_for(name, input, &self.dir, self.focus)
            && let Some(no) = (self.approve)(&need)
        {
            return Err(no);
        }
        Ok(())
    }

    pub fn run(&self, name: &str, input: &Value) -> Option<Result<String, String>> {
        Some(match name {
            "str_replace_based_edit_tool" => self.editor(input),
            "powershell" => self.powershell(input),
            "search_files" => self.search(input),
            "glob_files" => self.glob(input),
            _ => return None,
        })
    }

    fn editor(&self, input: &Value) -> Result<String, String> {
        let file = self.abs(input["path"].as_str());
        let shown = input["path"].as_str().unwrap_or("");
        let lines = |t: &str| t.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l).to_string()).collect::<Vec<_>>();
        let fp = json!({ "file_path": file.display().to_string() });
        match input["command"].as_str().unwrap_or("") {
            "view" => {
                if !file.exists() {
                    return Err(format!("{shown} does not exist."));
                }
                if file.is_dir() {
                    let mut out = Vec::new();
                    fn list(d: &Path, root: &Path, depth: usize, out: &mut Vec<String>) {
                        let Ok(entries) = fs::read_dir(d) else { return };
                        for e in entries.flatten() {
                            let n = e.file_name().to_string_lossy().into_owned();
                            if n.starts_with('.') || n == "node_modules" {
                                continue;
                            }
                            let p = e.path();
                            let is_dir = p.is_dir();
                            out.push(format!("{}{}", p.strip_prefix(root).unwrap_or(&p).display(), if is_dir { "\\" } else { "" }));
                            if is_dir && depth < 2 && out.len() < 500 {
                                list(&p, root, depth + 1, out);
                            }
                        }
                    }
                    list(&file, &file, 1, &mut out);
                    return Ok(clip(&if out.is_empty() { "(empty folder)".into() } else { out.join("\n") }, MAX_OUTPUT));
                }
                let buf = fs::read(&file).map_err(|e| e.to_string())?;
                if buf[..buf.len().min(8000)].contains(&0) {
                    return Err(format!("{shown} is a binary file; use powershell to inspect it."));
                }
                let all = lines(&String::from_utf8_lossy(&buf));
                let range = input["view_range"].as_array();
                let from = range.and_then(|r| r.first()?.as_i64()).unwrap_or(1).max(1) as usize;
                let to = range.and_then(|r| r.get(1)?.as_i64()).unwrap_or(-1);
                let end = if to < 0 { all.len() } else { (to as usize).min(all.len()) };
                let text: Vec<String> = all.iter().enumerate().skip(from - 1).take(end.saturating_sub(from - 1)).map(|(i, l)| format!("{}\t{l}", i + 1)).collect();
                Ok(clip(&text.join("\n"), MAX_OUTPUT))
            }
            "create" => {
                self.guarded("Write", &fp)?;
                if let Some(d) = file.parent() {
                    fs::create_dir_all(d).map_err(|e| e.to_string())?;
                }
                let before = fs::read_to_string(&file).ok();
                self.write_diffed(&file, before.as_deref(), input["file_text"].as_str().unwrap_or(""))?;
                Ok(format!("Created {}.", self.rel(&file)))
            }
            "str_replace" => {
                self.guarded("Edit", &fp)?;
                let text = fs::read_to_string(&file).map_err(|e| e.to_string())?;
                let old = input["old_str"].as_str().unwrap_or("");
                let count = if old.is_empty() { 0 } else { text.matches(old).count() };
                if count == 0 {
                    return Err("No match for old_str; view the file and copy the text exactly.".into());
                }
                if count > 1 {
                    return Err(format!("old_str matches {count} places; include more surrounding text so it is unique."));
                }
                self.write_diffed(&file, Some(&text), &text.replacen(old, input["new_str"].as_str().unwrap_or(""), 1))?;
                Ok(format!("Edited {}.", self.rel(&file)))
            }
            "insert" => {
                self.guarded("Edit", &fp)?;
                let before = fs::read_to_string(&file).map_err(|e| e.to_string())?;
                let mut all: Vec<String> = before.split('\n').map(String::from).collect();
                let at = (input["insert_line"].as_u64().unwrap_or(0) as usize).min(all.len());
                let add = input["insert_text"].as_str().or(input["new_str"].as_str()).unwrap_or("");
                for (k, l) in add.split('\n').enumerate() {
                    all.insert(at + k, l.to_string());
                }
                self.write_diffed(&file, Some(&before), &all.join("\n"))?;
                Ok(format!("Inserted into {} after line {at}.", self.rel(&file)))
            }
            c => Err(format!("Unknown command \"{c}\".")),
        }
    }

    fn powershell(&self, input: &Value) -> Result<String, String> {
        let command = input["command"].as_str().filter(|c| !c.trim().is_empty()).ok_or("\"command\" must be a non-empty string.")?;
        self.guarded("PowerShell", &json!({ "command": command }))?;
        let secs = input["timeout_sec"].as_u64().unwrap_or(120).clamp(5, 600);
        let script = format!("[Console]::OutputEncoding = [Text.Encoding]::UTF8; $ProgressPreference = 'SilentlyContinue'\n{}", prepare_command(command, &self.dir));
        let mut cmd = hidden("powershell.exe");
        cmd.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &script]).current_dir(&self.dir);
        let (code, out, why) = run_process(cmd, Duration::from_secs(secs), &self.stop);
        let out = clip(out.trim(), MAX_OUTPUT);
        let out = if out.is_empty() { "(no output)".to_string() } else { out };
        Ok(match (why.is_empty(), code) {
            (false, _) => format!("{out}\n(The command was {why}.)"),
            (true, Some(c)) if c != 0 => format!("{out}\n(exit code {c})"),
            _ => out,
        })
    }

    fn search(&self, input: &Value) -> Result<String, String> {
        let pattern = input["pattern"].as_str().unwrap_or("");
        let ci = if input["ignore_case"].as_bool() == Some(true) { "(?i)" } else { "" };
        let re = Regex::new(&format!("{ci}{pattern}")).map_err(|e| format!("Bad pattern: {e}"))?;
        let root = self.abs(input["path"].as_str());
        let glob = input["glob"].as_str().map(path_glob);
        let files_only = input["files_only"].as_bool() == Some(true);
        let mut out = Vec::new();
        let started = Instant::now();
        for f in walk(&root, 20_000) {
            if out.len() > 300 || started.elapsed() > Duration::from_secs(60) || self.stop.load(Ordering::SeqCst) {
                break;
            }
            if glob.as_ref().is_some_and(|g| !matches_glob(&root, &f, g)) || fs::metadata(&f).map(|m| m.len() > 4_000_000).unwrap_or(true) {
                continue;
            }
            let Ok(buf) = fs::read(&f) else { continue };
            if buf[..buf.len().min(8000)].contains(&0) {
                continue;
            }
            let text = String::from_utf8_lossy(&buf);
            for (i, line) in text.lines().enumerate() {
                if re.is_match(line) {
                    if files_only {
                        out.push(f.display().to_string());
                        break;
                    }
                    out.push(format!("{}:{}:{}", f.display(), i + 1, line.chars().take(300).collect::<String>()));
                }
            }
        }
        if out.is_empty() {
            return Ok("No matches.".into());
        }
        let more = if out.len() > 300 { format!("\n… {} more lines", out.len() - 300) } else { String::new() };
        Ok(clip(&(out.into_iter().take(300).collect::<Vec<_>>().join("\n") + &more), MAX_OUTPUT))
    }

    fn glob(&self, input: &Value) -> Result<String, String> {
        let root = self.abs(input["path"].as_str());
        let g = path_glob(input["glob"].as_str().unwrap_or("*"));
        let found: Vec<String> = walk(&root, 50_000).into_iter().filter(|f| matches_glob(&root, f, &g)).map(|f| f.display().to_string()).collect();
        if found.is_empty() {
            return Ok("No files found.".into());
        }
        let more = if found.len() > 300 { format!("\n… {} more", found.len() - 300) } else { String::new() };
        Ok(clip(&(found.iter().take(300).cloned().collect::<Vec<_>>().join("\n") + &more), MAX_OUTPUT))
    }
}

// ---------- Claude Code ----------

static SHIM_TARGET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)"%dp0%\\?([^"]+\.exe)""#).unwrap());

/// The exe behind a `claude` found on PATH: the native installer puts claude.exe there; npm puts a
/// claude.cmd shim that starts node_modules\@anthropic-ai\claude-code\bin\claude.exe.
pub fn exe_from_where(lines: &[&str], exists: &dyn Fn(&Path) -> bool, read: &dyn Fn(&Path) -> Option<String>) -> Option<PathBuf> {
    for line in lines.iter().map(|l| l.trim()).filter(|l| !l.is_empty()) {
        let p = Path::new(line);
        let lower = line.to_lowercase();
        if lower.ends_with(".exe") && exists(p) {
            return Some(p.to_path_buf());
        }
        if lower.ends_with(".cmd")
            && let Some(text) = read(p)
            && let Some(m) = SHIM_TARGET.captures(&text)
        {
            let exe = p.parent().unwrap_or(Path::new("")).join(&m[1]);
            if exists(&exe) {
                return Some(exe);
            }
        }
    }
    None
}

static CLAUDE: Mutex<Option<Option<PathBuf>>> = Mutex::new(None);

/// Claude Code installed on this PC (BADI_CLAUDE_CODE overrides; "none" turns it off), or None.
pub fn find_claude_code() -> Option<PathBuf> {
    let mut cache = CLAUDE.lock().unwrap();
    if let Some(c) = cache.as_ref() {
        return c.clone();
    }
    let found = match std::env::var("BADI_CLAUDE_CODE") {
        Ok(f) => (f != "none" && Path::new(&f).exists()).then(|| PathBuf::from(f)),
        Err(_) => {
            let out = hidden("where").arg("claude").output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
            let lines: Vec<&str> = out.lines().collect();
            let native = crate::guard::home().join(r".local\bin\claude.exe");
            exe_from_where(&lines, &|p| p.exists(), &|p| fs::read_to_string(p).ok()).or_else(|| native.exists().then_some(native))
        }
    };
    *cache = Some(found.clone());
    found
}

pub fn find_codex() -> Option<PathBuf> {
    let out = hidden("where").arg("codex").output().ok()?;
    String::from_utf8_lossy(&out.stdout).lines().map(str::trim).find(|l| l.to_lowercase().ends_with(".cmd") || l.to_lowercase().ends_with(".exe")).map(PathBuf::from)
}

/// The local link the helper processes use: "port:token" in BADI_LINK.
pub const LINK_ENV: &str = "BADI_LINK";

/// Answers the helper processes during one Claude Code task: `serve(request) -> response`.
struct Link {
    listener: TcpListener,
    token: String,
}

impl Link {
    fn open() -> Result<Link, String> {
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        // Randomly keyed hashers: a token no other process can guess.
        use std::hash::{BuildHasher, Hasher};
        let part = || std::collections::hash_map::RandomState::new().build_hasher().finish();
        let token = format!("{:016x}{:016x}", part(), part());
        Ok(Link { listener, token })
    }

    fn env(&self) -> String {
        format!("{}:{}", self.listener.local_addr().map(|a| a.port()).unwrap_or(0), self.token)
    }

    // Serves requests until `done` is set.
    fn serve(&self, done: &AtomicBool, handle: &(dyn Fn(&Value) -> Value + Sync)) {
        std::thread::scope(|s| {
            while !done.load(Ordering::SeqCst) {
                match self.listener.accept() {
                    Ok((stream, _)) => {
                        s.spawn(move || {
                            let _ = stream.set_nonblocking(false);
                            let mut line = String::new();
                            let mut reader = BufReader::new(&stream);
                            if reader.read_line(&mut line).is_err() {
                                return;
                            }
                            let Ok(req) = serde_json::from_str::<Value>(&line) else { return };
                            let reply = if req["token"].as_str() == Some(self.token.as_str()) { handle(&req) } else { json!({ "error": "bad token" }) };
                            let mut w = &stream;
                            let _ = writeln!(w, "{reply}");
                        });
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(30)),
                }
            }
        });
    }
}

fn link_request(req: Value) -> Result<Value, String> {
    let link = std::env::var(LINK_ENV).map_err(|_| "no link")?;
    let (port, token) = link.split_once(':').ok_or("bad link")?;
    let stream = TcpStream::connect(("127.0.0.1", port.parse::<u16>().map_err(|e| e.to_string())?)).map_err(|e| e.to_string())?;
    let mut req = req;
    req["token"] = json!(token);
    let mut w = &stream;
    writeln!(w, "{req}").map_err(|e| e.to_string())?;
    let mut line = String::new();
    BufReader::new(&stream).read_line(&mut line).map_err(|e| e.to_string())?;
    serde_json::from_str(&line).map_err(|e| e.to_string())
}

/// `badi.exe --agent-hook`: Claude Code's PreToolUse hook. Asks the app; refuses when it can't.
pub fn hook_main() {
    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);
    let event: Value = serde_json::from_str(&input).unwrap_or(Value::Null);
    let reply = link_request(json!({ "kind": "hook", "input": event })).unwrap_or_else(|e| {
        json!({ "hookSpecificOutput": { "hookEventName": "PreToolUse", "permissionDecision": "deny", "permissionDecisionReason": format!("aina is not reachable ({e}).") } })
    });
    println!("{reply}");
}

/// `badi.exe --agent-mcp`: Badi's tools as an MCP server over stdio (newline-delimited JSON-RPC).
pub fn mcp_main() {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };
        let Some(id) = msg.get("id").cloned() else { continue }; // notifications need no answer
        let result = match msg["method"].as_str().unwrap_or("") {
            "initialize" => Ok(json!({
                "protocolVersion": msg["params"]["protocolVersion"].as_str().unwrap_or("2025-06-18"),
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "badi", "version": env!("CARGO_PKG_VERSION") }
            })),
            "ping" => Ok(json!({})),
            "tools/list" => link_request(json!({ "kind": "tools" })).map(|t| json!({ "tools": t })),
            "tools/call" => link_request(json!({ "kind": "call", "name": msg["params"]["name"], "arguments": msg["params"]["arguments"] })),
            m => Err(format!("Method not found: {m}")),
        };
        let reply = match result {
            Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
            Err(e) => json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": e } }),
        };
        let _ = writeln!(out, "{reply}");
        let _ = out.flush();
    }
}

/// Runs one of Badi's tools for Claude Code.
pub type RunTool<'a> = &'a (dyn Fn(&str, &Value) -> Result<Vec<Value>, String> + Sync);

pub struct ClaudeCodeRun<'a> {
    pub exe: &'a Path,
    pub key: &'a str,
    pub model: &'a str,
    pub effort: &'a str,
    pub dir: &'a Path,
    pub focus: bool,
    pub append: &'a str,
    pub prompt: &'a str,
    /// Badi's tools for the agent (MCP): definitions, and how to run one.
    pub tools: Vec<Value>,
    pub run_tool: RunTool<'a>,
    pub approve: &'a (dyn Fn(&Need) -> Option<String> + Sync),
    pub log: &'a (dyn Fn(&str) + Sync),
    pub stop: &'a AtomicBool,
    /// Told about each Edit, MultiEdit and Write the hook lets through, as a diff of the whole file.
    pub on_diff: Option<&'a (dyn Fn(FileDiff) + Sync)>,
}

pub struct EngineError {
    pub message: String,
    /// It had begun working, so falling back to another engine would repeat its actions.
    pub started: bool,
}

/// Runs one task on Claude Code. Returns the final text and what it cost (Meter.summary's shape).
pub fn run_claude_code(r: &ClaudeCodeRun) -> Result<(String, Value), EngineError> {
    let fail = |m: String, started| EngineError { message: m, started };
    let link = Link::open().map_err(|e| fail(e, false))?;
    // A fresh config folder per task, deleted afterwards (the user's own ~/.claude is never touched).
    let config_dir = std::env::temp_dir().join(format!("badi-agent-{}", link.token.get(..12).unwrap_or("x")));
    fs::create_dir_all(&config_dir).map_err(|e| fail(e.to_string(), false))?;
    let me = std::env::current_exe().map_err(|e| fail(e.to_string(), false))?;
    let me_s = me.display().to_string();
    let append_file = config_dir.join("append.md");
    let _ = fs::write(&append_file, r.append);
    let settings = json!({ "hooks": { "PreToolUse": [{ "matcher": "*", "hooks": [{ "type": "command", "command": format!("\"{me_s}\" --agent-hook"), "timeout": 600 }] }] } });
    let mcp = json!({ "mcpServers": { "badi": { "command": me_s, "args": ["--agent-mcp"], "env": { LINK_ENV: link.env() } } } });
    let tools = BUILTIN_TOOLS.join(",");
    let allowed = format!("{tools},mcp__badi");

    let mut cmd = hidden(&r.exe.display().to_string());
    cmd.args(["-p", "--output-format", "stream-json", "--verbose", "--model", r.model, "--effort", r.effort])
        .arg("--append-system-prompt-file")
        .arg(&append_file)
        .args(["--setting-sources", "", "--no-session-persistence", "--tools", &tools, "--allowedTools", &allowed])
        .args(["--permission-mode", "bypassPermissions", "--allow-dangerously-skip-permissions", "--strict-mcp-config"])
        .arg("--mcp-config")
        .arg(mcp.to_string())
        .arg("--settings")
        .arg(settings.to_string())
        .current_dir(r.dir)
        .env("CLAUDE_CONFIG_DIR", &config_dir)
        .env("CLAUDE_CODE_DISABLE_AUTO_MEMORY", "1")
        .env("CLAUDE_CODE_USE_POWERSHELL_TOOL", "1")
        .env("CLAUDE_AGENT_SDK_CLIENT_APP", format!("BADI/{}", env!("CARGO_PKG_VERSION")))
        .env("ANTHROPIC_API_KEY", r.key)
        .env(LINK_ENV, link.env())
        .envs(badi_net::cloud().map(|base| {
            let headers = badi_net::task_headers().iter().map(|(k, v)| format!("{k}: {v}")).collect::<Vec<_>>().join("\n");
            [("ANTHROPIC_BASE_URL".to_string(), base), ("ANTHROPIC_CUSTOM_HEADERS".to_string(), headers)]
        }).into_iter().flatten())
        // Never let a stray token from the environment pick a different account than the key in Settings.
        .env_remove("ANTHROPIC_AUTH_TOKEN")
        .env_remove("CLAUDE_CODE_OAUTH_TOKEN")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let handle = |req: &Value| -> Value {
        match req["kind"].as_str() {
            Some("hook") => {
                let name = req["input"]["tool_name"].as_str().unwrap_or("");
                let input = &req["input"]["tool_input"];
                let deny = |why: String| {
                    (r.log)(&format!("✕ {why}"));
                    json!({ "hookSpecificOutput": { "hookEventName": "PreToolUse", "permissionDecision": "deny", "permissionDecisionReason": why } })
                };
                if let Some(why) = guard_tool(name, input, r.dir, r.focus) {
                    return deny(why);
                }
                if let Some(need) = approval_for(name, input, r.dir, r.focus)
                    && let Some(no) = (r.approve)(&need)
                {
                    return deny(no);
                }
                if let Some(c) = input["command"].as_str().filter(|_| name == "PowerShell")
                    && c.to_lowercase().contains("remove-torecyclebin")
                {
                    let mut updated = input.clone();
                    updated["command"] = json!(prepare_command(c, r.dir));
                    return json!({ "hookSpecificOutput": { "hookEventName": "PreToolUse", "permissionDecision": "allow", "updatedInput": updated } });
                }
                // Allowed: the edit it is about to make, worked out from the file as it is now.
                if let Some(f) = r.on_diff
                    && let Some(d) = crate::diff::for_tool(name, input, r.dir)
                {
                    f(d);
                }
                json!({})
            }
            Some("tools") => Value::Array(r.tools.iter().map(|t| json!({ "name": t["name"], "description": t["description"], "inputSchema": t["input_schema"] })).collect()),
            Some("call") => {
                let name = req["name"].as_str().unwrap_or("");
                let args = if req["arguments"].is_object() { req["arguments"].clone() } else { json!({}) };
                match (r.run_tool)(name, &args) {
                    Ok(content) => json!({ "content": content }),
                    Err(e) => json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
                }
            }
            _ => json!({ "error": "unknown request" }),
        }
    };

    let done = AtomicBool::new(false);
    let result = std::thread::scope(|s| {
        s.spawn(|| link.serve(&done, &handle));
        let result = (|| {
            let mut child = cmd.spawn().map_err(|e| fail(format!("Claude Code could not start: {e}"), false))?;
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(r.prompt.as_bytes());
            }
            let stderr = child.stderr.take();
            let errors = s.spawn(move || {
                let mut text = String::new();
                if let Some(mut e) = stderr {
                    let _ = e.read_to_string(&mut text);
                }
                text
            });
            let stdout = child.stdout.take().unwrap();
            let (tx, rx) = std::sync::mpsc::channel::<String>();
            s.spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    if tx.send(line).is_err() {
                        break;
                    }
                }
            });
            let mut started = false;
            let mut tool_calls = 0u64;
            let mut final_text: Option<Result<(String, Value), String>> = None;
            loop {
                if r.stop.load(Ordering::SeqCst) {
                    kill_tree(&mut child);
                    return Err(fail("stopped".into(), true));
                }
                match rx.recv_timeout(Duration::from_millis(200)) {
                    Ok(line) => {
                        let Ok(m) = serde_json::from_str::<Value>(&line) else { continue };
                        match m["type"].as_str() {
                            Some("assistant") => {
                                started = true;
                                for b in m["message"]["content"].as_array().into_iter().flatten().filter(|b| b["type"] == "tool_use") {
                                    tool_calls += 1;
                                    if let Some(l) = describe_tool(b["name"].as_str().unwrap_or(""), &b["input"], r.dir) {
                                        (r.log)(&l);
                                    }
                                }
                            }
                            Some("result") => {
                                // Claude Code reports its own totals.
                                let u = &m["usage"];
                                let line = json!({
                                    "turns": m["num_turns"].as_u64().unwrap_or(0), "tools": tool_calls, "screenshots": 0,
                                    "input": u["input_tokens"], "output": u["output_tokens"],
                                    "cacheRead": u["cache_read_input_tokens"], "cacheWrite": u["cache_creation_input_tokens"],
                                    "usd": m["total_cost_usd"].as_f64().unwrap_or(0.0), "ms": m["duration_ms"].as_u64().unwrap_or(0),
                                });
                                final_text = Some(if m["subtype"] == "success" {
                                    Ok((m["result"].as_str().unwrap_or("").trim().to_string(), line))
                                } else {
                                    Err(m["errors"][0].as_str().map(String::from).unwrap_or_else(|| m["subtype"].as_str().unwrap_or("error").to_string()))
                                });
                            }
                            _ => {}
                        }
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(_) => break,
                }
            }
            let _ = child.wait();
            let err_text = errors.join().unwrap_or_default();
            match final_text {
                Some(Ok((text, line))) => Ok((if text.is_empty() { "Done.".into() } else { text }, line)),
                Some(Err(e)) => Err(fail(e, started)),
                None => Err(fail(err_text.lines().last().unwrap_or("Claude Code stopped without an answer").to_string(), started)),
            }
        })();
        done.store(true, Ordering::SeqCst);
        result
    });
    let _ = fs::remove_dir_all(&config_dir);
    result
}

/// Runs one task on the Codex CLI (only when picked in the config).
pub fn run_codex(codex: &Path, dir: &Path, prompt: &str, log: &(dyn Fn(&str) + Sync), stop: &AtomicBool) -> Result<String, String> {
    let last = dir.join("tmp").join("last-message.txt");
    // codex is a .cmd shim, so it runs through cmd; the prompt goes over stdin ("-").
    let mut child = hidden("cmd.exe")
        .arg("/c")
        .arg(codex)
        .args(["exec", "--full-auto", "--skip-git-repo-check", "-C"])
        .arg(dir)
        .arg("--output-last-message")
        .arg(&last)
        .arg("-")
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    if let Some(mut s) = child.stdin.take() {
        let _ = s.write_all(prompt.as_bytes());
    }
    let out = child.stdout.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        for l in BufReader::new(out).lines().map_while(Result::ok) {
            let _ = tx.send(l);
        }
    });
    let mut tail = VecDeque::new();
    loop {
        if stop.load(Ordering::SeqCst) {
            kill_tree(&mut child);
            return Err("stopped".into());
        }
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(l) => {
                log(&l);
                tail.push_back(l);
                if tail.len() > 20 {
                    tail.pop_front();
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => break,
        }
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    match fs::read_to_string(&last) {
        Ok(t) if !t.trim().is_empty() => Ok(t.trim().to_string()),
        _ if status.success() => Ok(tail.into_iter().collect::<Vec<_>>().join("\n")),
        _ => Err(format!("codex exited with code {}", status.code().unwrap_or(-1))),
    }
}

/// A Host for agents: file changes go through `approve`, tool calls are logged, nothing is on screen.
pub struct AgentHost<'a> {
    pub approve: &'a (dyn Fn(&Need) -> Option<String> + Sync),
    pub log: &'a (dyn Fn(&str) + Sync),
    pub dir: PathBuf,
}

impl Host for AgentHost<'_> {
    fn approve(&self, need: &Need) -> Option<String> {
        (self.approve)(need)
    }
    fn confirm(&self, _: &str) -> (bool, String) {
        (false, String::new())
    }
    fn ask(&self, _: &str) -> String {
        String::new()
    }
    fn action_at(&self, _: i32, _: i32, _: &str) {}
    fn progress(&self, _: &str) {}
    fn action(&self, line: &str) {
        // "name {json}" from the loop -> the agent log's short line.
        let (name, args) = line.split_once(' ').unwrap_or((line, "{}"));
        let input = serde_json::from_str(args).unwrap_or(Value::Null);
        if let Some(l) = describe_tool(name, &input, &self.dir) {
            (self.log)(&l);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_exe_behind_a_claude_shim() {
        let shim = r"C:\Users\Sari\AppData\Roaming\npm\claude.cmd";
        let exe = PathBuf::from(r"C:\Users\Sari\AppData\Roaming\npm\node_modules\@anthropic-ai\claude-code\bin\claude.exe");
        let read = |_: &Path| Some(r#"@ECHO off\r\n"%dp0%\node_modules\@anthropic-ai\claude-code\bin\claude.exe" %*"#.to_string());
        let e = exe.clone();
        assert_eq!(exe_from_where(&[&shim.replace(".cmd", ""), shim], &|p| p == e, &read), Some(exe));
        assert_eq!(exe_from_where(&[r"C:\Users\Sari\.local\bin\claude.exe"], &|_| true, &read), Some(PathBuf::from(r"C:\Users\Sari\.local\bin\claude.exe")));
        assert_eq!(exe_from_where(&[shim], &|_| false, &read), None);
        assert_eq!(exe_from_where(&[], &|_| true, &read), None);
    }

    #[test]
    fn local_tools_edit_search_and_run_powershell_inside_the_folder() {
        let dir = crate::guard::real(&std::env::temp_dir().join(format!("badi-local-{}", std::process::id())));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(r"src\lib")).unwrap();
        let approve = |_: &Need| None;
        let diffs = Mutex::new(Vec::new());
        let on_diff = |d: FileDiff| diffs.lock().unwrap().push(d.line());
        let t = LocalTools { dir: dir.clone(), focus: false, stop: Arc::default(), approve: &approve, on_diff: Some(&on_diff) };
        let run = |n: &str, i: Value| t.run(n, &i).unwrap();
        assert!(run("str_replace_based_edit_tool", json!({ "command": "create", "path": r"src\lib\a.js", "file_text": "let x = 1;\nlet y = 2;\n" })).is_ok());
        assert!(run("str_replace_based_edit_tool", json!({ "command": "str_replace", "path": r"src\lib\a.js", "old_str": "x = 1", "new_str": "x = 3" })).is_ok());
        assert_eq!(*diffs.lock().unwrap(), ["✎ a.js +2 −0", "✎ a.js +1 −1"]);
        assert!(run("str_replace_based_edit_tool", json!({ "command": "view", "path": r"src\lib\a.js" })).unwrap().starts_with("1\tlet x = 3;"));
        assert!(run("search_files", json!({ "pattern": "Y = 2", "ignore_case": true })).unwrap().contains("a.js:2:let y = 2;"));
        assert!(run("glob_files", json!({ "glob": "**/*.js" })).unwrap().contains("a.js"));
        assert_eq!(run("glob_files", json!({ "glob": "*.md" })).unwrap(), "No files found.");
        assert_eq!(run("powershell", json!({ "command": "Write-Output halo" })).unwrap(), "halo");
        assert!(run("powershell", json!({ "command": r"Remove-Item ~\Downloads\x.zip" })).unwrap_err().contains("Remove-ToRecycleBin"));
        // Remove-ToRecycleBin works, and refuses a protected place found only at run time.
        fs::write(dir.join("buang.txt"), "x").unwrap();
        assert!(run("powershell", json!({ "command": "Remove-ToRecycleBin buang.txt" })).unwrap().contains("Moved to the Recycle Bin"));
        assert!(!dir.join("buang.txt").exists());
        let out = run("powershell", json!({ "command": r#"$p = [Environment]::GetFolderPath("Windows") + "\win.ini"; Remove-ToRecycleBin $p"# })).unwrap();
        assert!(out.contains("protected location"), "{out}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn long_output_keeps_both_ends() {
        let s = "a".repeat(50) + &"b".repeat(50);
        let c = clip(&s, 20);
        assert!(c.starts_with("aaaaaaaaaa\n…") && c.ends_with("bbbbbbbbbb") && c.contains("80 characters cut"));
    }
}
