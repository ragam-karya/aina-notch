// Weather and the shared place for the notch: the current weather from Open-Meteo (free, no key), city
// search through Open-Meteo's geocoding (Indonesia only), a place's name from BigDataCloud's free
// reverse geocoding (for a position from the laptop's location), and matching that place to a myQuran
// city so the prayer times can follow the same place. Kemenag's schedule is per regency/municipality,
// so a place also carries the one it lies in (`area`: Open-Meteo's admin2, BigDataCloud's level 5):
// Cikarang is "Kabupaten Bekasi", Purwokerto "Kabupaten Banyumas". Pure here: URLs, reading the
// responses, and the WMO weather codes.

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
    /// The regency or municipality it lies in, e.g. "Kabupaten Bekasi" (empty when unknown).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub area: String,
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

/// What people call a city that the geocoder knows by another name.
fn alias(name: &str) -> &str {
    match name.trim().to_lowercase().as_str() {
        "solo" => "Surakarta",
        "jogja" | "yogya" | "jogjakarta" => "Yogyakarta",
        _ => name,
    }
}

/// A city search, in Indonesia only (a bare "Bekasi" otherwise also finds Békasiki in Cameroon).
pub fn search_url(name: &str, lang: &str) -> String {
    format!(
        "https://geocoding-api.open-meteo.com/v1/search?name={}&count=6&language={}&format=json&countryCode=ID",
        enc(alias(name)),
        if lang == "en" { "en" } else { "id" }
    )
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
            let area = r["admin2"].as_str().unwrap_or("").to_string();
            Some(Place { name: r["name"].as_str()?.to_string(), lat: r["latitude"].as_f64()?, lon: r["longitude"].as_f64()?, region, area })
        })
        .collect()
}

