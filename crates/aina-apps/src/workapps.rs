// The pure parts of the work apps: which apps, which actions read and which change something (and how
// to say the change out loud), the prompt line, the white-label providers, and the Drive upload helpers.
// Port of the same parts of src/workApps.js, with its tests.

use std::sync::LazyLock;

use aina_agent::{AppChange, Need};
use regex::Regex;
use serde_json::{Map, Value, json};

pub struct App {
    pub slug: &'static str,
    pub name: &'static str,
}

/// Toolkit slugs in Composio, and the names people say.
pub const APPS: [App; 17] = [
    App { slug: "gmail", name: "Gmail" },
    App { slug: "googlecalendar", name: "Google Calendar" },
    App { slug: "googledrive", name: "Google Drive" },
    App { slug: "googledocs", name: "Google Docs" },
    App { slug: "googlesheets", name: "Google Sheets" },
    App { slug: "googletasks", name: "Google Tasks" },
    App { slug: "outlook", name: "Outlook" },
    App { slug: "microsoft_teams", name: "Microsoft Teams" },
    App { slug: "notion", name: "Notion" },
    App { slug: "linear", name: "Linear" },
    App { slug: "jira", name: "Jira" },
    App { slug: "clickup", name: "ClickUp" },
    App { slug: "hubspot", name: "HubSpot" },
    App { slug: "salesforce", name: "Salesforce" },
    App { slug: "dropbox", name: "Dropbox" },
    App { slug: "github", name: "GitHub" },
    App { slug: "slack", name: "Slack" },
];

/// Still being built: only the Google apps can be linked for now; the rest show "Segera" in Settings.
pub fn soon(slug: &str) -> bool {
    !(slug == "gmail" || slug.starts_with("google"))
}

pub fn slugs() -> Vec<&'static str> {
    APPS.iter().map(|a| a.slug).collect()
}

pub fn app_name(slug: &str) -> String {
    APPS.iter().find(|a| a.slug == slug.to_lowercase()).map(|a| a.name.to_string()).unwrap_or_else(|| slug.to_string())
}

/// Meta tools aina gives Claude. Connecting is aina's own job (Settings or connect_app).
pub const KEEP: [&str; 3] = ["COMPOSIO_SEARCH_TOOLS", "COMPOSIO_GET_TOOL_SCHEMAS", "COMPOSIO_MULTI_EXECUTE_TOOL"];
pub const MAX_RESULT: usize = 30_000;

static READ: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"_(LIST|GET|FETCH|SEARCH|FIND|READ|RETRIEVE|QUERY|LOOKUP|DOWNLOAD|EXPORT|VIEW|CHECK|COUNT|DESCRIBE)(_|$)").unwrap());
const VERBS: [&str; 35] = [
    "SEND", "REPLY", "FORWARD", "CREATE", "ADD", "INSERT", "APPEND", "UPLOAD", "DELETE", "REMOVE", "TRASH", "ARCHIVE", "UPDATE", "EDIT", "MODIFY", "PATCH", "MOVE", "RENAME", "COPY",
    "SHARE", "INVITE", "ASSIGN", "CLOSE", "MERGE", "POST", "PUBLISH", "SCHEDULE", "CANCEL", "ACCEPT", "DECLINE", "COMMENT", "MARK", "SET", "COMPLETE", "REOPEN",
];
static WRITE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!("_({})(_|$)", VERBS.join("|"))).unwrap());

/// Anything not plainly a read counts as a change, so an unknown action asks rather than slipping through.
pub fn changes(slug: &str) -> bool {
    let s = slug.to_uppercase();
    WRITE.is_match(&s) || !READ.is_match(&s)
}

/// The app a tool slug belongs to (GMAIL_SEND_EMAIL -> gmail), or None.
pub fn app_of(slug: &str) -> Option<&'static str> {
    let s = slug.to_uppercase();
    APPS.iter().filter(|a| s.starts_with(&format!("{}_", a.slug.to_uppercase()))).max_by_key(|a| a.slug.len()).map(|a| a.slug)
}

