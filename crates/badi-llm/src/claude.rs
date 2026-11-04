// Claude's Messages API over HTTPS with server-sent events (there is no official Rust SDK, so this is
// the raw HTTP the docs describe). Same request as v1's llm.stream(): cached system prompt, effort,
// thinking off where the model allows it, and the server-side fallback for refusals. Blocking; run it
// on a worker thread. `cancelled` is polled between events so Esc stops a reply at once.

use std::io::{BufRead, BufReader};
use std::time::Duration;

use reqwest::blocking::Client;
use serde_json::{Value, json};

const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";

/// Thinking makes answers slower to start. Where the model allows it, it is turned off for voice
/// answers: Opus 5 (at effort high or below), Sonnet 5.5 via between_tools. Opus 5.5 and Fable can't
/// turn it off, so they think adaptively. (v1 thinkingFor)
pub fn thinking_for(model: &str, want: bool, effort: &str) -> Value {
    if want || model.starts_with("claude-opus-5-5") || model.starts_with("claude-fable") || effort == "xhigh" || effort == "max" {
        return json!({ "type": "adaptive" });
    }
    if model == "claude-sonnet-5-5" {
        return json!({ "type": "between_tools" });
    }
    json!({ "type": "disabled" })
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

impl Usage {
    /// Estimated cost in US dollars (prices per million tokens; cache reads 0.1×, 0.05× on Opus 5.5;
    /// cache writes 1.25×). Same table as v1's usage.js, plus the current Haiku.
    pub fn usd(&self, model: &str) -> f64 {
        let (inp, out, read) = match model {
            m if m.starts_with("claude-opus-5-5") => (4.0, 20.0, 0.2),
            m if m.starts_with("claude-opus-5") => (5.0, 25.0, 0.5),
            m if m.starts_with("claude-sonnet-5") => (2.0, 10.0, 0.2),
            m if m.starts_with("claude-haiku-5-5") => (0.1, 0.5, 0.01),
            m if m.starts_with("claude-fable") => (10.0, 50.0, 0.25),
            _ => (5.0, 25.0, 0.5),
        };
        (self.input as f64 * inp + self.output as f64 * out + self.cache_read as f64 * read + self.cache_write as f64 * inp * 1.25) / 1e6
    }

    fn add(&mut self, u: &Value) {
        let n = |k: &str| u[k].as_u64();
        if let Some(v) = n("input_tokens") {
            self.input = v;
        }
        if let Some(v) = n("output_tokens") {
            self.output = v;
        }
        if let Some(v) = n("cache_read_input_tokens") {
            self.cache_read = v;
        }
        if let Some(v) = n("cache_creation_input_tokens") {
            self.cache_write = v;
        }
    }
}

pub struct Request<'a> {
    pub key: &'a str,
    pub model: &'a str,
    pub effort: &'a str,
    pub thinking: bool,
    pub system: &'a str,
    pub messages: Value,
    pub max_tokens: u32,
}

fn body(r: &Request, stream: bool) -> Value {
    json!({
        "model": r.model,
        "max_tokens": r.max_tokens,
        "thinking": thinking_for(r.model, r.thinking, r.effort),
        "output_config": { "effort": r.effort },
        "system": [{ "type": "text", "text": r.system, "cache_control": { "type": "ephemeral" } }],
        "messages": r.messages,
        "fallbacks": "default",
        "stream": stream,
    })
}

fn client(timeout: Duration) -> Result<Client, String> {
    Client::builder().timeout(timeout).build().map_err(|e| e.to_string())
}

fn post(r: &Request, stream: bool, timeout: Duration) -> Result<reqwest::blocking::Response, String> {
    if r.key.is_empty() {
        return Err("Anthropic API key belum diisi. Buka Settings dari ikon tray.".into());
    }
    let mut req = client(timeout)?
        .post(badi_net::url(badi_net::Service::Anthropic, "/v1/messages"))
        .header("x-api-key", r.key)
        .header("anthropic-version", "2023-06-01")
        .header("anthropic-beta", FALLBACK_BETA);
    for (k, v) in badi_net::task_headers() {
        req = req.header(k, v);
    }
    let res = req.json(&body(r, stream)).send().map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        let status = res.status();
        let text = res.text().unwrap_or_default();
        let err = serde_json::from_str::<Value>(&text).ok().map(|v| v["error"].clone()).unwrap_or(Value::Null);
        let msg = err["message"].as_str().map(String::from).unwrap_or(text);
        // Badi Cloud's own messages (plan, limits, sign-in) are written for the person: shown as is.
        if badi_net::cloud().is_some() && matches!(err["type"].as_str(), Some("usage_limit" | "plan_expired" | "plan_limit" | "authentication_error")) {
            return Err(msg);
        }
        return Err(format!("Claude {status}: {msg}"));
    }
    Ok(res)
}

/// Streams a reply: `on_text` gets each text delta. Returns the full text and the usage.
pub fn stream(r: &Request, cancelled: &dyn Fn() -> bool, on_text: &mut dyn FnMut(&str)) -> Result<(String, Usage), String> {
    let res = post(r, true, Duration::from_secs(120))?;
    let mut full = String::new();
    let mut usage = Usage::default();
    let mut stop = String::new();
    for line in BufReader::new(res).lines() {
        if cancelled() {
            return Err("cancelled".into());
        }
        let line = line.map_err(|e| e.to_string())?;
        let Some(data) = line.strip_prefix("data: ") else { continue };
        let Ok(ev) = serde_json::from_str::<Value>(data) else { continue };
        match ev["type"].as_str() {
            Some("content_block_delta") if ev["delta"]["type"] == "text_delta" => {
                let t = ev["delta"]["text"].as_str().unwrap_or("");
                full.push_str(t);
                on_text(t);
            }
            Some("message_start") => usage.add(&ev["message"]["usage"]),
            Some("message_delta") => {
                usage.add(&ev["usage"]);
                if let Some(s) = ev["delta"]["stop_reason"].as_str() {
                    stop = s.to_string();
                }
            }
            Some("error") => return Err(format!("Claude: {}", ev["error"]["message"].as_str().unwrap_or("stream error"))),
            _ => {}
        }
    }
    if stop == "refusal" {
        return Err("Permintaan ini ditolak oleh model.".into());
    }
    Ok((full, usage))
}

/// Readies the prompt cache and the connection the moment the user starts talking (max_tokens 0
/// generates nothing; only a cache write). Same model, thinking and system as the real call.
pub fn prewarm(r: &Request) -> Result<(), String> {
    let warm = Request { key: r.key, model: r.model, effort: r.effort, thinking: r.thinking, system: r.system, messages: json!([{ "role": "user", "content": "." }]), max_tokens: 0 };
    post(&warm, false, Duration::from_secs(30)).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thinking_is_off_where_the_model_allows_it() {
        assert_eq!(thinking_for("claude-opus-5", false, "low")["type"], "disabled");
        assert_eq!(thinking_for("claude-opus-5-5", false, "low")["type"], "adaptive");
        assert_eq!(thinking_for("claude-sonnet-5-5", false, "low")["type"], "between_tools");
        assert_eq!(thinking_for("claude-opus-5", false, "max")["type"], "adaptive");
    }

    #[test]
    fn costs_follow_the_price_table() {
        let u = Usage { input: 1_400, output: 200, cache_read: 4_000, cache_write: 0 };
        // Opus 5: 1400×5 + 200×25 + 4000×0.5 = 14000 per million.
        assert!((u.usd("claude-opus-5") - 0.014).abs() < 1e-9);
    }
}
