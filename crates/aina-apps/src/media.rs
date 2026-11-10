// Now playing for the notch: Windows' current media session (Spotify, a browser tab, …) read and
// controlled through WinRT directly, in place of v1's BadiMedia.exe helper. Each call runs on a short-
// lived thread with a time limit, since a stuck WinRT call must never hold up the notch. Without a
// session, play/pause and skipping still work through the media keys. Port of src/media.js, plus what
// v2's music card adds: the track's cover and timeline, seeking, the master volume, change events
// (so the notch knows a song is on without being hovered), and synced lyrics from LRCLIB. Covers and
// lyrics are kept in memory only, for the current track.

use std::sync::Mutex;
use std::sync::mpsc::{Sender, channel};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

const LIMIT: Duration = Duration::from_secs(3);
// The biggest cover passed to the notch (Spotify's are ~30 KB; browsers' a little more).
const MAX_ART_BYTES: u32 = 512 * 1024;

/// The last thing seen playing (or paused): { title, artist, album, app, playing, key, pos, dur, at,
/// volume, muted }, or null. pos/dur are milliseconds (null when the app reports no timeline), at is
/// when pos was true (Unix ms), volume is the master volume 0..1.
pub static LAST: Mutex<Value> = Mutex::new(Value::Null);
/// The cover of one track: (track key, "data:image/…;base64,…" or null).
static ART: Mutex<(String, Value)> = Mutex::new((String::new(), Value::Null));

/// "Spotify.exe" / "SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify" / "Chrome" -> "Spotify" / "Chrome".
pub fn app_label(id: &str) -> String {
    let tail = id.rsplit('!').next().unwrap_or(id);
    let tail = tail.strip_suffix(".exe").or_else(|| tail.strip_suffix(".EXE")).unwrap_or(tail);
    tail.rsplit('.').next().filter(|s| !s.is_empty()).unwrap_or(id).to_string()
}

/// One track's identity across reads (title and artist).
pub fn track_key(title: &str, artist: &str) -> String {
    format!("{title}\u{1}{artist}")
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn timed<T: Send + 'static>(f: impl FnOnce() -> Option<T> + Send + 'static) -> Option<T> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(LIMIT).ok().flatten()
}

#[cfg(windows)]
type Session = windows::Media::Control::GlobalSystemMediaTransportControlsSession;
#[cfg(windows)]
type Manager = windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager;

#[cfg(windows)]
fn session() -> Option<Session> {
    Manager::RequestAsync().ok()?.join().ok()?.GetCurrentSession().ok()
}

// Windows' timeline is in 100 ns ticks; DateTime counts them from 1601.
#[cfg(windows)]
const FILETIME_UNIX_TICKS: i64 = 116_444_736_000_000_000;

/// Position and length in ms. While playing, the position is carried forward from when the app last
/// reported it (apps update it only now and then).
#[cfg(windows)]
fn timeline(s: &Session, playing: bool) -> (Option<i64>, Option<i64>) {
    let Ok(t) = s.GetTimelineProperties() else { return (None, None) };
    let ms = |r: windows::core::Result<windows::Foundation::TimeSpan>| r.map(|ts| ts.Duration / 10_000).unwrap_or(0);
    let start = ms(t.StartTime());
    let dur = ms(t.EndTime()) - start;
    if dur <= 0 {
        return (None, None);
    }
    let mut pos = ms(t.Position()) - start;
    if playing && let Ok(u) = t.LastUpdatedTime() && u.UniversalTime > 0 {
        let lag = now_ms() - (u.UniversalTime - FILETIME_UNIX_TICKS) / 10_000;
        if (0..dur).contains(&lag) {
            pos += lag;
        }
    }
    (Some(pos.clamp(0, dur)), Some(dur))
}

