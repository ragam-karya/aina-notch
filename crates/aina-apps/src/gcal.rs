// Google Calendar through Composio: the person's calendars, their events in a date range (recurring
// series expanded, every page), and creating, changing and deleting an event. OAuth stays at Composio
// (aina holds only the Composio key); arguments are named from each tool's schema. Turning the
// provider's JSON into events is aina_core::calendar's job.

use aina_core::calendar::{CalendarInfo, args_for, calendars_from, find_array, find_str, naive_local, rfc3339, schema_has};
use chrono::TimeZone;
use serde_json::{Value, json};

use crate::composio::{self, Opts};

pub const LIST_CALENDARS: &str = "GOOGLECALENDAR_LIST_CALENDARS";
pub const LIST_EVENTS: &str = "GOOGLECALENDAR_EVENTS_LIST";
pub const CREATE: &str = "GOOGLECALENDAR_CREATE_EVENT";
pub const PATCH: &str = "GOOGLECALENDAR_PATCH_EVENT";
pub const DELETE: &str = "GOOGLECALENDAR_DELETE_EVENT";

const MAX_PAGES: usize = 8;
const CAL_ID: &[&str] = &["calendarId", "calendar_id"];
const EVENT_ID: &[&str] = &["eventId", "event_id"];
const TZ: &[&str] = &["timezone", "timeZone", "time_zone"];

/// The calendars the person can read events in.
pub fn calendars(o: &Opts) -> Result<Vec<CalendarInfo>, String> {
    let schema = composio::schema(o, LIST_CALENDARS);
    let mut out = Vec::new();
    let mut page: Option<String> = None;
    for _ in 0..MAX_PAGES {
        let args = args_for(&schema, &[(&["maxResults", "max_results"], json!(100)), (&["pageToken", "page_token"], page.clone().map(Value::from).unwrap_or(Value::Null))]);
        let data = composio::run(o, LIST_CALENDARS, &args)?;
        out.extend(calendars_from(&data));
        page = find_str(&data, &["nextPageToken", "next_page_token"]).map(String::from);
        if page.is_none() {
            break;
        }
    }
    if out.is_empty() {
        // No list (a key without the calendar-list scope): the primary calendar still works.
        out.push(CalendarInfo { id: "primary".into(), name: "Google Calendar".into(), color: None, access: "owner".into(), primary: true, selected: true });
    }
    Ok(out)
}

/// The raw events of one calendar in [from, to), recurring series expanded into occurrences.
pub fn events<Tz: TimeZone>(o: &Opts, tz: &Tz, zone: &str, calendar_id: &str, from: i64, to: i64) -> Result<Vec<Value>, String>
where
    Tz::Offset: std::fmt::Display,
{
    let schema = composio::schema(o, LIST_EVENTS);
    let mut out = Vec::new();
    let mut page: Option<String> = None;
    for _ in 0..MAX_PAGES {
        let args = args_for(&schema, &[
            (CAL_ID, json!(calendar_id)),
            (&["timeMin", "time_min"], json!(rfc3339(tz, from))),
            (&["timeMax", "time_max"], json!(rfc3339(tz, to))),
            (&["singleEvents", "single_events"], json!(true)),
            (&["orderBy", "order_by"], json!("startTime")),
            (&["maxResults", "max_results"], json!(250)),
            (&["timeZone", "time_zone", "timezone"], if zone.is_empty() { Value::Null } else { json!(zone) }),
            (&["pageToken", "page_token"], page.clone().map(Value::from).unwrap_or(Value::Null)),
        ]);
        let data = composio::run(o, LIST_EVENTS, &args)?;
        out.extend(find_array(&data, &["items", "events"]).cloned().unwrap_or_default());
        page = find_str(&data, &["nextPageToken", "next_page_token"]).map(String::from);
        if page.is_none() {
            break;
        }
    }
    Ok(out)
}

