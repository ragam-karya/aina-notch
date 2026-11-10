// Plays aina's voice on its own thread (the output stream can't move between threads). One clip at a
// time; a new clip or stop() cuts the current one (Esc, barge-in). `play` blocks until the clip ends
// or is cut, and calls `on_start(duration)` the moment sound starts, so the step's visuals and
// subtitle go out with the voice (v1's tts:started). The output stream closes after WARM without a
// clip (an open one keeps the audio engine mixing silence) and opens again with the next clip.

use std::io::Cursor;
use std::sync::mpsc::{RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rodio::{Decoder, OutputStream, Sink, Source, buffer::SamplesBuffer};

use crate::Clip;

const WARM: Duration = Duration::from_secs(30);

enum Cmd {
    Play(Clip, Box<dyn FnOnce(Duration) + Send>, Sender<bool>),
    Stop,
}

#[derive(Default)]
pub struct Player {
    tx: Mutex<Option<Sender<Cmd>>>,
}

impl Player {
    fn send(&self, cmd: Cmd) {
        let mut tx = self.tx.lock().unwrap();
        let cmd = match tx.as_ref() {
            Some(t) => match t.send(cmd) {
                Ok(()) => return,
                Err(back) => back.0,
            },
            None if matches!(cmd, Cmd::Stop) => return,
            None => cmd,
        };
        let (t, rx) = channel();
        std::thread::spawn(move || {
            // Without a sound device a clip still "starts", so its text and visuals are shown.
            let failed = |on_start: Box<dyn FnOnce(Duration) + Send>, done: Sender<bool>| {
                on_start(Duration::ZERO);
                let _ = done.send(false);
            };
            let Ok((_stream, handle)) = OutputStream::try_default() else {
                for cmd in rx {
                    if let Cmd::Play(_, on_start, done) = cmd {
                        failed(on_start, done);
                    }
                }
                return;
            };
            let current: Arc<Mutex<Option<Arc<Sink>>>> = Arc::new(Mutex::new(None));
            loop {
                let cmd = match rx.recv_timeout(WARM) {
                    Ok(cmd) => cmd,
                    // Quiet for a while: close the stream (the next clip starts a new thread).
                    Err(RecvTimeoutError::Timeout) if current.lock().unwrap().as_ref().is_none_or(|s| s.empty()) => return,
                    Err(RecvTimeoutError::Timeout) => continue,
                    Err(RecvTimeoutError::Disconnected) => return,
                };
                if let Some(s) = current.lock().unwrap().take() {
                    s.stop();
                }
                let Cmd::Play(clip, on_start, done) = cmd else { continue };
                let Ok(sink) = Sink::try_new(&handle) else {
                    failed(on_start, done);
                    continue;
                };
                let sink = Arc::new(sink);
                let duration = match clip {
                    Clip::Pcm { samples, rate } => {
                        let d = Duration::from_secs_f64(samples.len() as f64 / rate as f64);
                        sink.append(SamplesBuffer::new(1, rate, samples));
                        d
                    }
                    Clip::Wav(bytes) => match Decoder::new(Cursor::new(bytes)) {
                        Ok(src) => {
                            let d = src.total_duration().unwrap_or_default();
                            sink.append(src);
                            d
                        }
                        Err(_) => {
                            failed(on_start, done);
                            continue;
                        }
                    },
                };
                on_start(duration);
                *current.lock().unwrap() = Some(sink.clone());
                // Wait for the end on another thread so a Stop can come in meanwhile.
                std::thread::spawn(move || {
                    sink.sleep_until_end();
                    let _ = done.send(true);
                });
            }
        });
        let _ = t.send(cmd);
        *tx = Some(t);
    }

    /// Plays a clip; returns true when it played to the end (false when cut or it failed).
    pub fn play(&self, clip: Clip, on_start: impl FnOnce(Duration) + Send + 'static) -> bool {
        let (done, rx) = channel();
        self.send(Cmd::Play(clip, Box::new(on_start), done));
        rx.recv().unwrap_or(false)
    }

    pub fn stop(&self) {
        self.send(Cmd::Stop);
    }
}
