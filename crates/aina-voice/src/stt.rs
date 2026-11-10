// Speech to text through OpenAI (same model setting and language hint as v1's speech.transcribe).

use std::time::Duration;

use reqwest::blocking::{Client, multipart};

use crate::wav::wav;

pub fn transcribe(key: &str, model: &str, lang: Option<&str>, samples: &[i16], rate: u32) -> Result<String, String> {
    if key.is_empty() {
        return Err("OpenAI API key belum diisi (dipakai untuk mengubah suara jadi teks).".into());
    }
    let file = multipart::Part::bytes(wav(samples, rate)).file_name("speech.wav").mime_str("audio/wav").map_err(|e| e.to_string())?;
    let mut form = multipart::Form::new().text("model", model.to_string()).part("file", file);
    // A language hint makes short Indonesian phrases transcribe much more reliably.
    if let Some(l) = lang {
        form = form.text("language", l.to_string());
    }
    let res = Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?
        .post(aina_net::url(aina_net::Service::OpenAi, "/v1/audio/transcriptions"))
        .bearer_auth(key)
        .multipart(form)
        .send()
        .map_err(|e| e.to_string())?;
    let status = res.status();
    let body: serde_json::Value = res.json().map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("OpenAI {status}: {}", body["error"]["message"].as_str().unwrap_or("")));
    }
    Ok(body["text"].as_str().unwrap_or("").trim().to_string())
}
