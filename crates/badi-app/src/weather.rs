// The place Badi is in, and its weather, for the notch's left side. The place comes from the laptop's
// location (Windows' location service, by default) or a city the person picked in Settings; it is
// shared with the prayer times, whose city follows it unless the person picked one themselves.
// Weather from Open-Meteo every 30 minutes, kept in memory; the detected position is never saved,
// only a city the person picks (config.json `locationCity`).

use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use badi_core::weather::{self, Place};
use serde_json::{Map, Value, json};
use tauri::{AppHandle, Manager};

use crate::notch::{self, now_ms};
use crate::{AppState, i18n};

const REFRESH_MS: i64 = 30 * 60_000;
const RETRY_MS: i64 = 10 * 60_000;

#[derive(Default)]
struct Wx {
    place: Option<Place>,
    weather: Option<Value>,
    error: &'static str, // "" | "locationOff" | "noPlace" | "offline"
    busy: bool,
    tried: i64,
}

static WX: LazyLock<Mutex<Wx>> = LazyLock::new(|| Mutex::new(Wx::default()));

fn state(app: &AppHandle) -> tauri::State<'_, AppState> {
    app.state::<AppState>()
}

fn http() -> Option<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder().timeout(Duration::from_secs(15)).build().ok()
}

fn get_json(url: &str) -> Result<Value, String> {
    http().ok_or("no client")?.get(url).send().and_then(|r| r.json::<Value>()).map_err(|e| e.to_string())
}

fn mode(app: &AppHandle) -> String {
    state(app).store.get("locationMode").as_str().unwrap_or("auto").to_string()
}

fn manual_city(app: &AppHandle) -> Option<Place> {
    serde_json::from_value(state(app).store.get("locationCity")).ok().filter(|p: &Place| !p.name.is_empty())
}

// The prayer city as a place (when the laptop's location is unavailable): looked up by name.
fn place_from_prayer_city(app: &AppHandle) -> Option<Place> {
    let name = state(app).store.get("prayerCity")["name"].as_str()?.to_string();
    let found = get_json(&weather::search_url(&weather::prayer_keyword(&name), &i18n::lang(app))).ok()?;
    weather::parse_search(&found).into_iter().next()
}

/// Where Badi is: the picked city, or the laptop's position (named by reverse geocoding), or the prayer
/// city as a last resort. The error says what to do when none is known.
fn resolve(app: &AppHandle) -> Result<Place, &'static str> {
    if mode(app) == "city" {
        return manual_city(app).ok_or("noPlace");
    }
    match badi_apps::geo::position(Duration::from_secs(15)) {
        Ok((lat, lon)) => {
            let named = get_json(&weather::reverse_url(lat, lon, &i18n::lang(app))).map(|b| weather::parse_reverse(&b, lat, lon));
            Ok(named.unwrap_or(Place { name: String::new(), lat, lon, region: String::new() }))
        }
        Err(e) => {
            if std::env::var_os("BADI_LOG").is_some() {
                eprintln!("[weather] location: {e:?}");
            }
            place_from_prayer_city(app).ok_or(if e == badi_apps::geo::GeoError::Denied { "locationOff" } else { "noPlace" })
        }
    }
}

/// The prayer times follow the place, unless the person picked their city in Settings (no `auto`).
/// Shows the times only once the person has turned them on (the page's guide offers it).
fn share_with_prayer(app: &AppHandle, place: &Place) {
    let st = state(app);
    let current = st.store.get("prayerCity");
    let picked = current["id"].is_string() && current["auto"] != true;
    if picked || st.store.get("locationShare") == false || place.name.is_empty() {
        return;
    }
    let keyword = weather::prayer_keyword(&place.name);
    let Ok(body) = get_json(&badi_core::prayer::search_url(&keyword)) else { return };
    let Some((id, name)) = weather::best_prayer_city(&keyword, &badi_core::prayer::parse_cities(&body)) else { return };
    if current["id"].as_str() == Some(id.as_str()) {
        return;
    }
    let mut patch = Map::new();
    patch.insert("prayerCity".into(), json!({ "id": id, "name": name, "auto": true }));
    let _ = st.store.remember(&patch);
    crate::reminders::settings_changed(app);
}

