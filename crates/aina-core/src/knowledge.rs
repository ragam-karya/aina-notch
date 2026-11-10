// The user's knowledge base, the pure part: what a drop on the notch means when aina wasn't asking for
// anything (a project folder to work in, or files to know), and finding the passages that matter for a
// question. Retrieval is plain keyword scoring (rarer words weigh more), so only a few relevant
// passages go with each question instead of every file.

use std::collections::{HashMap, HashSet};

use serde_json::{Value, json};

use crate::notch::Entry;

/// Names inside a folder that make it a project to work in rather than documents to read.
const PROJECT_MARKERS: [&str; 14] = [".git", "package.json", "Cargo.toml", "pyproject.toml", "requirements.txt", "go.mod", "pom.xml", "build.gradle", "composer.json", "Gemfile", "CMakeLists.txt", "Makefile", "tsconfig.json", "index.html"];
const PROJECT_EXT: [&str; 2] = [".sln", ".csproj"];
const CODE_EXT: [&str; 18] = ["rs", "js", "ts", "tsx", "jsx", "py", "go", "java", "kt", "cs", "cpp", "c", "h", "php", "rb", "swift", "vue", "svelte"];

/// A folder is a project when it has a build or repo marker, or mostly code files.
pub fn is_project(names: &[String]) -> bool {
    if names.iter().any(|n| PROJECT_MARKERS.contains(&n.as_str()) || PROJECT_EXT.iter().any(|e| n.to_lowercase().ends_with(e))) {
        return true;
    }
    let ext = |n: &String| n.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default();
    let files: Vec<String> = names.iter().filter(|n| n.contains('.')).map(ext).collect();
    let code = files.iter().filter(|e| CODE_EXT.contains(&e.as_str())).count();
    code >= 3 && code * 2 > files.len()
}

/// A drop aina wasn't waiting for: one project folder → { focus }, anything else → { knowledge }.
pub fn auto_drop(entries: &[Entry], project: impl Fn(&str) -> bool) -> Value {
    if entries.is_empty() {
        return json!({ "problem": "empty" });
    }
    if let [only] = entries
        && only.dir
        && project(&only.path)
    {
        return json!({ "focus": only.path });
    }
    json!({ "knowledge": entries.iter().map(|e| e.path.as_str()).collect::<Vec<_>>() })
}

/// A passage of one file.
#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    pub source: String,
    pub text: String,
}

/// Cuts a file's text into passages of about `size` characters, on paragraph and then line breaks.
pub fn chunks(source: &str, text: &str, size: usize) -> Vec<Chunk> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let push = |cur: &mut String, out: &mut Vec<Chunk>| {
        let t = cur.trim();
        if !t.is_empty() {
            out.push(Chunk { source: source.to_string(), text: t.to_string() });
        }
        cur.clear();
    };
    for para in text.replace("\r\n", "\n").split("\n\n") {
        for line in para.lines() {
            // A single very long line (minified text, a PDF without breaks) is cut by characters.
            let chars: Vec<char> = line.chars().collect();
            for piece in chars.chunks(size.max(1)) {
                if cur.chars().count() + piece.len() > size {
                    push(&mut cur, &mut out);
                }
                cur.extend(piece);
                cur.push('\n');
            }
        }
        if cur.chars().count() > size / 2 {
            push(&mut cur, &mut out);
        } else {
            cur.push('\n');
        }
    }
    push(&mut cur, &mut out);
    out
}

// Common words that say nothing about a topic (Indonesian and English).
const STOP: [&str; 60] = [
    "yang", "dan", "di", "ke", "dari", "ini", "itu", "untuk", "dengan", "ada", "apa", "aku", "kamu", "saya", "anda", "tidak", "bisa", "akan", "juga", "sudah", "atau", "pada", "dalam", "jadi", "kalau", "gimana", "bagaimana", "mana", "nya", "lagi", "dong", "sih", "kok", "tolong", "coba", "the", "and", "for", "with", "that", "this", "what", "how", "are", "was", "you", "your", "can", "does", "from", "about", "have", "has", "not", "but", "they", "will", "into", "there", "which",
];

fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 3 && !STOP.contains(w))
        .map(String::from)
        .collect()
}

/// The passages that best match `query`, best first, up to `budget` characters. Each query word counts
/// once per passage, weighted by how rare it is across the passages; passages matching nothing are left
/// out.
pub fn search<'a>(all: &'a [Chunk], query: &str, budget: usize) -> Vec<&'a Chunk> {
    let q: HashSet<String> = words(query).into_iter().collect();
    if q.is_empty() || all.is_empty() {
        return vec![];
    }
    let sets: Vec<HashSet<String>> = all.iter().map(|c| words(&format!("{} {}", c.source, c.text)).into_iter().collect()).collect();
    let mut df: HashMap<&str, usize> = HashMap::new();
    for w in &q {
        df.insert(w, sets.iter().filter(|s| s.contains(w)).count());
    }
    let n = all.len() as f64;
    let mut scored: Vec<(f64, usize)> = sets
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let score: f64 = q.iter().filter(|w| s.contains(*w)).map(|w| (1.0 + n / df[w.as_str()] as f64).ln()).sum();
            (score, i)
        })
        .filter(|(s, _)| *s > 0.0)
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut used = 0;
    let mut out = Vec::new();
    for (_, i) in scored {
        let len = all[i].text.chars().count();
        if used + len > budget && !out.is_empty() {
            break;
        }
        used += len;
        out.push(&all[i]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(n: &[&str]) -> Vec<String> {
        n.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn projects_are_told_from_documents() {
        assert!(is_project(&names(&[".git", "README.md"])));
        assert!(is_project(&names(&["package.json", "src"])));
        assert!(is_project(&names(&["App.sln", "notes.txt"])));
        assert!(is_project(&names(&["a.py", "b.py", "c.py", "data.csv"])));
        assert!(!is_project(&names(&["laporan.pdf", "kontrak.docx", "catatan.md"])));
        assert!(!is_project(&names(&["a.py", "x.pdf", "y.pdf", "z.pdf", "w.docx"])));
        assert!(!is_project(&names(&[])));
    }

    #[test]
    fn a_drop_nobody_asked_for_is_understood() {
        let dir = |p: &str| Entry { path: p.into(), dir: true, size: 0 };
        let file = |p: &str| Entry { path: p.into(), dir: false, size: 10 };
        let proj = |p: &str| p.ends_with("app");
        assert_eq!(auto_drop(&[dir(r"C:\code\app")], proj), json!({ "focus": r"C:\code\app" }));
        assert_eq!(auto_drop(&[dir(r"C:\Docs\Kontrak")], proj), json!({ "knowledge": [r"C:\Docs\Kontrak"] }));
        assert_eq!(auto_drop(&[file(r"C:\a.pdf"), dir(r"C:\code\app")], proj), json!({ "knowledge": [r"C:\a.pdf", r"C:\code\app"] }));
        assert_eq!(auto_drop(&[], proj), json!({ "problem": "empty" }));
    }

    #[test]
    fn passages_are_cut_and_the_relevant_ones_found() {
        let text = format!("Harga paket Plus Rp 149 ribu per bulan.\n\n{}\n\nCuti tahunan karyawan 12 hari, diajukan seminggu sebelumnya.", "Lorem ipsum dolor sit amet. ".repeat(40));
        let all = chunks("kebijakan.md", &text, 400);
        assert!(all.len() >= 3, "{all:?}");
        assert!(all.iter().all(|c| c.text.chars().count() <= 400 + 2));
        let hits = search(&all, "berapa hari cuti tahunan aku?", 2000);
        assert!(hits[0].text.contains("Cuti tahunan"));
        assert!(hits.iter().all(|c| !c.text.starts_with("Lorem")));
        assert!(search(&all, "apa itu?", 2000).is_empty(), "stop words alone match nothing");
        // The file name counts too.
        assert!(!search(&all, "kebijakan", 2000).is_empty());
    }
}
