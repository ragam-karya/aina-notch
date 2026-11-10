// Timers and reminders for the notch: one countdown (or a pomodoro) at a time, plus reminders at a
// clock time. The countdown lives in memory only; reminders, the pomodoro settings and today's count
// of finished focus sessions are kept by the caller (config.json) so they survive a restart. Pure
// apart from the clock passed in. Port of src/timers.js, extended in v2: a configurable pomodoro
// cycle that can be paused, resumed, skipped, reset and stopped, and reminders that stay until done
// (snoozed, repeated, completed) for the notch's Reminder Center.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const WORK_MS: u64 = 25 * 60_000;
pub const BREAK_MS: u64 = 5 * 60_000;
pub const LONG_BREAK_MS: u64 = 15 * 60_000;
pub const LONG_EVERY: u32 = 4;
const DAY_MS: i64 = 86_400_000;

/// How a pomodoro cycle runs. Saved in config.json under `pomodoro` (camelCase).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PomodoroConfig {
    pub focus_min: u32,
    pub short_min: u32,
    pub long_min: u32,
    /// Focus sessions in one cycle; the cycle ends after the last one's break.
    pub sessions: u32,
    /// A long break after every this many focus sessions (and after the cycle's last one).
    pub long_every: u32,
    /// The next phase starts by itself; otherwise (the default) it waits, full, for Start, and the
    /// notch reminds the person to start it.
    pub auto_start: bool,
    /// A chime, a spoken line and a Windows notification when a phase ends.
    pub notify: bool,
}

impl Default for PomodoroConfig {
    fn default() -> Self {
        PomodoroConfig { focus_min: 25, short_min: 5, long_min: 15, sessions: LONG_EVERY, long_every: LONG_EVERY, auto_start: false, notify: true }
    }
}

impl PomodoroConfig {
    /// Read from config.json, every number kept within sane bounds.
    pub fn from_value(v: &Value) -> PomodoroConfig {
        let c: PomodoroConfig = serde_json::from_value(v.clone()).unwrap_or_default();
        PomodoroConfig {
            focus_min: c.focus_min.clamp(1, 180),
            short_min: c.short_min.clamp(1, 60),
            long_min: c.long_min.clamp(1, 90),
            sessions: c.sessions.clamp(1, 12),
            long_every: c.long_every.clamp(1, 12),
            ..c
        }
    }

    pub fn phase_ms(&self, phase: &str) -> u64 {
        let min = match phase {
            "work" => self.focus_min,
            "long" => self.long_min,
            _ => self.short_min,
        };
        min as u64 * 60_000
    }

    /// The break after focus session `round`.
    fn break_after(&self, round: u32) -> &'static str {
        if round % self.long_every == 0 || round >= self.sessions { "long" } else { "short" }
    }

    /// A short name from the numbers ("25/5", "50/10 ×3"), the same in every language.
    pub fn name(&self) -> String {
        let base = format!("{}/{}", self.focus_min, self.short_min);
        if self.sessions == LONG_EVERY { base } else { format!("{base} ×{}", self.sessions) }
    }
}

/// The preset patterns offered next to the person's own (Settings saves those in config.json).
pub fn builtin_presets() -> Vec<(&'static str, PomodoroConfig)> {
    let d = PomodoroConfig::default();
    vec![
        ("classic", d.clone()),
        ("deep", PomodoroConfig { focus_min: 50, short_min: 10, long_min: 30, sessions: 3, long_every: 3, ..d.clone() }),
        ("sprint", PomodoroConfig { focus_min: 15, short_min: 3, long_min: 10, ..d }),
    ]
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pomodoro {
    pub phase: &'static str, // "work" | "short" | "long"
    pub round: u32,          // the focus session this phase belongs to, 1..=sessions
}

#[derive(Debug, Clone, PartialEq)]
pub struct Timer {
    pub label: String,
    pub ms: u64,
    pub ends_at: i64,
    /// Paused (or waiting for Start): the time it had left.
    pub paused_left: Option<u64>,
    pub pomodoro: Option<Pomodoro>,
}

/// A local reminder (or local calendar event, when it has an end or is all-day). Older configs saved
/// only id, at and text; every other field has a default, so they load unchanged.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reminder {
    pub id: String,
    pub at: i64,
    pub text: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<i64>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub all_day: bool,
    #[serde(default = "yes")]
    pub notify: bool,
    /// "daily" | "weekdays" | "weekly" | "monthly".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeat: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub done: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub done_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snooze: Option<i64>,
    /// Its notice has gone off (a past reminder not yet done is overdue).
    #[serde(default, skip_serializing_if = "is_false")]
    pub fired: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tz: Option<String>,
    #[serde(default)]
    pub created: i64,
    #[serde(default)]
    pub updated: i64,
}

