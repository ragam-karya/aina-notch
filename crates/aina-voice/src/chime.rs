// Short synthesized chimes (no bundled audio), the same notes as v1's worker.js: a hold starts, a
// question is sent, a guided step advances, a background task starts or finishes. They play on their
// own output stream, so they never cut aina's voice.

use std::time::Duration;

use rodio::{OutputStream, Sink, buffer::SamplesBuffer};

const RATE: u32 = 44_100;

fn notes(name: &str) -> &'static [(f32, f32)] {
    match name {
        "start" => &[(660.0, 0.06), (880.0, 0.08)],
        "step" => &[(784.0, 0.05), (1046.0, 0.07)],
        "sent" => &[(988.0, 0.045), (740.0, 0.06)],
        "agent-start" => &[(523.0, 0.07), (659.0, 0.07), (784.0, 0.09)],
        "agent-done" => &[(784.0, 0.07), (988.0, 0.07), (1318.0, 0.12)],
        // The ends of a timer and of the pomodoro's phases, each its own (a 0 Hz note is a rest), and
        // longer than the cues above so they are heard across the room:
        // a plain timer rings like an alarm: three quick beeps and a high one;
        "timer-done" => &[(880.0, 0.13), (0.0, 0.09), (880.0, 0.13), (0.0, 0.09), (880.0, 0.13), (0.0, 0.09), (1175.0, 0.34)],
        // a focus session ends falling and soft: rest now;
        "focus-done" => &[(1046.0, 0.2), (880.0, 0.2), (784.0, 0.2), (659.0, 0.45)],
        // a break ends rising, twice: back to work;
        "break-done" => &[(523.0, 0.09), (659.0, 0.09), (784.0, 0.12), (0.0, 0.1), (659.0, 0.09), (784.0, 0.09), (1046.0, 0.32)],
        // the whole cycle is done: a small fanfare.
        "cycle-done" => &[(784.0, 0.12), (988.0, 0.12), (1175.0, 0.12), (1568.0, 0.42)],
        _ => &[],
    }
}

/// The chime's samples: sine notes with a quick rise and an exponential fall, each starting at 80 % of
/// the one before (v1: gain 0.0001 → 0.12 in 10 ms, back to 0.0001 by the note's end).
pub fn samples(name: &str) -> Vec<f32> {
    let notes = notes(name);
    // Each note starts at 80 % of the one before; a rest takes its whole length.
    let total: f32 = notes.iter().map(|n| if n.0 == 0.0 { n.1 } else { n.1 * 0.8 }).sum::<f32>() + notes.last().map(|n| n.1 * 0.2 + 0.02).unwrap_or(0.0);
    let mut out = vec![0.0f32; (total * RATE as f32) as usize + 1];
    let mut start = 0.0f32;
    for &(freq, dur) in notes {
        if freq == 0.0 {
            // A rest: the next note waits the whole of it.
            start += dur;
            continue;
        }
        let first = (start * RATE as f32) as usize;
        let len = (dur * RATE as f32) as usize;
        for i in 0..len {
            let t = i as f32 / RATE as f32;
            let gain = if t < 0.01 { 0.12 * (t / 0.01) } else { 0.12 * (0.0001f32 / 0.12).powf((t - 0.01) / (dur - 0.01)) };
            if let Some(s) = out.get_mut(first + i) {
                *s += gain * (std::f32::consts::TAU * freq * t).sin();
            }
        }
        start += dur * 0.8;
    }
    out
}

/// Plays a chime in the background; unknown names are ignored.
pub fn play(name: &str) {
    let samples = samples(name);
    if samples.len() < 2 {
        return;
    }
    std::thread::spawn(move || {
        let Ok((_stream, handle)) = OutputStream::try_default() else { return };
        let Ok(sink) = Sink::try_new(&handle) else { return };
        sink.append(SamplesBuffer::new(1, RATE, samples));
        sink.sleep_until_end();
        std::thread::sleep(Duration::from_millis(30));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chimes_are_short_and_quiet() {
        for name in ["start", "step", "sent", "agent-start", "agent-done"] {
            let s = samples(name);
            assert!(s.len() > 1000 && s.len() < RATE as usize / 2, "{name}: {}", s.len());
            assert!(s.iter().all(|x| x.abs() <= 0.25), "{name} too loud");
        }
        assert!(samples("nope").len() < 2);
    }

    #[test]
    fn the_ends_of_timers_are_longer_and_all_different() {
        let ends = ["timer-done", "focus-done", "break-done", "cycle-done"];
        for name in ends {
            let s = samples(name);
            let secs = s.len() as f32 / RATE as f32;
            assert!((0.5..2.0).contains(&secs), "{name}: {secs} s");
            assert!(s.iter().all(|x| x.abs() <= 0.25), "{name} too loud");
        }
        for (i, a) in ends.iter().enumerate() {
            for b in &ends[i + 1..] {
                assert_ne!(notes(a), notes(b), "{a} and {b} sound the same");
            }
        }
    }
}
