// aina Cloud in the app: signing in with Google in the browser, which hands the login back through a
// aina:// link (with a hash of this PC's id, for the free starter credits once per PC), the account and
// its credits in Settings and the notch, paying in the browser, signing out and deleting the account.
// The app keeps no API keys: every provider call goes to the cloud with the account token (aina-net,
// aina-store), realtime sessions included, every plan gets every model and voice (they cost credits),
// and aina says when credits run low. Signed out, aina asks to sign in instead of answering (`require`).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use aina_agent::apps::shell_open;
use reqwest::blocking::Client;
use serde_json::{Map, Value, json};
use tauri::{AppHandle, Emitter, Manager};

use crate::notch;
use crate::AppState;

// Set when building (AINA_CLOUD_URL=… npm run build), so an installed app needs no environment.
const DEFAULT_URL: &str = match option_env!("AINA_CLOUD_URL") {
    Some(u) => u,
    None => "https://be.aina.ai.id",
};
// Set when building (AINA_WEB_URL=…), like AINA_CLOUD_URL.
const DEFAULT_WEB_URL: &str = match option_env!("AINA_WEB_URL") {
    Some(u) => u,
    None => "https://aina.ai.id/api",
};
const POLL: Duration = Duration::from_secs(60);

/// The last account seen (plan, credits), and whether "credits are low" was said already.
static ACCOUNT: Mutex<Value> = Mutex::new(Value::Null);
static WARNED: Mutex<bool> = Mutex::new(false);
// Refused renewal charges already told (the first refresh after a start only remembers the count).
static RENEW_FAILURES: Mutex<i64> = Mutex::new(i64::MAX);
/// The sign-in this app started: its state and when. Only an aina:// link carrying it is accepted, so a
/// link from someone else can't sign this app into their account.
static PENDING: Mutex<Option<(String, std::time::Instant)>> = Mutex::new(None);
const PENDING_TTL: Duration = Duration::from_secs(15 * 60);

fn state(app: &AppHandle) -> tauri::State<'_, AppState> {
    app.state::<AppState>()
}

pub fn base(app: &AppHandle) -> String {
    custom_base(app).unwrap_or_else(|| DEFAULT_URL.into())
}

/// A cloud address set by hand (AINA_CLOUD_URL when running, or the cloudUrl setting), e.g. a dev Worker.
fn custom_base(app: &AppHandle) -> Option<String> {
    std::env::var("AINA_CLOUD_URL").ok().or_else(|| state(app).store.get("cloudUrl").as_str().filter(|s| !s.is_empty()).map(String::from))
}

/// Where the pages the browser opens for reading live (Ketentuan, Privasi): the same cloud through the
/// website's /api proxy, so the browser shows the website's address instead of the Worker's. The API
/// calls and signing in stay on base(). A cloud address set by hand is used for both.
pub fn web_base(app: &AppHandle) -> String {
    custom_base(app).unwrap_or_else(|| DEFAULT_WEB_URL.into())
}

fn token(app: &AppHandle) -> Option<String> {
    state(app).store.own_secret("ainaCloudToken")
}

pub fn signed_in(app: &AppHandle) -> bool {
    token(app).is_some()
}

/// aina answers only through aina Cloud (the app keeps no API keys). The notch is free without an
/// account; signed out, asking aina shows a panel with Masuk / Lihat paket, and the caller stops.
pub fn require(app: &AppHandle) -> bool {
    if !signed_in(app) {
        show_signin(app);
        return false;
    }
    // Credits gone (or less than one left): no paid call at all, not even listening to the question.
    if no_credits() {
        out_of_credits(app);
        return false;
    }
    true
}

/// The last account seen has no whole credit left (an unlimited account shows none of these).
fn no_credits() -> bool {
    let c = &account()["credits"];
    c["out"] == true || c["left"].as_i64() == Some(0)
}

/// Says the out-of-credits line (aina_llm::persona::out_line: free, kept on disk, talk::say_line), then
/// leaves the Beli kredit panel in the notch.
pub fn out_of_credits(app: &AppHandle) {
    let a = app.clone();
    std::thread::spawn(move || {
        let en = crate::i18n::lang(&a) == "en";
        crate::talk::say_line(&a, &aina_llm::persona::out_line(en));
        crate::turn::set_phase(&a, "idle");
        let line = if en { "Add credits to keep going now, or wait until your allowance renews." } else { "Tambah kredit untuk lanjut sekarang, atau tunggu jatahmu diperbarui." };
        credits_out(&a, line);
    });
}

