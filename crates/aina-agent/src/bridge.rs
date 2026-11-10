// Local bridge to the aina browser extension: a WebSocket server on 127.0.0.1 only. The extension
// proves itself with the pairing code shown in Settings, and the Origin must be an extension origin.
// Port of src/brain/bridge.js.
//
// Wire protocol (JSON text frames):
//   extension -> app: { type: 'hello', browser, version } | { type: 'result', id, content, isError } | { type: 'ping' }
//   app -> extension: { type: 'call', id, name, input } | { type: 'end' } | { type: 'pong' } | { type: 'welcome' }

use std::collections::HashMap;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use regex::Regex;
use serde_json::{Value, json};
use tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tungstenite::http::StatusCode;
use tungstenite::protocol::CloseFrame;
use tungstenite::protocol::frame::coding::CloseCode;
use tungstenite::Message;

const CALL_TIMEOUT: Duration = Duration::from_secs(45);
const MAX_PAYLOAD: usize = 32 * 1024 * 1024;
static EXTENSION_ORIGIN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(chrome|moz|ms-browser|edge)-extension://").unwrap());

/// Constant-time compare so the code can't be guessed byte by byte.
pub fn same_token(a: &str, b: &str) -> bool {
    let (x, y) = (a.as_bytes(), b.as_bytes());
    x.len() == y.len() && !x.is_empty() && x.iter().zip(y).fold(0u8, |acc, (p, q)| acc | (p ^ q)) == 0
}

enum Out {
    Text(String),
    Close(u16, &'static str),
}

type Reply = Result<(Vec<Value>, bool), String>;

#[derive(Default)]
struct Inner {
    client: Option<(u64, Sender<Out>)>,
    info: Option<(String, String)>,
    calls: HashMap<u64, Sender<Reply>>,
    failures: Vec<Instant>,
}

type OnStatus = Arc<dyn Fn(Value) + Send + Sync>;

#[derive(Clone)]
pub struct Bridge {
    inner: Arc<Mutex<Inner>>,
    seq: Arc<AtomicU64>,
    token: Arc<dyn Fn() -> String + Send + Sync>,
    listening: Arc<AtomicBool>,
    on_status: Arc<Mutex<Option<OnStatus>>>,
}

impl Bridge {
    /// `token` returns the current pairing code.
    pub fn new(token: impl Fn() -> String + Send + Sync + 'static) -> Bridge {
        Bridge { inner: Arc::default(), seq: Arc::default(), token: Arc::new(token), listening: Arc::default(), on_status: Arc::default() }
    }

    /// Called with status() whenever the extension connects, says hello or goes away.
    pub fn on_status(&self, f: impl Fn(Value) + Send + Sync + 'static) {
        *self.on_status.lock().unwrap() = Some(Arc::new(f));
    }

    fn changed(&self) {
        let f = self.on_status.lock().unwrap().clone();
        if let Some(f) = f {
            f(self.status());
        }
    }