/// The current track, or null when nothing is playing or paused. Also kept in LAST.
pub fn now() -> Value {
    #[cfg(windows)]
    let found = timed(|| {
        use windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status;
        let s = session()?;
        let props = s.TryGetMediaPropertiesAsync().ok()?.join().ok()?;
        let title = props.Title().ok()?.to_string();
        if title.is_empty() {
            return None;
        }
        let artist = props.Artist().map(|a| a.to_string()).unwrap_or_default();
        let info = s.GetPlaybackInfo().ok();
        let playing = info.as_ref().and_then(|i| i.PlaybackStatus().ok()) == Some(Status::Playing);
        let app = s.SourceAppUserModelId().map(|a| a.to_string()).unwrap_or_default();
        let (pos, dur) = timeline(&s, playing);
        // What this player lets others do (not every app allows skipping, seeking, shuffle or repeat).
        let controls = info.as_ref().and_then(|i| i.Controls().ok());
        let can = |f: fn(&windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackControls) -> windows::core::Result<bool>| controls.as_ref().and_then(|c| f(c).ok()).unwrap_or(false);
        let caps = json!({
            "playPause": can(|c| c.IsPlayPauseToggleEnabled()),
            "next": can(|c| c.IsNextEnabled()),
            "previous": can(|c| c.IsPreviousEnabled()),
            "seek": can(|c| c.IsPlaybackPositionEnabled()) && dur.is_some(),
            "shuffle": can(|c| c.IsShuffleEnabled()),
            "repeat": can(|c| c.IsRepeatEnabled()),
        });
        let shuffle = info.as_ref().and_then(|i| i.IsShuffleActive().ok()).and_then(|r| r.Value().ok());
        let repeat = info.as_ref().and_then(|i| i.AutoRepeatMode().ok()).and_then(|r| r.Value().ok()).map(|m| match m.0 {
            1 => "track",
            2 => "list",
            _ => "none",
        });
        Some(json!({
            "title": title,
            "artist": artist,
            "album": props.AlbumTitle().map(|a| a.to_string()).unwrap_or_default(),
            "app": app_label(&app),
            "source": app,
            "playing": playing,
            "key": track_key(&title, &artist),
            "pos": pos,
            "dur": dur,
            "at": now_ms(),
            "caps": caps,
            "shuffle": shuffle,
            "repeat": repeat,
        }))
    });
    #[cfg(not(windows))]
    let found: Option<Value> = None;
    let mut v = found.unwrap_or(Value::Null);
    if let (Some(o), Some((level, muted))) = (v.as_object_mut(), volume()) {
        o.insert("volume".into(), json!((level * 100.0).round() / 100.0));
        o.insert("muted".into(), json!(muted));
    }
    *LAST.lock().unwrap() = v.clone();
    v
}

/// The cover of the track `key` as a data: URL, read from Windows once per track; null without one.
pub fn art(key: &str) -> Value {
    {
        let a = ART.lock().unwrap();
        if a.0 == key {
            return a.1.clone();
        }
    }
    #[cfg(windows)]
    let found = timed(|| {
        use base64::Engine;
        use windows::Storage::Streams::{Buffer, DataReader, InputStreamOptions};
        let props = session()?.TryGetMediaPropertiesAsync().ok()?.join().ok()?;
        let stream = props.Thumbnail().ok()?.OpenReadAsync().ok()?.join().ok()?;
        let size = u32::try_from(stream.Size().ok()?).ok().filter(|n| (1..=MAX_ART_BYTES).contains(n))?;
        let buf = stream.ReadAsync(&Buffer::Create(size).ok()?, size, InputStreamOptions::None).ok()?.join().ok()?;
        let mut bytes = vec![0u8; buf.Length().ok()? as usize];
        DataReader::FromBuffer(&buf).ok()?.ReadBytes(&mut bytes).ok()?;
        let mime = stream.ContentType().map(|c| c.to_string()).ok().filter(|c| c.starts_with("image/")).unwrap_or_else(|| sniff_image(&bytes).to_string());
        Some(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(&bytes)))
    });
    #[cfg(not(windows))]
    let found: Option<String> = None;
    let v = found.map(Value::String).unwrap_or(Value::Null);
    *ART.lock().unwrap() = (key.to_string(), v.clone());
    v
}

