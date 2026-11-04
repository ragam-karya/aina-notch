// Background Badi: named agents, each with its own workspace folder (agents/<slug>/ with agent.json,
// AGENTS.md, output/ and tmp/, the same layout as v1, so agents made in v1 open here), chat log and
// routines. The engines that run a task are in engines.rs. Port of the storage and prompt parts of
// src/agents.js.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value, json};

use crate::guard::real;

pub const INSTRUCTIONS: &str = r#"You are a Sprite: one of the specialist helpers aina (the main AI) spawns to work in the background on this person's Windows PC. They usually ask aina by voice, and aina hands the job to you. Your final message appears in your chat with them.

Your working folder is yours. Its AGENTS.md says who you are and holds your notes. When you learn something durable about this job (an account, a preference, a decision), add it to the Notes section with today's date; never store passwords or tokens there. Newer dated notes win, and what the person tells you now beats everything.

How to work:
- Work out what they want, then do it. Ask only when the result depends on something only they know; do the part that doesn't depend on the answer first, then ask one bundled set of questions. Never invent their facts.
- Use the web for outside facts and the shell for files. Never claim something happened unless it did.
- Confirm before sending email or spending money. When it is unclear which of their files they want deleted, ask first.
- Put deliverables in output/ and work files in tmp/. When done, output/ holds only what they asked for. Check it before saying it's done.

Final message: lead with the result, match the length to the ask, write plainly and warmly. Then append, each on its own lines and never mentioned in the prose:
NEXT_ACTIONS:
- 1 to 4 short imperative offers of real follow-up work, each under 40 characters
ARTIFACTS:
- absolute paths or URLs of things this turn created, most important first (omit the block if none)
If they ask for something on a cadence ("every hour", "every morning"), do it once now and also append:
ROUTINE:
every_minutes: <whole number, at least 2>
title: <two to four words>
task: <self-contained instruction for each later run>"#;

const MAX_MESSAGES: usize = 50; // chat history kept per agent

pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

pub fn slugify(name: &str) -> String {
    let mut s = String::new();
    for c in name.to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            s.push(c);
        } else if !s.ends_with('-') {
            s.push('-');
        }
    }
    let s: String = s.trim_matches('-').chars().take(40).collect();
    if s.is_empty() { "badi".into() } else { s }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Blocks {
    pub body: String,
    pub next_actions: Vec<String>,
    pub artifacts: Vec<String>,
    pub routine: Option<Value>,
}

static HEAD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\s*(NEXT_ACTIONS|ARTIFACTS|ROUTINE):\s*$").unwrap());
static ITEM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*[-*]\s*").unwrap());
static KV: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(every_minutes|title|task)\s*:\s*(.+)$").unwrap());

/// Splits an agent's final message into the prose and its NEXT_ACTIONS / ARTIFACTS / ROUTINE blocks.
pub fn parse_blocks(text: &str) -> Blocks {
    let mut out = Blocks { body: text.to_string(), ..Blocks::default() };
    let Some(m) = HEAD.find(text) else { return out };
    out.body = text[..m.start()].trim().to_string();
    let mut section = "";
    let mut routine = Map::new();
    let mut has_routine = false;
    for line in text[m.start()..].lines() {
        if let Some(c) = HEAD.captures(line) {
            section = if &c[1] == "NEXT_ACTIONS" { "next" } else if &c[1] == "ARTIFACTS" { "art" } else { "routine" };
            has_routine |= section == "routine";
            continue;
        }
        let item = ITEM.replace(line, "").trim().to_string();
        if item.is_empty() {
            continue;
        }
        match section {
            "next" => out.next_actions.push(item.chars().take(60).collect()),
            "art" => out.artifacts.push(item),
            "routine" => {
                if let Some(kv) = KV.captures(&item) {
                    let v = if &kv[1] == "every_minutes" {
                        let n: i64 = kv[2].trim().chars().take_while(char::is_ascii_digit).collect::<String>().parse().unwrap_or(60);
                        json!(n.max(2))
                    } else {
                        json!(kv[2].trim())
                    };
                    routine.insert(kv[1].to_string(), v);
                }
            }
            _ => {}
        }
    }
    if has_routine && routine.contains_key("every_minutes") && routine.contains_key("task") {
        out.routine = Some(Value::Object(routine));
    }
    out
}

