// Text to speech. Port of v1's speech.js synthesize(): ElevenLabs (default: about 0.15 s per sentence
// once warm) or OpenAI (usually 1.5–2 s but stalls at random for 35–55 s), plus the Windows voices
// (free, offline). A stalled cloud request is overtaken rather than waited for: every HEDGE without an
// answer another identical request starts alongside it (up to ATTEMPTS); the first to finish wins.
// Short lines are kept in memory so they play instantly next time. Only aina's fixed lines (greeting,
// wake replies, filler, out of credits) are kept on disk (`disk`), and those are spoken by aina Cloud
// for free (`cloud_line`).

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::time::{Duration, Instant};

use reqwest::blocking::Client;
use serde_json::json;

use crate::Clip;
use crate::wav::pcm16;

pub const HEDGE: Duration = Duration::from_millis(1200);
pub const ATTEMPTS: usize = 3;
pub const GIVE_UP: Duration = Duration::from_secs(12);
const CACHE_MAX_CHARS: usize = 160;
// Room for the filler and every wake reply (fetched at start) plus recent short lines.
const CACHE_MAX_CLIPS: usize = 40;
const PCM_RATE: u32 = 24_000;

#[derive(Clone, Debug)]
pub enum Voice {
    Eleven { key: String, voice: String, model: String, lang: String },
    OpenAi { key: String, voice: String, model: String },
    /// A Windows voice by name ("" = the default for the language).
    System { name: String, lang: String },
}

impl Voice {
    pub fn cache_key(&self, text: &str) -> String {
        match self {
            Voice::Eleven { voice, lang, .. } => format!("eleven|{voice}|{lang}|{text}"),
            Voice::OpenAi { voice, .. } => format!("openai|{voice}|{text}"),
            Voice::System { name, lang } => format!("system|{name}|{lang}|{text}"),
        }
    }
}

// A refused key or request: asking again gives the same answer, so it is not retried.
const REFUSED: &str = "[refused] ";

/// True when the voice service refused the request itself (a wrong key, a bad voice id), as opposed to
/// being slow or briefly down.
pub fn is_refused(e: &str) -> bool {
    e.starts_with(REFUSED)
}

// The error for an unsuccessful answer: 4xx except 408/429 is a refusal.
fn failure(service: &str, status: reqwest::StatusCode, body: String) -> String {
    let refused = status.is_client_error() && status.as_u16() != 408 && status.as_u16() != 429;
    format!("{}{service} {status}: {}", if refused { REFUSED } else { "" }, body.chars().take(160).collect::<String>())
}

/// Up to `attempts` identical overlapping requests, staggered by `hedge`; returns the first success.
/// A failure starts the next attempt at once, except a refusal, which is returned at once. Late answers
/// are dropped.
pub fn hedged<T: Send + 'static>(request: impl Fn(usize) -> Result<T, String> + Send + Sync + 'static, hedge: Duration, attempts: usize, give_up: Duration) -> Result<T, String> {
    let request = std::sync::Arc::new(request);
    let (tx, rx) = channel::<(usize, Result<T, String>)>();
    let launch = |i: usize| {
        let (tx, req) = (tx.clone(), request.clone());
        std::thread::spawn(move || {
            let _ = tx.send((i, req(i)));
        });
    };
    let start = Instant::now();
    launch(0);
    let (mut launched, mut failed, mut last_err) = (1, 0, String::from("no answer"));
    loop {
        let wait = if launched < attempts { hedge } else { give_up.saturating_sub(start.elapsed()) };
        match rx.recv_timeout(wait) {
            Ok((_, Ok(v))) => return Ok(v),
            Ok((_, Err(e))) if is_refused(&e) => return Err(e),
            Ok((_, Err(e))) => {
                failed += 1;
                last_err = e;
                if failed >= attempts {
                    return Err(last_err);
                }
                if launched < attempts && launched == failed {
                    launch(launched);
                    launched += 1;
                }
            }
            Err(RecvTimeoutError::Timeout) if launched < attempts => {
                launch(launched);
                launched += 1;
            }
            Err(_) => return Err(format!("The voice took too long ({last_err}).")),
        }
        if start.elapsed() > give_up {
            return Err(format!("The voice took too long ({last_err})."));
        }
    }
}

