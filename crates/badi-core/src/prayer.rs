// Prayer times for the Reminder Center from myQuran's free API (api.myquran.com, no key), which serves
// Kemenag's (Bimas Islam) schedule per city with all eight times: imsak, subuh, terbit, dhuha,
// dzuhur, ashar, maghrib, isya. One request per month for the chosen city, kept in config.json and
// read from there until the month or the city changes. Pure here: the URLs, reading the responses,
// the cache, and which time is next.

use std::collections::BTreeMap;

use chrono::{NaiveDate, NaiveTime, TimeZone};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Every time of the day, in order.
pub const KEYS: [&str; 8] = ["imsak", "subuh", "terbit", "dhuha", "dzuhur", "ashar", "maghrib", "isya"];
/// The prayers that can be ticked off (dhuha too, as a sunnah one); imsak and terbit are only markers.
pub const PRAYERS: [&str; 6] = ["subuh", "dhuha", "dzuhur", "ashar", "maghrib", "isya"];
/// The five obligatory ones: notified by default.
pub const FARD: [&str; 5] = ["subuh", "dzuhur", "ashar", "maghrib", "isya"];

const API: &str = "https://api.myquran.com/v2/sholat";

fn enc(s: &str) -> String {
    s.trim().bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// Finding a city (regency or municipality) by name.
pub fn search_url(keyword: &str) -> String {
    format!("{API}/kota/cari/{}", enc(keyword))
}

/// A month's schedule for a city id.
pub fn month_url(city_id: &str, year: i32, month: u32) -> String {
    format!("{API}/jadwal/{}/{year}/{month:02}", enc(city_id))
}

/// The cities a search found: (id, name), e.g. ("1221", "KOTA BEKASI").
pub fn parse_cities(body: &Value) -> Vec<(String, String)> {
    body["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| Some((c["id"].as_str().map(String::from).or(c["id"].as_u64().map(|n| n.to_string()))?, c["lokasi"].as_str()?.to_string())))
        .collect()
}

/// A month of times as kept in config.json (`prayerCache`): "YYYY-MM-DD" -> the eight "HH:MM".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Cache {
    pub city_id: String,
    pub month: String, // "YYYY-MM"
    pub days: BTreeMap<String, Vec<String>>,
    pub fetched: i64,
}

impl Cache {
    pub fn has(&self, city_id: &str, date: NaiveDate) -> bool {
        self.city_id == city_id && self.days.contains_key(&date.format("%Y-%m-%d").to_string())
    }

    /// Adds a fetched month, dropping days more than a month old so the cache stays small.
    pub fn add(&mut self, city_id: &str, month: &str, days: BTreeMap<String, Vec<String>>, today: NaiveDate, now: i64) {
        if self.city_id != city_id {
            self.days.clear();
        }
        self.city_id = city_id.to_string();
        self.month = month.to_string();
        self.fetched = now;
        self.days.extend(days);
        let oldest = (today - chrono::Days::new(31)).format("%Y-%m-%d").to_string();
        self.days.retain(|d, _| *d >= oldest);
    }
}

/// Reads a month: { status: true, data: { lokasi, jadwal: [{ date: "2026-10-29", imsak: "04:05", … }] } }.
/// A failed request ({ status: false, message }) is an Err with its message.
pub fn parse_month(body: &Value) -> Result<BTreeMap<String, Vec<String>>, String> {
    if body["status"] == false {
        return Err(body["message"].as_str().unwrap_or("Jadwal salat tidak ditemukan").to_string());
    }
    let mut out = BTreeMap::new();
    for d in body["data"]["jadwal"].as_array().into_iter().flatten() {
        let Some(date) = d["date"].as_str().and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()) else { continue };
        let times: Vec<String> = KEYS.iter().map(|k| d[*k].as_str().unwrap_or("").trim().to_string()).collect();
        if times.iter().all(|t| NaiveTime::parse_from_str(t, "%H:%M").is_ok()) {
            out.insert(date.format("%Y-%m-%d").to_string(), times);
        }
    }
    if out.is_empty() { Err("Jadwal salat kosong".into()) } else { Ok(out) }
}

/// One day's times as epoch ms in the PC's time zone, in KEYS order.
pub fn day<Tz: TimeZone>(tz: &Tz, cache: &Cache, date: NaiveDate) -> Option<Vec<(&'static str, i64)>> {
    let times = cache.days.get(&date.format("%Y-%m-%d").to_string())?;
    KEYS.iter()
        .zip(times)
        .map(|(k, t)| {
            let time = NaiveTime::parse_from_str(t, "%H:%M").ok()?;
            Some((*k, tz.from_local_datetime(&date.and_time(time)).earliest()?.timestamp_millis()))
        })
        .collect()
}