/// The last few chat turns, so a fresh engine session knows what "that" refers to.
pub fn recent_chat(messages: &[Value], n: usize) -> String {
    let start = messages.len().saturating_sub(n + 1);
    let end = messages.len().saturating_sub(1);
    let lines: Vec<String> = messages
        .get(start..end)
        .unwrap_or(&[])
        .iter()
        .filter_map(|m| {
            let text = m["text"].as_str().filter(|t| !t.is_empty())?;
            let who = if m["role"] == "assistant" { "You" } else { "They" };
            Some(format!("{who}: {}", text.chars().take(1500).collect::<String>()))
        })
        .collect();
    if lines.is_empty() { String::new() } else { format!("Recent chat with this person, oldest first:\n{}\n\n", lines.join("\n")) }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    ClaudeCode,
    Local,
    Codex,
}

impl Engine {
    /// The name the Agents window shows (v1: 'sdk' | 'local' | 'codex').
    pub fn id(self) -> &'static str {
        match self {
            Engine::ClaudeCode => "sdk",
            Engine::Local => "local",
            Engine::Codex => "codex",
        }
    }
}

/// Which engine runs a task: agentEngine "auto" (default) uses an installed Claude Code, else Badi's
/// own; "local" always Badi's own; "codex" the Codex CLI when it is installed.
pub fn pick_engine(choice: &str, claude_code: bool, codex: bool) -> Engine {
    if choice == "codex" && codex {
        Engine::Codex
    } else if choice != "local" && claude_code {
        Engine::ClaudeCode
    } else {
        Engine::Local
    }
}

/// How an agent works in the person's own project (their focus folder).
pub fn project_rules(project: &Path, notes_dir: &Path) -> String {
    format!(
        r#"You are working on the person's project at {p} (their focus folder). Run commands and read and change code there. Your own notes stay in {notes}; for this project, deliverables go in {reports}, not output/.

Code work:
- Reviews, git diffs, bug hunts and security audits are read-only: change no code. In a git repo look at git status, git diff (staged and unstaged) and git log for context; read the files involved; search with ripgrep. Write the full report with your file-writing tool (not a shell heredoc or echo) to badi-reports\<YYYY-MM-DD-HHmm>-<review|diff|security|bugs>.md: a short summary, then numbered findings, each with file:line, what is wrong and why it matters, severity (high, medium or low) and a suggested fix. Your final message is a two or three sentence summary (how many findings, the most serious one) and the report path.
- When the person asks you to fix ("perbaiki", "fix"), read the newest report in badi-reports (or the one they name) and fix the findings they asked for, nothing else. Then run the project's tests or build if it has them, and mark each fixed finding in the report with ✅ and what changed.
- In this folder you may create and edit files without asking. Deleting asks the person and goes to the Recycle Bin (delete_paths or Remove-ToRecycleBin). Never commit, push or rewrite git history unless they ask."#,
        p = project.display(),
        notes = notes_dir.join("AGENTS.md").display(),
        reports = project.join("badi-reports").display(),
    )
}

pub struct SystemParts<'a> {
    pub agents_md: &'a str,
    pub dir: &'a Path,
    pub desktop: bool,
    pub engine: Engine,
    pub project: Option<&'a Path>,
    pub notes_dir: &'a Path,
    pub tone: &'a str,
    pub language_instruction: &'a str,
    pub today: &'a str,
    /// The work-app line, when Composio is set up (not for project agents).
    pub apps: &'a str,
}

