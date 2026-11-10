// The Reminder Center's event model: local reminders and Google Calendar events normalized into one
// shape that the notch's three views (Agenda, Timeline, Calendar) all read. Pure: the clock, the time
// zone and the provider's JSON come in as arguments, so every rule here is tested without a network.

use std::collections::HashMap;

use chrono::{DateTime, Datelike, Days, Months, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Weekday};
use serde::Serialize;
use serde_json::{Value, json};

use crate::timers::Reminder;

pub const DAY_MS: i64 = 86_400_000;

/// One item in any view. `id` is stable across refreshes: "local:<id>" (a repeat's later occurrences
/// "local:<id>@<ms>") or "google:<calendar id>:<event id>", so nothing is matched by title or time.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalEvent {
    pub id: String,
    pub source: &'static str, // "local" | "google"
    pub kind: &'static str,   // "reminder" | "event"
    pub local_id: Option<String>,
    pub calendar_id: Option<String>,
    pub remote_id: Option<String>,
    pub calendar: Option<String>,
    pub color: Option<String>,
    pub title: String,
    pub description: String,
    pub start: i64,
    pub end: Option<i64>,
    pub all_day: bool,
    pub time_zone: String,
    pub notify: bool,
    pub recurrence: Option<String>,
    pub recurring: bool,
    pub projected: bool, // a later occurrence of a local repeat, shown but not stored on its own
    pub done: bool,
    pub snoozed_until: Option<i64>,
    pub editable: bool,
    pub read_only_reason: Option<&'static str>, // "readOnlyCalendar" | "readOnlyEvent"
    pub sync: &'static str,                     // "local" | "synced" | "pending"
    pub synced_at: Option<i64>,
    pub created: i64,
    pub updated: i64,
    pub link: Option<String>,
    pub actions: Vec<&'static str>,
}

impl CalEvent {
    /// When it asks for attention: a snoozed reminder at its new time.
    pub fn due(&self) -> i64 {
        self.snoozed_until.unwrap_or(self.start)
    }

    /// Whether any part of it falls in [from, to). An event without an end is a moment.
    pub fn overlaps(&self, from: i64, to: i64) -> bool {
        self.start < to && self.end.unwrap_or(self.start + 1).max(self.start + 1) > from
    }
}

/// The actions the notch offers for an event, by its source and permissions. A projected repeat
/// occurrence is changed through its series (edit, delete) and isn't completed ahead of time.
pub fn actions_for(e: &CalEvent) -> Vec<&'static str> {
    match (e.source, e.sync) {
        ("google", "pending") => vec!["open"],
        ("google", _) if e.editable => vec!["open", "edit", "delete"],
        ("google", _) => vec!["open"],
        _ if e.projected => vec!["edit", "delete"],
        _ if e.kind == "reminder" && e.done => vec!["undo", "delete"],
        _ if e.kind == "reminder" => vec!["done", "snooze", "edit", "delete"],
        _ => vec!["edit", "delete"],
    }
}

// ---------- time ----------

fn at_local<Tz: TimeZone>(tz: &Tz, naive: NaiveDateTime) -> i64 {
    // A local time skipped by a clock change moves an hour on; an ambiguous one takes the first.
    tz.from_local_datetime(&naive).earliest().or_else(|| tz.from_local_datetime(&(naive + chrono::Duration::hours(1))).earliest()).map(|d| d.timestamp_millis()).unwrap_or(0)
}

fn local<Tz: TimeZone>(tz: &Tz, ms: i64) -> NaiveDateTime {
    tz.timestamp_millis_opt(ms).single().map(|d| d.naive_local()).unwrap_or_default()
}

/// Local midnight of the day `ms` falls on.
pub fn day_start<Tz: TimeZone>(tz: &Tz, ms: i64) -> i64 {
    at_local(tz, local(tz, ms).date().and_time(NaiveTime::MIN))
}

