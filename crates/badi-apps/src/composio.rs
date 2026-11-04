// Composio for Badi, without its Node SDK. Two kinds of key:
// - a project API key (ak_…): a tool-router session over REST (backend.composio.dev/api/v3.1) for this
//   PC's user id, linked through the session's link endpoint, with white-label auth configs and Drive
//   uploads staged in Composio's file storage;
// - a Composio Connect consumer key (ck_…): Composio's hosted MCP endpoint (streamable HTTP), where the
//   person's own Composio account holds the links; linking and status go through
//   COMPOSIO_MANAGE_CONNECTIONS.
// Both give the same meta tools. Nothing is written to disk; OAuth tokens live at Composio.
// Port of the session parts of src/workApps.js.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use md5::{Digest, Md5};
use reqwest::blocking::{Client, RequestBuilder, Response};
use serde_json::{Map, Value, json};

use crate::workapps::{KEEP, drive_file, mime_of, own_auth_configs, slugs, to_anthropic, upload_field};

// Composio's REST API: direct, or through Badi Cloud (which sets the user id and the sign-in configs).
fn api_url(path: &str) -> String {
    badi_net::url(badi_net::Service::Composio, &format!("/api/v3.1{path}"))
}
const CONNECT_URL: &str = "https://connect.composio.dev/mcp";
const SESSION_TTL: Duration = Duration::from_secs(30 * 60);
const LINK_WAIT: Duration = Duration::from_secs(10 * 60);
const UPLOAD_SLUG: &str = "GOOGLEDRIVE_UPLOAD_FILE";

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Opts {
    pub api_key: String,
    pub user_id: String,
    pub timezone: String,
    pub auth_configs: Value,
}

pub fn is_connect_key(key: &str) -> bool {
    key.starts_with("ck_")
}

/// A meta tool's result: { successful, error, data }.
#[derive(Debug, Clone)]
pub struct Outcome {
    pub ok: bool,
    pub error: String,
    pub data: Value,
}

fn client() -> Client {
    Client::builder().timeout(Duration::from_secs(120)).build().unwrap()
}

fn read(res: Result<Response, reqwest::Error>) -> Result<Value, String> {
    let res = res.map_err(|e| e.to_string())?;
    let status = res.status();
    let text = res.text().unwrap_or_default();
    let body: Value = serde_json::from_str(&text).unwrap_or(Value::String(text.clone()));
    if !status.is_success() {
        let msg = body["error"]["message"].as_str().or(body["message"].as_str()).or(body["error"].as_str()).map(String::from).unwrap_or(text);
        return Err(format!("Composio {status}: {}", msg.chars().take(300).collect::<String>()));
    }
    Ok(body)
}

// ---------- Connect key: MCP over streamable HTTP ----------

struct Mcp {
    key: String,
    session: Option<String>,
    seq: u64,
}

impl Mcp {
    fn post(&mut self, method: &str, params: Value, notify: bool) -> Result<Value, String> {
        self.seq += 1;
        let mut msg = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        if !notify {
            msg["id"] = json!(self.seq);
        }
        let mut req: RequestBuilder = client()
            .post(CONNECT_URL)
            .header("x-consumer-api-key", &self.key)
            .header("Accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", "2025-06-18")
            .json(&msg);
        if let Some(s) = &self.session {
            req = req.header("Mcp-Session-Id", s);
        }
        let res = req.send().map_err(|e| e.to_string())?;
        if let Some(s) = res.headers().get("mcp-session-id").and_then(|v| v.to_str().ok()) {
            self.session = Some(s.to_string());
        }
        let status = res.status();
        let sse = res.headers().get("content-type").and_then(|v| v.to_str().ok()).is_some_and(|c| c.contains("event-stream"));
        let text = res.text().map_err(|e| e.to_string())?;
        if !status.is_success() {
            return Err(format!("Composio {status}: {}", text.chars().take(300).collect::<String>()));
        }
        if notify {
            return Ok(Value::Null);
        }
        // The answer is the JSON body, or the event in the stream with this request's id.
        let reply: Value = if sse {
            text.lines()
                .filter_map(|l| l.strip_prefix("data:"))
                .filter_map(|d| serde_json::from_str::<Value>(d.trim()).ok())
                .find(|m| m["id"] == self.seq)
                .ok_or("Composio sent no answer")?
        } else {
            serde_json::from_str(&text).map_err(|e| e.to_string())?
        };
        if let Some(e) = reply.get("error") {
            return Err(e["message"].as_str().unwrap_or("MCP error").to_string());
        }
        Ok(reply["result"].clone())
    }