/// The sign-in panel in the notch: Masuk / Lihat paket / Nanti.
pub fn show_signin(app: &AppHandle) {
    let en = crate::i18n::lang(app) == "en";
    let free = PLANS.lock().unwrap().as_ref().and_then(|(p, _)| p["free"]["credits"].as_i64()).unwrap_or(10);
    let (title, line) = if en {
        ("aina's AI needs an account".to_string(), format!("The notch is free. Sign in for {free} free credits."))
    } else {
        ("Tanya aina pakai AI butuh akun".to_string(), format!("Notch tetap gratis. Masuk, dapat {free} kredit coba."))
    };
    let actions = vec![
        notch::Action::new("signin:login", if en { "Sign in" } else { "Masuk" }, "primary", Some(Arc::new(|a: &AppHandle| {
            notch::clear_panel_kind(a, "signin");
            if let Err(e) = login(a) {
                eprintln!("[cloud] {e}");
            }
        }))),
        notch::Action::new("signin:plans", if en { "See plans" } else { "Lihat paket" }, "", Some(Arc::new(|a: &AppHandle| {
            notch::clear_panel_kind(a, "signin");
            notch::open_settings_at(a, "cloudSection");
        }))),
        notch::Action::new("signin:later", if en { "Later" } else { "Nanti" }, "", Some(Arc::new(|a: &AppHandle| notch::clear_panel_kind(a, "signin")))),
    ];
    notch::show_panel(app, "signin", &title, &[line], actions, 12_000, Map::new());
    // Warm the price list, so the next panel names the real free credits.
    let a = app.clone();
    std::thread::spawn(move || {
        plans(&a);
    });
}

/// A turn failed: when it was because the credits ran out (they ran out during it), aina says the
/// out-of-credits line; any other error shows as before.
pub fn turn_error(app: &AppHandle, e: impl Into<String>) {
    let e = e.into();
    if signed_in(app) && refresh(app).is_ok() && no_credits() {
        return out_of_credits(app);
    }
    crate::windows::broadcast(app, "error", e);
}

/// Credits ran out: the line, with a button to Settings → Akun.
fn credits_out(app: &AppHandle, message: &str) {
    let en = crate::i18n::lang(app) == "en";
    let buy = notch::Action::new("credits:buy", if en { "Buy credits" } else { "Beli kredit" }, "primary", Some(Arc::new(|a: &AppHandle| {
        notch::clear_panel_kind(a, "credits");
        notch::open_settings_at(a, "cloudSection");
    })));
    let later = notch::Action::new("credits:later", if en { "Later" } else { "Nanti" }, "", Some(Arc::new(|a: &AppHandle| notch::clear_panel_kind(a, "credits"))));
    let title = if en { "Out of credits" } else { "Kredit habis" };
    notch::show_panel(app, "credits", title, &[message.to_string()], vec![buy, later], 15_000, Map::new());
}

/// Starts signing in with Google in the browser; the aina:// link brings the login back.
pub fn login(app: &AppHandle) -> Result<(), String> {
    use std::hash::{BuildHasher, Hasher};
    let part = || std::collections::hash_map::RandomState::new().build_hasher().finish();
    let app_state = format!("{:016x}{:016x}{:016x}{:016x}", part(), part(), part(), part());
    *PENDING.lock().unwrap() = Some((app_state.clone(), std::time::Instant::now()));
    shell_open(&format!("{}/login?app_state={app_state}", base(app)))
}

fn http() -> Client {
    Client::builder().timeout(Duration::from_secs(20)).build().unwrap()
}

// One call to the cloud: the JSON answer, or the message it gave.
fn call(app: &AppHandle, method: &str, path: &str, body: Option<Value>) -> Result<Value, String> {
    let url = format!("{}{path}", base(app));
    let mut req = match method {
        "GET" => http().get(url),
        "DELETE" => http().delete(url),
        _ => http().post(url),
    };
    if let Some(t) = token(app) {
        req = req.header("x-api-key", t);
    }
    if let Some(b) = body {
        req = req.json(&b);
    }
    let res = req.send().map_err(|_| "aina Cloud belum bisa dihubungi. Cek koneksi internetmu.".to_string())?;
    let status = res.status();
    let v: Value = res.json().unwrap_or(Value::Null);
    if !status.is_success() {
        return Err(v["error"]["message"].as_str().map(String::from).unwrap_or_else(|| format!("aina Cloud {status}")));
    }
    Ok(v)
}