/// The image type from its first bytes (Windows sometimes reports no content type).
pub fn sniff_image(b: &[u8]) -> &'static str {
    match b {
        [0x89, b'P', b'N', b'G', ..] => "image/png",
        [b'G', b'I', b'F', ..] => "image/gif",
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => "image/webp",
        _ => "image/jpeg",
    }
}

/// play_pause | next | previous. Returns whether the media session took it (else press the media key).
pub fn control(action: &str) -> Result<bool, String> {
    if !["play_pause", "next", "previous"].contains(&action) {
        return Err(format!("Unknown media action \"{action}\"."));
    }
    #[cfg(windows)]
    {
        let a = action.to_string();
        Ok(timed(move || {
            let s = session()?;
            let op = match a.as_str() {
                "play_pause" => s.TryTogglePlayPauseAsync(),
                "next" => s.TrySkipNextAsync(),
                _ => s.TrySkipPreviousAsync(),
            };
            op.ok()?.join().ok()
        })
        .unwrap_or(false))
    }
    #[cfg(not(windows))]
    Ok(false)
}

/// Jumps to `ms` into the current track. False when the app doesn't allow seeking.
pub fn seek(ms: i64) -> bool {
    #[cfg(windows)]
    return timed(move || {
        let s = session()?;
        let start = s.GetTimelineProperties().ok()?.StartTime().ok()?.Duration;
        s.TryChangePlaybackPositionAsync(start + ms.max(0) * 10_000).ok()?.join().ok()
    })
    .unwrap_or(false);
    #[cfg(not(windows))]
    {
        let _ = ms;
        false
    }
}

/// Shuffle on or off. False when the player doesn't take it.
pub fn set_shuffle(on: bool) -> bool {
    #[cfg(windows)]
    return timed(move || session()?.TryChangeShuffleActiveAsync(on).ok()?.join().ok()).unwrap_or(false);
    #[cfg(not(windows))]
    {
        let _ = on;
        false
    }
}

/// The repeat mode: "none" | "list" | "track". False when the player doesn't take it.
pub fn set_repeat(mode: &str) -> bool {
    #[cfg(windows)]
    {
        use windows::Media::MediaPlaybackAutoRepeatMode as Mode;
        let m = match mode {
            "track" => Mode::Track,
            "list" => Mode::List,
            _ => Mode::None,
        };
        timed(move || session()?.TryChangeAutoRepeatModeAsync(m).ok()?.join().ok()).unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        let _ = mode;
        false
    }
}

/// Brings the player forward: a packaged app ("…!App") through shell:AppsFolder, a desktop one
/// ("Spotify.exe", "chrome.exe") by its main window. False when it can't be found.
pub fn open_source(source: &str) -> bool {
    if source.is_empty() {
        return false;
    }
    #[cfg(windows)]
    if !source.contains('!') && focus_app_window(source) {
        return true;
    }
    aina_agent::apps::shell_open(&format!("shell:AppsFolder\\{source}")).is_ok()
}

/// The first visible, unowned, titled window of the process `exe`, restored and brought forward.
#[cfg(windows)]
fn focus_app_window(exe: &str) -> bool {
    use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM};
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW};
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GW_OWNER, GetWindow, GetWindowTextLengthW, GetWindowThreadProcessId, IsIconic, IsWindowVisible, SW_RESTORE, SetForegroundWindow, ShowWindow};
    use windows::core::{BOOL, PWSTR};

    struct Find {
        exe: String,
        hwnd: Option<HWND>,
    }
    unsafe extern "system" fn each(h: HWND, l: LPARAM) -> BOOL {
        let find = unsafe { &mut *(l.0 as *mut Find) };
        let visible = unsafe { IsWindowVisible(h).as_bool() && GetWindowTextLengthW(h) > 0 && GetWindow(h, GW_OWNER).is_err() };
        if !visible {
            return true.into();
        }
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
        let Ok(p) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }) else { return true.into() };
        let mut buf = [0u16; 520];
        let mut len = buf.len() as u32;
        let named = unsafe { QueryFullProcessImageNameW(p, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len) }.is_ok();
        let _ = unsafe { CloseHandle(p) };
        let path = String::from_utf16_lossy(&buf[..len as usize]).to_lowercase();
        if named && path.rsplit(['\\', '/']).next() == Some(find.exe.as_str()) {
            find.hwnd = Some(h);
            return false.into();
        }
        true.into()
    }

    let mut exe = exe.to_lowercase();
    if !exe.ends_with(".exe") {
        exe.push_str(".exe");
    }
    let mut find = Find { exe, hwnd: None };
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut find as *mut Find as isize));
        let Some(h) = find.hwnd else { return false };
        if IsIconic(h).as_bool() {
            let _ = ShowWindow(h, SW_RESTORE);
        }
        SetForegroundWindow(h).as_bool()
    }
}