    fn open(key: &str) -> Result<Mcp, String> {
        let mut m = Mcp { key: key.into(), session: None, seq: 0 };
        m.post("initialize", json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "badi", "version": env!("CARGO_PKG_VERSION") } }), false)?;
        m.post("notifications/initialized", json!({}), true)?;
        Ok(m)
    }

    fn call(&mut self, name: &str, args: &Value) -> Result<Outcome, String> {
        let r = self.post("tools/call", json!({ "name": name, "arguments": args }), false)?;
        let text = r["content"].as_array().into_iter().flatten().filter(|c| c["type"] == "text").filter_map(|c| c["text"].as_str()).collect::<Vec<_>>().join("\n");
        if r["isError"] == true {
            return Ok(Outcome { ok: false, error: if text.is_empty() { "The app tool failed.".into() } else { text }, data: Value::Null });
        }
        Ok(match serde_json::from_str::<Value>(&text) {
            Ok(v) if v.get("successful").is_some() => Outcome { ok: v["successful"] != false, error: v["error"].as_str().unwrap_or("").into(), data: v["data"].clone() },
            Ok(v) => Outcome { ok: true, error: String::new(), data: v },
            Err(_) => Outcome { ok: true, error: String::new(), data: Value::String(text) },
        })
    }
}

// ---------- the session (either kind) ----------

enum Kind {
    Project { session_id: String },
    Connect(Mcp),
}

pub struct Session {
    key: String,
    user_id: String,
    brands: String,
    at: Instant,
    kind: Kind,
    // Listed the first time a task needs them (`tools()`), not when the session opens: linking an app
    // or reading which apps are linked needs no tool list, and listing 17 apps' tools page by page is
    // what made switching an app on slow.
    tools: Option<Vec<Value>>,
}

fn project(key: &str) -> impl Fn(RequestBuilder) -> RequestBuilder + '_ {
    move |r| r.header("x-api-key", key)
}

impl Session {
    fn open(o: &Opts) -> Result<Session, String> {
        let brands = own_auth_configs(&o.auth_configs).map(|m| Value::Object(m).to_string()).unwrap_or_default();
        if is_connect_key(&o.api_key) {
            let mcp = Mcp::open(&o.api_key)?;
            return Ok(Session { key: o.api_key.clone(), user_id: o.user_id.clone(), brands, at: Instant::now(), kind: Kind::Connect(mcp), tools: None });
        }
        let auth = project(&o.api_key);
        let mut body = json!({
            "user_id": o.user_id,
            "toolkits": { "enable": slugs() },
            "manage_connections": { "enable": false },
            "workbench": { "enable": false }, // no remote code sandbox: results come straight back to Claude
        });
        if let Some(ac) = own_auth_configs(&o.auth_configs) {
            body["auth_configs"] = Value::Object(ac); // white-labeled apps sign in with the person's own OAuth app
        }
        if !o.timezone.is_empty() {
            body["experimental"] = json!({ "assistive_prompt_config": { "user_timezone": o.timezone } });
        }
        let s = read(auth(client().post(api_url("/tool_router/session")).json(&body)).send())?;
        let session_id = s["session_id"].as_str().ok_or("Composio gave no session")?.to_string();
        Ok(Session { key: o.api_key.clone(), user_id: o.user_id.clone(), brands, at: Instant::now(), kind: Kind::Project { session_id }, tools: None })
    }