/// The range the views load for a month: its first to its last day, widened to whole Monday-first
/// weeks (the calendar grid shows them).
pub fn month_range<Tz: TimeZone>(tz: &Tz, year: i32, month: u32) -> (i64, i64) {
    let first = NaiveDate::from_ymd_opt(year, month, 1).unwrap_or_default();
    let next = first.checked_add_months(Months::new(1)).unwrap_or(first);
    let back = first.weekday().num_days_from_monday() as u64;
    let start = first.checked_sub_days(Days::new(back)).unwrap_or(first);
    let ahead = (7 - next.weekday().num_days_from_monday() as u64) % 7;
    let end = next.checked_add_days(Days::new(ahead)).unwrap_or(next);
    (at_local(tz, start.and_time(NaiveTime::MIN)), at_local(tz, end.and_time(NaiveTime::MIN)))
}

/// The next occurrence of a repeat after the one at `at`: "daily", "weekdays" (Mon–Fri), "weekly" or
/// "monthly" (the same day, or the month's last day when it is shorter). The local clock time stays.
pub fn step<Tz: TimeZone>(tz: &Tz, at: i64, repeat: &str) -> Option<i64> {
    let t = local(tz, at);
    let next = match repeat {
        "daily" => t.checked_add_days(Days::new(1))?,
        "weekly" => t.checked_add_days(Days::new(7))?,
        "weekdays" => {
            let skip = match t.weekday() {
                Weekday::Fri => 3,
                Weekday::Sat => 2,
                _ => 1,
            };
            t.checked_add_days(Days::new(skip))?
        }
        "monthly" => t.checked_add_months(Months::new(1))?,
        _ => return None,
    };
    Some(at_local(tz, next))
}

/// The first occurrence strictly after `after`, starting from the one at `at`.
pub fn next_after<Tz: TimeZone>(tz: &Tz, at: i64, repeat: &str, after: i64) -> Option<i64> {
    let mut t = at;
    for _ in 0..4000 {
        if t > after {
            return Some(t);
        }
        t = step(tz, t, repeat)?;
    }
    None
}

/// The quick-add shortcuts: in 5, 15 or 60 minutes, later today (17:00, or in 3 hours once it is past
/// 14:00), and tomorrow at 09:00.
pub fn shortcuts<Tz: TimeZone>(tz: &Tz, now: i64) -> Value {
    let t = local(tz, now);
    let today = t.date();
    let later = if t.time() < NaiveTime::from_hms_opt(14, 0, 0).unwrap() { at_local(tz, today.and_hms_opt(17, 0, 0).unwrap()) } else { now + 3 * 3_600_000 };
    let tomorrow = at_local(tz, today.succ_opt().unwrap_or(today).and_hms_opt(9, 0, 0).unwrap());
    let round = |ms: i64| ms - ms.rem_euclid(60_000);
    json!({ "5m": round(now + 5 * 60_000), "15m": round(now + 15 * 60_000), "1h": round(now + 3_600_000), "later": round(later), "tomorrow": tomorrow })
}

// ---------- local reminders ----------

/// A local reminder in [from, to): the stored one (with its done and snooze state) and, for a repeat,
/// its later occurrences in the range, at most `max`.
pub fn from_local<Tz: TimeZone>(tz: &Tz, r: &Reminder, from: i64, to: i64, max: usize) -> Vec<CalEvent> {
    let kind = if r.end.is_some() || r.all_day { "event" } else { "reminder" };
    let base = CalEvent {
        id: format!("local:{}", r.id),
        source: "local",
        kind,
        local_id: Some(r.id.clone()),
        calendar_id: None,
        remote_id: None,
        calendar: None,
        color: None,
        title: r.text.clone(),
        description: r.note.clone(),
        start: r.at,
        end: r.end,
        all_day: r.all_day,
        time_zone: r.tz.clone().unwrap_or_default(),
        notify: r.notify,
        recurrence: r.repeat.clone(),
        recurring: r.repeat.is_some(),
        projected: false,
        done: r.done,
        snoozed_until: r.snooze,
        editable: true,
        read_only_reason: None,
        sync: "local",
        synced_at: None,
        created: r.created,
        updated: r.updated,
        link: None,
        actions: vec![],
    };
    let length = r.end.map(|e| e - r.at);
    let mut out = Vec::new();
    let mut at = r.at;
    let mut first = true;
    while out.len() < max {
        let mut e = base.clone();
        if !first {
            e.id = format!("local:{}@{at}", r.id);
            e.start = at;
            e.end = length.map(|l| at + l);
            e.projected = true;
            e.done = false;
            e.snoozed_until = None;
        }
        // A snoozed reminder shows on its new time's day too.
        if e.overlaps(from, to) || (first && e.snoozed_until.is_some_and(|s| s >= from && s < to)) {
            e.actions = actions_for(&e);
            out.push(e);
        }
        let Some(repeat) = &r.repeat else { break };
        match step(tz, at, repeat) {
            Some(n) if n < to => at = n,
            _ => break,
        }
        first = false;
    }
    out
}

