// The microphone, on its own thread (audio streams can't move between threads). Opening the mic takes
// 100 ms to over a second (Bluetooth), which used to eat the start of short questions, so it stays open
// for a while after a recording and follow-ups start instantly; then it is released so Windows'
// "microphone in use" indicator goes away. Same as v1's worker (MIC_WARM_MS).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rodio::cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rodio::cpal::{self, SampleFormat};

use crate::wav::{resample, to_i16};

pub const RATE: u32 = 16_000;
const WARM: Duration = Duration::from_secs(20);

/// A finished recording: 16 kHz mono.
pub struct Recording {
    pub samples: Vec<i16>,
    pub duration: Duration,
}

enum Cmd {
    Start,
    Stop(Sender<Result<Recording, String>>),
}

struct Shared {
    recording: AtomicBool,
    buf: Mutex<Vec<f32>>,
    level: Mutex<Box<dyn Fn(f32) + Send>>,
}

pub struct Mic {
    tx: Mutex<Option<Sender<Cmd>>>,
    shared: Arc<Shared>,
}

impl Mic {
    /// `on_level` gets the input level (0…1) about 30 times a second while recording.
    pub fn new(on_level: impl Fn(f32) + Send + 'static) -> Mic {
        Mic { tx: Mutex::new(None), shared: Arc::new(Shared { recording: AtomicBool::new(false), buf: Mutex::new(Vec::new()), level: Mutex::new(Box::new(on_level)) }) }
    }

    fn send(&self, cmd: Cmd) {
        let mut tx = self.tx.lock().unwrap();
        if let Some(t) = tx.as_ref() {
            match t.send(cmd) {
                Ok(()) => return,
                Err(back) => return self.restart(&mut tx, back.0),
            }
        }
        self.restart(&mut tx, cmd);
    }

    fn restart(&self, tx: &mut Option<Sender<Cmd>>, cmd: Cmd) {
        let (t, rx) = channel();
        let shared = self.shared.clone();
        std::thread::spawn(move || run(rx, shared));
        let _ = t.send(cmd);
        *tx = Some(t);
    }

    pub fn start(&self) {
        self.send(Cmd::Start);
    }

    pub fn stop(&self) -> Result<Recording, String> {
        let (tx, rx) = channel();
        self.send(Cmd::Stop(tx));
        rx.recv_timeout(Duration::from_secs(5)).map_err(|_| "The microphone did not answer.".to_string())?
    }
}

// Owns the stream: opened on Start, kept warm, dropped after WARM without a recording.
fn run(rx: Receiver<Cmd>, shared: Arc<Shared>) {
    let mut stream: Option<(cpal::Stream, u32)> = None;
    let mut started = Instant::now();
    loop {
        match rx.recv_timeout(WARM) {
            Ok(Cmd::Start) => {
                if stream.is_none() {
                    match open(shared.clone()) {
                        Ok(s) => stream = Some(s),
                        Err(e) => {
                            (shared.level.lock().unwrap())(-1.0);
                            eprintln!("[mic] {e}");
                            continue;
                        }
                    }
                }
                shared.buf.lock().unwrap().clear();
                started = Instant::now();
                shared.recording.store(true, Ordering::SeqCst);
            }
            Ok(Cmd::Stop(reply)) => {
                shared.recording.store(false, Ordering::SeqCst);
                let raw = std::mem::take(&mut *shared.buf.lock().unwrap());
                let r = match &stream {
                    Some((_, rate)) => Ok(Recording { samples: to_i16(&resample(&raw, *rate, RATE)), duration: started.elapsed() }),
                    None => Err("No microphone is available. Check Windows' sound settings.".to_string()),
                };
                let _ = reply.send(r);
            }
            Err(RecvTimeoutError::Timeout) if !shared.recording.load(Ordering::SeqCst) => stream = None,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn open(shared: Arc<Shared>) -> Result<(cpal::Stream, u32), String> {
    let device = cpal::default_host().default_input_device().ok_or("No microphone is available. Check Windows' sound settings.")?;
    let config = device.default_input_config().map_err(|e| e.to_string())?;
    let (rate, channels) = (config.sample_rate().0, config.channels() as usize);
    let mut level_acc = (0f32, 0usize);
    let every = (rate / 30) as usize;
    let mut on_data = move |mono: &mut dyn Iterator<Item = f32>| {
        if !shared.recording.load(Ordering::Relaxed) {
            return;
        }
        let mut buf = shared.buf.lock().unwrap();
        for s in mono {
            buf.push(s);
            level_acc.0 += s * s;
            level_acc.1 += 1;
            if level_acc.1 >= every {
                let rms = (level_acc.0 / level_acc.1 as f32).sqrt();
                (shared.level.lock().unwrap())((rms * 4.0).min(1.0));
                level_acc = (0.0, 0);
            }
        }
    };
    let err = |e| eprintln!("[mic] {e}");
    let stream = match config.sample_format() {
        SampleFormat::F32 => device.build_input_stream(&config.into(), move |d: &[f32], _: &_| on_data(&mut d.chunks(channels).map(|f| f.iter().sum::<f32>() / channels as f32)), err, None),
        SampleFormat::I16 => device.build_input_stream(
            &config.into(),
            move |d: &[i16], _: &_| on_data(&mut d.chunks(channels).map(|f| f.iter().map(|s| *s as f32 / 32768.0).sum::<f32>() / channels as f32)),
            err,
            None,
        ),
        other => return Err(format!("Unsupported microphone format {other:?}.")),
    }
    .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok((stream, rate))
}