    /// The meta tools Claude gets (search, schemas, multi-execute), listed once per session.
    pub fn tools(&mut self) -> Result<Vec<Value>, String> {
        if let Some(t) = &self.tools {
            return Ok(t.clone());
        }
        let key = self.key.clone();
        let tools: Vec<Value> = match &mut self.kind {
            Kind::Connect(mcp) => {
                let listed = mcp.post("tools/list", json!({}), false)?;
                listed["tools"].as_array().into_iter().flatten().filter(|t| KEEP.contains(&t["name"].as_str().unwrap_or(""))).map(to_anthropic).collect()
            }
            Kind::Project { session_id } => {
                let mut tools = Vec::new();
                let mut cursor: Option<String> = None;
                loop {
                    let mut url = api_url(&format!("/tool_router/session/{session_id}/tools?limit=100"));
                    if let Some(c) = &cursor {
                        url.push_str(&format!("&cursor={c}"));
                    }
                    let page = read(project(&key)(client().get(url)).send())?;
                    tools.extend(page["items"].as_array().into_iter().flatten().filter(|t| KEEP.contains(&t["slug"].as_str().or(t["name"].as_str()).unwrap_or(""))).map(to_anthropic));
                    cursor = page["next_cursor"].as_str().map(String::from);
                    // Every meta tool is in: the rest of the pages are the apps' own tools.
                    if cursor.is_none() || KEEP.iter().all(|k| tools.iter().any(|t| t["name"] == *k)) {
                        break;
                    }
                }
                tools
            }
        };
        self.tools = Some(tools.clone());
        Ok(tools)
    }

    pub fn execute(&mut self, name: &str, args: &Value) -> Result<Outcome, String> {
        let key = self.key.clone();
        match &mut self.kind {
            Kind::Connect(mcp) => mcp.call(name, args),
            Kind::Project { session_id } => {
                let (path, body) = if name.starts_with("COMPOSIO_") { ("execute_meta", json!({ "slug": name, "arguments": args })) } else { ("execute", json!({ "tool_slug": name, "arguments": args })) };
                let r = read(project(&key)(client().post(api_url(&format!("/tool_router/session/{session_id}/{path}"))).json(&body)).send())?;
                let error = match &r["error"] {
                    Value::Null => String::new(),
                    Value::String(s) => s.clone(),
                    e => e.to_string(),
                };
                Ok(Outcome { ok: error.is_empty(), error, data: r["data"].clone() })
            }
        }
    }

    /// slug -> an active account id, or None.
    pub fn status(&mut self) -> Result<HashMap<String, Option<String>>, String> {
        let key = self.key.clone();
        match &mut self.kind {
            Kind::Connect(mcp) => {
                let list: Vec<Value> = slugs().iter().map(|s| json!({ "name": s, "action": "list" })).collect();
                let r = mcp.call("COMPOSIO_MANAGE_CONNECTIONS", &json!({ "toolkits": list }))?;
                Ok(slugs()
                    .iter()
                    .map(|s| {
                        let id = r.data["results"][*s]["accounts"].as_array().into_iter().flatten().find(|a| a["status"].as_str().is_some_and(|x| x.eq_ignore_ascii_case("active"))).and_then(|a| a["id"].as_str().map(String::from));
                        (s.to_string(), id)
                    })
                    .collect())
            }
            Kind::Project { session_id } => {
                let mut out = HashMap::new();
                let mut cursor: Option<String> = None;
                loop {
                    let mut url = api_url(&format!("/tool_router/session/{session_id}/toolkits?limit=50"));
                    if let Some(c) = &cursor {
                        url.push_str(&format!("&cursor={c}"));
                    }
                    let page = read(project(&key)(client().get(url)).send())?;
                    for t in page["items"].as_array().into_iter().flatten() {
                        out.insert(t["slug"].as_str().unwrap_or("").to_lowercase(), active_account(t));
                    }
                    cursor = page["next_cursor"].as_str().map(String::from);
                    if cursor.is_none() {
                        break;
                    }
                }
                Ok(out)
            }
        }
    }

