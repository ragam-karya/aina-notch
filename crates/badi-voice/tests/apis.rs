// Real APIs and audio devices, ignored by default (they cost a little and make sound). Keys come from
// the environment or v1's .env:
//   cargo test -p badi-voice --test apis -- --ignored --nocapture
use std::time::{Duration, Instant};

use badi_voice::Clip;
use badi_voice::player::Player;
use badi_voice::stt::transcribe;
use badi_voice::tts::{Tts, Voice};
use badi_voice::wav::resample;

fn key(name: &str) -> Option<String> {
    std::env::var(name).ok().or_else(|| {
        let env = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../.env")).ok()?;
        env.lines().find_map(|l| l.strip_prefix(&format!("{name}=")).map(|v| v.trim().trim_matches('"').to_string()))
    })
}

#[test]
#[ignore]
fn elevenlabs_speaks_and_openai_hears_it_back() {
    let (Some(eleven), Some(openai)) = (key("ELEVENLABS_API_KEY"), key("OPENAI_API_KEY")) else { panic!("keys missing") };
    let voice = Voice::Eleven { key: eleven, voice: "hNbh1PL2BQ5XtGBJqEiu".into(), model: "eleven_flash_v2_5".into(), lang: "id".into() };
    let started = Instant::now();
    let Clip::Pcm { samples, rate } = Tts::default().synthesize("Halo, aku aina. Tombol simpannya ada di kanan atas.", &voice).unwrap() else { panic!("pcm") };
    println!("tts: {} ms, {:.1} s of audio", started.elapsed().as_millis(), samples.len() as f64 / rate as f64);
    let f: Vec<f32> = samples.iter().map(|s| *s as f32 / 32768.0).collect();
    let pcm16k: Vec<i16> = resample(&f, rate, 16_000).iter().map(|s| (s * 32767.0) as i16).collect();
    let started = Instant::now();
    let heard = transcribe(&openai, "gpt-4o-mini-transcribe", Some("id"), &pcm16k, 16_000).unwrap();
    println!("stt: {} ms: {heard}", started.elapsed().as_millis());
    let lower = heard.to_lowercase();
    assert!(lower.contains("badi") || lower.contains("simpan"), "{heard}");
}

#[test]
#[ignore]
fn a_windows_voice_plays_and_reports_its_start() {
    let clip = Tts::default().synthesize("Tes suara.", &Voice::System { name: String::new(), lang: "id".into() }).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let ended = Player::default().play(clip, move |d| tx.send(d).unwrap());
    let d = rx.recv_timeout(Duration::from_secs(1)).unwrap();
    println!("played {} ms", d.as_millis());
    assert!(ended && d > Duration::from_millis(200));
}