/// Mutes or unmutes the default output. Returns whether it is muted now.
pub fn toggle_mute() -> Option<bool> {
    #[cfg(windows)]
    return timed(|| {
        let v = endpoint().ok()?;
        unsafe {
            let muted = !v.GetMute().ok()?.as_bool();
            v.SetMute(muted, std::ptr::null()).ok()?;
            Some(muted)
        }
    });
    #[cfg(not(windows))]
    None
}

#[cfg(windows)]
fn endpoint() -> windows::core::Result<windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume> {
    use windows::Win32::Media::Audio::{IMMDeviceEnumerator, MMDeviceEnumerator, eConsole, eRender};
    use windows::Win32::System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx};
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let devices: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        devices.GetDefaultAudioEndpoint(eRender, eConsole)?.Activate(CLSCTX_ALL, None)
    }
}

/// The master volume of the default output (0..1) and whether it is muted.
pub fn volume() -> Option<(f32, bool)> {
    #[cfg(windows)]
    return timed(|| {
        let v = endpoint().ok()?;
        unsafe { Some((v.GetMasterVolumeLevelScalar().ok()?, v.GetMute().ok()?.as_bool())) }
    });
    #[cfg(not(windows))]
    None
}

/// Sets the master volume (0..1); a level above zero also unmutes.
pub fn set_volume(level: f32) -> bool {
    let level = level.clamp(0.0, 1.0);
    #[cfg(windows)]
    return timed(move || {
        let v = endpoint().ok()?;
        unsafe {
            v.SetMasterVolumeLevelScalar(level, std::ptr::null()).ok()?;
            if level > 0.0 {
                let _ = v.SetMute(false, std::ptr::null());
            }
        }
        Some(true)
    })
    .unwrap_or(false);
    #[cfg(not(windows))]
    {
        let _ = level;
        false
    }
}

// ---- change events ----

#[cfg(windows)]
struct Watch {
    manager: Manager,
    session: Option<(Session, i64, i64)>, // the session listened to, and its two event tokens
    tx: Mutex<Sender<()>>,
}
#[cfg(windows)]
static WATCH: Mutex<Option<Watch>> = Mutex::new(None);

/// Sends () on `tx` whenever Windows says the current session, its track or its play state changed.
/// False when the events can't be had (the caller polls instead).
pub fn watch(tx: Sender<()>) -> bool {
    #[cfg(windows)]
    return timed(move || {
        use windows::Foundation::TypedEventHandler;
        let manager = Manager::RequestAsync().ok()?.join().ok()?;
        let on_session = Mutex::new(tx.clone());
        manager
            .CurrentSessionChanged(&TypedEventHandler::new(move |_, _| {
                rebind();
                let _ = on_session.lock().unwrap().send(());
                Ok(())
            }))
            .ok()?;
        *WATCH.lock().unwrap() = Some(Watch { manager, session: None, tx: Mutex::new(tx) });
        rebind();
        Some(true)
    })
    .unwrap_or(false);
    #[cfg(not(windows))]
    {
        let _ = tx;
        false
    }
}