    /// Starts linking an app: the sign-in URL, and the account to wait for (project keys).
    fn link(&mut self, slug: &str) -> Result<(String, Option<String>), String> {
        let key = self.key.clone();
        match &mut self.kind {
            Kind::Connect(mcp) => {
                let r = mcp.call("COMPOSIO_MANAGE_CONNECTIONS", &json!({ "toolkits": [{ "name": slug, "action": "add" }] }))?;
                let url = r.data["results"][slug]["redirect_url"].as_str().ok_or_else(|| if r.error.is_empty() { format!("Composio gave no sign-in link for {slug}.") } else { r.error.clone() })?;
                Ok((url.to_string(), None))
            }
            Kind::Project { session_id } => {
                let r = read(project(&key)(client().post(api_url(&format!("/tool_router/session/{session_id}/link"))).json(&json!({ "toolkit": slug }))).send())?;
                Ok((r["redirect_url"].as_str().ok_or("Composio gave no sign-in link")?.to_string(), r["connected_account_id"].as_str().map(String::from)))
            }
        }
    }

    fn unlink(&mut self, slug: &str) -> Result<(), String> {
        let key = self.key.clone();
        match &mut self.kind {
            Kind::Connect(mcp) => {
                let r = mcp.call("COMPOSIO_MANAGE_CONNECTIONS", &json!({ "toolkits": [{ "name": slug, "action": "list" }] }))?;
                let remove: Vec<Value> = r.data["results"][slug]["accounts"].as_array().into_iter().flatten().filter_map(|a| a["id"].as_str()).map(|id| json!({ "name": slug, "action": "remove", "account_id": id })).collect();
                if !remove.is_empty() {
                    mcp.call("COMPOSIO_MANAGE_CONNECTIONS", &json!({ "toolkits": remove }))?;
                }
                Ok(())
            }
            Kind::Project { .. } => match self.status()?.get(slug).cloned() {
                Some(Some(id)) if id != "linked" => {
                    read(project(&key)(client().delete(api_url(&format!("/connected_accounts/{id}")))).send())?;
                    Ok(())
                }
                Some(Some(_)) => Err(format!("Composio tidak memberi id akun {slug}, jadi belum bisa diputus dari sini.")),
                _ => Ok(()),
            },
        }
    }
}

/// The linked account of one item of a session's toolkit list, when it is usable. The list documents
/// `connected_account` only as an object or null: it may sit under `connection`, and may come without a
/// status, so an account with an id and no failed status counts as linked.
fn active_account(item: &Value) -> Option<String> {
    let acc = [&item["connected_account"], &item["connection"]["connected_account"]].into_iter().find(|a| a.is_object())?;
    let status = acc["status"].as_str().unwrap_or("ACTIVE");
    if !status.eq_ignore_ascii_case("active") || acc["is_disabled"] == true {
        return None;
    }
    Some(acc["id"].as_str().unwrap_or("linked").to_string())
}

// One session per key (and user id and white-label set), reused for a while.
static CACHED: Mutex<Option<Session>> = Mutex::new(None);

/// Runs `f` on the session for these options, opening (or renewing) it when needed.
pub fn with_session<T>(o: &Opts, f: impl FnOnce(&mut Session) -> Result<T, String>) -> Result<T, String> {
    if o.api_key.is_empty() {
        return Err("Add a Composio API key in Settings first.".into());
    }
    let brands = own_auth_configs(&o.auth_configs).map(|m| Value::Object(m).to_string()).unwrap_or_default();
    let mut cached = CACHED.lock().unwrap();
    let fresh = cached.as_ref().is_some_and(|s| s.key == o.api_key && s.user_id == o.user_id && s.brands == brands && s.at.elapsed() < SESSION_TTL);
    if !fresh {
        *cached = Some(Session::open(o)?);
    }
    f(cached.as_mut().unwrap())
}