// The price list rarely changes: Settings asks for it on every reload, so it is fetched at most every PLANS_TTL.
static PLANS: Mutex<Option<(Value, std::time::Instant)>> = Mutex::new(None);
const PLANS_TTL: Duration = Duration::from_secs(10 * 60);

fn plans(app: &AppHandle) -> Value {
    if let Some((p, at)) = PLANS.lock().unwrap().as_ref()
        && at.elapsed() < PLANS_TTL
    {
        return p.clone();
    }
    match call(app, "GET", "/v1/plans", None) {
        Ok(p) => {
            *PLANS.lock().unwrap() = Some((p.clone(), std::time::Instant::now()));
            p
        }
        // Offline: the last list we had, if any.
        Err(_) => PLANS.lock().unwrap().as_ref().map(|(p, _)| p.clone()).unwrap_or(Value::Null),
    }
}

/// The cached account (null when signed out or not fetched yet).
pub fn account() -> Value {
    ACCOUNT.lock().unwrap().clone()
}

/// Credits for the notch's peek: { credits, low, plan }, or null.
pub fn notch_usage() -> Value {
    let a = account();
    if a.is_null() { Value::Null } else { json!({ "credits": a["credits"]["left"], "low": a["credits"]["low"], "plan": a["planName"] }) }
}

/// Fetches the account, keeps it, tells Settings and the notch, and says once when credits run low.
pub fn refresh(app: &AppHandle) -> Result<Value, String> {
    if !signed_in(app) {
        *ACCOUNT.lock().unwrap() = Value::Null;
        return Ok(Value::Null);
    }
    let a = call(app, "GET", "/v1/account", None)?;
    let changed = *ACCOUNT.lock().unwrap() != a;
    *ACCOUNT.lock().unwrap() = a.clone();
    // Only a real change reaches Settings: it reloads on cloud:changed by asking cloud:status, which
    // refreshes again, so telling it every time made it ask over and over.
    if !changed {
        return Ok(a);
    }
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.emit("cloud:changed", Value::Null);
    }
    notch::send_status(app);
    let c = &a["credits"];
    let low = c["low"] == true && c["out"] != true;
    let mut warned = WARNED.lock().unwrap();
    if low && !*warned {
        let line = if a["subscribed"] == true {
            format!("Kredit bulanan tinggal {}. Kalau habis sebelum siklus baru, Top-Up bisa dibeli di Settings → Akun.", c["left"])
        } else {
            format!("Kredit aina tinggal {}. Top-Up atau langganan ada di Settings → Akun.", c["left"])
        };
        notch::show_notice(app, "info", &line, &[], vec![], 6000);
    }
    *warned = low;
    drop(warned);
    // A renewal charge the card refused: said once per refusal.
    let failures = a["renewal"]["failures"].as_i64().unwrap_or(0);
    let mut seen = RENEW_FAILURES.lock().unwrap();
    if failures > *seen {
        let line = if a["renewal"]["stopped"] == true {
            "Kartumu menolak perpanjangan langganan aina, jadi perpanjangan otomatis berhenti. Pilih paket lagi di Settings → Akun.".to_string()
        } else {
            format!("Perpanjangan langganan aina ditolak kartumu ({failures}/{}). aina coba lagi besok; paketmu tetap aktif.", a["renewal"]["tries"])
        };
        notch::show_notice(app, "info", &line, &[], vec![], 8000);
    }
    *seen = failures;
    Ok(a)
}

/// At start: route through the cloud when signed in, and keep the account fresh.
pub fn start(app: &AppHandle) {
    if signed_in(app) {
        aina_net::set_cloud(Some(base(app)));
    }
    let app = app.clone();
    std::thread::spawn(move || {
        loop {
            if signed_in(&app)
                && let Err(e) = refresh(&app)
            {
                eprintln!("[cloud] {e}");
            }
            std::thread::sleep(POLL);
        }
    });
}