fn eleven(text: &str, key: &str, voice: &str, model: &str, lang: &str) -> Result<Clip, String> {
    let res = Client::builder()
        .timeout(GIVE_UP)
        .build()
        .map_err(|e| e.to_string())?
        .post(aina_net::url(aina_net::Service::ElevenLabs, &format!("/v1/text-to-speech/{voice}/stream?output_format=pcm_{PCM_RATE}")))
        .header("xi-api-key", key)
        // The language of the reply keeps names and numbers pronounced the right way.
        .json(&json!({ "text": text, "model_id": model, "language_code": lang }))
        .send()
        .map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        let status = res.status();
        return Err(failure("ElevenLabs", status, res.text().unwrap_or_default()));
    }
    Ok(Clip::Pcm { samples: pcm16(&res.bytes().map_err(|e| e.to_string())?), rate: PCM_RATE })
}

/// One of aina's fixed lines (aina-llm persona::Line) spoken by aina Cloud for free (POST /v1/voice/line):
/// the app names the line, the cloud composes its text, and sends it back with the audio. Only for a
/// cloud voice while signed in (None otherwise).
pub fn cloud_line(voice: &Voice, id: &str, en: bool, name: &str, wake: bool) -> Option<Result<(Clip, String), String>> {
    let base = aina_net::cloud()?;
    let (token, engine, voice_id, model) = match voice {
        Voice::Eleven { key, voice, model, .. } => (key, "eleven", voice, model),
        Voice::OpenAi { key, voice, model } => (key, "openai", voice, model),
        Voice::System { .. } => return None,
    };
    let body = json!({ "line": id, "lang": if en { "en" } else { "id" }, "name": name, "wake": wake, "engine": engine, "voice": voice_id, "model": model });
    let call = || -> Result<(Clip, String), String> {
        let mut req = Client::builder().timeout(GIVE_UP).build().map_err(|e| e.to_string())?.post(format!("{base}/v1/voice/line")).bearer_auth(token).json(&body);
        for (k, v) in aina_net::task_headers() {
            req = req.header(k, v);
        }
        let res = req.send().map_err(|e| e.to_string())?;
        if !res.status().is_success() {
            let status = res.status();
            return Err(failure("aina Cloud", status, res.text().unwrap_or_default()));
        }
        let text = res.headers().get("x-aina-text").and_then(|v| v.to_str().ok()).map(percent_decode).unwrap_or_default();
        Ok((Clip::Pcm { samples: pcm16(&res.bytes().map_err(|e| e.to_string())?), rate: PCM_RATE }, text))
    };
    Some(call())
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        match (b[i], b.get(i + 1).copied().and_then(hex), b.get(i + 2).copied().and_then(hex)) {
            (b'%', Some(h), Some(l)) => {
                out.push((h * 16 + l) as u8);
                i += 3;
            }
            (c, _, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// aina's fixed lines kept on disk (voice-cache\ in aina's data folder), so each is fetched once per
/// voice: a file per line, named by a hash of its voice and text, holding the text and its PCM audio.
/// Nothing else is written there (answers stay in memory only).
pub mod disk {
    use std::path::{Path, PathBuf};

    use super::{Clip, Voice};

    const MAGIC: &[u8; 4] = b"AVL1";

    fn file(dir: &Path, voice: &Voice, text: &str) -> PathBuf {
        // FNV-1a: stable across runs and builds (std's hasher is not).
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in voice.cache_key(text).bytes() {
            h = (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3);
        }
        dir.join(format!("{h:016x}.pcm"))
    }

    /// The line's clip and text (as spoken), when kept.
    pub fn load(dir: &Path, voice: &Voice, text: &str) -> Option<(Clip, String)> {
        let b = std::fs::read(file(dir, voice, text)).ok()?;
        let num = |at: usize| b.get(at..at + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]));
        if b.get(..4)? != MAGIC {
            return None;
        }
        let (rate, len) = (num(4)?, num(8)? as usize);
        let spoken = String::from_utf8(b.get(12..12 + len)?.to_vec()).ok()?;
        let pcm = b.get(12 + len..)?;
        if pcm.is_empty() || pcm.len() % 2 != 0 {
            return None;
        }
        let samples = pcm.as_chunks::<2>().0.iter().map(|c| i16::from_le_bytes(*c)).collect();
        Some((Clip::Pcm { samples, rate }, spoken))
    }

    /// Keeps a line's clip (PCM only) under the text it was asked for, with the text as spoken.
    pub fn save(dir: &Path, voice: &Voice, text: &str, clip: &Clip, spoken: &str) -> std::io::Result<()> {
        let Clip::Pcm { samples, rate } = clip else { return Ok(()) };
        std::fs::create_dir_all(dir)?;
        let mut b = Vec::with_capacity(12 + spoken.len() + samples.len() * 2);
        b.extend_from_slice(MAGIC);
        b.extend_from_slice(&rate.to_le_bytes());
        b.extend_from_slice(&(spoken.len() as u32).to_le_bytes());
        b.extend_from_slice(spoken.as_bytes());
        for s in samples {
            b.extend_from_slice(&s.to_le_bytes());
        }
        // Written whole, then moved in place, so a half-written file is never read.
        let path = file(dir, voice, text);
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, b)?;
        std::fs::rename(tmp, path)
    }
}