pub fn forget() {
    *CACHED.lock().unwrap() = None;
}

/// Which apps are linked: slug -> linked.
pub fn status(o: &Opts) -> Result<HashMap<String, bool>, String> {
    with_session(o, |s| s.status()).map(|m| m.into_iter().map(|(k, v)| (k, v.is_some())).collect())
}

/// Starts linking an app; `done` runs on another thread once it is linked (or gives up after 10 minutes).
pub fn connect(o: &Opts, slug: &str, done: impl FnOnce(Result<(), String>) + Send + 'static) -> Result<String, String> {
    let (url, account) = with_session(o, |s| s.link(slug))?;
    let (o, slug) = (o.clone(), slug.to_string());
    CANCELLED.lock().unwrap().retain(|s| *s != slug);
    std::thread::spawn(move || {
        let until = Instant::now() + LINK_WAIT;
        let mut round = 0;
        while Instant::now() < until {
            std::thread::sleep(Duration::from_secs(4));
            round += 1;
            if CANCELLED.lock().unwrap().contains(&slug) {
                return done(Err(CANCELLED_ERR.into()));
            }
            // The new account itself, quick to read; every fourth round (or without one) also the
            // session's list, in case the account read is refused or the app linked another account.
            let status = account.as_ref().and_then(|id| read(project(&o.api_key)(client().get(api_url(&format!("/connected_accounts/{id}")))).send()).ok()).and_then(|a| a["status"].as_str().map(str::to_uppercase));
            // The sign-in was refused or ran out: no point waiting for it.
            if matches!(status.as_deref(), Some("FAILED" | "EXPIRED" | "REVOKED")) {
                return done(Err(format!("{slug} sign-in failed or expired.")));
            }
            let linked = status.as_deref() == Some("ACTIVE") || ((account.is_none() || round % 4 == 0) && with_session(&o, |s| s.status()).is_ok_and(|m| m.get(&slug).is_some_and(Option::is_some)));
            if linked {
                forget();
                return done(Ok(()));
            }
        }
        done(Err(format!("{slug} was not linked within 10 minutes.")));
    });
    Ok(url)
}

/// Stops waiting for a sign-in (the person cancelled it, or closed the page without finishing): its
/// `done` gets CANCELLED_ERR within a few seconds.
pub fn cancel(slug: &str) {
    let mut c = CANCELLED.lock().unwrap();
    if !c.iter().any(|s| s == slug) {
        c.push(slug.to_string());
    }
}

pub const CANCELLED_ERR: &str = "cancelled";
static CANCELLED: Mutex<Vec<String>> = Mutex::new(Vec::new());

pub fn disconnect(o: &Opts, slug: &str) -> Result<(), String> {
    with_session(o, |s| s.unlink(slug))?;
    forget();
    Ok(())
}

fn need_project(o: &Opts) -> Result<(), String> {
    if is_connect_key(&o.api_key) {
        return Err("Login atas nama sendiri butuh Project API key Composio (ak_…). Key Composio Connect (ck_…) selalu login atas nama Composio.".into());
    }
    if o.api_key.is_empty() {
        return Err("Simpan Composio API key dulu.".into());
    }
    Ok(())
}

