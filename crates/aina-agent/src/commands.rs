// The guard every background-agent tool call passes (both engines): what a file write or a shell
// command may change, what needs the person's OK first, the Recycle Bin helper for PowerShell, and
// the one-line progress shown in the agent's log. Port of guardTool, approvalFor, prepareCommand,
// pathsIn and describeTool in src/agentTools.js.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use fancy_regex::Regex as Fancy;
use regex::Regex;
use serde_json::Value;

use crate::Need;
use crate::guard::{expand_path, fixed_folders, fixed_reason, home, inside, lower, protected_reason, protected_roots, resolve};

fn fancy(re: &str) -> Fancy {
    Fancy::new(re).unwrap()
}

fn hit(re: &Fancy, s: &str) -> bool {
    re.is_match(s).unwrap_or(false)
}

static START: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(?:[a-z]:[\\/]|\\\\|~(?:[\\/]|$)|\$home\b|\$\{?env:|%[a-z0-9_()]+%|\.\.(?:[\\/]|$))").unwrap());
static SPLIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\s;|&(){},=]+").unwrap());
static PARAM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^-\w+:").unwrap());

/// Paths written out in a command, as written: quoted ones (may hold spaces) and bare ones.
pub fn raw_paths_in(cmd: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = String::new();
    let chars: Vec<char> = cmd.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if (c == '"' || c == '\'')
            && let Some(end) = chars[i + 1..].iter().position(|&d| d == c)
        {
            let inner: String = chars[i + 1..i + 1 + end].iter().collect();
            if START.is_match(&inner) {
                found.push(inner);
            }
            rest.push(' ');
            i += end + 2;
            continue;
        }
        rest.push(c);
        i += 1;
    }
    for tok in SPLIT.split(&rest) {
        let t = PARAM.replace(tok, "");
        if START.is_match(&t) {
            found.push(t.into_owned());
        }
    }
    found
}

pub fn paths_in(cmd: &str) -> Vec<String> {
    raw_paths_in(cmd).iter().map(|p| expand_path(p)).collect()
}

// Commands that can wreck the PC no matter where they run.
static NEVER: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        r"(?i)\bformat(-volume)?\s+[a-z]:",
        r"(?i)\b(diskpart|bcdedit|vssadmin|cipher\s+/w|takeown|icacls|reg\s+delete|mkfs|dd\s+if=)\b",
        r"(?i)\b(clear-disk|initialize-disk|remove-partition|format-volume|stop-computer|restart-computer|set-executionpolicy|clear-recyclebin)\b",
        r"(?i)\bshutdown(\.exe)?\s+/[srp]",
        r"(?i)\b(remove-itemproperty|set-itemproperty|new-itemproperty)\b.*\bHK(LM|CU|EY_)",
        r"(?i)\bHK(LM|CU):",
    ]
    .iter()
    .map(|r| Regex::new(r).unwrap())
    .collect()
});
static DELETE: LazyLock<Fancy> = LazyLock::new(|| fancy(r"(?i)(^|[\s;|&({])(rm|rmdir|rd|del|erase|remove-item|ri)(?=[\s;|&)}]|$)"));
static MOVE: LazyLock<Fancy> = LazyLock::new(|| fancy(r"(?i)(^|[\s;|&({])(move-item|mv|move|mi|rename-item|ren|rni)(?=[\s;|&)}]|$)"));
static CHANGE: LazyLock<Fancy> = LazyLock::new(|| {
    fancy(r"(?i)(^|[\s;|&({])(copy-item|cp|copy|cpi|new-item|ni|mkdir|md|set-content|add-content|out-file|clear-content|sc|ac|clc|robocopy|xcopy|expand-archive|compress-archive|remove-torecyclebin|tee-object)(?=[\s;|&)}]|$)|>")
});
// A variable other than the pipeline item: its value can't be checked before the command runs.
static UNKNOWN_VAR: LazyLock<Fancy> = LazyLock::new(|| fancy(r"(?i)\$(?!_\b|_\.|psitem\b|true\b|false\b|null\b|env:|\{env:|home\b)[a-z_]"));
static RECYCLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)remove-torecyclebin").unwrap());

const RECYCLE_HINT: &str = "Delete things outside your working folder with Remove-ToRecycleBin <path> (it also takes pipeline input, e.g. Get-ChildItem *.tmp | Remove-ToRecycleBin); it sends them to the Recycle Bin so the person can undo it.";