fn is_false(b: &bool) -> bool {
    !*b
}
fn yes() -> bool {
    true
}

impl Reminder {
    pub fn new(id: String, at: i64, text: String) -> Reminder {
        Reminder { id, at, text, notify: true, ..Default::default() }
    }

    /// When it goes off: its snooze time, else its time.
    pub fn due(&self) -> i64 {
        self.snooze.unwrap_or(self.at)
    }

    fn waiting(&self) -> bool {
        !self.done && !self.fired
    }
}

/// Done reminders (and past ones never done) are kept this long for the views, then dropped.
const KEEP_PAST_MS: i64 = 14 * DAY_MS;

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// A countdown ended. For a pomodoro: what ended, what comes next ("work" | "short" | "long", or
    /// "done" when the cycle is over) and for how many minutes, and whether it starts by itself.
    Timer { label: String, pomodoro: Option<(&'static str, &'static str, u64)> },
    /// A reminder is due; `late` when aina was not running at its time (more than 5 minutes ago).
    Remind { reminder: Reminder, late: bool },
}

/// Finished focus sessions on one local day.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Stats {
    pub day: i64,
    pub done: u32,
}

#[derive(Default)]
pub struct Timers {
    pub timer: Option<Timer>,
    pub reminders: Vec<Reminder>,
    pub config: PomodoroConfig,
    pub stats: Stats,
    /// The stats changed since the caller last saved them.
    pub stats_changed: bool,
    /// The local time zone's offset from UTC, for what "today" means.
    pub utc_offset_ms: i64,
    seq: u64,
}

impl Timers {
    /// Restores reminders saved in config.json, dropping malformed ones (as v1 did).
    pub fn new(saved: &Value) -> Timers {
        let mut t = Timers::default();
        for r in saved.as_array().into_iter().flatten() {
            let (Some(at), Some(text)) = (r["at"].as_f64().filter(|a| a.is_finite()), r["text"].as_str().filter(|s| !s.is_empty())) else { continue };
            let mut rem: Reminder = serde_json::from_value(r.clone()).unwrap_or_else(|_| Reminder::new(String::new(), at as i64, text.into()));
            rem.at = at as i64;
            if rem.id.is_empty() {
                rem.id = t.new_id(at as i64);
            }
            t.reminders.push(rem);
        }
        t.reminders.sort_by_key(|r| r.at);
        t
    }

    fn new_id(&mut self, salt: i64) -> String {
        self.seq += 1;
        format!("{:x}{:x}", salt.unsigned_abs() % 0xfffff, self.seq)
    }

    fn day(&self, now: i64) -> i64 {
        (now + self.utc_offset_ms).div_euclid(DAY_MS)
    }

    /// Focus sessions finished today.
    pub fn done_today(&self, now: i64) -> u32 {
        if self.stats.day == self.day(now) { self.stats.done } else { 0 }
    }

    pub fn set_timer(&mut self, now: i64, ms: u64, label: &str) {
        self.timer = Some(Timer { label: label.into(), ms, ends_at: now + ms as i64, paused_left: None, pomodoro: None });
    }

    /// A pomodoro with the current settings, starting with focus session 1.
    pub fn start_pomodoro(&mut self, now: i64, label: &str) {
        let ms = self.config.phase_ms("work");
        self.timer = Some(Timer { label: label.into(), ms, ends_at: now + ms as i64, paused_left: None, pomodoro: Some(Pomodoro { phase: "work", round: 1 }) });
    }