/// Registers the person's OAuth app for every app of a provider. Returns { slug: "ac_…" }; on a
/// failure halfway, the configs already made are removed again.
pub fn brand(o: &Opts, slugs: &[&str], client_id: &str, client_secret: &str) -> Result<Map<String, Value>, String> {
    need_project(o)?;
    if client_id.trim().is_empty() || client_secret.trim().is_empty() {
        return Err("Isi Client ID dan Client Secret dari OAuth app-mu.".into());
    }
    let mut made = Map::new();
    for slug in slugs {
        let body = json!({
            "toolkit": { "slug": slug },
            "auth_config": { "type": "use_custom_auth", "name": format!("aina ({})", crate::workapps::app_name(slug)), "authScheme": "OAUTH2",
                "credentials": { "client_id": client_id.trim(), "client_secret": client_secret.trim() } }
        });
        match read(project(&o.api_key)(client().post(api_url("/auth_configs")).json(&body)).send()) {
            Ok(r) => {
                made.insert(slug.to_string(), r["auth_config"]["id"].clone());
            }
            Err(e) => {
                let ids: Vec<String> = made.values().filter_map(|v| v.as_str().map(String::from)).collect();
                unbrand(o, &ids);
                return Err(e);
            }
        }
    }
    Ok(made)
}

/// Removes auth configs (already gone is fine).
pub fn unbrand(o: &Opts, ids: &[String]) {
    for id in ids {
        let _ = project(&o.api_key)(client().delete(api_url(&format!("/auth_configs/{id}")))).send();
    }
}

/// Runs one app tool by its slug (GOOGLECALENDAR_EVENTS_LIST…) and returns its data. A project key
/// executes it in the session; a Connect key has only the meta tools, so it goes through the
/// multi-execute tool and comes back unwrapped. A tool that reports a failure is an Err.
pub fn run(o: &Opts, slug: &str, args: &Value) -> Result<Value, String> {
    if !is_connect_key(&o.api_key) {
        let r = with_session(o, |s| s.execute(slug, args))?;
        return if r.ok { Ok(r.data) } else { Err(if r.error.is_empty() { r.data.to_string() } else { r.error }) };
    }
    let call = json!({ "tools": [{ "tool_slug": slug, "arguments": args }], "sync_response_to_workbench": false });
    let r = with_session(o, |s| s.execute("COMPOSIO_MULTI_EXECUTE_TOOL", &call))?;
    if !r.ok {
        return Err(r.error);
    }
    let first = r.data["results"].get(0).cloned().unwrap_or(r.data);
    let inner = if first["response"].is_object() { first["response"].clone() } else { first };
    if inner["successful"] == false || inner["error"].as_str().is_some_and(|e| !e.is_empty()) {
        return Err(inner["error"].as_str().map(String::from).unwrap_or_else(|| inner.to_string()));
    }
    Ok(if inner["data"].is_null() { inner } else { inner["data"].clone() })
}

static SCHEMAS: Mutex<Option<(String, HashMap<String, Value>)>> = Mutex::new(None);

/// A tool's input schema ({ properties… }), fetched once per key, or Null when Composio doesn't say.
/// Callers name their arguments from it, so a renamed field doesn't break them.
pub fn schema(o: &Opts, slug: &str) -> Value {
    if let Some((k, m)) = SCHEMAS.lock().unwrap().as_ref()
        && k == &o.api_key
        && let Some(v) = m.get(slug)
    {
        return v.clone();
    }
    let found = if is_connect_key(&o.api_key) {
        with_session(o, |s| s.execute("COMPOSIO_GET_TOOL_SCHEMAS", &json!({ "tool_slugs": [slug] }))).ok().map(|r| first_with_properties(&r.data))
    } else {
        read(project(&o.api_key)(client().get(api_url(&format!("/tools/{slug}")))).send()).ok().map(|t| first_with_properties(&t["input_parameters"]))
    };
    let v = found.unwrap_or(Value::Null);
    let mut cache = SCHEMAS.lock().unwrap();
    if cache.as_ref().is_none_or(|(k, _)| k != &o.api_key) {
        *cache = Some((o.api_key.clone(), HashMap::new()));
    }
    if !v.is_null() {
        cache.as_mut().unwrap().1.insert(slug.to_string(), v.clone());
    }
    v
}