/// Finds the place (when needed) and fetches its weather, in the background; at most every 30 minutes
/// unless `force`, and after a failure not again for 10 minutes.
pub fn refresh(app: &AppHandle, force: bool) {
    {
        let mut w = WX.lock().unwrap();
        let fresh = w.weather.as_ref().and_then(|v| v["at"].as_i64()).is_some_and(|at| now_ms() - at < REFRESH_MS);
        if w.busy || (!force && (fresh || now_ms() - w.tried < RETRY_MS)) {
            return;
        }
        w.busy = true;
        w.tried = now_ms();
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let lang = i18n::lang(&app);
        let result = resolve(&app).and_then(|place| {
            let body = get_json(&weather::forecast_url(&place)).map_err(|_| "offline")?;
            let wx = weather::parse_forecast(&body, &lang, now_ms()).ok_or("offline")?;
            Ok((place, wx))
        });
        let shared = {
            let mut w = WX.lock().unwrap();
            w.busy = false;
            match result {
                Ok((place, wx)) => {
                    w.place = Some(place.clone());
                    w.weather = Some(wx);
                    w.error = "";
                    Some(place)
                }
                Err(e) => {
                    w.error = e;
                    None
                }
            }
        };
        if let Some(place) = shared {
            share_with_prayer(&app, &place);
        }
        notch::send_status(&app);
    });
}

/// What the notch's left side shows: { temp, kind, text, day, max, min, feels, place } or { error }.
pub fn status() -> Value {
    let w = WX.lock().unwrap();
    let mut v = w.weather.clone().unwrap_or(json!({}));
    v["place"] = json!(w.place.as_ref().map(|p| p.name.clone()));
    v["error"] = json!(w.error);
    v
}

/// Settings changed the place: look again now.
pub fn settings_changed(app: &AppHandle) {
    {
        let mut w = WX.lock().unwrap();
        w.weather = None;
        w.tried = 0;
    }
    // A new place may mean a new prayer city, unless the person picked one.
    let current = state(app).store.get("prayerCity");
    if current["auto"] == true {
        let mut patch = Map::new();
        patch.insert("prayerCity".into(), Value::Null);
        let _ = state(app).store.remember(&patch);
    }
    refresh(app, true);
}

/// Settings' calls: the place now ("location:get"), a fresh detection ("location:detect") and a city
/// search ("location:search").
pub fn invoke(app: &AppHandle, ch: &str, arg: &Value) -> Result<Value, String> {
    match ch {
        "location:get" => {
            let w = WX.lock().unwrap();
            Ok(json!({ "mode": mode(app), "place": w.place, "error": w.error, "city": manual_city(app), "share": state(app).store.get("locationShare") != false }))
        }
        "location:detect" => match badi_apps::geo::position(Duration::from_secs(20)) {
            Ok((lat, lon)) => {
                let place = get_json(&weather::reverse_url(lat, lon, &i18n::lang(app))).map(|b| weather::parse_reverse(&b, lat, lon)).unwrap_or(Place { name: String::new(), lat, lon, region: String::new() });
                Ok(json!(place))
            }
            Err(badi_apps::geo::GeoError::Denied) => Err("locationOff".into()),
            Err(badi_apps::geo::GeoError::Timeout) => Err("timeout".into()),
            Err(badi_apps::geo::GeoError::Other(e)) => Err(e),
        },
        "location:search" => {
            let q = arg["q"].as_str().unwrap_or("").trim().to_string();
            if q.chars().count() < 2 {
                return Ok(json!([]));
            }
            let body = get_json(&weather::search_url(&q, &i18n::lang(app)))?;
            Ok(json!(weather::parse_search(&body)))
        }
        _ => Err(format!("unknown: {ch}")),
    }
}

/// Every 5 minutes: the weather when it is due (and the place, while none is known).
pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(3));
        loop {
            refresh(&app, false);
            std::thread::sleep(Duration::from_secs(5 * 60));
        }
    });
}

// BADI_DEMO: weather given, no request.
#[allow(dead_code)]
pub fn demo(app: &AppHandle, place: &str, temp: i64, code: i64) {
    {
        let mut w = WX.lock().unwrap();
        let (kind, text) = weather::describe(code, &i18n::lang(app));
        w.place = Some(Place { name: place.into(), lat: 0.0, lon: 0.0, region: String::new() });
        w.weather = Some(json!({ "temp": temp, "kind": kind, "text": text, "day": true, "max": temp + 2, "min": temp - 5, "feels": temp + 4, "code": code, "at": now_ms() }));
        w.error = "";
        w.tried = now_ms();
    }
    notch::send_status(app);
}