    /// True when a countdown was running. Ends the whole pomodoro cycle.
    pub fn stop_timer(&mut self) -> bool {
        self.timer.take().is_some()
    }

    /// What the countdown has left (ms), paused or running.
    pub fn left(&self, now: i64) -> Option<u64> {
        self.timer.as_ref().map(|t| t.paused_left.unwrap_or_else(|| (t.ends_at - now).max(0) as u64))
    }

    /// Holds the countdown where it is. False when there is none or it is already held.
    pub fn pause(&mut self, now: i64) -> bool {
        let left = self.left(now);
        match self.timer.as_mut() {
            Some(t) if t.paused_left.is_none() => {
                t.paused_left = left;
                true
            }
            _ => false,
        }
    }

    /// Starts a held (or waiting) countdown again from where it was.
    pub fn resume(&mut self, now: i64) -> bool {
        match self.timer.as_mut() {
            Some(t) if t.paused_left.is_some() => {
                t.ends_at = now + t.paused_left.take().unwrap_or(0) as i64;
                true
            }
            _ => false,
        }
    }

    /// The current phase back to its full length; a held one stays held.
    pub fn reset(&mut self, now: i64) -> bool {
        let Some(t) = self.timer.as_mut() else { return false };
        if t.paused_left.is_some() {
            t.paused_left = Some(t.ms);
        } else {
            t.ends_at = now + t.ms as i64;
        }
        true
    }

    /// Ends the current phase now and moves to the next (a plain countdown just ends). A skipped
    /// focus session does not count as finished. Returns what happened, like tick() would.
    pub fn skip(&mut self, now: i64) -> Option<Event> {
        self.timer.as_ref()?;
        Some(self.advance(now, false))
    }

    /// From the phase that just ended to the next one.
    fn advance(&mut self, now: i64, finished: bool) -> Event {
        let t = self.timer.take().expect("a timer to advance");
        let Some(Pomodoro { phase, round }) = t.pomodoro else {
            return Event::Timer { label: t.label, pomodoro: None };
        };
        if phase == "work" && finished {
            let day = self.day(now);
            if self.stats.day != day {
                self.stats = Stats { day, done: 0 };
            }
            self.stats.done += 1;
            self.stats_changed = true;
        }
        let (next, round) = match phase {
            "work" => (self.config.break_after(round), round),
            _ if round >= self.config.sessions => ("done", round),
            _ => ("work", round + 1),
        };
        if next == "done" {
            return Event::Timer { label: t.label, pomodoro: Some((phase, "done", 0)) };
        }
        let ms = self.config.phase_ms(next);
        let paused_left = (!self.config.auto_start).then_some(ms);
        self.timer = Some(Timer { label: t.label.clone(), ms, ends_at: now + ms as i64, paused_left, pomodoro: Some(Pomodoro { phase: next, round }) });
        Event::Timer { label: t.label, pomodoro: Some((phase, next, ms / 60_000)) }
    }

    /// When the whole cycle would end if it ran on from now without pauses.
    pub fn cycle_ends_at(&self, now: i64) -> Option<i64> {
        let t = self.timer.as_ref()?;
        let p = t.pomodoro.as_ref()?;
        let mut at = now + self.left(now).unwrap_or(0) as i64;
        let (mut phase, mut round) = (p.phase, p.round);
        loop {
            let (next, r) = match phase {
                "work" => (self.config.break_after(round), round),
                _ if round >= self.config.sessions => return Some(at),
                _ => ("work", round + 1),
            };
            at += self.config.phase_ms(next) as i64;
            (phase, round) = (next, r);
        }
    }

    /// Adds a reminder, kept sorted by time. The caller saves `reminders` afterwards.
    pub fn add_reminder(&mut self, at: i64, text: &str) -> Reminder {
        self.put(Reminder::new(String::new(), at, text.into()), None)
    }