    /// Listens on 127.0.0.1:port in the background.
    pub fn start(&self, port: u16) -> Result<(), String> {
        let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|e| format!("bridge: {e}"))?;
        self.listening.store(true, Ordering::SeqCst);
        let me = self.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let me = me.clone();
                std::thread::spawn(move || me.serve(stream));
            }
        });
        Ok(())
    }

    pub fn connected(&self) -> bool {
        self.inner.lock().unwrap().client.is_some()
    }

    pub fn status(&self) -> Value {
        let inner = self.inner.lock().unwrap();
        let listening = self.listening.load(Ordering::SeqCst);
        match (&inner.client, &inner.info) {
            (Some(_), Some((browser, version))) => json!({ "connected": true, "browser": browser, "version": version, "listening": listening }),
            (c, _) => json!({ "connected": c.is_some(), "listening": listening }),
        }
    }

    // Checks the Origin and the pairing code, with a limit on wrong guesses (a 6-digit code is only
    // safe with one).
    fn check(&self, req: &Request) -> Result<(), (u16, &'static str)> {
        let origin = req.headers().get("origin").and_then(|o| o.to_str().ok()).unwrap_or("");
        if !EXTENSION_ORIGIN.is_match(origin) {
            return Err((403, "Forbidden"));
        }
        let token = req.uri().query().unwrap_or("").split('&').find_map(|kv| kv.strip_prefix("token=")).unwrap_or("").to_string();
        let mut inner = self.inner.lock().unwrap();
        inner.failures.retain(|t| t.elapsed() < Duration::from_secs(60));
        if inner.failures.len() >= 10 {
            return Err((429, "Too many attempts"));
        }
        if !same_token(&token, &(self.token)()) {
            inner.failures.push(Instant::now());
            return Err((401, "Wrong pairing code"));
        }
        Ok(())
    }

    #[allow(clippy::result_large_err)] // the handshake callback type is tungstenite's
    fn serve(&self, stream: TcpStream) {
        let callback = |req: &Request, res: Response| -> Result<Response, ErrorResponse> {
            match self.check(req) {
                Ok(()) => Ok(res),
                Err((code, why)) => {
                    let mut e = ErrorResponse::new(Some(why.to_string()));
                    *e.status_mut() = StatusCode::from_u16(code).unwrap_or(StatusCode::FORBIDDEN);
                    Err(e)
                }
            }
        };
        let config = tungstenite::protocol::WebSocketConfig::default().max_message_size(Some(MAX_PAYLOAD)).max_frame_size(Some(MAX_PAYLOAD));
        let Ok(mut ws) = tungstenite::accept_hdr_with_config(stream, callback, Some(config)) else { return };
        let _ = ws.get_mut().set_read_timeout(Some(Duration::from_millis(50)));

        // The newest extension connection wins (e.g. after the browser restarts).
        let id = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
        let (tx, rx): (Sender<Out>, Receiver<Out>) = channel();
        if let Some((_, old)) = self.inner.lock().unwrap().client.replace((id, tx)) {
            let _ = old.send(Out::Close(4000, "replaced"));
        }
        self.inner.lock().unwrap().info = None;
        let _ = ws.send(Message::text(json!({ "type": "welcome" }).to_string()));

        loop {
            let mut closing = false;
            while let Ok(out) = rx.try_recv() {
                match out {
                    Out::Text(t) => {
                        let _ = ws.send(Message::text(t));
                    }
                    Out::Close(code, why) => {
                        let _ = ws.close(Some(CloseFrame { code: CloseCode::from(code), reason: why.into() }));
                        closing = true;
                    }
                }
            }
            match ws.read() {
                Ok(Message::Text(t)) => self.on_message(&mut ws, t.as_str()),
                Ok(Message::Close(_)) => break,
                Ok(_) => {}
                Err(tungstenite::Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                    if closing {
                        break;
                    }
                }
                Err(_) => break,
            }
        }

        let gone = {
            let mut inner = self.inner.lock().unwrap();
            let mine = inner.client.as_ref().is_some_and(|(c, _)| *c == id);
            if mine {
                inner.client = None;
                inner.info = None;
                for (_, call) in inner.calls.drain() {
                    let _ = call.send(Err("The browser extension disconnected.".into()));
                }
            }
            mine
        };
        if gone {
            self.changed();
        }
    }

    fn on_message(&self, ws: &mut tungstenite::WebSocket<TcpStream>, data: &str) {
        let Ok(msg) = serde_json::from_str::<Value>(data) else { return };
        match msg["type"].as_str() {
            Some("ping") => {
                let _ = ws.send(Message::text(json!({ "type": "pong" }).to_string()));
            }
            Some("hello") => {
                let clip = |v: &Value, n: usize, d: &str| v.as_str().unwrap_or(d).chars().take(n).collect::<String>();
                self.inner.lock().unwrap().info = Some((clip(&msg["browser"], 40, "browser"), clip(&msg["version"], 20, "")));
                self.changed();
            }
            Some("result") => {
                let Some(id) = msg["id"].as_u64() else { return };
                if let Some(call) = self.inner.lock().unwrap().calls.remove(&id) {
                    let content = match &msg["content"] {
                        Value::Array(a) => a.clone(),
                        Value::Null => vec![json!({ "type": "text", "text": "OK" })],
                        v => vec![json!({ "type": "text", "text": v.as_str().map(String::from).unwrap_or_else(|| v.to_string()) })],
                    };
                    let _ = call.send(Ok((content, msg["isError"].as_bool().unwrap_or(false))));
                }
            }
            _ => {}
        }
    }

    fn send(&self, msg: Value) -> bool {
        match &self.inner.lock().unwrap().client {
            Some((_, tx)) => tx.send(Out::Text(msg.to_string())).is_ok(),
            None => false,
        }
    }

    /// Runs one browser toolset member in the extension (the BrowserExecutor of v1).
    pub fn run(&self, name: &str, input: &Value) -> Result<Vec<Value>, String> {
        let id = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
        let (tx, rx) = channel();
        self.inner.lock().unwrap().calls.insert(id, tx);
        let input = if input.is_object() { input.clone() } else { json!({}) };
        if !self.send(json!({ "type": "call", "id": id, "name": name, "input": input })) {
            self.inner.lock().unwrap().calls.remove(&id);
            return Err("The aina browser extension is not connected.".into());
        }
        let got = rx.recv_timeout(CALL_TIMEOUT);
        self.inner.lock().unwrap().calls.remove(&id);
        match got {
            Ok(Ok((content, false))) => Ok(content),
            Ok(Ok((content, true))) => {
                let text = content.iter().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join(" ");
                let text = text.strip_prefix("Error:").map(str::trim_start).unwrap_or(&text).to_string();
                Err(if text.is_empty() { "Browser action failed.".into() } else { text })
            }
            Ok(Err(e)) => Err(e),
            Err(_) => Err(format!("The browser did not answer \"{name}\" in time.")),
        }
    }

    /// Tells the extension the task is over so it can detach the debugger.
    pub fn end_session(&self) {
        self.send(json!({ "type": "end" }));
    }

    /// Kicks the current extension, e.g. after a new pairing code.
    pub fn disconnect(&self) {
        if let Some((_, tx)) = &self.inner.lock().unwrap().client {
            let _ = tx.send(Out::Close(4001, "pairing changed"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_compare_exactly() {
        assert!(same_token("123456", "123456"));
        assert!(!same_token("123456", "123457"));
        assert!(!same_token("", ""));
        assert!(!same_token("12345", "123456"));
    }

    // A fake extension against a real server: wrong origin and wrong code are refused, then a paired
    // client answers a call.
    #[test]
    fn pairs_and_runs_a_call() {
        let bridge = Bridge::new(|| "424242".into());
        let port = 47_000 + (std::process::id() % 900) as u16;
        bridge.start(port).unwrap();
        let req = |origin: &str, token: &str| {
            let mut r = tungstenite::client::IntoClientRequest::into_client_request(format!("ws://127.0.0.1:{port}/?token={token}")).unwrap();
            r.headers_mut().insert("origin", origin.parse().unwrap());
            r
        };
        assert!(tungstenite::connect(req("https://evil.example", "424242")).is_err());
        assert!(tungstenite::connect(req("chrome-extension://abc", "000000")).is_err());
        let (mut ws, _) = tungstenite::connect(req("chrome-extension://abc", "424242")).unwrap();
        assert!(ws.read().unwrap().to_text().unwrap().contains("welcome"));
        ws.send(Message::text(r#"{"type":"hello","browser":"Chrome","version":"1.2"}"#)).unwrap();
        let b = bridge.clone();
        let call = std::thread::spawn(move || b.run("navigate", &json!({ "url": "https://example.com" })));
        let msg: Value = serde_json::from_str(ws.read().unwrap().to_text().unwrap()).unwrap();
        assert_eq!(msg["name"], "navigate");
        ws.send(Message::text(json!({ "type": "result", "id": msg["id"], "content": [{ "type": "text", "text": "Navigated" }] }).to_string())).unwrap();
        assert_eq!(call.join().unwrap().unwrap()[0]["text"], "Navigated");
        assert_eq!(bridge.status()["browser"], "Chrome");
        drop(ws);
    }
}