/// What to write to an event.
pub struct Draft<'a> {
    pub title: &'a str,
    pub description: &'a str,
    pub start: i64,
    pub end: i64,
    pub zone: &'a str,
    pub rrule: Option<String>,
}

/// Whether the create tool can take a recurrence rule.
pub fn can_repeat(o: &Opts) -> bool {
    schema_has(&composio::schema(o, CREATE), &["recurrence"])
}

/// Creates an event; returns its Google id once Google has it.
pub fn create<Tz: TimeZone>(o: &Opts, tz: &Tz, calendar_id: &str, d: &Draft) -> Result<String, String> {
    let schema = composio::schema(o, CREATE);
    let minutes = ((d.end - d.start) / 60_000).max(1);
    // Some versions take a length (hours + minutes), others an end time.
    let by_length = schema_has(&schema, &["event_duration_hour", "event_duration_minutes"]) && !schema.is_null();
    let mut pairs: Vec<(&[&str], Value)> = vec![
        (CAL_ID, json!(calendar_id)),
        (&["summary", "title"], json!(d.title)),
        (&["description"], if d.description.is_empty() { Value::Null } else { json!(d.description) }),
        (&["start_datetime", "start_time", "startDateTime"], json!(naive_local(tz, d.start))),
        (TZ, if d.zone.is_empty() { Value::Null } else { json!(d.zone) }),
        (&["recurrence"], d.rrule.as_ref().map(|r| json!([r])).unwrap_or(Value::Null)),
    ];
    if by_length {
        pairs.push((&["event_duration_hour"], json!(minutes / 60)));
        pairs.push((&["event_duration_minutes"], json!(minutes % 60)));
    } else {
        pairs.push((&["end_datetime", "end_time", "endDateTime"], json!(naive_local(tz, d.end))));
    }
    let data = composio::run(o, CREATE, &args_for(&schema, &pairs))?;
    find_str(&data, &["id", "event_id", "eventId"]).map(String::from).ok_or_else(|| "Google didn't confirm the new event.".to_string())
}

/// Changes an event's title, description and time (only the fields given; other fields stay).
pub fn patch<Tz: TimeZone>(o: &Opts, tz: &Tz, calendar_id: &str, event_id: &str, d: &Draft) -> Result<(), String> {
    let schema = composio::schema(o, PATCH);
    let pairs: Vec<(&[&str], Value)> = vec![
        (CAL_ID, json!(calendar_id)),
        (EVENT_ID, json!(event_id)),
        (&["summary", "title"], json!(d.title)),
        (&["description"], json!(d.description)),
        (&["start_time", "start_datetime", "startDateTime"], json!(naive_local(tz, d.start))),
        (&["end_time", "end_datetime", "endDateTime"], json!(naive_local(tz, d.end))),
        (TZ, if d.zone.is_empty() { Value::Null } else { json!(d.zone) }),
    ];
    composio::run(o, PATCH, &args_for(&schema, &pairs)).map(|_| ())
}

/// Deletes an event (one occurrence, for an expanded recurring event's id).
pub fn delete(o: &Opts, calendar_id: &str, event_id: &str) -> Result<(), String> {
    let schema = composio::schema(o, DELETE);
    composio::run(o, DELETE, &args_for(&schema, &[(CAL_ID, json!(calendar_id)), (EVENT_ID, json!(event_id))])).map(|_| ())
}

/// A local repeat as Google's RRULE.
pub fn rrule(repeat: &str) -> Option<String> {
    Some(
        match repeat {
            "daily" => "RRULE:FREQ=DAILY",
            "weekdays" => "RRULE:FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR",
            "weekly" => "RRULE:FREQ=WEEKLY",
            "monthly" => "RRULE:FREQ=MONTHLY",
            _ => return None,
        }
        .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeats_become_rrules() {
        assert_eq!(rrule("weekdays").as_deref(), Some("RRULE:FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR"));
        assert_eq!(rrule(""), None);
    }
}