    /// Adds `r` (a new id when it has none) or replaces the one with its id, keeping when it was made.
    /// A changed time arms it again. Returns what was stored.
    pub fn put(&mut self, mut r: Reminder, now: Option<i64>) -> Reminder {
        if r.id.is_empty() {
            r.id = self.new_id(r.at);
        }
        let now = now.unwrap_or(r.updated.max(r.created));
        match self.reminders.iter_mut().find(|x| x.id == r.id) {
            Some(old) => {
                r.created = old.created;
                if old.at != r.at || old.repeat != r.repeat {
                    r.fired = false;
                    r.snooze = None;
                } else {
                    r.fired = old.fired && r.snooze == old.snooze;
                }
                r.updated = now;
                *old = r.clone();
            }
            None => {
                r.created = if r.created > 0 { r.created } else { now };
                r.updated = now;
                self.reminders.push(r.clone());
            }
        }
        self.reminders.sort_by_key(|r| r.at);
        r
    }

    /// Marks a reminder done. A repeating one moves on to its next occurrence instead, so the series
    /// goes on. `next` gives that occurrence (the caller knows the time zone).
    pub fn complete(&mut self, id: &str, now: i64, next: impl Fn(&Reminder) -> Option<i64>) -> bool {
        let Some(r) = self.reminders.iter_mut().find(|r| r.id == id) else { return false };
        match r.repeat.as_ref().and_then(|_| next(r)) {
            Some(at) => {
                r.at = at;
                r.fired = false;
            }
            None => {
                r.done = true;
                r.done_at = Some(now);
            }
        }
        r.snooze = None;
        r.updated = now;
        self.reminders.sort_by_key(|r| r.at);
        true
    }

    /// Undoes "done".
    pub fn reopen(&mut self, id: &str, now: i64) -> bool {
        let Some(r) = self.reminders.iter_mut().find(|r| r.id == id) else { return false };
        r.done = false;
        r.done_at = None;
        r.updated = now;
        true
    }

    /// Puts a reminder off until `until`: it goes off again then.
    pub fn snooze(&mut self, id: &str, until: i64, now: i64) -> bool {
        let Some(r) = self.reminders.iter_mut().find(|r| r.id == id) else { return false };
        r.snooze = Some(until);
        r.fired = false;
        r.done = false;
        r.updated = now;
        true
    }

    pub fn remove_reminder(&mut self, id: &str) -> bool {
        let before = self.reminders.len();
        self.reminders.retain(|r| r.id != id);
        self.reminders.len() != before
    }

    /// Whether tick() has anything to watch.
    pub fn busy(&self) -> bool {
        self.timer.is_some() || self.reminders.iter().any(Reminder::waiting)
    }

    /// Reminders still to go off, soonest first (the notch's older pages).
    pub fn upcoming(&self) -> Vec<&Reminder> {
        let mut list: Vec<&Reminder> = self.reminders.iter().filter(|r| r.waiting()).collect();
        list.sort_by_key(|r| r.due());
        list
    }

    /// Fires what is due. A pomodoro rolls on through its cycle by itself until stopped (each next
    /// phase waits for Start when auto-start is off). A due reminder stays (overdue until done); a
    /// repeating one moves on to its next occurrence (`next`, from the caller's time zone). Reminders
    /// past for two weeks are dropped. Returns the events and whether the reminders changed (so the
    /// caller saves them).
    pub fn tick_with(&mut self, now: i64, next: impl Fn(&Reminder) -> Option<i64>) -> (Vec<Event>, bool) {
        let mut events = Vec::new();
        if self.timer.as_ref().is_some_and(|t| t.paused_left.is_none() && now >= t.ends_at) {
            events.push(self.advance(now, true));
        }
        let mut saved = false;
        for r in self.reminders.iter_mut().filter(|r| r.waiting() && r.due() <= now) {
            saved = true;
            if r.notify {
                events.push(Event::Remind { reminder: r.clone(), late: now - r.due() > 5 * 60_000 });
            }
            match r.repeat.as_ref().and_then(|_| next(r)) {
                Some(at) => r.at = at,
                None => r.fired = true,
            }
            r.snooze = None;
        }
        let before = self.reminders.len();
        self.reminders.retain(|r| !(r.done || r.fired) || now - r.done_at.unwrap_or(r.due()).max(r.end.unwrap_or(0)) < KEEP_PAST_MS);
        saved |= self.reminders.len() != before;
        if saved {
            self.reminders.sort_by_key(|r| r.at);
        }
        (events, saved)
    }

