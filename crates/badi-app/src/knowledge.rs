// The user's knowledge base: files and folders dropped on the notch that Badi keeps using until they
// are removed. Only their paths are kept (config.json `knowledge`); the text is read from the files
// themselves (plain text, code, Markdown, Word and PDF) into an index in memory, rebuilt when a file
// changes. Each question gets only the few passages that match it (badi_core::knowledge::search), and
// realtime, the Brain and the agents get the list of what is there.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use badi_core::knowledge::{Chunk, chunks, search};
use serde_json::{Map, Value, json};
use tauri::{AppHandle, Manager};

use crate::AppState;

const TEXT_EXT: [&str; 32] = [
    "txt", "md", "markdown", "csv", "tsv", "json", "html", "htm", "xml", "yaml", "yml", "log", "ini", "toml", "rtf", "sql", "rs", "js", "ts", "tsx", "jsx", "py", "go", "java", "kt", "cs", "cpp", "c", "h", "php", "rb", "css",
];
const SKIP_DIRS: [&str; 8] = ["node_modules", ".git", "target", "dist", "build", "__pycache__", ".venv", "bin"];
const MAX_FILES: usize = 1500;
const MAX_FILE_BYTES: u64 = 15 * 1024 * 1024;
const MAX_DEPTH: usize = 5;
const PASSAGE: usize = 900;
/// The passages sent with one question (about 2,500 tokens at most).
const BUDGET: usize = 9000;

struct Index {
    key: Vec<(PathBuf, SystemTime)>, // every file read, with its time: a change rebuilds the index
    chunks: Vec<Chunk>,
}

static INDEX: Mutex<Option<Index>> = Mutex::new(None);

fn state(app: &AppHandle) -> tauri::State<'_, AppState> {
    app.state::<AppState>()
}

/// What is in the knowledge base (paths still on disk).
pub fn items(app: &AppHandle) -> Vec<PathBuf> {
    state(app).store.get("knowledge").as_array().into_iter().flatten().filter_map(Value::as_str).map(PathBuf::from).filter(|p| p.exists()).collect()
}

fn save(app: &AppHandle, list: &[PathBuf]) {
    let mut patch = Map::new();
    patch.insert("knowledge".into(), json!(list.iter().map(|p| p.display().to_string()).collect::<Vec<_>>()));
    let _ = state(app).store.remember(&patch);
    crate::notch::send_status(app);
}

/// Adds paths (already there ones are skipped); returns the names added.
pub fn add(app: &AppHandle, paths: &[PathBuf]) -> Vec<String> {
    let mut list = items(app);
    let mut added = Vec::new();
    for p in paths {
        let p = badi_agent::guard::real(p);
        if !list.contains(&p) {
            added.push(name(&p));
            list.push(p);
        }
    }
    save(app, &list);
    added
}

pub fn remove(app: &AppHandle, path: &Path) {
    let list: Vec<PathBuf> = items(app).into_iter().filter(|p| p != path).collect();
    save(app, &list);
}

pub fn name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string())
}

// Every readable file under the knowledge base, with its modified time.
fn files(roots: &[PathBuf]) -> Vec<(PathBuf, SystemTime)> {
    let mut out = Vec::new();
    let mut stack: Vec<(PathBuf, usize)> = roots.iter().map(|r| (r.clone(), 0)).collect();
    while let Some((p, depth)) = stack.pop() {
        if out.len() >= MAX_FILES {
            break;
        }
        let Ok(meta) = fs::metadata(&p) else { continue };
        if meta.is_dir() {
            if depth >= MAX_DEPTH {
                continue;
            }
            let Ok(rd) = fs::read_dir(&p) else { continue };
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().to_string();
                if n.starts_with('.') || SKIP_DIRS.contains(&n.as_str()) {
                    continue;
                }
                stack.push((e.path(), depth + 1));
            }
        } else if meta.len() <= MAX_FILE_BYTES && readable(&p) {
            out.push((p, meta.modified().unwrap_or(SystemTime::UNIX_EPOCH)));
        }
    }
    out.sort();
    out
}