fn under(dir: &Path, p: &str) -> PathBuf {
    let p = Path::new(p);
    if p.is_absolute() { p.to_path_buf() } else { dir.join(p) }
}

fn file_arg(input: &Value) -> Option<&str> {
    input["file_path"].as_str().or(input["notebook_path"].as_str())
}

/// Why a tool call is refused, or None to let it run. `focus`: dir is the person's focus folder (their
/// own project), not the agent's scratch folder, so even there deletes go to the Recycle Bin.
pub fn guard_tool(name: &str, input: &Value, dir: &Path, focus: bool) -> Option<String> {
    if ["Write", "Edit", "NotebookEdit"].contains(&name) {
        let file = file_arg(input)?;
        let why = protected_reason(&under(dir, file), Some(dir))?;
        return Some(format!("Not allowed: {why}. Change files in the person's own folders or your working folder."));
    }
    if name != "Bash" && name != "PowerShell" {
        return None;
    }
    let cmd = input["command"].as_str().unwrap_or("");
    if NEVER.iter().any(|re| re.is_match(cmd)) {
        return Some("That command can damage the PC or its settings, so it is not allowed.".into());
    }
    let deletes = hit(&DELETE, cmd);
    let moves = hit(&MOVE, cmd);
    if !deletes && !moves && !hit(&CHANGE, cmd) {
        return None; // reading and running things is fine
    }
    let paths: Vec<PathBuf> = paths_in(cmd).iter().map(|p| under(dir, p)).collect();
    for p in &paths {
        if let Some(why) = protected_reason(p, Some(dir)) {
            return Some(format!("Not allowed: {why}."));
        }
    }
    // A drive or a main folder (Desktop…) may be a destination, but never the thing deleted or moved:
    // with a delete anywhere in the command, or right after a move/rename verb (its source).
    for raw in raw_paths_in(cmd) {
        let p = under(dir, &expand_path(&raw));
        let Some(why) = fixed_reason(&p) else { continue };
        let source = Regex::new(&format!(r#"(?i)(move-item|mv|move|mi|rename-item|ren|rni)\s+(-(path|literalpath)\s+)?["']?{}"#, regex::escape(&raw))).unwrap();
        if deletes || RECYCLE.is_match(cmd) || (moves && source.is_match(cmd)) {
            return Some(format!("Not allowed: {why}; work with what is inside it instead."));
        }
    }
    if deletes {
        let outside = paths.iter().any(|p| !inside(p, dir));
        if focus || outside || hit(&UNKNOWN_VAR, cmd) {
            return Some(format!("Not run. {RECYCLE_HINT}"));
        }
    }
    None
}

static ACTS: LazyLock<Vec<(&str, Fancy)>> = LazyLock::new(|| {
    vec![
        ("delete", fancy(r"(?i)(^|[\s;|&({])(rm|rmdir|rd|del|erase|remove-item|ri|remove-torecyclebin)(?=[\s;|&)}]|$)")),
        ("move", fancy(r"(?i)(^|[\s;|&({])(move-item|mv|move|mi|robocopy)(?=[\s;|&)}]|$)")),
        ("rename", fancy(r"(?i)(^|[\s;|&({])(rename-item|ren|rni)(?=[\s;|&)}]|$)")),
        ("copy", fancy(r"(?i)(^|[\s;|&({])(copy-item|cp|copy|cpi|xcopy)(?=[\s;|&)}]|$)")),
        ("create", fancy(r"(?i)(^|[\s;|&({])(new-item|ni|mkdir|md|expand-archive|compress-archive)(?=[\s;|&)}]|$)")),
        ("write", fancy(r"(?i)(^|[\s;|&({])(set-content|add-content|out-file|clear-content|sc|ac|clc|tee-object)(?=[\s;|&)}]|$)|>")),
    ]
});
static NULL_REDIRECT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)2>\s*\$null|\*>\s*\$null|>\s*\$null").unwrap());

/// The folder a change happens in (the deepest one holding every path it touches), named the way a
/// person would say it.
pub fn place_of(paths: &[PathBuf]) -> Option<String> {
    let dirs: Vec<Vec<String>> = paths
        .iter()
        .map(|p| {
            let full = resolve(p);
            let base = full.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let d = if full.extension().is_some() || base.contains(['*', '?']) { full.parent().map(Path::to_path_buf).unwrap_or(full) } else { full };
            d.to_string_lossy().split('\\').filter(|s| !s.is_empty()).map(String::from).collect()
        })
        .collect();
    let first = dirs.first()?;
    let mut common = Vec::new();
    for (i, seg) in first.iter().enumerate() {
        if dirs.iter().all(|d| d.get(i).is_some_and(|s| s.eq_ignore_ascii_case(seg))) {
            common.push(seg.clone());
        } else {
            break;
        }
    }
    match common.len() {
        0 => None,
        1 => Some(format!("{}\\", common[0])),
        _ => common.last().cloned(),
    }
}

fn base_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string())
}