/// The engine's working instructions on top of the agent's AGENTS.md.
pub fn agent_system(s: &SystemParts) -> String {
    let sdk = s.engine == Engine::ClaudeCode;
    let mut parts = vec![s.agents_md.to_string(), s.tone.to_string(), s.language_instruction.to_string()];
    if let Some(p) = s.project {
        parts.push(project_rules(p, s.notes_dir));
    }
    parts.push(format!(
        "Today is {today}. Your working folder is {dir}; relative paths are inside it. Shell commands run in PowerShell on Windows.\n\nOrganizing the person's files is part of your job: you may create, edit, move, rename, copy and delete files and folders in their own folders (Desktop, Documents, Downloads, Pictures…) and on their data drives. Prefer the file tools ({pre}find_files, list_folder, move_paths, copy_paths, rename_path, create_folder, delete_paths): one call can move or delete many items, and the person is asked once. Use PowerShell for what they can't do. Windows, program folders, AppData and hidden settings folders are off limits, and drives or main folders like Desktop can't themselves be deleted or moved. Changes outside your working folder may first ask the person for permission; if they decline, don't work around it. Delete anything outside your working folder with Remove-ToRecycleBin <path> (also takes pipeline input); it goes to the Recycle Bin so it can be undone. Use -LiteralPath for names with brackets. Before a big reorganization, list what you found and how you will sort it, then do it; never overwrite a file with the same name, rename it instead (\"file (2).pdf\"). Finish with a short summary of what moved where.",
        today = s.today,
        dir = s.dir.display(),
        pre = if sdk { "mcp__badi__" } else { "" },
    ));
    if !s.apps.is_empty() {
        let line = if sdk { s.apps.replace("COMPOSIO_", "mcp__badi__COMPOSIO_").replace("connect_app", "mcp__badi__connect_app") } else { s.apps.to_string() };
        parts.push(line);
    }
    if s.engine == Engine::Local {
        parts.push("Tools: str_replace_based_edit_tool views, creates and edits files; powershell runs commands (git too); search_files (contents) and glob_files (names) search with ripgrep; web_search and web_fetch reach the web.".into());
    }
    if s.desktop {
        parts.push(format!(
            "Desktop tools ({}) operate the person's apps: list_windows, get_window_state (accessibility tree with refs), click_element, set_value, focus_window, open_app, open_url. They may be using the PC, so touch their apps only when the task needs it.",
            if sdk { "mcp__badi__*" } else { "list_windows and the rest" }
        ));
    }
    parts.retain(|p| !p.is_empty());
    parts.join("\n\n")
}

/// The agents folder (agents/<slug>/…).
pub struct Store {
    pub root: PathBuf,
}

impl Store {
    pub fn new(root: PathBuf) -> Store {
        Store { root }
    }

    pub fn dir(&self, slug: &str) -> PathBuf {
        self.root.join(slug)
    }