/// Moves the track and play-state listeners to the session that is current now.
#[cfg(windows)]
fn rebind() {
    use windows::Foundation::TypedEventHandler;
    let mut guard = WATCH.lock().unwrap();
    let Some(w) = guard.as_mut() else { return };
    if let Some((s, a, b)) = w.session.take() {
        let _ = s.RemoveMediaPropertiesChanged(a);
        let _ = s.RemovePlaybackInfoChanged(b);
    }
    let Ok(s) = w.manager.GetCurrentSession() else { return };
    let tx = w.tx.lock().unwrap().clone();
    let (t1, t2) = (Mutex::new(tx.clone()), Mutex::new(tx));
    let a = s.MediaPropertiesChanged(&TypedEventHandler::new(move |_, _| {
        let _ = t1.lock().unwrap().send(());
        Ok(())
    }));
    let b = s.PlaybackInfoChanged(&TypedEventHandler::new(move |_, _| {
        let _ = t2.lock().unwrap().send(());
        Ok(())
    }));
    if let (Ok(a), Ok(b)) = (a, b) {
        w.session = Some((s, a, b));
    }
}

/// The virtual key of a media action, for the key fallback.
pub fn media_key(action: &str) -> u16 {
    match action {
        "next" => 0xB0,     // VK_MEDIA_NEXT_TRACK
        "previous" => 0xB1, // VK_MEDIA_PREV_TRACK
        _ => 0xB3,          // VK_MEDIA_PLAY_PAUSE
    }
}

// ---- lyrics (LRCLIB) ----

const LRCLIB: &str = "https://lrclib.net/api";

/// Synced lyrics as (ms, line), from LRC text ("[01:02.34] line"; a line may carry several stamps).
pub fn parse_lrc(text: &str) -> Vec<(u64, String)> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let mut rest = raw.trim();
        let mut stamps = Vec::new();
        while let Some(body) = rest.strip_prefix('[') {
            let Some(end) = body.find(']') else { break };
            let (tag, after) = (&body[..end], &body[end + 1..]);
            let Some((m, s)) = tag.split_once(':') else { break };
            let (Ok(m), Ok(s)) = (m.trim().parse::<u64>(), s.trim().parse::<f64>()) else { break };
            stamps.push(m * 60_000 + (s * 1000.0).round() as u64);
            rest = after;
        }
        let line = rest.trim().to_string();
        out.extend(stamps.into_iter().map(|t| (t, line.clone())));
    }
    out.sort_by_key(|(t, _)| *t);
    out
}

/// What to search for: a video's "Artist - Song (Official Video)" title becomes artist and song, and
/// bracketed extras ("(Lyrics)", "[4K]", "ft. …") are dropped.
pub fn clean_query(title: &str, artist: &str) -> (String, String) {
    let mut t = title.to_string();
    for (open, close) in [('(', ')'), ('[', ']')] {
        while let (Some(a), Some(b)) = (t.find(open), t.find(close)) {
            if b < a {
                break;
            }
            t.replace_range(a..=b, "");
        }
    }
    if let Some(i) = t.to_lowercase().find(" ft.").or_else(|| t.to_lowercase().find(" feat.")) {
        t.truncate(i);
    }
    let mut artist = artist.trim().to_string();
    // A video channel ("Pizza Music", "… - Topic") is no artist; a "A - B" title names one.
    if let Some((a, b)) = t.split_once(" - ") {
        if artist.is_empty() || !t.to_lowercase().contains(&artist.to_lowercase()) || artist.ends_with("- Topic") {
            artist = a.trim().to_string();
        }
        t = b.to_string();
    }
    let artist = artist.trim_end_matches(" - Topic").trim().to_string();
    (t.split_whitespace().collect::<Vec<_>>().join(" "), artist)
}