    /// tick_with on the stored UTC offset (repeats step by whole local days).
    pub fn tick(&mut self, now: i64) -> (Vec<Event>, bool) {
        let tz = chrono::FixedOffset::east_opt((self.utc_offset_ms / 1000) as i32).unwrap_or(chrono::FixedOffset::east_opt(0).unwrap());
        self.tick_with(now, |r| crate::calendar::next_after(&tz, r.at, r.repeat.as_deref().unwrap_or(""), now))
    }

    /// What the notch shows. `timer` keeps v1's fields (label, endsAt, ms, phase, round) and adds
    /// left, paused, sessions and cycleEndsAt; `pomodoro` is the settings and today's count.
    pub fn snapshot(&self, now: i64) -> Value {
        let timer = self.timer.as_ref().map(|t| {
            let left = self.left(now).unwrap_or(0);
            json!({
                "label": t.label,
                "endsAt": now + left as i64,
                "ms": t.ms,
                "left": left,
                "paused": t.paused_left.is_some(),
                "phase": t.pomodoro.as_ref().map(|p| p.phase),
                "round": t.pomodoro.as_ref().map(|p| p.round),
                "sessions": t.pomodoro.as_ref().map(|_| self.config.sessions),
                "longEvery": t.pomodoro.as_ref().map(|_| self.config.long_every),
                "cycleEndsAt": self.cycle_ends_at(now),
            })
        });
        json!({
            "timer": timer,
            "reminders": self.upcoming().into_iter().take(3).collect::<Vec<_>>(),
            "pomodoro": { "config": self.config, "today": self.done_today(now) },
        })
    }
}

// The timer and reminder cases of test/notch.test.js, plus v2's pomodoro controls.
#[cfg(test)]
mod tests {
    use super::*;

    fn phase(t: &Timers) -> (&'static str, u32) {
        let p = t.timer.as_ref().unwrap().pomodoro.as_ref().unwrap();
        (p.phase, p.round)
    }

    #[test]
    fn timer_counts_down_and_fires_once() {
        let mut t = Timers::default();
        let mut now = 1_000_000;
        t.set_timer(now, 60_000, "teh");
        now += 59_000;
        assert!(t.tick(now).0.is_empty());
        now += 1_000;
        let mut fired = t.tick(now).0;
        fired.extend(t.tick(now).0);
        assert_eq!(fired, vec![Event::Timer { label: "teh".into(), pomodoro: None }]);
        assert_eq!(t.snapshot(now)["timer"], Value::Null);
    }

    #[test]
    fn a_pomodoro_cycle_runs_its_sessions_with_long_breaks_then_ends() {
        let mut t = Timers { config: PomodoroConfig { auto_start: true, ..Default::default() }, ..Default::default() };
        t.start_pomodoro(1_000_000, "");
        let mut fired = Vec::new();
        while t.timer.is_some() {
            let end = t.timer.as_ref().unwrap().ends_at;
            fired.extend(t.tick(end).0);
        }
        let steps: Vec<_> = fired.iter().map(|e| match e { Event::Timer { pomodoro: Some((a, b, _)), .. } => (*a, *b), _ => ("?", "?") }).collect();
        assert_eq!(steps, [("work", "short"), ("short", "work"), ("work", "short"), ("short", "work"), ("work", "short"), ("short", "work"), ("work", "long"), ("long", "done")]);
        assert_eq!(fired[0], Event::Timer { label: "".into(), pomodoro: Some(("work", "short", BREAK_MS / 60_000)) });
        assert_eq!(t.stats.done, 4);
        assert!(!t.busy());
    }

