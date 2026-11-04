// Sprites: the specialist helpers Badi (the main AI) spawns for background work. Each kind has a name,
// a specialty and its own folder under agents/<id>/ (memory in AGENTS.md, chat, output/, routines);
// every task is a new instance of a kind, so two coding tasks at once are Ciro 1 and Ciro 2.

/// A kind of Sprite.
#[derive(Debug, PartialEq, Eq)]
pub struct Kind {
    pub id: &'static str,
    pub name: &'static str,
    pub icon: &'static str,
    /// "Coding Sprite" / "Sprite coding", shown next to the name.
    pub role: [&'static str; 2],
    /// The short status while it works ("Coding" / "Ngoding").
    pub working: [&'static str; 2],
    /// Who it is, for its AGENTS.md and the prompt that picks a kind.
    pub purpose: &'static str,
}

pub const KINDS: [Kind; 6] = [
    Kind { id: "ciro", name: "Ciro", icon: "💻", role: ["Coding Sprite", "Sprite coding"], working: ["Coding", "Ngoding"], purpose: "Coding, debugging and testing: writes and fixes code, reviews diffs, hunts bugs and security issues, runs builds and tests." },
    Kind { id: "mino", name: "Mino", icon: "🔎", role: ["Research Sprite", "Sprite riset"], working: ["Researching", "Riset"], purpose: "Research, documentation and finding information: searches the web, compares sources and writes summaries and docs." },
    Kind { id: "routine", name: "Routine", icon: "⏰", role: ["Scheduling Sprite", "Sprite jadwal"], working: ["Scheduling", "Menjadwalkan"], purpose: "Reminders, scheduling and recurring routines: jobs that run on a cadence (\"every morning…\")." },
    Kind { id: "filo", name: "Filo", icon: "📁", role: ["Files Sprite", "Sprite file"], working: ["Organizing", "Merapikan"], purpose: "Files and documents: finds, sorts, moves, renames and converts files, reads and drafts documents." },
    Kind { id: "nilo", name: "Nilo", icon: "✉️", role: ["Mail Sprite", "Sprite email"], working: ["Writing", "Menulis"], purpose: "Email and communication: reads and drafts email and messages, follows up in the person's work apps." },
    Kind { id: "daro", name: "Daro", icon: "📊", role: ["Data Sprite", "Sprite data"], working: ["Analyzing", "Menganalisis"], purpose: "Data and analysis: works with spreadsheets, CSVs and numbers, makes charts and reports." },
];

pub fn kind(id: &str) -> Option<&'static Kind> {
    let id = id.trim().to_lowercase();
    KINDS.iter().find(|k| k.id == id)
}

/// The kind for a task with none named: Ciro for project work (a focus folder is set), else Mino.
pub fn default_kind(project: bool) -> &'static Kind {
    &KINDS[if project { 0 } else { 1 }]
}

/// A [DELEGATE] argument: "ciro|fix the build" names the Sprite; without a known kind in front, the
/// whole text is the task and the kind is the default.
pub fn parse_delegate(arg: &str, project: bool) -> (&'static Kind, String) {
    if let Some((head, rest)) = arg.split_once('|')
        && let Some(k) = kind(head)
        && !rest.trim().is_empty()
    {
        return (k, rest.trim().to_string());
    }
    (default_kind(project), arg.trim().to_string())
}

/// The lowest instance number not in use by a kind's live Sprites.
pub fn next_number(used: &[u32]) -> u32 {
    (1..).find(|n| !used.contains(n)).unwrap_or(1)
}

/// "Ciro" alone, "Ciro 2" while another Ciro is on screen.
pub fn display_name(kind: &Kind, num: u32, alone: bool) -> String {
    if alone { kind.name.to_string() } else { format!("{} {num}", kind.name) }
}

/// The list of kinds for the main prompt: "ciro (coding, …), mino (…)".
pub fn prompt_list() -> String {
    KINDS.iter().map(|k| format!("{} ({})", k.id, k.purpose.split(':').next().unwrap_or(k.purpose).to_lowercase())).collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_delegate_names_its_sprite_or_gets_the_default() {
        let (k, t) = parse_delegate("ciro|fix the failing build", false);
        assert_eq!((k.name, t.as_str()), ("Ciro", "fix the failing build"));
        let (k, t) = parse_delegate(" Nilo | balas email Sari", false);
        assert_eq!((k.name, t.as_str()), ("Nilo", "balas email Sari"));
        // No known kind in front: the whole text is the task.
        let (k, t) = parse_delegate("compare a|b testing tools", false);
        assert_eq!((k.name, t.as_str()), ("Mino", "compare a|b testing tools"));
        assert_eq!(parse_delegate("review the diff", true).0.name, "Ciro");
        assert_eq!(parse_delegate("ciro|", false).1, "ciro|");
    }

    #[test]
    fn instances_are_numbered_from_the_lowest_free_number() {
        assert_eq!(next_number(&[]), 1);
        assert_eq!(next_number(&[1, 2]), 3);
        assert_eq!(next_number(&[2]), 1);
        let ciro = kind("CIRO").unwrap();
        assert_eq!(display_name(ciro, 1, true), "Ciro");
        assert_eq!(display_name(ciro, 2, false), "Ciro 2");
        assert!(prompt_list().starts_with("ciro (coding, debugging and testing"));
    }
}
