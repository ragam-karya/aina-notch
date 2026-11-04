// "Kerjakan untukku": Badi operating the user's PC. One agent loop (brain.rs) with the computer toolset
// on a real display (desktop.rs), the browser toolset through the extension (bridge.rs), web search and
// fetch, and Badi's own tools: apps and links (apps.rs), files with their guard (files.rs, guard.rs),
// UI Automation, and questions to the user. The app supplies everything that talks to the user (Host).

pub mod apps;
pub mod agents;
pub mod brain;
pub mod commands;
pub mod diff;
pub mod engines;
pub mod focus;
pub mod bridge;
#[cfg(windows)]
pub mod desktop;
pub mod files;
pub mod guard;
pub mod sprites;

use std::path::PathBuf;

/// A change Badi wants to make, for the user's OK: kind "files", op (delete, move, copy, rename,
/// create, edit, write, overwrite), the names as a person says them, the folder, and every path touched.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Need {
    pub kind: String,
    pub op: String,
    pub names: Vec<String>,
    pub place: Option<String>,
    pub new_name: Option<String>,
    pub paths: Vec<PathBuf>,
    pub detail: String,
    /// For a command: what it does, in plain verbs (delete, move, rename, copy, create, write).
    pub acts: Vec<String>,
    /// For kind "app": what each work-app action changes.
    pub changes: Vec<AppChange>,
}

/// One change in a work app, for the spoken question: GMAIL_SEND_EMAIL to budi@x.com ->
/// { verb: "SEND", object: "email", app: "Gmail", target: "budi@x.com" }.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AppChange {
    pub verb: String,
    pub object: String,
    pub app: Option<String>,
    pub target: String,
}

/// What the loop needs from the app. Every method may block (questions wait for the user).
pub trait Host: Sync {
    /// Asks before a file change. None = go ahead; Some(why) = the user said no.
    fn approve(&self, need: &Need) -> Option<String>;
    /// A yes/no question; returns (approved, what they said).
    fn confirm(&self, question: &str) -> (bool, String);
    /// An open question; returns the answer ("" when none came).
    fn ask(&self, question: &str) -> String;
    /// Badi is about to act at this physical desktop point (click, drag, move, scroll, type).
    fn action_at(&self, x: i32, y: i32, kind: &str);
    /// A progress update between tool calls (thinking summaries, short texts).
    fn progress(&self, text: &str);
    /// One line for the log per tool call.
    fn action(&self, line: &str);
}