/// What a change does, for the spoken question: the verb, the app and who or what it touches.
pub fn describe_change(slug: &str, args: &Value) -> AppChange {
    let s = slug.to_uppercase();
    let app = app_of(slug);
    let rest = match app {
        Some(a) => s[a.len() + 1..].to_string(),
        None => s.clone(),
    };
    let words: Vec<&str> = rest.split('_').collect();
    let verb = VERBS.iter().find(|v| words.contains(v)).copied().unwrap_or("RUN");
    let object = words.iter().filter(|w| **w != verb && !["A", "AN", "THE", "NEW"].contains(w)).copied().collect::<Vec<_>>().join(" ").to_lowercase();
    let target = ["recipient_email", "to", "email", "channel", "channel_name", "title", "summary", "subject", "name", "issue_title"]
        .iter()
        .find_map(|k| args[*k].as_str().filter(|v| !v.trim().is_empty()).map(String::from))
        .or_else(|| args["to"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")))
        .unwrap_or_default();
    AppChange { verb: verb.into(), object, app: app.map(app_name), target: target.chars().take(80).collect() }
}

/// What a COMPOSIO_MULTI_EXECUTE_TOOL call would change, or None when it only reads.
pub fn approval_for(name: &str, input: &Value) -> Option<Need> {
    if name != "COMPOSIO_MULTI_EXECUTE_TOOL" {
        return None;
    }
    let writes: Vec<&Value> = input["tools"].as_array().into_iter().flatten().filter(|c| changes(c["tool_slug"].as_str().unwrap_or(""))).collect();
    if writes.is_empty() {
        return None;
    }
    let args = |c: &Value| if c["arguments"].is_object() { c["arguments"].clone() } else { json!({}) };
    let changes: Vec<AppChange> = writes.iter().map(|c| describe_change(c["tool_slug"].as_str().unwrap_or(""), &args(c))).collect();
    let detail = writes.iter().map(|c| format!("{} {}", c["tool_slug"].as_str().unwrap_or(""), args(c).to_string().chars().take(200).collect::<String>())).collect::<Vec<_>>().join("\n");
    Some(Need { kind: "app".into(), op: changes[0].verb.to_lowercase(), changes, detail, ..Need::default() })
}

/// A tool in Anthropic's shape from Composio's (OpenAI-style function, or name/input_schema, or raw).
pub fn to_anthropic(tool: &Value) -> Value {
    let f = if tool["function"].is_object() { &tool["function"] } else { tool };
    // Raw Composio tools carry the real name in slug (their name is a short label).
    let name = f["slug"].as_str().or(f["name"].as_str()).unwrap_or("");
    let schema = [&f["parameters"], &f["input_schema"], &f["inputSchema"], &f["input_parameters"]].into_iter().find(|v| v.is_object()).cloned().unwrap_or(json!({ "type": "object", "properties": {} }));
    json!({ "name": name, "description": f["description"].as_str().unwrap_or(""), "input_schema": schema })
}

pub fn clip(text: &str) -> String {
    if text.chars().count() <= MAX_RESULT {
        return text.to_string();
    }
    format!("{}\n… (cut; ask for less, e.g. fewer results or specific fields)", text.chars().take(MAX_RESULT).collect::<String>())
}

/// For the prompt: which apps can be used now (switched on and linked). `apps`: (slug, linked, enabled).
pub fn prompt_line(apps: &[(String, bool, bool)]) -> String {
    let names: Vec<String> = apps.iter().filter(|a| a.1 && a.2).map(|a| app_name(&a.0)).collect();
    let off: Vec<String> = apps.iter().filter(|a| !a.2).map(|a| app_name(&a.0)).collect();
    let ready = if names.is_empty() { "none ready yet".to_string() } else { format!("ready now: {}", names.join(", ")) };
    let off = if off.is_empty() { String::new() } else { format!("; switched off by the user (never use them): {}", off.join(", ")) };
    format!(
        "Work apps through Composio: {ready}{off}. For Gmail, Google Calendar/Drive/Docs/Sheets/Tasks, Outlook, Teams, Notion, Linear, Jira, ClickUp, HubSpot, Salesforce, Dropbox, GitHub and Slack, use COMPOSIO_SEARCH_TOOLS to find the right tool, then COMPOSIO_MULTI_EXECUTE_TOOL to run it, instead of opening the app or website on screen. Read before you act (find the thread, event or issue first). aina asks the person before anything is sent, created, changed or deleted, so don't ask them yourself as well. If an app they need isn't linked, call connect_app for it and tell them to finish signing in in the browser."
    )
}

/// aina's tool for linking a work app.
pub fn connect_app_tool() -> Value {
    json!({ "name": "connect_app", "description": "Link one of the user's work apps (Gmail, Google Calendar, Slack, Notion…) so the app tools can use it. Opens the app's sign-in page in their browser; they finish signing in there.",
        "input_schema": { "type": "object", "properties": { "app": { "type": "string", "description": "App name, e.g. \"Gmail\" or \"Google Calendar\"" } }, "required": ["app"] } })
}

/// Uploads local files to Google Drive (binary files too, which the Composio tools can't take directly).
pub fn upload_to_drive_tool() -> Value {
    json!({ "name": "upload_to_drive", "description": "Upload files from this PC to the user's Google Drive (any file type, up to 100 MB each). Find the exact paths first with find_files or list_folder. aina asks the user before uploading.",
        "input_schema": { "type": "object", "properties": { "paths": { "type": "array", "items": { "type": "string" }, "description": "Full paths of the files" } }, "required": ["paths"] } })
}

/// The app a name the model or the person used points at (Gmail, "google calendar", microsoft_teams).
pub fn find_app(name: &str) -> Option<&'static App> {
    let want: String = name.to_lowercase().chars().filter(char::is_ascii_lowercase).collect();
    APPS.iter().find(|a| a.slug.replace('_', "") == want || a.name.to_lowercase().chars().filter(char::is_ascii_lowercase).collect::<String>() == want)
}

// ---------- white-label sign-in ----------

pub const REDIRECT_URI: &str = "https://backend.composio.dev/api/v1/auth-apps/add";

pub struct Brand {
    pub id: &'static str,
    pub name: String,
    pub slugs: Vec<&'static str>,
    pub console: Option<&'static str>,
}

pub fn brands() -> Vec<Brand> {
    let mut b = vec![
        Brand { id: "google", name: "Google (Gmail, Calendar, Drive, Docs, Sheets, Tasks)".into(), slugs: vec!["gmail", "googlecalendar", "googledrive", "googledocs", "googlesheets", "googletasks"], console: Some("https://console.cloud.google.com/apis/credentials") },
        Brand { id: "microsoft", name: "Microsoft (Outlook, Teams)".into(), slugs: vec!["outlook", "microsoft_teams"], console: Some("https://entra.microsoft.com/#view/Microsoft_AAD_RegisteredApps/ApplicationsListBlade") },
        Brand { id: "slack", name: "Slack".into(), slugs: vec!["slack"], console: Some("https://api.slack.com/apps") },
        Brand { id: "github", name: "GitHub".into(), slugs: vec!["github"], console: Some("https://github.com/settings/developers") },
        Brand { id: "notion", name: "Notion".into(), slugs: vec!["notion"], console: Some("https://www.notion.so/profile/integrations") },
    ];
    for slug in ["linear", "jira", "clickup", "hubspot", "salesforce", "dropbox"] {
        b.push(Brand { id: slug, name: app_name(slug), slugs: vec![slug], console: None });
    }
    b
}

/// Only auth configs for aina's apps, or None when there are none.
pub fn own_auth_configs(cfg: &Value) -> Option<Map<String, Value>> {
    let out: Map<String, Value> = cfg
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(slug, id)| APPS.iter().any(|a| a.slug == slug.as_str()) && id.as_str().is_some_and(|i| i.starts_with("ac_")))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    (!out.is_empty()).then_some(out)
}

