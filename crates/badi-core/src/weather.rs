// Weather and the shared place for the notch: the current weather from Open-Meteo (free, no key), city
// search through Open-Meteo's geocoding, a place's name from BigDataCloud's free reverse geocoding (for
// a position from the laptop's location), and matching that place to a myQuran city so the prayer
// times can follow the same place. Pure here: URLs, reading the responses, and the WMO weather codes.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// A place: where the weather (and, when shared, the prayer times) are for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Place {
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub region: String,
}

fn enc(s: &str) -> String {
    s.trim().bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

pub fn forecast_url(p: &Place) -> String {
    format!(
        "https://api.open-meteo.com/v1/forecast?latitude={:.4}&longitude={:.4}&current=temperature_2m,apparent_temperature,weather_code,is_day&daily=temperature_2m_max,temperature_2m_min&timezone=auto&forecast_days=1",
        p.lat, p.lon
    )
}

pub fn search_url(name: &str, lang: &str) -> String {
    format!("https://geocoding-api.open-meteo.com/v1/search?name={}&count=6&language={}&format=json", enc(name), if lang == "en" { "en" } else { "id" })
}

pub fn reverse_url(lat: f64, lon: f64, lang: &str) -> String {
    format!("https://api.bigdatacloud.net/data/reverse-geocode-client?latitude={lat:.4}&longitude={lon:.4}&localityLanguage={}", if lang == "en" { "en" } else { "id" })
}

/// What a WMO weather code looks like: an icon kind and words for it.
pub fn describe(code: i64, lang: &str) -> (&'static str, &'static str) {
    let en = lang == "en";
    match code {
        0 => ("clear", if en { "Clear" } else { "Cerah" }),
        1 | 2 => ("partly", if en { "Partly cloudy" } else { "Cerah berawan" }),
        3 => ("cloudy", if en { "Cloudy" } else { "Berawan" }),
        45 | 48 => ("fog", if en { "Fog" } else { "Berkabut" }),
        51..=57 => ("drizzle", if en { "Drizzle" } else { "Gerimis" }),
        61..=67 | 80..=82 => ("rain", if en { "Rain" } else { "Hujan" }),
        71..=77 | 85 | 86 => ("snow", if en { "Snow" } else { "Salju" }),
        95..=99 => ("storm", if en { "Thunderstorm" } else { "Hujan petir" }),
        _ => ("cloudy", if en { "Cloudy" } else { "Berawan" }),
    }
}

/// The notch's weather line from a forecast response, or None when it has no current values.
pub fn parse_forecast(body: &Value, lang: &str, at: i64) -> Option<Value> {
    let c = &body["current"];
    let temp = c["temperature_2m"].as_f64()?;
    let code = c["weather_code"].as_i64().unwrap_or(3);
    let (kind, text) = describe(code, lang);
    Some(json!({
        "temp": temp.round() as i64,
        "feels": c["apparent_temperature"].as_f64().map(|v| v.round() as i64),
        "code": code,
        "kind": kind,
        "text": text,
        "day": c["is_day"] != 0,
        "max": body["daily"]["temperature_2m_max"][0].as_f64().map(|v| v.round() as i64),
        "min": body["daily"]["temperature_2m_min"][0].as_f64().map(|v| v.round() as i64),
        "at": at,
    }))
}

/// Places a city search found, best first.
pub fn parse_search(body: &Value) -> Vec<Place> {
    body["results"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| {
            let region = [r["admin1"].as_str(), r["country"].as_str()].into_iter().flatten().collect::<Vec<_>>().join(", ");
            Some(Place { name: r["name"].as_str()?.to_string(), lat: r["latitude"].as_f64()?, lon: r["longitude"].as_f64()?, region })
        })
        .collect()
}

/// A position's place name: the city, else the locality, else the region.
pub fn parse_reverse(body: &Value, lat: f64, lon: f64) -> Place {
    let pick = |k: &str| body[k].as_str().filter(|s| !s.trim().is_empty()).map(String::from);
    let name = pick("city").or_else(|| pick("locality")).or_else(|| pick("principalSubdivision")).unwrap_or_default();
    let region = [pick("principalSubdivision"), pick("countryName")].into_iter().flatten().collect::<Vec<_>>().join(", ");
    Place { name, lat, lon, region }
}

/// The word to look up on myQuran for a place: its name without "Kota"/"Kabupaten"/"Kab." in front.
pub fn prayer_keyword(name: &str) -> String {
    let lower = name.trim().to_lowercase();
    for prefix in ["kota administrasi ", "kotamadya ", "kabupaten ", "kota ", "kab. ", "kab "] {
        if let Some(rest) = lower.strip_prefix(prefix) {
            return rest.trim().to_string();
        }
    }
    lower
}

/// Of myQuran's matches, the one that is the place: the city ("KOTA X") before the regency
/// ("KAB. X") of the same name, an exact name before a partial one.
pub fn best_prayer_city(keyword: &str, found: &[(String, String)]) -> Option<(String, String)> {
    let k = keyword.trim().to_uppercase();
    let rank = |name: &str| {
        let n = name.to_uppercase();
        let bare = n.trim_start_matches("KOTA ").trim_start_matches("KAB. ").trim_start_matches("KAB ").to_string();
        match (bare == k, n.starts_with("KOTA ")) {
            (true, true) => 0,
            (true, false) => 1,
            (false, true) => 2,
            _ => 3,
        }
    };
    found.iter().min_by_key(|(_, n)| rank(n)).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_open_meteo_current_weather() {
        // As api.open-meteo.com answered for Bekasi.
        let body = json!({ "current": { "temperature_2m": 29.7, "apparent_temperature": 34.7, "weather_code": 51, "is_day": 1 }, "daily": { "temperature_2m_max": [31.9], "temperature_2m_min": [24.6] } });
        let w = parse_forecast(&body, "id", 5).unwrap();
        assert_eq!((w["temp"].as_i64(), w["kind"].as_str(), w["text"].as_str(), w["max"].as_i64(), w["min"].as_i64()), (Some(30), Some("drizzle"), Some("Gerimis"), Some(32), Some(25)));
        assert_eq!(w["day"], true);
        assert!(parse_forecast(&json!({ "error": true }), "id", 5).is_none());
        assert_eq!(describe(95, "en"), ("storm", "Thunderstorm"));
        assert_eq!(describe(0, "id").0, "clear");
    }

    #[test]
    fn urls_and_places() {
        let p = Place { name: "Bekasi".into(), lat: -6.2383, lon: 106.9756, region: String::new() };
        assert!(forecast_url(&p).starts_with("https://api.open-meteo.com/v1/forecast?latitude=-6.2383&longitude=106.9756&current="));
        assert_eq!(search_url("Banda Aceh", "id"), "https://geocoding-api.open-meteo.com/v1/search?name=Banda%20Aceh&count=6&language=id&format=json");
        let found = parse_search(&json!({ "results": [{ "name": "Kota Bekasi", "latitude": -6.2349, "longitude": 106.9896, "admin1": "Jawa Barat", "country": "Indonesia" }, { "name": "x" }] }));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].region, "Jawa Barat, Indonesia");
        let r = parse_reverse(&json!({ "city": "Bekasi", "locality": "Bekasi Barat", "principalSubdivision": "Jawa", "countryName": "Indonesia" }), -6.2, 106.9);
        assert_eq!((r.name.as_str(), r.region.as_str()), ("Bekasi", "Jawa, Indonesia"));
        assert_eq!(parse_reverse(&json!({ "city": "", "locality": "Cikarang" }), 0.0, 0.0).name, "Cikarang");
    }

    #[test]
    fn a_place_finds_its_myquran_city() {
        assert_eq!(prayer_keyword("Kota Bekasi"), "bekasi");
        assert_eq!(prayer_keyword("Kabupaten Bogor"), "bogor");
        assert_eq!(prayer_keyword("Bandung"), "bandung");
        let found = vec![("1203".to_string(), "KAB. BEKASI".to_string()), ("1221".to_string(), "KOTA BEKASI".to_string()), ("9".to_string(), "KOTA BEKASI TIMUR".to_string())];
        assert_eq!(best_prayer_city("bekasi", &found).unwrap().0, "1221");
        assert_eq!(best_prayer_city("bekasi", &found[..1]).unwrap().0, "1203");
        assert!(best_prayer_city("x", &[]).is_none());
    }
}