fn ext(p: &Path) -> String {
    p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

fn readable(p: &Path) -> bool {
    let e = ext(p);
    TEXT_EXT.contains(&e.as_str()) || e == "docx" || e == "pdf"
}

/// A file's text: plain text as is, Word from its document XML, PDF through pdf-extract.
fn read_text(p: &Path) -> Option<String> {
    match ext(p).as_str() {
        "docx" => {
            let mut zip = zip::ZipArchive::new(fs::File::open(p).ok()?).ok()?;
            let mut xml = String::new();
            zip.by_name("word/document.xml").ok()?.read_to_string(&mut xml).ok()?;
            Some(docx_text(&xml))
        }
        // pdf-extract panics on some broken files; a bad PDF is just skipped.
        "pdf" => {
            let bytes = fs::read(p).ok()?;
            std::panic::catch_unwind(move || pdf_extract::extract_text_from_mem(&bytes).ok()).ok().flatten()
        }
        _ => Some(String::from_utf8_lossy(&fs::read(p).ok()?).into_owned()),
    }
}

// Word's XML to text: a paragraph per </w:p>, tags dropped, entities read.
fn docx_text(xml: &str) -> String {
    let xml = xml.replace("</w:p>", "\n\n").replace("<w:tab/>", "\t").replace("<w:br/>", "\n");
    let mut out = String::with_capacity(xml.len() / 3);
    let mut in_tag = false;
    for c in xml.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'")
}

// The passages of every file, rebuilt only when a file was added, removed or changed.
fn with_index<T>(app: &AppHandle, f: impl FnOnce(&[Chunk]) -> T) -> T {
    let key = files(&items(app));
    let mut idx = INDEX.lock().unwrap();
    if idx.as_ref().is_none_or(|i| i.key != key) {
        let mut all = Vec::new();
        for (p, _) in &key {
            if let Some(text) = read_text(p) {
                all.extend(chunks(&name(p), &text, PASSAGE));
            }
        }
        if std::env::var_os("BADI_LOG").is_some() {
            eprintln!("[knowledge] indexed {} files, {} passages", key.len(), all.len());
        }
        *idx = Some(Index { key, chunks: all });
    }
    f(&idx.as_ref().unwrap().chunks)
}

/// Reads the files ahead (PDFs take a while), so the next question doesn't wait for it.
pub fn warm(app: &AppHandle) {
    if !items(app).is_empty() {
        with_index(app, |_| ());
    }
}

/// The passages that match a question, as context for Claude; None when nothing matches.
pub fn context_for(app: &AppHandle, question: &str) -> Option<String> {
    if items(app).is_empty() || question.trim().is_empty() {
        return None;
    }
    with_index(app, |all| {
        let hits = search(all, question, BUDGET);
        if hits.is_empty() {
            return None;
        }
        let body: Vec<String> = hits.iter().map(|c| format!("[{}]\n{}", c.source, c.text)).collect();
        Some(format!(
            "From the user's knowledge base (their own files; use what helps answer and say which file it came from, ignore the rest):\n\n{}",
            body.join("\n\n")
        ))
    })
}

/// One line naming the knowledge base, for the realtime, Brain and agent prompts ("" when empty).
pub fn summary_line(app: &AppHandle) -> String {
    let list = items(app);
    if list.is_empty() {
        return String::new();
    }
    let paths: Vec<String> = list.iter().map(|p| p.display().to_string()).collect();
    format!("The user's knowledge base (their own reference files, use them when relevant): {}.", paths.join("; "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_xml_becomes_paragraphs() {
        let xml = r#"<w:document><w:body><w:p><w:r><w:t>Cuti 12 hari</w:t></w:r></w:p><w:p><w:r><w:t>R&amp;D</w:t><w:tab/><w:t>ok</w:t></w:r></w:p></w:body></w:document>"#;
        assert_eq!(docx_text(xml).trim(), "Cuti 12 hari\n\nR&D\tok");
    }
}