// The first object with "properties", looked for a few levels down (schema responses differ per key kind).
fn first_with_properties(v: &Value) -> Value {
    fn walk(v: &Value, depth: u8) -> Option<&Value> {
        if v.get("properties").is_some_and(Value::is_object) {
            return Some(v);
        }
        if depth == 0 {
            return None;
        }
        match v {
            Value::Object(o) => o.values().find_map(|c| walk(c, depth - 1)),
            Value::Array(a) => a.iter().find_map(|c| walk(c, depth - 1)),
            _ => None,
        }
    }
    walk(v, 4).cloned().unwrap_or(Value::Null)
}

static UPLOAD_FIELD: Mutex<Option<(String, String)>> = Mutex::new(None);

/// Uploads one local file to the person's Google Drive: staged in Composio's storage, then the Drive
/// tool saves it. `on_stage("save")` fires once the bytes are with Composio. Returns (name, id, link).
pub fn upload(o: &Opts, path: &std::path::Path, on_stage: &dyn Fn(&str)) -> Result<(String, Option<String>, Option<String>), String> {
    if is_connect_key(&o.api_key) {
        return Err("NEEDS_PROJECT_KEY".into());
    }
    let auth = project(&o.api_key);
    let field = {
        let cached = UPLOAD_FIELD.lock().unwrap().clone();
        match cached {
            Some((k, f)) if k == o.api_key => f,
            _ => {
                let tool = read(auth(client().get(api_url(&format!("/tools/{UPLOAD_SLUG}")))).send())?;
                let f = upload_field(&tool["input_parameters"]).unwrap_or_else(|| "file_to_upload".into());
                *UPLOAD_FIELD.lock().unwrap() = Some((o.api_key.clone(), f.clone()));
                f
            }
        }
    };
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let mime = mime_of(&name);
    let md5 = Md5::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect::<String>();
    let req = read(auth(client().post(api_url("/files/upload/request"))).json(&json!({ "filename": name, "mimetype": mime, "md5": md5, "tool_slug": UPLOAD_SLUG, "toolkit_slug": "googledrive" })).send())?;
    let s3key = req["key"].as_str().ok_or("Composio gave no upload slot")?.to_string();
    let put = Client::builder().timeout(Duration::from_secs(600)).build().unwrap().put(req["new_presigned_url"].as_str().ok_or("Composio gave no upload URL")?).header("Content-Type", mime).body(bytes).send().map_err(|e| e.to_string())?;
    if !put.status().is_success() {
        return Err(format!("Failed to upload file to storage: {}", put.status()));
    }
    on_stage("save");
    let args = json!({ field: { "name": name, "mimetype": mime, "s3key": s3key } });
    let mut r = with_session(o, |s| s.execute(UPLOAD_SLUG, &args)).unwrap_or(Outcome { ok: false, error: "session".into(), data: Value::Null });
    if !r.ok {
        // The session path may not take a staged file; the direct tool call does.
        let d = read(auth(client().post(api_url(&format!("/tools/execute/{UPLOAD_SLUG}")))).json(&json!({ "user_id": o.user_id, "arguments": args, "version": "latest" })).send())?;
        r = Outcome { ok: d["successful"] != false && d["error"].is_null(), error: d["error"].as_str().unwrap_or("").into(), data: d["data"].clone() };
    }
    if !r.ok {
        return Err(if r.error.is_empty() { r.data.to_string().chars().take(300).collect() } else { r.error });
    }
    Ok(drive_file(&r.data, &name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_linked_account_counts_with_or_without_a_status() {
        assert_eq!(active_account(&json!({ "connected_account": { "id": "ca_1", "status": "ACTIVE" } })), Some("ca_1".into()));
        assert_eq!(active_account(&json!({ "connected_account": { "id": "ca_2" } })), Some("ca_2".into()));
        assert_eq!(active_account(&json!({ "connection": { "connected_account": { "id": "ca_3", "status": "active" } } })), Some("ca_3".into()));
        assert_eq!(active_account(&json!({ "connected_account": { "id": "ca_4", "status": "INITIATED" } })), None);
        assert_eq!(active_account(&json!({ "connected_account": null })), None);
    }
}