/// The model to call: every plan gets every model aina Cloud offers (a bigger one costs more credits).
pub fn model(_app: &AppHandle, configured: &str) -> String {
    configured.to_string()
}

/// On aina Cloud, the voice picked in Settings: "elevenlabs" (the default) or "openai" (fewer credits).
/// None when not on the cloud.
pub fn voice(app: &AppHandle) -> Option<String> {
    let tts = state(app).store.get("tts").as_str().unwrap_or("elevenlabs").to_string();
    signed_in(app).then(|| if tts == "openai" { "openai".into() } else { "elevenlabs".into() })
}

/// This PC's id for the free starter credits: a SHA-256 of Windows' MachineGuid, so the id itself never
/// leaves the PC.
#[cfg(windows)]
fn hwid() -> Option<String> {
    use ::windows::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RRF_SUBKEY_WOW6464KEY, RegGetValueW};
    use ::windows::core::w;
    use sha2::{Digest, Sha256};
    let mut buf = [0u16; 128];
    let mut size = (buf.len() * 2) as u32;
    let read = unsafe { RegGetValueW(HKEY_LOCAL_MACHINE, w!(r"SOFTWARE\Microsoft\Cryptography"), w!("MachineGuid"), RRF_RT_REG_SZ | RRF_SUBKEY_WOW6464KEY, None, Some(buf.as_mut_ptr().cast()), Some(&mut size)) };
    if read.is_err() {
        return None;
    }
    let guid = String::from_utf16_lossy(&buf[..(size as usize / 2).saturating_sub(1)]).trim().to_lowercase();
    if guid.is_empty() {
        return None;
    }
    Some(Sha256::digest(format!("aina-free:{guid}").as_bytes()).iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(not(windows))]
fn hwid() -> Option<String> {
    None
}

/// The app's link scheme.
pub const SCHEME: &str = "aina://";

/// An aina:// link: aina://login?code=…&state=… from the browser after Google. Trades the one-use code
/// for this PC's token, then shows the account.
pub fn handle_link(app: &AppHandle, link: &str) {
    let Some(rest) = link.strip_prefix(SCHEME) else { return };
    // aina://open (the "Buka aplikasi" button on aina Cloud's pages, e.g. after paying on the web): the
    // account as it is now, in Settings.
    if rest.trim_end_matches('/') == "open" {
        let _ = refresh(app);
        crate::windows::open_settings(app);
        return;
    }
    let Some(query) = rest.strip_prefix("login?").or_else(|| rest.strip_prefix("login/?")) else { return };
    let param = |k: &str| query.split('&').find_map(|kv| kv.strip_prefix(&format!("{k}="))).unwrap_or("").to_string();
    let (code, got_state) = (param("code"), param("state"));
    let expected = PENDING.lock().unwrap().take();
    let fresh = expected.as_ref().is_some_and(|(s, at)| crate::brain::same_text(s, &got_state) && at.elapsed() < PENDING_TTL);
    if !fresh {
        notch::show_notice(app, "info", "Link masuk ini bukan dari aina di PC ini. Masuk lagi dari Settings → Akun.", &[], vec![], 6000);
        return;
    }
    let device = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "PC".into());
    let result = call(app, "POST", "/v1/auth/exchange", Some(json!({ "code": code, "app_state": got_state, "device": device, "hwid": hwid().unwrap_or_default() }))).and_then(|r| {
        let token = r["token"].as_str().ok_or("aina Cloud gave no token")?.to_string();
        let mut patch = Map::new();
        patch.insert("ainaCloudToken".into(), json!(token));
        state(app).store.save(&patch)?;
        Ok(r)
    });
    match result {
        Ok(r) => {
            aina_net::set_cloud(Some(base(app)));
            aina_apps::composio::forget();
            let _ = refresh(app);
            crate::settings::prefetch(app); // the filler and wake replies, now that there's a voice
            let who = r["account"]["email"].as_str().unwrap_or("").to_string();
            let line = if r["account"]["plan"] == "beta" {
                format!("Halo {who}! {} kredit beta siap dipakai.", r["account"]["credits"]["left"])
            } else if r["freeCredits"].as_i64().unwrap_or(0) > 0 {
                format!("Halo {who}! {} kredit gratis sudah masuk.", r["freeCredits"])
            } else {
                format!("Masuk sebagai {who}.")
            };
            notch::show_notice(app, "info", &format!("✓ {line}"), &[], vec![], 6000);
            crate::windows::open_settings(app);
        }
        Err(e) => notch::show_notice(app, "info", &e, &[], vec![], 8000),
    }
}