/// A position's place name: the city, else the locality, else the region. Its regency/municipality is
/// the level-5 division, named "Kabupaten X" or "Kota X" from its description ("kabupaten di …",
/// "regency in …", "kota di …", "city in …").
pub fn parse_reverse(body: &Value, lat: f64, lon: f64) -> Place {
    let pick = |k: &str| body[k].as_str().filter(|s| !s.trim().is_empty()).map(String::from);
    let name = pick("city").or_else(|| pick("locality")).or_else(|| pick("principalSubdivision")).unwrap_or_default();
    let region = [pick("principalSubdivision"), pick("countryName")].into_iter().flatten().collect::<Vec<_>>().join(", ");
    let area = body["localityInfo"]["administrative"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|a| a["adminLevel"] == 5)
        .find_map(|a| {
            let n = a["name"].as_str()?.trim();
            let d = a["description"].as_str().unwrap_or("").to_lowercase();
            Some(if d.starts_with("kabupaten") || d.starts_with("regency") {
                format!("Kabupaten {n}")
            } else if d.starts_with("kota") || d.starts_with("city") {
                format!("Kota {n}")
            } else {
                n.to_string()
            })
        })
        .unwrap_or_default();
    Place { name, lat, lon, region, area }
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

/// "Kota X" -> (Some(true), "X"), "Kabupaten X" / "KAB. X" -> (Some(false), "X"), else (None, name), with
/// the name in capitals and only letters and digits, so "Bandarlampung" meets "BANDAR LAMPUNG".
fn kind_and_bare(name: &str) -> (Option<bool>, String) {
    let lower = name.trim().to_lowercase();
    let mut kind = None;
    let mut rest = lower.as_str();
    let prefixes = [
        ("kota administrasi ", Some(true)),
        ("kotamadya ", Some(true)),
        ("kodya ", Some(true)),
        ("kota ", Some(true)),
        ("kabupaten ", Some(false)),
        ("kab. ", Some(false)),
        ("kab ", Some(false)),
        ("daerah istimewa ", None),
        ("di ", None),
    ];
    for (prefix, k) in prefixes {
        if let Some(r) = rest.strip_prefix(prefix) {
            kind = k;
            rest = r;
            break;
        }
    }
    (kind, rest.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_uppercase())
}

/// Of myQuran's cities (all of them, from `prayer::all_url`), the one a place lies in: its
/// regency/municipality first, then its own name; an exact name before one that only starts with a
/// city's name ("Jakarta Pusat" is "KOTA JAKARTA"), the same kind (kota/kabupaten) before the other,
/// and a kota when the kind is unknown. None rather than a guess when nothing matches.
pub fn match_prayer_city(place: &Place, cities: &[(String, String)]) -> Option<(String, String)> {
    let cities: Vec<_> = cities.iter().map(|(id, n)| (id, n, kind_and_bare(n))).collect();
    let keys: Vec<_> = [place.area.as_str(), place.name.as_str()].into_iter().filter(|k| !k.trim().is_empty()).map(kind_and_bare).filter(|(_, b)| b.len() >= 3).collect();
    let pick = |want: Option<bool>, fits: &dyn Fn(&str) -> bool| {
        cities
            .iter()
            .filter(|(_, _, (_, b))| fits(b))
            .min_by_key(|(_, _, (k, b))| (*k != Some(want.unwrap_or(true)), std::cmp::Reverse(b.len())))
            .map(|(id, n, _)| ((*id).clone(), (*n).clone()))
    };
    keys.iter()
        .find_map(|(want, bare)| pick(*want, &|b| b == bare))
        .or_else(|| keys.iter().find_map(|(want, bare)| pick(*want, &|b| b.len() >= 5 && bare.starts_with(b))))
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
        let p = Place { name: "Bekasi".into(), lat: -6.2383, lon: 106.9756, ..Default::default() };
        assert!(forecast_url(&p).starts_with("https://api.open-meteo.com/v1/forecast?latitude=-6.2383&longitude=106.9756&current="));
        assert_eq!(search_url("Banda Aceh", "id"), "https://geocoding-api.open-meteo.com/v1/search?name=Banda%20Aceh&count=6&language=id&format=json&countryCode=ID");
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
        let all: Vec<(String, String)> = [
            ("1203", "KAB. BEKASI"),
            ("1221", "KOTA BEKASI"),
            ("1301", "KOTA JAKARTA"),
            ("1402", "KAB. BANYUMAS"),
            ("1014", "KOTA BANDAR LAMPUNG"),
            ("1505", "KOTA YOGYAKARTA"),
            ("1633", "KAB. MALANG"),
            ("1634", "KOTA MALANG"),
            ("0319", "KOTA SOLOK"),
            ("0507", "KOTA TANJUNG PINANG"),
        ]
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
        let at = |name: &str, area: &str| match_prayer_city(&Place { name: name.into(), area: area.into(), ..Default::default() }, &all).map(|c| c.1);
        // As Open-Meteo's geocoding names them (name, admin2).
        assert_eq!(at("Cikarang", "Kabupaten Bekasi").as_deref(), Some("KAB. BEKASI"));
        assert_eq!(at("Kota Bekasi", "Kotamadya Bekasi").as_deref(), Some("KOTA BEKASI"));
        assert_eq!(at("Kota Purwokerto", "Kabupaten Banyumas").as_deref(), Some("KAB. BANYUMAS"));
        assert_eq!(at("Bandarlampung", "Kotamadya Bandarlampung").as_deref(), Some("KOTA BANDAR LAMPUNG"));
        assert_eq!(at("DI Yogyakarta", "Kota Yogyakarta").as_deref(), Some("KOTA YOGYAKARTA"));
        assert_eq!(at("Kabupaten Malang", "Kotamadya Malang").as_deref(), Some("KOTA MALANG"));
        assert_eq!(at("Jakarta Pusat", "Kotamadya Jakarta Pusat").as_deref(), Some("KOTA JAKARTA"));
        assert_eq!(at("Jakarta", "").as_deref(), Some("KOTA JAKARTA"));
        assert_eq!(at("Bekasi", "Kodya Bekasi").as_deref(), Some("KOTA BEKASI"));
        assert_eq!(at("Tanjungpinang", "").as_deref(), Some("KOTA TANJUNG PINANG"));
        // A place whose kind is unknown: a kota first.
        assert_eq!(at("Bekasi", "").as_deref(), Some("KOTA BEKASI"));
        // Nothing alike: no guess.
        assert_eq!(at("Lampung", "Kabupaten Sanggau"), None);
        assert_eq!(at("", ""), None);
    }

    #[test]
    fn search_is_indonesian_and_knows_nicknames() {
        assert!(search_url("Solo", "id").contains("name=Surakarta&") && search_url("Solo", "id").ends_with("&countryCode=ID"));
        assert!(search_url("jogja", "en").contains("name=Yogyakarta&"));
        let found = parse_search(&json!({ "results": [{ "name": "Cikarang", "latitude": -6.26, "longitude": 107.15, "admin1": "Jawa Barat", "admin2": "Kabupaten Bekasi", "country": "Indonesia" }] }));
        assert_eq!(found[0].area, "Kabupaten Bekasi");
        let r = parse_reverse(&json!({ "city": "Cikarang", "localityInfo": { "administrative": [{ "adminLevel": 4, "name": "Jawa Barat" }, { "adminLevel": 5, "name": "Bekasi", "description": "kabupaten di Provinsi Jawa Barat" }] } }), -6.26, 107.15);
        assert_eq!(r.area, "Kabupaten Bekasi");
        let r = parse_reverse(&json!({ "city": "Malang", "localityInfo": { "administrative": [{ "adminLevel": 5, "name": "Malang", "description": "city in East Java" }] } }), 0.0, 0.0);
        assert_eq!(r.area, "Kota Malang");
    }
}