// ---------- Google Calendar ----------

/// A calendar the person can see. `access` is Google's accessRole: owner, writer, reader, freeBusyReader.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarInfo {
    pub id: String,
    pub name: String,
    pub color: Option<String>,
    pub access: String,
    pub primary: bool,
    pub selected: bool,
}

impl CalendarInfo {
    pub fn writable(&self) -> bool {
        self.access == "owner" || self.access == "writer"
    }
}

/// The first array under any of `keys`, looked for in the value and up to three levels down, since
/// Composio wraps the provider's response differently per tool and per key kind.
pub fn find_array<'a>(v: &'a Value, keys: &[&str]) -> Option<&'a Vec<Value>> {
    fn walk<'a>(v: &'a Value, keys: &[&str], depth: u8) -> Option<&'a Vec<Value>> {
        let o = v.as_object()?;
        for k in keys {
            if let Some(a) = o.get(*k).and_then(Value::as_array) {
                return Some(a);
            }
        }
        if depth == 0 {
            return None;
        }
        o.values().find_map(|c| walk(c, keys, depth - 1))
    }
    walk(v, keys, 3)
}

/// The same for a string (the next page's token).
pub fn find_str<'a>(v: &'a Value, keys: &[&str]) -> Option<&'a str> {
    fn walk<'a>(v: &'a Value, keys: &[&str], depth: u8) -> Option<&'a str> {
        let o = v.as_object()?;
        for k in keys {
            if let Some(s) = o.get(*k).and_then(Value::as_str).filter(|s| !s.is_empty()) {
                return Some(s);
            }
        }
        if depth == 0 {
            return None;
        }
        o.values().find_map(|c| walk(c, keys, depth - 1))
    }
    walk(v, keys, 3)
}

/// The calendars in a calendar-list response. Free/busy-only calendars carry no event details.
pub fn calendars_from(data: &Value) -> Vec<CalendarInfo> {
    find_array(data, &["items", "calendars"])
        .into_iter()
        .flatten()
        .filter_map(|c| {
            let id = c["id"].as_str()?.to_string();
            let access = c["accessRole"].as_str().or(c["access_role"].as_str()).unwrap_or("reader").to_string();
            (access != "freeBusyReader").then(|| CalendarInfo {
                name: c["summaryOverride"].as_str().or(c["summary"].as_str()).unwrap_or(&id).to_string(),
                color: c["backgroundColor"].as_str().map(String::from),
                primary: c["primary"] == true,
                selected: c["selected"] != false,
                access,
                id,
            })
        })
        .collect()
}

// A start or end: a dateTime with its offset, or an all-day date (its local midnight).
fn when<Tz: TimeZone>(tz: &Tz, v: &Value) -> Option<(i64, bool)> {
    if let Some(s) = v["dateTime"].as_str() {
        return DateTime::parse_from_rfc3339(s).ok().map(|d| (d.timestamp_millis(), false));
    }
    let d = NaiveDate::parse_from_str(v["date"].as_str()?, "%Y-%m-%d").ok()?;
    Some((at_local(tz, d.and_time(NaiveTime::MIN)), true))
}

fn stamp(v: &Value) -> i64 {
    v.as_str().and_then(|s| DateTime::parse_from_rfc3339(s).ok()).map(|d| d.timestamp_millis()).unwrap_or(0)
}