/// aina:// opens this app: registered for the current user (no admin rights), pointing at this exe,
/// at every start so an update's new exe takes over. An MSIX package declares it in its manifest
/// instead (its HKCU writes stay inside the package).
#[cfg(windows)]
pub fn register_scheme() {
    if crate::package::packaged() {
        return;
    }
    use ::windows::Win32::System::Registry::{HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey, RegCreateKeyExW, RegSetValueExW};
    use ::windows::core::{HSTRING, PCWSTR};
    let Ok(exe) = std::env::current_exe() else { return };
    let set = |path: &str, name: Option<&str>, value: &str| unsafe {
        let mut key = HKEY::default();
        if RegCreateKeyExW(HKEY_CURRENT_USER, &HSTRING::from(path), None, None, REG_OPTION_NON_VOLATILE, KEY_WRITE, None, &mut key, None).is_err() {
            return;
        }
        let data: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes = std::slice::from_raw_parts(data.as_ptr() as *const u8, data.len() * 2);
        let name = name.map(HSTRING::from);
        let _ = RegSetValueExW(key, name.as_ref().map(|n| PCWSTR(n.as_ptr())).unwrap_or(PCWSTR::null()), None, REG_SZ, Some(bytes));
        let _ = RegCloseKey(key);
    };
    set(r"Software\Classes\aina", None, "URL:aina");
    set(r"Software\Classes\aina", Some("URL Protocol"), "");
    set(r"Software\Classes\aina\shell\open\command", None, &format!("\"{}\" \"%1\"", exe.display()));
}

fn signed_out(app: &AppHandle) {
    crate::realtime::stop(app);
    let _ = state(app).store.clear_key("ainaCloudToken");
    aina_net::set_cloud(None);
    aina_apps::composio::forget();
    *ACCOUNT.lock().unwrap() = Value::Null;
    notch::send_status(app);
}

pub fn invoke(app: &AppHandle, ch: &str, arg: &Value) -> Result<Value, String> {
    match ch {
        "cloud:status" => {
            let account = if signed_in(app) { refresh(app).unwrap_or_else(|_| account()) } else { Value::Null };
            let plans = plans(app);
            Ok(json!({ "account": if account.is_null() { Value::Null } else { account }, "plans": plans, "url": base(app), "cloudOnly": true }))
        }
        "cloud:login" => {
            login(app)?;
            Ok(Value::Null)
        }
        "cloud:logout" => {
            let _ = call(app, "POST", "/v1/auth/logout", None);
            signed_out(app);
            Ok(Value::Null)
        }
        "cloud:deleteAccount" => {
            call(app, "DELETE", "/v1/account", None)?;
            signed_out(app);
            Ok(Value::Null)
        }
        "cloud:checkout" => {
            let r = call(app, "POST", "/v1/checkout", Some(json!({ "item": arg.as_str().unwrap_or("") })))?;
            shell_open(r["url"].as_str().ok_or("no payment page")?)?;
            // The credits arrive once Midtrans says it's paid; look again in a moment.
            let a = app.clone();
            std::thread::spawn(move || {
                for _ in 0..20 {
                    std::thread::sleep(Duration::from_secs(15));
                    let _ = refresh(&a);
                }
            });
            Ok(Value::Null)
        }
        // Card subscriptions: stop or restart renewing, or forget the saved card.
        "cloud:cancelRenewal" | "cloud:resumeRenewal" | "cloud:dropCard" => {
            let (method, path) = match ch {
                "cloud:cancelRenewal" => ("POST", "/v1/subscription/cancel"),
                "cloud:resumeRenewal" => ("POST", "/v1/subscription/resume"),
                _ => ("DELETE", "/v1/subscription/card"),
            };
            call(app, method, path, None)?;
            refresh(app)
        }
        "cloud:open" => {
            let path = arg.as_str().unwrap_or("/");
            let path = if path.starts_with('/') { path.to_string() } else { "/".into() };
            shell_open(&format!("{}{path}", web_base(app))).map(|_| Value::Null)
        }
        _ => Err(format!("unknown channel: {ch}")),
    }
}
