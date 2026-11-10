// Where aina's network calls go. With the user's own keys (BYOK) they go straight to Anthropic,
// OpenAI, ElevenLabs and Composio. Signed in to aina Cloud, the same requests go to the cloud, which
// holds the keys, checks the plan and bills the account (the app's "key" is then its account token).
// The cloud also learns which kind of work a call belongs to, and its task, so a task begun within the
// limits may finish: set per thread with `with_task`.

use std::cell::RefCell;
use std::sync::RwLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Service {
    Anthropic,
    OpenAi,
    ElevenLabs,
    Composio,
}

impl Service {
    fn direct(self) -> &'static str {
        match self {
            Service::Anthropic => "https://api.anthropic.com",
            Service::OpenAi => "https://api.openai.com",
            Service::ElevenLabs => "https://api.elevenlabs.io",
            Service::Composio => "https://backend.composio.dev",
        }
    }

    // Where the cloud serves it.
    fn cloud_prefix(self) -> &'static str {
        match self {
            Service::Anthropic | Service::OpenAi => "",
            Service::ElevenLabs => "/eleven",
            Service::Composio => "/composio",
        }
    }
}

static CLOUD: RwLock<Option<String>> = RwLock::new(None);

/// Signed in (Some(base URL)) or not (None).
pub fn set_cloud(base: Option<String>) {
    *CLOUD.write().unwrap() = base.map(|b| b.trim_end_matches('/').to_string());
}

pub fn cloud() -> Option<String> {
    CLOUD.read().unwrap().clone()
}

/// The URL for a provider path ("/v1/messages", "/api/v3.1/tools/X").
pub fn url(service: Service, path: &str) -> String {
    match cloud() {
        Some(base) => format!("{base}{}{path}", service.cloud_prefix()),
        None => format!("{}{path}", service.direct()),
    }
}

thread_local! {
    static TASK: RefCell<Option<(String, String)>> = const { RefCell::new(None) };
}

/// Runs `f` with this thread's calls marked as one task: kind "kerjakan" | "latar", and its id.
pub fn with_task<T>(kind: &str, id: &str, f: impl FnOnce() -> T) -> T {
    let prev = TASK.with(|t| t.replace(Some((kind.into(), id.into()))));
    let out = f();
    TASK.with(|t| *t.borrow_mut() = prev);
    out
}

/// Headers for a call to the cloud (none when going straight to a provider).
pub fn task_headers() -> Vec<(&'static str, String)> {
    if cloud().is_none() {
        return vec![];
    }
    TASK.with(|t| match &*t.borrow() {
        Some((kind, id)) => vec![("x-aina-kind", kind.clone()), ("x-aina-task", id.clone())],
        None => vec![("x-aina-kind", "tanya".into())],
    })
}

/// A short random-enough task id.
pub fn task_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    format!("t{:x}", std::collections::hash_map::RandomState::new().build_hasher().finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_and_task_headers() {
        set_cloud(None);
        assert_eq!(url(Service::Anthropic, "/v1/messages"), "https://api.anthropic.com/v1/messages");
        assert!(task_headers().is_empty());
        set_cloud(Some("https://be.aina.ai.id/".into()));
        assert_eq!(url(Service::Anthropic, "/v1/messages"), "https://be.aina.ai.id/v1/messages");
        assert_eq!(url(Service::ElevenLabs, "/v1/text-to-speech/x/stream"), "https://be.aina.ai.id/eleven/v1/text-to-speech/x/stream");
        assert_eq!(url(Service::Composio, "/api/v3.1/tools/X"), "https://be.aina.ai.id/composio/api/v3.1/tools/X");
        assert_eq!(task_headers(), vec![("x-aina-kind", "tanya".to_string())]);
        let inside = with_task("kerjakan", "t1", task_headers);
        assert_eq!(inside[1], ("x-aina-task", "t1".to_string()));
        assert_eq!(task_headers()[0].1, "tanya", "outside the task again");
        set_cloud(None);
    }
}
