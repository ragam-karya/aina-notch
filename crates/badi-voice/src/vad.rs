// When a hands-free recording ("Hey Badi") ends by itself: after the user speaks and then pauses, when
// nothing is said at all, or at a hard limit. The noise floor adapts, so a fan or AC isn't speech.
// Port of v1's worker.js autoStop, fed the same 0…1 input level.

const START_LEVEL: f32 = 0.12;
const STOP_LEVEL: f32 = 0.07;
const START_FRAMES: u32 = 3;
const PAUSE_MS: f64 = 1100.0;
const NO_SPEECH_MS: f64 = 6000.0;
const MAX_MS: f64 = 20_000.0;

#[derive(Debug, Default)]
pub struct AutoStop {
    started: f64,
    floor: f32,
    loud: u32,
    speaking: bool,
    quiet_since: Option<f64>,
    done: bool,
}

impl AutoStop {
    pub fn new(now_ms: f64) -> AutoStop {
        AutoStop { started: now_ms, ..AutoStop::default() }
    }

    /// One level reading. Returns Some(speech_heard) once, when the recording should stop.
    pub fn feed(&mut self, level: f32, now_ms: f64) -> Option<bool> {
        if self.done {
            return None;
        }
        let elapsed = now_ms - self.started;
        if !self.speaking {
            self.floor = self.floor * 0.95 + level * 0.05;
            self.loud = if level > START_LEVEL.max(self.floor * 2.5) { self.loud + 1 } else { 0 };
            if self.loud >= START_FRAMES {
                self.speaking = true;
                self.quiet_since = None;
            } else if elapsed > NO_SPEECH_MS {
                self.done = true;
                return Some(false);
            }
            return None;
        }
        if level < STOP_LEVEL.max(self.floor * 1.6) {
            self.quiet_since.get_or_insert(now_ms);
        } else {
            self.quiet_since = None;
        }
        if self.quiet_since.is_some_and(|q| now_ms - q > PAUSE_MS) || elapsed > MAX_MS {
            self.done = true;
            return Some(true);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(levels: impl Iterator<Item = f32>) -> Option<(bool, f64)> {
        let mut a = AutoStop::new(0.0);
        for (i, l) in levels.enumerate() {
            let t = i as f64 * 33.0;
            if let Some(s) = a.feed(l, t) {
                return Some((s, t));
            }
        }
        None
    }

    #[test]
    fn speech_then_a_pause_stops() {
        let levels = std::iter::repeat_n(0.02, 10).chain(std::iter::repeat_n(0.4, 40)).chain(std::iter::repeat(0.02));
        let (speech, at) = run(levels.take(200)).unwrap();
        assert!(speech);
        assert!(at > 50.0 * 33.0 && at < 50.0 * 33.0 + 1500.0, "{at}");
    }

    #[test]
    fn silence_gives_up_and_a_noisy_room_raises_the_bar() {
        assert_eq!(run(std::iter::repeat_n(0.02, 400)).map(|r| r.0), Some(false));
        // A steady hum at 0.1 (under the start level) for a long while lifts the floor to about 0.1, so
        // speech now has to be well above it: 0.2 is not enough, and the recording gives up.
        let hum = run(std::iter::repeat_n(0.1, 150).chain(std::iter::repeat_n(0.2, 100)));
        assert_eq!(hum.map(|r| r.0), Some(false));
    }

    #[test]
    fn a_long_monologue_hits_the_limit() {
        let (speech, at) = run(std::iter::repeat_n(0.5, 1000)).unwrap();
        assert!(speech && at > MAX_MS);
    }
}
