// aina's voice: the microphone (mic.rs), speech to text (stt.rs), text to speech with hedged requests
// and a small in-memory cache (tts.rs), and playback (player.rs). Everything blocks; callers run it on
// worker threads. Nothing is written to disk: recordings and audio live in memory only.

pub mod chime;
pub mod mic;
pub mod player;
pub mod stt;
pub mod tts;
pub mod vad;
pub mod wav;

/// Audio ready to play: 16-bit mono PCM at `rate`, or a whole WAV file (Windows voices).
#[derive(Clone, Debug, PartialEq)]
pub enum Clip {
    Pcm { samples: Vec<i16>, rate: u32 },
    Wav(Vec<u8>),
}