/// One Google event in the shared shape, or None for a deleted (cancelled) or unreadable one.
/// Recurring series come expanded (singleEvents), so each occurrence has its own id.
pub fn google_event<Tz: TimeZone>(tz: &Tz, cal: &CalendarInfo, raw: &Value, synced_at: i64) -> Option<CalEvent> {
    if raw["status"] == "cancelled" {
        return None;
    }
    let event_id = raw["id"].as_str()?;
    let (start, all_day) = when(tz, &raw["start"])?;
    let end = when(tz, &raw["end"]).map(|e| e.0).or(all_day.then_some(start + DAY_MS));
    // Birthdays and events copied from Gmail can't be edited, nor can a locked event or one on a
    // calendar the person only reads.
    let fixed_type = matches!(raw["eventType"].as_str(), Some("birthday" | "fromGmail"));
    let read_only_reason = if !cal.writable() {
        Some("readOnlyCalendar")
    } else if raw["locked"] == true || fixed_type {
        Some("readOnlyEvent")
    } else {
        None
    };
    let mut e = CalEvent {
        id: format!("google:{}:{event_id}", cal.id),
        source: "google",
        kind: "event",
        local_id: None,
        calendar_id: Some(cal.id.clone()),
        remote_id: Some(event_id.to_string()),
        calendar: Some(cal.name.clone()),
        color: raw["backgroundColor"].as_str().map(String::from).or(cal.color.clone()),
        title: raw["summary"].as_str().unwrap_or("").trim().to_string(),
        description: raw["description"].as_str().unwrap_or("").to_string(),
        start,
        end,
        all_day,
        time_zone: raw["start"]["timeZone"].as_str().unwrap_or("").to_string(),
        notify: raw["reminders"]["useDefault"] != false || find_array(&raw["reminders"], &["overrides"]).is_some_and(|o| !o.is_empty()),
        recurrence: None,
        recurring: raw["recurringEventId"].is_string() || raw["recurrence"].is_array(),
        projected: false,
        done: false,
        snoozed_until: None,
        editable: read_only_reason.is_none(),
        read_only_reason,
        sync: "synced",
        synced_at: Some(synced_at),
        created: stamp(&raw["created"]),
        updated: stamp(&raw["updated"]),
        link: raw["htmlLink"].as_str().map(String::from),
        actions: vec![],
    };
    e.actions = actions_for(&e);
    Some(e)
}

/// Local and remote events as one list, by start time. Events are told apart by their stable id only:
/// the same id seen twice (two pages, a refresh racing an edit) is kept once, the later copy winning;
/// two different events with the same title and time both stay.
pub fn merge(lists: impl IntoIterator<Item = Vec<CalEvent>>) -> Vec<CalEvent> {
    let mut by_id: HashMap<String, CalEvent> = HashMap::new();
    for e in lists.into_iter().flatten() {
        by_id.insert(e.id.clone(), e);
    }
    let mut out: Vec<CalEvent> = by_id.into_values().collect();
    out.sort_by(|a, b| (a.start, !a.all_day, &a.title, &a.id).cmp(&(b.start, !b.all_day, &b.title, &b.id)));
    out
}

/// The nearest item still ahead, for the compact notch: not done, not all-day, due at or after now.
pub fn next_up(events: &[CalEvent], now: i64) -> Option<&CalEvent> {
    events.iter().filter(|e| !e.done && !e.all_day && e.due() >= now).min_by_key(|e| e.due())
}

/// What a failed sync means, from the error text: "auth" (sign in again), "scope" (more access needed),
/// "rate" (too many requests), "offline", or "error".
pub fn sync_error(message: &str) -> &'static str {
    let m = message.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| m.contains(w));
    if has(&["insufficient", "scope", "permission denied"]) {
        "scope"
    } else if has(&["401", "unauthorized", "invalid_grant", "expired", "revoked", "no connected account", "not connected", "connected account", "reauth"]) {
        "auth"
    } else if has(&["429", "rate limit", "ratelimit", "quota"]) {
        "rate"
    } else if has(&["timed out", "timeout", "error sending request", "dns", "connection", "network", "offline"]) {
        "offline"
    } else {
        "error"
    }
}

/// A provider tool's arguments, named the way its input schema names them: for each value, the first
/// of its candidate names that the schema has, or the first candidate when the schema is unknown.
pub fn args_for(schema: &Value, pairs: &[(&[&str], Value)]) -> Value {
    let props = schema.get("properties").unwrap_or(schema).as_object();
    let mut out = serde_json::Map::new();
    for (names, value) in pairs {
        if value.is_null() {
            continue;
        }
        let name = match props {
            Some(p) if !p.is_empty() => names.iter().find(|n| p.contains_key(**n)).copied(),
            _ => names.first().copied(),
        };
        if let Some(n) = name {
            out.insert(n.to_string(), value.clone());
        }
    }
    Value::Object(out)
}