/// Synced lyrics for a track from LRCLIB (free, no account): the exact match first, then a search.
/// None when there are none, or only unsynced ones. Sends only the title, artist, album and length.
pub fn lyrics(title: &str, artist: &str, album: &str, dur_ms: Option<i64>) -> Option<Vec<(u64, String)>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(8))
        .user_agent(concat!("aina/", env!("CARGO_PKG_VERSION"), " (Windows desktop assistant)"))
        .build()
        .ok()?;
    let synced = |v: &Value| v["syncedLyrics"].as_str().map(parse_lrc).filter(|l| !l.is_empty());
    let (track, who) = clean_query(title, artist);
    if track.is_empty() {
        return None;
    }
    let mut q = vec![("track_name", track.clone()), ("artist_name", who.clone())];
    if !album.is_empty() {
        q.push(("album_name", album.to_string()));
    }
    if let Some(d) = dur_ms.filter(|d| *d > 0) {
        q.push(("duration", (d / 1000).to_string()));
    }
    if let Ok(r) = client.get(format!("{LRCLIB}/get")).query(&q).send()
        && r.status().is_success()
        && let Some(l) = r.json::<Value>().ok().as_ref().and_then(synced)
    {
        return Some(l);
    }
    let found: Value = client.get(format!("{LRCLIB}/search")).query(&[("track_name", track), ("artist_name", who)]).send().ok()?.json().ok()?;
    found.as_array()?.iter().find_map(synced)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_labels() {
        assert_eq!(app_label("Spotify.exe"), "Spotify");
        assert_eq!(app_label("SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify"), "Spotify");
        assert_eq!(app_label("Chrome"), "Chrome");
        assert_eq!(app_label("Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic"), "ZuneMusic");
        assert!(control("louder").is_err());
    }

    #[test]
    fn lrc_lines() {
        let l = parse_lrc("[ar:Someone]\n[00:12.50] First line\n[01:02.00][02:03.10]Chorus\n[00:20.00]\nplain text");
        assert_eq!(l, vec![(12_500, "First line".into()), (20_000, String::new()), (62_000, "Chorus".into()), (123_100, "Chorus".into())]);
    }

    #[test]
    fn queries() {
        assert_eq!(clean_query("gnash - i hate u, i love u (Lyrics) ft. olivia o'brien", "Pizza Music"), ("i hate u, i love u".into(), "gnash".into()));
        assert_eq!(clean_query("Blinding Lights", "The Weeknd"), ("Blinding Lights".into(), "The Weeknd".into()));
        assert_eq!(clean_query("Tulus - Hati-Hati di Jalan [Official Music Video]", "Tulus"), ("Hati-Hati di Jalan".into(), "Tulus".into()));
        assert_eq!(clean_query("Hati-Hati di Jalan", "Tulus - Topic"), ("Hati-Hati di Jalan".into(), "Tulus".into()));
    }

    #[test]
    fn images() {
        assert_eq!(sniff_image(&[0x89, b'P', b'N', b'G', 0]), "image/png");
        assert_eq!(sniff_image(&[0xFF, 0xD8, 0xFF]), "image/jpeg");
        assert_eq!(track_key("a", "b"), "a\u{1}b");
    }
}

#[cfg(test)]
mod live {
    // cargo test -p aina-apps live -- --ignored --nocapture: what Windows says is playing now, and its lyrics.
    #[test]
    #[ignore]
    fn reads_now_playing() {
        let now = super::now();
        println!("now playing: {now}");
        println!("volume: {:?}", super::volume());
        if let Some(key) = now["key"].as_str() {
            println!("cover: {} chars", super::art(key).as_str().map(str::len).unwrap_or(0));
            let l = super::lyrics(now["title"].as_str().unwrap_or(""), now["artist"].as_str().unwrap_or(""), now["album"].as_str().unwrap_or(""), now["dur"].as_i64());
            println!("lyrics: {:?}", l.map(|l| l.into_iter().take(3).collect::<Vec<_>>()));
        }
    }

    // A known song through LRCLIB, with a video-style title.
    #[test]
    #[ignore]
    fn finds_lyrics() {
        let l = super::lyrics("gnash - i hate u, i love u (Lyrics) ft. olivia o'brien", "Pizza Music", "", None).expect("synced lyrics");
        println!("{} lines, first: {:?}", l.len(), &l[..3.min(l.len())]);
    }
}