// ---------- Drive upload helpers ----------

/// The argument that takes the file: the property marked file_uploadable (directly or in anyOf/oneOf).
pub fn upload_field(schema: &Value) -> Option<String> {
    fn marked(p: &Value) -> bool {
        p["file_uploadable"] == true || ["anyOf", "oneOf"].iter().any(|k| p[*k].as_array().is_some_and(|a| a.iter().any(marked)))
    }
    schema["properties"].as_object()?.iter().find(|(_, p)| marked(p)).map(|(k, _)| k.clone())
}

/// The uploaded file's name, id and link from whatever shape the tool returns.
pub fn drive_file(data: &Value, name: &str) -> (String, Option<String>, Option<String>) {
    let d = [&data["response_data"], &data["file"], data].into_iter().find(|v| v.is_object()).cloned().unwrap_or(json!({}));
    let s = |k: &str| d[k].as_str().map(String::from);
    let id = s("id").or(s("fileId")).or(s("file_id"));
    let link = s("webViewLink").or(s("web_view_link")).or(s("alternateLink")).or_else(|| id.as_ref().map(|i| format!("https://drive.google.com/file/d/{i}/view")));
    (s("name").or(s("title")).unwrap_or_else(|| name.to_string()), id, link)
}

/// The file's content type from its extension (Drive shows it as that kind of file).
pub fn mime_of(name: &str) -> &'static str {
    match name.rsplit('.').next().unwrap_or("").to_lowercase().as_str() {
        _ if !name.contains('.') => "application/octet-stream",
        "pdf" => "application/pdf",
        "txt" => "text/plain",
        "md" => "text/markdown",
        "csv" => "text/csv",
        "json" => "application/json",
        "html" | "htm" => "text/html",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "heic" => "image/heic",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "m4a" => "audio/mp4",
        "zip" => "application/zip",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_pass_changes_ask() {
        for s in ["GMAIL_SEND_EMAIL", "GOOGLECALENDAR_CREATE_EVENT", "SLACK_SENDS_A_MESSAGE_TO_A_SLACK_CHANNEL", "LINEAR_UPDATE_ISSUE", "GMAIL_REPLY_TO_THREAD", "NOTION_ARCHIVE_PAGE", "SOMETHING_ODD"] {
            assert!(changes(s), "{s}");
        }
        for s in ["GMAIL_FETCH_EMAILS", "GOOGLECALENDAR_EVENTS_LIST", "SLACK_LIST_ALL_CHANNELS", "GITHUB_GET_A_REPOSITORY", "NOTION_SEARCH_NOTION_PAGE"] {
            assert!(!changes(s), "{s}");
        }
        assert_eq!(approval_for("COMPOSIO_SEARCH_TOOLS", &json!({ "queries": [{ "use_case": "send an email" }] })), None);
        assert_eq!(approval_for("COMPOSIO_MULTI_EXECUTE_TOOL", &json!({ "tools": [{ "tool_slug": "GMAIL_FETCH_EMAILS", "arguments": { "max_results": 3 } }] })), None);
        let need = approval_for("COMPOSIO_MULTI_EXECUTE_TOOL", &json!({ "tools": [{ "tool_slug": "GMAIL_SEND_EMAIL", "arguments": { "recipient_email": "budi@contoh.id", "body": "hi" } }] })).unwrap();
        assert_eq!(need.kind, "app");
        assert_eq!(need.changes, [AppChange { verb: "SEND".into(), object: "email".into(), app: Some("Gmail".into()), target: "budi@contoh.id".into() }]);
        assert!(need.detail.contains("GMAIL_SEND_EMAIL"));
        assert_eq!(describe_change("GOOGLECALENDAR_CREATE_EVENT", &json!({ "summary": "Sync tim" })), AppChange { verb: "CREATE".into(), object: "event".into(), app: Some("Google Calendar".into()), target: "Sync tim".into() });
        assert_eq!(describe_change("MICROSOFT_TEAMS_CHATS_POST_MESSAGE", &json!({ "chat_id": "x" })).app.as_deref(), Some("Microsoft Teams"));
    }

    #[test]
    fn tools_apps_and_brands() {
        let schema = json!({ "type": "object", "properties": { "q": { "type": "string" } } });
        assert_eq!(to_anthropic(&json!({ "type": "function", "function": { "name": "COMPOSIO_SEARCH_TOOLS", "description": "Find tools", "parameters": schema } })), json!({ "name": "COMPOSIO_SEARCH_TOOLS", "description": "Find tools", "input_schema": schema }));
        assert_eq!(to_anthropic(&json!({ "slug": "X", "description": "d", "input_parameters": schema }))["name"], "X");
        assert_eq!(APPS.len(), 17);
        let line = prompt_line(&[("gmail".into(), true, true), ("notion".into(), true, false), ("slack".into(), false, true)]);
        assert!(line.contains("ready now: Gmail; switched off by the user (never use them): Notion."));
        assert!(line.contains("COMPOSIO_MULTI_EXECUTE_TOOL") && line.contains("connect_app"));
        let mut all: Vec<&str> = brands().iter().flat_map(|b| b.slugs.clone()).collect();
        all.sort();
        let mut want = slugs();
        want.sort();
        assert_eq!(all, want);
        assert_eq!(own_auth_configs(&json!({ "gmail": "ac_1", "slack": "", "evil": "ac_2", "github": "x" })), json!({ "gmail": "ac_1" }).as_object().cloned());
        assert_eq!(own_auth_configs(&json!({})), None);
        assert_eq!(app_of("GMAIL_SEND_EMAIL"), Some("gmail"));
        assert_eq!(app_of("GOOGLECALENDAR_EVENTS_LIST"), Some("googlecalendar"));
        assert_eq!(app_of("MICROSOFT_TEAMS_CHATS_POST_MESSAGE"), Some("microsoft_teams"));
        assert_eq!(app_of("COMPOSIO_SEARCH_TOOLS"), None);
        assert_eq!(find_app("Google Calendar").unwrap().slug, "googlecalendar");
        assert_eq!(find_app("microsoft_teams").unwrap().slug, "microsoft_teams");
    }

    #[test]
    fn drive_upload_helpers() {
        let staged = json!({ "type": "object", "file_uploadable": true });
        assert_eq!(upload_field(&json!({ "properties": { "folder_to_upload_to": { "type": "string" }, "file_to_upload": staged } })).as_deref(), Some("file_to_upload"));
        assert_eq!(upload_field(&json!({ "properties": { "file": { "anyOf": [{ "type": "null" }, staged] } } })).as_deref(), Some("file"));
        assert_eq!(upload_field(&json!({ "properties": { "name": { "type": "string" } } })), None);
        assert_eq!(drive_file(&json!({ "response_data": { "id": "abc", "name": "a.pdf", "webViewLink": "https://drive.google.com/x" } }), "a.pdf"), ("a.pdf".into(), Some("abc".into()), Some("https://drive.google.com/x".into())));
        assert_eq!(drive_file(&json!({ "id": "abc" }), "b.pdf").2.as_deref(), Some("https://drive.google.com/file/d/abc/view"));
        assert_eq!(drive_file(&Value::Null, "c.pdf"), ("c.pdf".into(), None, None));
        assert_eq!(mime_of("Laporan Q3.PDF"), "application/pdf");
        assert_eq!(mime_of("foto.jpeg"), "image/jpeg");
        assert_eq!(mime_of("data.xlsx"), "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet");
        assert_eq!(mime_of("README"), "application/octet-stream");
    }
}