/// What a tool call would change outside the agent's own folder, for the person to approve, or None
/// when it stays inside or changes nothing. A changing command whose targets can't be seen
/// (variables) asks too.
pub fn approval_for(name: &str, input: &Value, dir: &Path, focus: bool) -> Option<Need> {
    if ["Write", "Edit", "NotebookEdit"].contains(&name) {
        let full = under(dir, file_arg(input)?);
        if inside(&full, dir) {
            return None;
        }
        let kind = if name == "Write" { "write" } else { "edit" };
        let detail = full.display().to_string();
        return Some(Need { kind: kind.into(), op: kind.into(), names: vec![base_name(&full)], paths: vec![full], detail, ..Need::default() });
    }
    if name != "Bash" && name != "PowerShell" {
        return None;
    }
    let cmd = input["command"].as_str().unwrap_or("");
    if !hit(&DELETE, cmd) && !hit(&MOVE, cmd) && !hit(&CHANGE, cmd) {
        return None;
    }
    let outside: Vec<PathBuf> = paths_in(cmd).iter().map(|p| under(dir, p)).filter(|p| !inside(p, dir)).collect();
    // In a focus folder creating, editing and moving inside are free; deleting always asks.
    let deletes_here = focus && (hit(&DELETE, cmd) || RECYCLE.is_match(cmd));
    // Variables can't be checked before the command runs: outside a focus folder that asks; inside one
    // (the person's own project, where scripts use $vars all the time) only written paths and deletes count.
    if outside.is_empty() && !deletes_here && (focus || !hit(&UNKNOWN_VAR, cmd)) {
        return None;
    }
    let quiet = NULL_REDIRECT.replace_all(cmd, "");
    let mut acts: Vec<String> = ACTS.iter().filter(|(_, re)| hit(re, &quiet)).map(|(a, _)| a.to_string()).collect();
    if acts.is_empty() {
        acts.push("write".into());
    }
    let place = if outside.is_empty() { None } else { place_of(&outside) };
    Some(Need { kind: "command".into(), op: acts[0].clone(), acts, place, paths: outside, detail: cmd.into(), ..Need::default() })
}