fn openai(text: &str, key: &str, voice: &str, model: &str) -> Result<Clip, String> {
    let res = Client::builder()
        .timeout(GIVE_UP)
        .build()
        .map_err(|e| e.to_string())?
        .post(aina_net::url(aina_net::Service::OpenAi, "/v1/audio/speech"))
        .bearer_auth(key)
        .json(&json!({ "model": model, "voice": voice, "input": text, "response_format": "pcm" }))
        .send()
        .map_err(|e| e.to_string())?;
    if !res.status().is_success() {
        let status = res.status();
        return Err(failure("OpenAI", status, res.text().unwrap_or_default()));
    }
    Ok(Clip::Pcm { samples: pcm16(&res.bytes().map_err(|e| e.to_string())?), rate: PCM_RATE })
}

// A Windows voice through WinRT (offline). Picks `name` if installed, else one for the language.
#[cfg(windows)]
fn system(text: &str, name: &str, lang: &str) -> Result<Clip, String> {
    use windows::Media::SpeechSynthesis::SpeechSynthesizer;
    use windows::Storage::Streams::DataReader;
    use windows::core::HSTRING;
    let e = |e: windows::core::Error| e.message();
    let synth = SpeechSynthesizer::new().map_err(e)?;
    let voices = SpeechSynthesizer::AllVoices().map_err(e)?;
    let want = |v: &windows::Media::SpeechSynthesis::VoiceInformation| {
        let n = v.DisplayName().map(|s| s.to_string()).unwrap_or_default();
        let l = v.Language().map(|s| s.to_string().to_lowercase()).unwrap_or_default();
        (!name.is_empty() && n == name, l.starts_with(lang))
    };
    let chosen = voices.into_iter().find(|v| want(v).0).or_else(|| SpeechSynthesizer::AllVoices().ok()?.into_iter().find(|v| want(v).1));
    if let Some(v) = chosen {
        synth.SetVoice(&v).map_err(e)?;
    }
    let stream = synth.SynthesizeTextToStreamAsync(&HSTRING::from(text)).map_err(e)?.join().map_err(e)?;
    let size = stream.Size().map_err(e)? as u32;
    let reader = DataReader::CreateDataReader(&stream).map_err(e)?;
    reader.LoadAsync(size).map_err(e)?.join().map_err(e)?;
    let mut bytes = vec![0u8; size as usize];
    reader.ReadBytes(&mut bytes).map_err(e)?;
    Ok(Clip::Wav(bytes))
}

#[cfg(not(windows))]
fn system(_: &str, _: &str, _: &str) -> Result<Clip, String> {
    Err("Windows voices are not available here.".into())
}

pub struct Tts {
    cache: Mutex<VecDeque<(String, Clip)>>,
}

impl Default for Tts {
    fn default() -> Self {
        Tts { cache: Mutex::new(VecDeque::new()) }
    }
}

impl Tts {
    /// The line's audio if it is already in memory (never fetches): the filler plays only when ready.
    pub fn cached(&self, text: &str, voice: &Voice) -> Option<Clip> {
        let k = voice.cache_key(text);
        self.cache.lock().unwrap().iter().find(|(ck, _)| *ck == k).map(|(_, c)| c.clone())
    }