/// The next time after `now` and when the one before it was (the bar fills from that to the next).
/// Past isya the next is tomorrow's imsak, when known.
pub fn next(today: &[(&'static str, i64)], tomorrow_first: Option<i64>, now: i64) -> Option<(&'static str, i64, i64)> {
    let mut prev = today.last()?.1 - 86_400_000;
    for (k, at) in today {
        if *at > now {
            return Some((k, *at, prev));
        }
        prev = *at;
    }
    tomorrow_first.map(|t| (KEYS[0], t, prev))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;
    use serde_json::json;

    fn month() -> Value {
        // As api.myquran.com/v2/sholat/jadwal/1203/2026/10 answers (two days, one malformed).
        json!({ "status": true, "data": { "id": 1203, "lokasi": "KAB. BEKASI", "daerah": "JAWA BARAT", "jadwal": [
            { "tanggal": "Kamis, 29/10/2026", "imsak": "04:01", "subuh": "04:11", "terbit": "05:27", "dhuha": "05:54", "dzuhur": "11:41", "ashar": "15:00", "maghrib": "17:51", "isya": "19:02", "date": "2026-10-29" },
            { "tanggal": "Jumat, 30/10/2026", "imsak": "-", "date": "2026-10-30" }
        ] } })
    }

    #[test]
    fn urls_for_search_and_month() {
        assert_eq!(search_url("kota bekasi"), "https://api.myquran.com/v2/sholat/kota/cari/kota%20bekasi");
        assert_eq!(month_url("1221", 2026, 3), "https://api.myquran.com/v2/sholat/jadwal/1221/2026/03");
        let found = parse_cities(&json!({ "status": true, "data": [{ "id": "1203", "lokasi": "KAB. BEKASI" }, { "id": "1221", "lokasi": "KOTA BEKASI" }] }));
        assert_eq!(found[1], ("1221".into(), "KOTA BEKASI".into()));
        assert!(parse_cities(&json!({ "status": false, "message": "x" })).is_empty());
    }

    #[test]
    fn reads_all_eight_times_and_turns_a_day_into_moments() {
        let days = parse_month(&month()).unwrap();
        assert_eq!(days.len(), 1);
        assert_eq!(days["2026-10-29"], ["04:01", "04:11", "05:27", "05:54", "11:41", "15:00", "17:51", "19:02"]);
        let d = NaiveDate::from_ymd_opt(2026, 10, 29).unwrap();
        let mut cache = Cache::default();
        cache.add("1203", "2026-10", days, d, 7);
        assert!(cache.has("1203", d) && !cache.has("1221", d), "another city needs its own fetch");
        let wib = FixedOffset::east_opt(7 * 3600).unwrap();
        let times = day(&wib, &cache, d).unwrap();
        assert_eq!(times.len(), 8);
        assert_eq!(times[3], ("dhuha", wib.with_ymd_and_hms(2026, 10, 29, 5, 54, 0).unwrap().timestamp_millis()));
        assert!(parse_month(&json!({ "status": false, "message": "Kota tidak ditemukan" })).unwrap_err().contains("Kota"));
    }

    #[test]
    fn the_cache_forgets_other_cities_and_old_days() {
        let d = NaiveDate::from_ymd_opt(2026, 12, 15).unwrap();
        let mut cache = Cache::default();
        cache.add("1203", "2026-10", parse_month(&month()).unwrap(), d, 1);
        assert!(cache.days.is_empty(), "October is more than a month before 15 December");
        cache.add("1203", "2026-10", parse_month(&month()).unwrap(), NaiveDate::from_ymd_opt(2026, 10, 29).unwrap(), 1);
        cache.add("1221", "2026-10", BTreeMap::new(), NaiveDate::from_ymd_opt(2026, 10, 29).unwrap(), 2);
        assert!(cache.days.is_empty() && cache.city_id == "1221");
    }

    #[test]
    fn next_time_rolls_over_to_tomorrow() {
        let t = [("imsak", 5), ("subuh", 10), ("dzuhur", 20), ("isya", 50)];
        assert_eq!(next(&t, Some(105), 15), Some(("dzuhur", 20, 10)));
        assert_eq!(next(&t, Some(105), 1), Some(("imsak", 5, 50 - 86_400_000)));
        assert_eq!(next(&t, Some(105), 55), Some(("imsak", 105, 50)));
        assert_eq!(next(&t, None, 55), None);
    }
}