    #[test]
    fn pause_resume_reset_skip_and_stop_differ() {
        let mut t = Timers::default();
        let mut now = 1_000_000;
        t.start_pomodoro(now, "laporan");
        now += 60_000;
        assert!(t.pause(now));
        assert!(!t.pause(now));
        now += 600_000; // held: nothing moves, nothing fires
        assert!(t.tick(now).0.is_empty());
        assert_eq!(t.left(now), Some(WORK_MS - 60_000));
        assert_eq!(t.snapshot(now)["timer"]["paused"], true);
        assert!(t.resume(now));
        assert_eq!(t.left(now + 1_000), Some(WORK_MS - 61_000));
        // Reset: the same phase, full again.
        assert!(t.reset(now));
        assert_eq!((phase(&t), t.left(now)), (("work", 1), Some(WORK_MS)));
        // Skip: on to the break, and a skipped session isn't counted.
        assert!(matches!(t.skip(now), Some(Event::Timer { pomodoro: Some(("work", "short", 5)), .. })));
        assert_eq!(phase(&t), ("short", 1));
        assert_eq!(t.done_today(now), 0);
        // Stop: the whole cycle.
        assert!(t.stop_timer());
        assert!(t.skip(now).is_none());
    }

    #[test]
    fn without_auto_start_the_next_phase_waits_for_start() {
        let mut t = Timers { config: PomodoroConfig { auto_start: false, ..Default::default() }, ..Default::default() };
        t.start_pomodoro(0, "");
        let (fired, _) = t.tick(WORK_MS as i64);
        assert_eq!(fired.len(), 1);
        let later = WORK_MS as i64 + 3_600_000;
        assert!(t.tick(later).0.is_empty());
        assert_eq!((phase(&t), t.left(later)), (("short", 1), Some(BREAK_MS)));
        assert!(t.resume(later));
        assert_eq!(t.left(later), Some(BREAK_MS));
    }

    #[test]
    fn settings_shape_the_cycle_and_its_estimate() {
        let cfg = PomodoroConfig::from_value(&json!({ "focusMin": 50, "shortMin": 10, "longMin": 30, "sessions": 3, "longEvery": 2, "autoStart": true, "notify": false, "bogus": 1 }));
        assert_eq!((cfg.focus_min, cfg.sessions, cfg.long_every, cfg.notify), (50, 3, 2, false));
        assert_eq!(PomodoroConfig::from_value(&json!({ "focusMin": 0, "sessions": 99 })).focus_min, 1);
        assert_eq!(cfg.name(), "50/10 ×3");
        assert_eq!(PomodoroConfig::default().name(), "25/5");
        let mut t = Timers { config: cfg, ..Default::default() };
        t.start_pomodoro(0, "");
        // 50 + 10 (short after 1) + 50 + 30 (long after 2) + 50 + 30 (long after the last) minutes.
        assert_eq!(t.cycle_ends_at(0), Some(220 * 60_000));
        assert_eq!(t.snapshot(0)["timer"]["cycleEndsAt"], 220 * 60_000);
        assert_eq!(t.snapshot(0)["pomodoro"]["config"]["focusMin"], 50);
        assert_eq!(builtin_presets().len(), 3);
    }

    #[test]
    fn todays_count_starts_over_the_next_day() {
        let mut t = Timers { utc_offset_ms: 7 * 3_600_000, ..Default::default() }; // WIB
        let day = 20_000 * DAY_MS - 7 * 3_600_000; // local midnight
        t.start_pomodoro(day, "");
        t.tick(day + WORK_MS as i64);
        assert_eq!(t.done_today(day + WORK_MS as i64), 1);
        assert!(t.stats_changed);
        assert_eq!(t.done_today(day + DAY_MS), 0);
    }

    #[test]
    fn reminders_sorted_fired_when_due_late_ones_say_so() {
        let mut now = 1_000_000;
        let mut t = Timers::default();
        t.add_reminder(now + 120_000, "meeting Sari");
        t.add_reminder(now + 60_000, "minum");
        assert_eq!(t.reminders.iter().map(|r| r.text.as_str()).collect::<Vec<_>>(), ["minum", "meeting Sari"]);
        now += 60_000;
        let (due, saved) = t.tick(now);
        assert!(saved);
        assert!(matches!(&due[..], [Event::Remind { reminder, late: false }] if reminder.text == "minum"));
        // It stays, overdue, until it is done; only one is still to go off.
        assert_eq!(t.reminders.len(), 2);
        assert_eq!(t.upcoming().len(), 1);
        assert!(t.tick(now).0.is_empty(), "goes off once");
        now += 3_600_000; // aina was closed past the reminder
        let (due, _) = t.tick(now);
        assert!(matches!(&due[..], [Event::Remind { late: true, .. }]));
        assert!(!t.busy());
        // Two weeks on, past reminders are dropped.
        let (_, saved) = t.tick(now + KEEP_PAST_MS + 1);
        assert!(saved && t.reminders.is_empty());
    }