    /// agent.json as stored, or None.
    pub fn get(&self, slug: &str) -> Option<Value> {
        let text = fs::read_to_string(self.dir(slug).join("agent.json")).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// Every agent, the most recently used first.
    pub fn list(&self) -> Vec<Value> {
        let _ = fs::create_dir_all(&self.root);
        let mut list: Vec<Value> = fs::read_dir(&self.root)
            .map(|r| r.flatten().filter(|e| e.path().is_dir()).filter_map(|e| self.get(&e.file_name().to_string_lossy())).collect())
            .unwrap_or_default();
        list.sort_by_key(|m| std::cmp::Reverse(m["updatedAt"].as_i64().unwrap_or(0)));
        list
    }

    pub fn save_meta(&self, slug: &str, patch: Map<String, Value>) -> Result<Value, String> {
        let mut meta = self.get(slug).unwrap_or(json!({}));
        for (k, v) in patch {
            meta[k.as_str()] = v;
        }
        meta["updatedAt"] = json!(now_ms());
        if let Some(o) = meta.as_object_mut() {
            o.remove("running");
        }
        fs::write(self.dir(slug).join("agent.json"), serde_json::to_string_pretty(&meta).unwrap()).map_err(|e| e.to_string())?;
        Ok(meta)
    }

    pub fn create(&self, name: &str, purpose: &str, workdir: Option<&Path>) -> Result<Value, String> {
        let base = slugify(name);
        let mut slug = base.clone();
        let mut i = 2;
        while self.dir(&slug).exists() {
            slug = format!("{base}-{i}");
            i += 1;
        }
        let dir = self.dir(&slug);
        for sub in ["output", "tmp"] {
            fs::create_dir_all(dir.join(sub)).map_err(|e| e.to_string())?;
        }
        let today = chrono::Local::now().format("%Y-%m-%d");
        let who = if purpose.is_empty() { "A general helper." } else { purpose };
        fs::write(dir.join("AGENTS.md"), format!("# {name}\n\n{INSTRUCTIONS}\n\n## Who you are\nYou are {name}. {who}\n\n## Notes\n- {today}: created.\n")).map_err(|e| e.to_string())?;
        let mut m = Map::new();
        m.insert("slug".into(), json!(slug));
        m.insert("name".into(), json!(name));
        m.insert("purpose".into(), json!(purpose));
        if let Some(w) = workdir {
            m.insert("workdir".into(), json!(w.display().to_string()));
        }
        m.insert("createdAt".into(), json!(now_ms()));
        m.insert("messages".into(), json!([]));
        m.insert("routines".into(), json!([]));
        self.save_meta(&slug, m)
    }

    /// The agent that works in a focus folder: one per project.
    pub fn ensure_project(&self, folder: &Path) -> Result<Value, String> {
        let key = crate::guard::lower(folder);
        if let Some(m) = self.list().into_iter().find(|c| c["workdir"].as_str().is_some_and(|w| crate::guard::lower(Path::new(w)) == key)) {
            return Ok(m);
        }
        let name = folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Project".into());
        let purpose = format!("Works on the project in {}: code reviews, git diffs, bug and security hunts, and fixes when asked.", folder.display());
        self.create(&name, &purpose, Some(folder))
    }

    pub fn ensure_default(&self) -> Result<Value, String> {
        match self.get("general") {
            Some(m) => Ok(m),
            None => self.create("General", "Handles one-off tasks handed over by voice.", None),
        }
    }

    /// A Sprite kind's folder (agents/<id>/), made on first use.
    pub fn ensure_sprite(&self, kind: &crate::sprites::Kind) -> Result<Value, String> {
        if let Some(m) = self.get(kind.id) {
            return Ok(m);
        }
        let m = self.create(kind.name, kind.purpose, None)?;
        let slug = m["slug"].as_str().unwrap_or(kind.id).to_string();
        let mut patch = Map::new();
        patch.insert("sprite".into(), json!(kind.id));
        self.save_meta(&slug, patch)
    }

    /// The Sprites, in the order of KINDS (agents from before Sprites stay on disk, unlisted).
    pub fn sprites(&self) -> Vec<Value> {
        crate::sprites::KINDS.iter().filter_map(|k| self.ensure_sprite(k).ok()).collect()
    }

    /// The project folder an agent works in, if it has one and it still exists.
    pub fn project(meta: &Value) -> Option<PathBuf> {
        let w = Path::new(meta["workdir"].as_str()?);
        w.is_dir().then(|| real(w))
    }

    pub fn remove(&self, slug: &str) -> Result<(), String> {
        fs::remove_dir_all(self.dir(slug)).map_err(|e| e.to_string())
    }

    pub fn add_message(&self, slug: &str, mut msg: Value) -> Result<(), String> {
        let meta = self.get(slug).ok_or("no such agent")?;
        msg["at"] = json!(now_ms());
        let mut messages = meta["messages"].as_array().cloned().unwrap_or_default();
        messages.push(msg);
        let skip = messages.len().saturating_sub(MAX_MESSAGES);
        let mut m = Map::new();
        m.insert("messages".into(), Value::Array(messages.into_iter().skip(skip).collect()));
        self.save_meta(slug, m).map(|_| ())
    }

    /// Work files don't outlive the task that made them; only output/ is kept.
    pub fn clear_tmp(&self, slug: &str) {
        let tmp = self.dir(slug).join("tmp");
        let _ = fs::remove_dir_all(&tmp);
        let _ = fs::create_dir_all(&tmp);
    }

    pub fn add_routine(&self, slug: &str, routine: &Value) -> Result<(), String> {
        let meta = self.get(slug).ok_or("no such agent")?;
        let mut routines = meta["routines"].as_array().cloned().unwrap_or_default();
        let mut r = routine.clone();
        r["id"] = json!(format!("{:x}", now_ms()));
        r["paused"] = json!(false);
        routines.push(r);
        let mut m = Map::new();
        m.insert("routines".into(), Value::Array(routines));
        self.save_meta(slug, m).map(|_| ())
    }

    /// Changes a routine (pause, resume), or removes it with a null patch.
    pub fn set_routine(&self, slug: &str, id: &str, patch: &Value) -> Result<(), String> {
        let meta = self.get(slug).ok_or("no such agent")?;
        let routines: Vec<Value> = meta["routines"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|r| !(patch.is_null() && r["id"] == id))
            .map(|mut r| {
                if r["id"] == id
                    && let Some(p) = patch.as_object()
                {
                    for (k, v) in p {
                        r[k.as_str()] = v.clone();
                    }
                }
                r
            })
            .collect();
        let mut m = Map::new();
        m.insert("routines".into(), Value::Array(routines));
        self.save_meta(slug, m).map(|_| ())
    }
}

/// A routine is due when its period has passed on the wall clock since it last ran (or was
/// scheduled), so a PC that slept through a run catches up once when it wakes.
pub fn routine_due(last_ms: i64, every_minutes: i64, now: i64) -> bool {
    now - last_ms >= every_minutes.max(2) * 60_000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_are_split_from_the_prose() {
        let b = parse_blocks("Sudah aku rangkum.\n\nNEXT_ACTIONS:\n- Kirim ke email\n- Bikin grafik\nARTIFACTS:\n- C:\\x\\output\\r.md\nROUTINE:\nevery_minutes: 1\ntitle: Cek harga\ntask: Cek harga emas\n");
        assert_eq!(b.body, "Sudah aku rangkum.");
        assert_eq!(b.next_actions, ["Kirim ke email", "Bikin grafik"]);
        assert_eq!(b.artifacts, ["C:\\x\\output\\r.md"]);
        assert_eq!(b.routine.unwrap(), json!({ "every_minutes": 2, "title": "Cek harga", "task": "Cek harga emas" }));
        assert_eq!(parse_blocks("Halo").body, "Halo");
        assert_eq!(parse_blocks("x\nROUTINE:\ntitle: a\n").routine, None);
    }

    #[test]
    fn engines_and_names() {
        assert_eq!(pick_engine("auto", true, true), Engine::ClaudeCode);
        assert_eq!(pick_engine("auto", false, true), Engine::Local);
        assert_eq!(pick_engine("local", true, false), Engine::Local);
        assert_eq!(pick_engine("codex", true, true), Engine::Codex);
        assert_eq!(pick_engine("codex", false, false), Engine::Local);
        assert_eq!(slugify("Riset Harga Emas!"), "riset-harga-emas");
        assert_eq!(slugify("???"), "badi");
    }

    #[test]
    fn a_fresh_session_gets_the_recent_chat_without_the_new_message() {
        let msgs = [json!({ "role": "user", "text": "cari harga emas" }), json!({ "role": "assistant", "text": "Rp1,9 juta per gram." }), json!({ "role": "user", "text": "simpan ke file" })];
        let ctx = recent_chat(&msgs, 8);
        assert!(ctx.contains("They: cari harga emas\nYou: Rp1,9 juta per gram."));
        assert!(!ctx.contains("simpan ke file"));
        assert_eq!(recent_chat(&[json!({ "role": "user", "text": "halo" })], 8), "");
    }

    #[test]
    fn agents_are_stored_like_v1_and_routines_catch_up_after_sleep() {
        let root = std::env::temp_dir().join(format!("badi-agents-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let s = Store::new(root.clone());
        let g = s.ensure_default().unwrap();
        assert_eq!(g["slug"], "general");
        assert!(s.dir("general").join("AGENTS.md").exists() && s.dir("general").join("output").is_dir());
        assert_eq!(s.create("General", "", None).unwrap()["slug"], "general-2");
        s.add_message("general", json!({ "role": "user", "text": "halo" })).unwrap();
        s.add_routine("general", &json!({ "every_minutes": 60, "title": "Cek", "task": "cek" })).unwrap();
        let id = s.get("general").unwrap()["routines"][0]["id"].as_str().unwrap().to_string();
        s.set_routine("general", &id, &json!({ "paused": true })).unwrap();
        assert_eq!(s.get("general").unwrap()["routines"][0]["paused"], true);
        s.set_routine("general", &id, &Value::Null).unwrap();
        assert_eq!(s.get("general").unwrap()["routines"], json!([]));
        assert_eq!(s.get("general").unwrap()["messages"][0]["text"], "halo");
        assert_eq!(s.list().len(), 2);
        assert!(!routine_due(0, 60, 59 * 60_000) && routine_due(0, 60, 8 * 3_600_000));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn each_sprite_kind_gets_one_folder_and_old_agents_stay_unlisted() {
        let root = std::env::temp_dir().join(format!("badi-sprites-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let s = Store::new(root.clone());
        s.ensure_default().unwrap(); // an agent from before Sprites
        let list = s.sprites();
        let slugs: Vec<&str> = list.iter().filter_map(|m| m["slug"].as_str()).collect();
        assert_eq!(slugs, ["ciro", "mino", "routine", "filo", "nilo", "daro"]);
        assert_eq!(list[0]["sprite"], "ciro");
        assert!(fs::read_to_string(s.dir("ciro").join("AGENTS.md")).unwrap().contains("You are Ciro. Coding, debugging and testing"));
        assert_eq!(s.sprites().len(), 6, "a second call makes no new folders");
        assert!(s.get("general").is_some());
        let _ = fs::remove_dir_all(&root);
    }
}