    pub fn synthesize(&self, text: &str, voice: &Voice) -> Result<Clip, String> {
        let key = (text.chars().count() <= CACHE_MAX_CHARS).then(|| voice.cache_key(text));
        if let Some(k) = &key {
            let mut c = self.cache.lock().unwrap();
            if let Some(i) = c.iter().position(|(ck, _)| ck == k) {
                let hit = c.remove(i).unwrap();
                let clip = hit.1.clone();
                c.push_back(hit); // most recently used
                return Ok(clip);
            }
        }
        let clip = match voice.clone() {
            Voice::Eleven { key, voice, model, lang } => {
                let t = text.to_string();
                hedged(move |_| eleven(&t, &key, &voice, &model, &lang), HEDGE, ATTEMPTS, GIVE_UP)?
            }
            Voice::OpenAi { key, voice, model } => {
                let t = text.to_string();
                hedged(move |_| openai(&t, &key, &voice, &model), HEDGE, ATTEMPTS, GIVE_UP)?
            }
            Voice::System { name, lang } => system(text, &name, &lang)?,
        };
        if let Some(k) = key {
            let mut c = self.cache.lock().unwrap();
            c.push_back((k, clip.clone()));
            if c.len() > CACHE_MAX_CLIPS {
                c.pop_front();
            }
        }
        Ok(clip)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    const MS: Duration = Duration::from_millis(1);

    #[test]
    fn a_stalled_request_is_overtaken_by_the_next_one() {
        let r = hedged(|i| {
            if i == 0 {
                std::thread::sleep(MS * 500);
            }
            Ok(i)
        }, MS * 30, 3, MS * 2000);
        assert_eq!(r, Ok(1));
    }

    #[test]
    fn a_failure_starts_the_next_attempt_at_once_and_all_failing_is_an_error() {
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let started = Instant::now();
        let r: Result<(), String> = hedged(move |i| {
            c.fetch_add(1, Ordering::SeqCst);
            Err(format!("fail {i}"))
        }, MS * 1000, 3, MS * 5000);
        assert!(r.unwrap_err().starts_with("fail"));
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert!(started.elapsed() < MS * 500); // no waiting for the hedge timer after failures
    }

    #[test]
    fn a_refusal_is_not_retried() {
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let r: Result<(), String> = hedged(move |_| {
            c.fetch_add(1, Ordering::SeqCst);
            Err(failure("ElevenLabs", reqwest::StatusCode::BAD_REQUEST, "invalid_api_key".into()))
        }, MS * 1000, 3, MS * 5000);
        assert!(is_refused(&r.unwrap_err()));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(!is_refused(&failure("OpenAI", reqwest::StatusCode::TOO_MANY_REQUESTS, String::new())));
    }

    #[test]
    fn short_lines_come_from_the_cache() {
        let tts = Tts::default();
        let v = Voice::System { name: "x".into(), lang: "id".into() };
        tts.cache.lock().unwrap().push_back((v.cache_key("halo"), Clip::Wav(vec![1, 2])));
        assert_eq!(tts.synthesize("halo", &v), Ok(Clip::Wav(vec![1, 2])));
    }

    #[test]
    fn fixed_lines_are_kept_on_disk_per_voice() {
        let dir = std::env::temp_dir().join(format!("aina-voice-cache-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let eleven = Voice::Eleven { key: "t".into(), voice: "v1".into(), model: "m".into(), lang: "id".into() };
        let other = Voice::Eleven { key: "t".into(), voice: "v2".into(), model: "m".into(), lang: "id".into() };
        let clip = Clip::Pcm { samples: vec![0, 1, -1, i16::MAX, i16::MIN], rate: 24_000 };
        assert_eq!(disk::load(&dir, &eleven, "Pagi, Ayu!"), None, "nothing kept yet");
        disk::save(&dir, &eleven, "Pagi, Ayu!", &clip, "Pagi, Ayu! (as spoken)").unwrap();
        assert_eq!(disk::load(&dir, &eleven, "Pagi, Ayu!"), Some((clip.clone(), "Pagi, Ayu! (as spoken)".into())));
        // Another voice or another text is another clip.
        assert_eq!(disk::load(&dir, &other, "Pagi, Ayu!"), None);
        assert_eq!(disk::load(&dir, &eleven, "Pagi, Budi!"), None);
        // A damaged file is ignored rather than played.
        for f in std::fs::read_dir(&dir).unwrap() {
            let p = f.unwrap().path();
            let mut b = std::fs::read(&p).unwrap();
            b.truncate(13);
            std::fs::write(&p, b).unwrap();
        }
        assert_eq!(disk::load(&dir, &eleven, "Pagi, Ayu!"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn line_text_comes_back_percent_encoded() {
        assert_eq!(percent_decode("Pagi%2C%20Ren%C3%A9e%21"), "Pagi, Renée!");
        assert_eq!(percent_decode("Ya%3F"), "Ya?");
        assert_eq!(percent_decode("100%"), "100%");
        // Signed out there is no cloud to ask.
        let v = Voice::OpenAi { key: "t".into(), voice: "alloy".into(), model: "gpt-4o-mini-tts".into() };
        if aina_net::cloud().is_none() {
            assert!(cloud_line(&v, "filler.0", false, "", false).is_none());
        }
        assert!(cloud_line(&Voice::System { name: String::new(), lang: "id".into() }, "filler.0", false, "", false).is_none());
    }
}