/// Whether a schema has any of these fields (an unknown schema is assumed to have them).
pub fn schema_has(schema: &Value, names: &[&str]) -> bool {
    match schema.get("properties").unwrap_or(schema).as_object() {
        Some(p) if !p.is_empty() => names.iter().any(|n| p.contains_key(*n)),
        _ => true,
    }
}

/// Local naive "YYYY-MM-DDTHH:MM:SS" for a moment, as Google's create tools take it with a time zone.
pub fn naive_local<Tz: TimeZone>(tz: &Tz, ms: i64) -> String {
    local(tz, ms).format("%Y-%m-%dT%H:%M:%S").to_string()
}

/// RFC 3339 with the zone's offset, for timeMin/timeMax.
pub fn rfc3339<Tz: TimeZone>(tz: &Tz, ms: i64) -> String
where
    Tz::Offset: std::fmt::Display,
{
    tz.timestamp_millis_opt(ms).single().map(|d| d.to_rfc3339()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;

    fn wib() -> FixedOffset {
        FixedOffset::east_opt(7 * 3600).unwrap()
    }
    fn ms(s: &str) -> i64 {
        at_local(&wib(), NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap())
    }
    fn reminder(id: &str, at: i64, text: &str) -> Reminder {
        Reminder::new(id.into(), at, text.into())
    }
    fn cal(access: &str) -> CalendarInfo {
        CalendarInfo { id: "work@x.com".into(), name: "Kerja".into(), color: Some("#039be5".into()), access: access.into(), primary: false, selected: true }
    }

    #[test]
    fn month_ranges_cover_whole_weeks() {
        // October 2026 starts on a Thursday and ends on a Saturday.
        let (from, to) = month_range(&wib(), 2026, 10);
        assert_eq!(from, ms("2026-09-28 00:00"));
        assert_eq!(to, ms("2026-11-02 00:00"));
        // February 2027: 28 days, starting on a Monday.
        let (from, to) = month_range(&wib(), 2027, 2);
        assert_eq!(from, ms("2027-02-01 00:00"));
        assert_eq!(to, ms("2027-03-01 00:00"));
        assert_eq!(day_start(&wib(), ms("2026-10-29 23:59")), ms("2026-10-29 00:00"));
    }

    #[test]
    fn repeats_step_by_the_local_calendar() {
        let tz = wib();
        assert_eq!(step(&tz, ms("2026-10-29 09:00"), "daily"), Some(ms("2026-10-30 09:00")));
        assert_eq!(step(&tz, ms("2026-10-30 09:00"), "weekdays"), Some(ms("2026-11-02 09:00"))); // Fri → Mon
        assert_eq!(step(&tz, ms("2026-10-29 09:00"), "weekly"), Some(ms("2026-11-05 09:00")));
        assert_eq!(step(&tz, ms("2027-01-31 09:00"), "monthly"), Some(ms("2027-02-28 09:00")));
        assert_eq!(step(&tz, ms("2026-10-29 09:00"), "yearly"), None);
        assert_eq!(next_after(&tz, ms("2026-10-01 09:00"), "daily", ms("2026-10-29 10:00")), Some(ms("2026-10-30 09:00")));
    }

    #[test]
    fn shortcuts_are_relative_or_fixed_times() {
        let tz = wib();
        let s = shortcuts(&tz, ms("2026-10-29 10:07"));
        assert_eq!(s["15m"], ms("2026-10-29 10:22"));
        assert_eq!(s["later"], ms("2026-10-29 17:00"));
        assert_eq!(s["tomorrow"], ms("2026-10-30 09:00"));
        assert_eq!(shortcuts(&tz, ms("2026-10-29 15:00"))["later"], ms("2026-10-29 18:00"));
    }

    #[test]
    fn local_repeats_project_their_occurrences_in_range() {
        let tz = wib();
        let mut r = reminder("a1", ms("2026-10-29 08:00"), "Minum obat");
        r.repeat = Some("daily".into());
        r.done = true;
        let list = from_local(&tz, &r, ms("2026-10-29 00:00"), ms("2026-11-01 00:00"), 50);
        assert_eq!(list.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), [
            "local:a1".to_string(),
            format!("local:a1@{}", ms("2026-10-30 08:00")),
            format!("local:a1@{}", ms("2026-10-31 08:00")),
        ]);
        assert!(list[0].done && !list[1].done && list[1].projected);
        assert_eq!(list[1].actions, ["edit", "delete"]);
        assert_eq!(list[0].actions, ["undo", "delete"]);
        // Outside the range: nothing.
        assert!(from_local(&tz, &reminder("b", ms("2026-12-01 08:00"), "x"), ms("2026-10-29 00:00"), ms("2026-11-01 00:00"), 50).is_empty());
    }

    #[test]
    fn google_events_normalize_timed_all_day_and_cancelled() {
        let tz = wib();
        let timed = json!({ "id": "e1", "summary": "Rapat", "start": { "dateTime": "2026-10-29T23:30:00+07:00", "timeZone": "Asia/Jakarta" }, "end": { "dateTime": "2026-10-30T01:00:00+07:00" }, "htmlLink": "https://calendar.google.com/x", "updated": "2026-10-28T10:00:00Z" });
        let e = google_event(&tz, &cal("writer"), &timed, 5).unwrap();
        assert_eq!(e.id, "google:work@x.com:e1");
        assert_eq!((e.start, e.end), (ms("2026-10-29 23:30"), Some(ms("2026-10-30 01:00"))));
        // Crossing midnight: on both days.
        assert!(e.overlaps(ms("2026-10-29 00:00"), ms("2026-10-30 00:00")));
        assert!(e.overlaps(ms("2026-10-30 00:00"), ms("2026-10-31 00:00")));
        assert_eq!(e.actions, ["open", "edit", "delete"]);
        let all_day = json!({ "id": "e2", "summary": "Libur", "start": { "date": "2026-10-30" }, "end": { "date": "2026-10-31" } });
        let d = google_event(&tz, &cal("writer"), &all_day, 5).unwrap();
        assert!(d.all_day);
        assert!(d.overlaps(ms("2026-10-30 00:00"), ms("2026-10-31 00:00")));
        assert!(!d.overlaps(ms("2026-10-31 00:00"), ms("2026-11-01 00:00")));
        let no_end = json!({ "id": "e3", "start": { "dateTime": "2026-10-29T10:00:00Z" } });
        assert_eq!(google_event(&tz, &cal("owner"), &no_end, 5).unwrap().end, None);
        assert!(google_event(&tz, &cal("writer"), &json!({ "id": "e4", "status": "cancelled", "start": { "date": "2026-10-30" } }), 5).is_none());
    }

    #[test]
    fn read_only_google_events_offer_only_open() {
        let tz = wib();
        let raw = json!({ "id": "e1", "summary": "Ulang tahun", "start": { "date": "2026-10-30" }, "end": { "date": "2026-10-31" } });
        let e = google_event(&tz, &cal("reader"), &raw, 1).unwrap();
        assert_eq!((e.editable, e.read_only_reason), (false, Some("readOnlyCalendar")));
        assert_eq!(e.actions, ["open"]);
        let bday = json!({ "id": "e2", "eventType": "birthday", "start": { "date": "2026-10-30" } });
        assert_eq!(google_event(&tz, &cal("owner"), &bday, 1).unwrap().read_only_reason, Some("readOnlyEvent"));
        let mut pending = google_event(&tz, &cal("owner"), &raw, 1).unwrap();
        pending.sync = "pending";
        assert_eq!(actions_for(&pending), ["open"]);
        let local = from_local(&tz, &reminder("r", ms("2026-10-30 09:00"), "x"), ms("2026-10-30 00:00"), ms("2026-10-31 00:00"), 5);
        assert_eq!(local[0].actions, ["done", "snooze", "edit", "delete"]);
    }

    #[test]
    fn merging_dedupes_by_id_not_by_title_and_time() {
        let tz = wib();
        let raw = |id: &str, title: &str| json!({ "id": id, "summary": title, "start": { "dateTime": "2026-10-29T09:00:00+07:00" }, "end": { "dateTime": "2026-10-29T10:00:00+07:00" } });
        let page1 = vec![google_event(&tz, &cal("owner"), &raw("e1", "Standup"), 1).unwrap()];
        let page2 = vec![google_event(&tz, &cal("owner"), &raw("e1", "Standup (diubah)"), 2).unwrap(), google_event(&tz, &cal("owner"), &raw("e2", "Standup"), 2).unwrap()];
        let local = from_local(&tz, &reminder("l1", ms("2026-10-29 09:00"), "Standup"), ms("2026-10-29 00:00"), ms("2026-10-30 00:00"), 5);
        let all = merge([local, page1, page2]);
        assert_eq!(all.len(), 3);
        assert_eq!(all.iter().filter(|e| e.title == "Standup").count(), 2);
        assert!(all.iter().any(|e| e.title == "Standup (diubah)"));
        // All-day items lead the day.
        let day = google_event(&tz, &cal("owner"), &json!({ "id": "d", "start": { "date": "2026-10-29" }, "end": { "date": "2026-10-30" } }), 1).unwrap();
        assert!(merge([all, vec![day]])[0].all_day);
    }

    #[test]
    fn next_up_skips_done_past_and_all_day_and_follows_snooze() {
        let tz = wib();
        let now = ms("2026-10-29 10:00");
        let mut a = reminder("a", ms("2026-10-29 09:00"), "lewat");
        a.snooze = Some(ms("2026-10-29 10:10"));
        let mut b = reminder("b", ms("2026-10-29 10:05"), "selesai");
        b.done = true;
        let c = reminder("c", ms("2026-10-29 10:30"), "nanti");
        let list: Vec<CalEvent> = [a, b, c].iter().flat_map(|r| from_local(&tz, r, ms("2026-10-29 00:00"), ms("2026-10-30 00:00"), 5)).collect();
        assert_eq!(next_up(&list, now).unwrap().title, "lewat");
    }

    #[test]
    fn sync_errors_are_classified_for_the_notch() {
        assert_eq!(sync_error("Composio 401: token expired"), "auth");
        assert_eq!(sync_error("No connected account found for googlecalendar"), "auth");
        assert_eq!(sync_error("Request had insufficient authentication scopes."), "scope");
        assert_eq!(sync_error("Composio 429 Too Many Requests"), "rate");
        assert_eq!(sync_error("error sending request for url"), "offline");
        assert_eq!(sync_error("something odd"), "error");
    }

    #[test]
    fn provider_arguments_follow_the_schema() {
        let schema = json!({ "properties": { "calendar_id": {}, "timeMin": {}, "time_max": {} } });
        let a = args_for(&schema, &[(&["calendarId", "calendar_id"], json!("primary")), (&["timeMin", "time_min"], json!("a")), (&["timeMax", "time_max"], json!("b")), (&["pageToken"], json!("t")), (&["q"], Value::Null)]);
        assert_eq!(a, json!({ "calendar_id": "primary", "timeMin": "a", "time_max": "b" }));
        // Unknown schema: the first names.
        assert_eq!(args_for(&json!({}), &[(&["calendarId", "calendar_id"], json!("p"))]), json!({ "calendarId": "p" }));
        assert!(schema_has(&schema, &["timeMin"]) && !schema_has(&schema, &["recurrence"]) && schema_has(&Value::Null, &["x"]));
    }

    #[test]
    fn composio_wrappers_are_unwrapped() {
        let wrapped = json!({ "response_data": { "items": [{ "id": "a" }], "nextPageToken": "p2" } });
        assert_eq!(find_array(&wrapped, &["items"]).unwrap().len(), 1);
        assert_eq!(find_str(&wrapped, &["nextPageToken"]), Some("p2"));
        let cals = calendars_from(&json!({ "data": { "calendars": [{ "id": "a", "summary": "A", "accessRole": "owner", "primary": true }, { "id": "b", "accessRole": "freeBusyReader" }] } }));
        assert_eq!(cals.len(), 1);
        assert!(cals[0].primary && cals[0].writable());
    }
}