/// Defines Remove-ToRecycleBin for a PowerShell command that uses it. It re-checks each resolved path
/// at run time (variables included) against the same protected locations.
pub fn prepare_command(cmd: &str, dir: &Path) -> String {
    if !RECYCLE.is_match(cmd) {
        return cmd.to_string();
    }
    let q = |s: &str| format!("'{}'", s.replace('\'', "''"));
    let roots = protected_roots().iter().map(|r| q(r)).collect::<Vec<_>>().join(",");
    let fixed = fixed_folders().iter().map(|r| q(r)).collect::<Vec<_>>().join(",");
    let home0 = q(&lower(&home()));
    let own = q(&lower(dir));
    let f = format!(
        r#"function Remove-ToRecycleBin {{ param([Parameter(Mandatory=$true, Position=0, ValueFromPipeline=$true, ValueFromPipelineByPropertyName=$true)][Alias('FullName','LiteralPath')][string[]]$Path)
 begin {{ Add-Type -AssemblyName Microsoft.VisualBasic; $roots = @({roots}); $fixed = @({fixed}); $home0 = {home0}; $own = {own} }}
 process {{ foreach ($p in $Path) {{
  $full = (Resolve-Path -LiteralPath $p -ErrorAction Stop).ProviderPath; $l = $full.ToLower().TrimEnd('\')
  if (-not ($l -eq $own -or $l.StartsWith($own + '\'))) {{
   if ($roots | Where-Object {{ $l -eq $_ -or $l.StartsWith($_ + '\') }}) {{ Write-Error "Not allowed: $full is a protected location."; continue }}
   if ($fixed -contains $l -or $l -match '^[a-z]:$' -or ($l.StartsWith($home0 + '\') -and $l.Substring($home0.Length + 1).StartsWith('.'))) {{ Write-Error "Not allowed: $full can't be deleted."; continue }}
  }}
  if (Test-Path -LiteralPath $full -PathType Container) {{ [Microsoft.VisualBasic.FileIO.FileSystem]::DeleteDirectory($full, 'OnlyErrorDialogs', 'SendToRecycleBin') }}
  else {{ [Microsoft.VisualBasic.FileIO.FileSystem]::DeleteFile($full, 'OnlyErrorDialogs', 'SendToRecycleBin') }}
  "Moved to the Recycle Bin: $full" }} }} }}"#
    );
    format!("{f}\n{cmd}")
}

fn rel(p: &str, dir: &Path) -> String {
    if p.is_empty() {
        return String::new();
    }
    let full = under(dir, p);
    if !inside(&full, dir) {
        return p.to_string();
    }
    let (f, d) = (resolve(&full).display().to_string(), resolve(dir).display().to_string());
    let r = f.get(d.len()..).unwrap_or("").trim_start_matches('\\').to_string();
    if r.is_empty() { ".".into() } else { r }
}

fn short(s: &str, n: usize) -> String {
    if s.chars().count() > n { format!("{}…", s.chars().take(n - 1).collect::<String>()) } else { s.to_string() }
}

/// One short progress line for a tool call, for the agent's log in its window.
pub fn describe_tool(name: &str, input: &Value, dir: &Path) -> Option<String> {
    let name = name.strip_prefix("mcp__aina__").unwrap_or(name);
    let s = |k: &str| input[k].as_str().unwrap_or("").to_string();
    Some(match name {
        "str_replace_based_edit_tool" => {
            let verb = match s("command").as_str() {
                "view" => "Read",
                "create" => "Write",
                _ => "Edit",
            };
            format!("▸ {verb} {}", rel(&s("path"), dir))
        }
        "powershell" | "Bash" | "PowerShell" => format!("▸ $ {}", short(&s("command"), 70)),
        "search_files" => format!("▸ Grep {}", short(&s("pattern"), 50)),
        "glob_files" => format!("▸ Glob {}", short(&s("glob"), 50)),
        "Read" | "Write" | "Edit" => format!("▸ {name} {}", rel(&s("file_path"), dir)),
        "Glob" | "Grep" => format!("▸ {name} {}", short(&s("pattern"), 50)),
        "WebSearch" => format!("▸ Web: {}", short(&s("query"), 60)),
        "WebFetch" => format!("▸ Web: {}", short(&s("url"), 60)),
        "TodoWrite" => return None,
        _ => {
            let what = match input["paths"].as_array() {
                Some(paths) => {
                    let first = paths.first().and_then(Value::as_str).unwrap_or("");
                    let more = if paths.len() > 1 { format!(" +{}", paths.len() - 1) } else { String::new() };
                    let to = input["to"].as_str().map(|t| format!(" → {t}")).unwrap_or_default();
                    format!("{first}{more}{to}")
                }
                None => ["name", "window", "ref", "url", "pattern", "path"].iter().find_map(|k| input[*k].as_str().map(String::from)).unwrap_or_default(),
            };
            if what.is_empty() { format!("▸ {name}") } else { format!("▸ {name} {}", short(&what, 40)) }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn agent() -> PathBuf {
        home().join(r"AppData\Roaming\aina\agents\general") // the real layout: inside AppData
    }

    #[test]
    fn own_files_can_change_system_and_app_data_cannot() {
        let a = agent();
        let h = home();
        let w = |f: &str| guard_tool("Write", &json!({ "file_path": f }), &a, false);
        assert_eq!(w(&a.join(r"output\laporan.md").display().to_string()), None);
        assert_eq!(w(r"output\laporan.md"), None);
        assert_eq!(w(&h.join(r"Documents\Catatan\x.md").display().to_string()), None);
        assert_eq!(w(r"D:\Proyek\readme.txt"), None);
        assert!(w(r"C:\Windows\System32\drivers\etc\hosts").unwrap().contains("Not allowed"));
        assert!(w(r"C:\Program Files\App\x.ini").unwrap().contains("Not allowed"));
        assert!(w(&h.join(r"AppData\Roaming\aina\config.json").display().to_string()).unwrap().contains("app-data"));
        assert!(w(&format!(r"{}\..\other\x.md", a.display())).unwrap().contains("app-data"));
        assert!(w(&format!(r"{}-evil\x.md", a.display())).unwrap().contains("app-data"));
        assert!(w(&h.join(r".ssh\config").display().to_string()).unwrap().contains("hidden settings"));
        assert!(inside(Path::new(&format!(r"{}\tmp\a.txt", a.display().to_string().to_uppercase())), &a));
        assert_eq!(guard_tool("Read", &json!({ "file_path": r"C:\Windows\win.ini" }), &a, false), None);
    }

    #[test]
    fn powershell_organizes_folders_and_deletes_go_to_the_recycle_bin() {
        let a = agent();
        let h = home().display().to_string();
        let run = |c: &str| guard_tool("PowerShell", &json!({ "command": c }), &a, false);
        let ok = |c: &str| assert_eq!(run(c), None, "{c}");
        let no = |c: &str, want: &str| assert!(run(c).is_some_and(|w| w.contains(want)), "{c}: {:?}", run(c));
        ok("Get-ChildItem output");
        ok(r#"python -c "print(1)""#);
        ok(r"Remove-Item tmp\scratch.txt");
        ok("Get-ChildItem tmp | ForEach-Object { Remove-Item $_.FullName }");
        ok(r#"New-Item -ItemType Directory "$env:USERPROFILE\Downloads\PDF""#);
        ok(r"Get-ChildItem ~\Downloads -Filter *.pdf | Move-Item -Destination ~\Downloads\PDF");
        ok(&format!(r#"Rename-Item "{h}\Documents\laporan lama.docx" "laporan 2025.docx""#));
        ok(r"Copy-Item D:\Foto\a.jpg D:\Backup\");
        ok(r"Move-Item ~\Downloads\x.txt ~\Desktop");
        ok(r"Remove-ToRecycleBin ~\Downloads\old.zip");
        ok(r"Get-ChildItem D:\Temp -Filter *.tmp | Remove-ToRecycleBin");
        ok(r#"Set-Content -Path ~\Documents\todo.txt -Value "beli kopi""#);
        no(r"Remove-Item ~\Downloads\old.zip", "Remove-ToRecycleBin");
        no(&format!(r#"Remove-Item -Recurse -Force "{h}\Documents\Arsip""#), "Remove-ToRecycleBin");
        no(r"del %USERPROFILE%\Desktop\*.*", "Remove-ToRecycleBin");
        no("Remove-Item $target", "Remove-ToRecycleBin");
        no(r"Get-ChildItem D:\Temp | Remove-Item", "Remove-ToRecycleBin");
        no(r"Remove-ToRecycleBin ~\Desktop", "main folders");
        no(r"Move-Item ~\Documents D:\Docs", "main folders");
        no(&format!(r#"Rename-Item "{h}\Downloads" Unduhan"#), "main folders");
        no(r"Remove-ToRecycleBin D:\", "whole drive");
        no(r"Move-Item C:\Windows\notepad.exe ~\Desktop", "Not allowed");
        no(r#"Remove-ToRecycleBin "C:\Program Files\App""#, "Not allowed");
        no(r#"Set-Content $env:APPDATA\aina\config.json "{}""#, "Not allowed");
        no(r"Copy-Item x.txt ~\.ssh\", "Not allowed");
        for c in ["format D:", "Format-Volume -DriveLetter D", r"reg delete HKCU\Software\X /f", r"Set-ItemProperty HKCU:\Software\X -Name a -Value 1", "Clear-RecycleBin -Force", "shutdown /s /t 0", "Stop-Computer"] {
            no(c, "");
        }
        assert!(guard_tool("Bash", &json!({ "command": "rm -rf C:/Users" }), &a, false).is_some());
    }

    #[test]
    fn the_recycle_helper_is_defined_only_when_used() {
        let a = agent();
        assert_eq!(prepare_command("Get-Date", &a), "Get-Date");
        let cmd = prepare_command("Remove-ToRecycleBin x.txt", &a);
        assert!(cmd.starts_with("function Remove-ToRecycleBin") && cmd.contains("SendToRecycleBin") && cmd.ends_with("\nRemove-ToRecycleBin x.txt"));
        assert_eq!(paths_in(r#"Move-Item "C:\a b\c.txt" D:\x -Destination:E:\y"#), [r"C:\a b\c.txt", r"D:\x", r"E:\y"]);
    }

    #[test]
    fn changes_outside_the_agent_folder_ask_first() {
        let a = agent();
        let need = |n: &str, i: Value| approval_for(n, &i, &a, false);
        assert_eq!(need("Write", json!({ "file_path": r"output\a.md" })), None);
        let w = need("Write", json!({ "file_path": r"D:\Proyek\a.md" })).unwrap();
        assert_eq!((w.kind.as_str(), w.detail.as_str()), ("write", r"D:\Proyek\a.md"));
        let tidy = need("PowerShell", json!({ "command": r#"$d="D:\Unduhan"; New-Item -ItemType Directory "D:\Unduhan\PDF" | Out-Null; Move-Item "D:\Unduhan\a.pdf" "D:\Unduhan\PDF"; Get-ChildItem D:\Unduhan -Filter *.tmp | Remove-ToRecycleBin 2>$null"# })).unwrap();
        assert_eq!((tidy.acts, tidy.place.as_deref()), (vec!["delete".to_string(), "move".into(), "create".into()], Some("Unduhan")));
        assert_eq!(need("PowerShell", json!({ "command": r#"Rename-Item $f "baru.txt""# })).unwrap().place, None);
        assert_eq!(need("Edit", json!({ "file_path": r"D:\Proyek\a.md" })).unwrap().kind, "edit");
        assert_eq!(need("PowerShell", json!({ "command": r"Get-ChildItem ~\Downloads" })), None);
        assert_eq!(need("PowerShell", json!({ "command": r"New-Item -ItemType Directory output\pdf" })), None);
        assert_eq!(need("PowerShell", json!({ "command": r"Move-Item ~\Downloads\a.pdf ~\Documents" })).unwrap().kind, "command");
        assert_eq!(need("PowerShell", json!({ "command": r"Remove-ToRecycleBin D:\old.zip" })).unwrap().kind, "command");
    }

    #[test]
    fn in_a_focus_folder_edits_are_free_and_deletes_ask() {
        let f = Path::new(r"D:\Dev\weather-invitation");
        assert!(guard_tool("PowerShell", &json!({ "command": "Remove-Item old.js" }), f, true).unwrap().contains("Remove-ToRecycleBin"));
        assert_eq!(guard_tool("PowerShell", &json!({ "command": "Remove-ToRecycleBin old.js" }), f, true), None);
        assert_eq!(guard_tool("PowerShell", &json!({ "command": r"Remove-Item tmp\x" }), f, false), None);
        assert_eq!(approval_for("Write", &json!({ "file_path": r"src\app.js" }), f, true), None);
        assert_eq!(approval_for("PowerShell", &json!({ "command": r"New-Item -ItemType Directory src\lib; Move-Item a.js src\lib" }), f, true), None);
        assert_eq!(approval_for("PowerShell", &json!({ "command": "Remove-ToRecycleBin old.js" }), f, true).unwrap().kind, "command");
        assert_eq!(approval_for("PowerShell", &json!({ "command": "git status; git diff" }), f, true), None);
    }

    #[test]
    fn progress_lines_are_short_and_relative() {
        let d = Path::new(r"D:\agents\general");
        assert_eq!(describe_tool("Write", &json!({ "file_path": r"D:\agents\general\output\laporan.md" }), d).unwrap(), r"▸ Write output\laporan.md");
        assert_eq!(describe_tool("PowerShell", &json!({ "command": "Get-Date" }), d).unwrap(), "▸ $ Get-Date");
        assert_eq!(describe_tool("WebSearch", &json!({ "query": "harga emas hari ini" }), d).unwrap(), "▸ Web: harga emas hari ini");
        assert_eq!(describe_tool("mcp__aina__open_app", &json!({ "name": "Excel" }), d).unwrap(), "▸ open_app Excel");
        assert_eq!(describe_tool("TodoWrite", &json!({}), d), None);
    }
}