    #[test]
    fn reminders_complete_snooze_and_repeat() {
        let now = 1_000_000;
        let mut t = Timers::default();
        let a = t.add_reminder(now + 60_000, "minum");
        let mut daily = Reminder::new(String::new(), now + 30_000, "obat".into());
        daily.repeat = Some("daily".into());
        let daily = t.put(daily, Some(now));
        // Snoozed: goes off at the new time, not the old one.
        assert!(t.snooze(&a.id, now + 600_000, now));
        let (due, _) = t.tick(now + 120_000);
        assert!(matches!(&due[..], [Event::Remind { reminder, .. }] if reminder.text == "obat"));
        // The daily one moved a day on instead of being used up.
        let d = t.reminders.iter().find(|r| r.id == daily.id).unwrap();
        assert_eq!((d.at, d.fired), (now + 30_000 + DAY_MS, false));
        let (due, _) = t.tick(now + 600_000);
        assert!(matches!(&due[..], [Event::Remind { reminder, .. }] if reminder.text == "minum"));
        // Done: a one-off is done, a repeat moves on.
        assert!(t.complete(&a.id, now + 700_000, |_| None));
        assert!(t.reminders.iter().find(|r| r.id == a.id).unwrap().done);
        assert!(t.complete(&daily.id, now + 700_000, |r| Some(r.at + DAY_MS)));
        assert_eq!(t.reminders.iter().find(|r| r.id == daily.id).unwrap().at, now + 30_000 + 2 * DAY_MS);
        assert!(t.reopen(&a.id, now + 800_000));
        assert!(!t.reminders.iter().find(|r| r.id == a.id).unwrap().done);
        // A silent one (no notification) doesn't speak but is still marked as gone off.
        let mut quiet = Reminder::new(String::new(), now + 900_000, "diam".into());
        quiet.notify = false;
        let quiet = t.put(quiet, Some(now));
        assert!(t.tick(now + 900_000).0.is_empty());
        assert!(t.reminders.iter().find(|r| r.id == quiet.id).unwrap().fired);
    }

    #[test]
    fn editing_a_reminder_keeps_its_id_and_rearms_a_new_time() {
        let mut t = Timers::default();
        let r = t.add_reminder(5_000, "a");
        t.tick(6_000);
        let mut e = t.reminders[0].clone();
        e.text = "b".into();
        e.at = 9_000;
        let stored = t.put(e, Some(7_000));
        assert_eq!((stored.id, stored.text.as_str(), stored.fired, t.reminders.len()), (r.id, "b", false, 1));
        assert!(t.busy());
    }

    #[test]
    fn old_and_new_saved_reminders_load() {
        let t = Timers::new(&json!([{ "id": "x", "at": 5000, "text": "lama" }, { "id": "y", "at": 4000, "text": "baru", "repeat": "weekly", "note": "catatan", "notify": false, "done": true }]));
        assert_eq!(t.reminders[0].text, "baru");
        assert_eq!((t.reminders[0].repeat.as_deref(), t.reminders[0].notify, t.reminders[0].done), (Some("weekly"), false, true));
        assert!(t.reminders[1].notify);
        let saved = serde_json::to_value(&t.reminders[1]).unwrap();
        assert_eq!(saved, json!({ "id": "x", "at": 5000, "text": "lama", "notify": true, "created": 0, "updated": 0 }));
    }

    #[test]
    fn reminders_restored_from_config_and_removable() {
        let mut t = Timers::new(&json!([{ "at": 5000, "text": "a" }, { "at": "x", "text": "bad" }, null]));
        assert_eq!(t.reminders.len(), 1);
        let id = t.reminders[0].id.clone();
        assert!(t.remove_reminder(&id));
        assert!(!t.remove_reminder("nope"));
    }
}
