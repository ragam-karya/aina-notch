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

/// Cuts a "feat. …" / "ft …" / "featuring …" tail, with or without the dot, in any case.
fn strip_feat(s: &str) -> &str {
    let low = format!("{} ", s.to_ascii_lowercase());
    let cut = [" featuring ", " feat. ", " feat ", " ft. ", " ft "].iter().filter_map(|p| low.find(p)).min();
    match cut {
        Some(i) if i < s.len() => &s[..i],
        _ => s,
    }
}

/// Letters and digits only, lowercased: "Dewa 19" and "Dewa19" compare equal.
fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// A " - …" tail that names a version of the song, not the song: Spotify's "Song - Remastered 2011",
/// "- Live", "- Radio Edit", "- From \"Film\"". Only the tail's first word counts, and beyond
/// "Remastered" only for a music app (one that gives an album), so a video's "Artist - Live Forever"
/// stays a song.
fn is_version(tail: &str, music_app: bool) -> bool {
    let low = tail.trim().to_lowercase();
    let words: Vec<&str> = low.split_whitespace().collect();
    let Some(first) = words.first() else { return false };
    let year = first.len() == 4 && first.chars().all(|c| c.is_ascii_digit());
    if first.starts_with("remaster") || (year && words.get(1).is_some_and(|w| w.starts_with("remaster"))) {
        return true;
    }
    const WORDS: [&str; 20] = [
        "live", "remix", "edit", "radio", "acoustic", "akustik", "version", "versi", "mono", "stereo", "demo", "instrumental", "from", "unplugged", "single", "original",
        "extended", "bonus", "deluxe", "sped",
    ];
    music_app && WORDS.contains(first)
}

/// Bracketed extras out: "(Official Video)", "[4K]", "【MV】", and an unclosed "(…" to the end.
fn drop_brackets(t: &str) -> String {
    let mut t = t.to_string();
    for (open, close) in [('(', ')'), ('[', ']'), ('【', '】'), ('〔', '〕')] {
        while let Some(a) = t.find(open) {
            match t[a..].find(close) {
                Some(b) => t.replace_range(a..a + b + close.len_utf8(), " "),
                None => t.truncate(a),
            }
        }
    }
    t
}

/// One part of a title cleaned: "Lirik Lagu …" in front, "… Official Music Video" / "Lyrics" / "HD"
/// at the end, "feat …" tails and runs of spaces go.
fn tidy(part: &str) -> String {
    let mut s = strip_feat(part.trim()).trim().to_string();
    for p in ["lirik lagu ", "video lirik ", "lirik ", "lyrics ", "lyric "] {
        if s.len() > p.len() && s.to_ascii_lowercase().starts_with(p) {
            s = s[p.len()..].trim().to_string();
        }
    }
    const TAILS: [&str; 19] = [
        "official music video", "official lyric video", "official lyrics video", "official video", "official audio", "official mv", "music video", "lyric video",
        "lyrics video", "video lirik", "video klip", "video clip", "lirik", "lyrics", "lyric", "audio", "mv", "hd", "4k",
    ];
    loop {
        let low = s.to_ascii_lowercase();
        let Some(p) = TAILS.iter().find(|p| low.ends_with(&format!(" {p}"))) else { break };
        s.truncate(s.len() - p.len() - 1);
        s = s.trim_end_matches([' ', '-', '|', ',']).to_string();
    }
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// What to search for, as (song, artist). A video's "Artist - Song (Official Video)" title gives both
/// (a channel such as "Pizza Music" or "… - Topic" is no artist), and so does `Artist "Song"`; extras
/// are dropped: brackets, "| …", hashtags, "- YouTube", "Lirik", "feat …", a channel's "@" and "VEVO".
/// A music app's own title ("Song - Remastered 2011" by Queen, with an album) keeps its artist.
pub fn clean_query(title: &str, artist: &str, album: &str) -> (String, String) {
    let music_app = !album.trim().is_empty() && !artist.trim().is_empty();
    let mut t = title.trim().to_string();
    if t.to_ascii_lowercase().ends_with(" - youtube") {
        t.truncate(t.len() - " - youtube".len());
    }
    let mut t = drop_brackets(&t);
    if let Some(i) = t.find(" | ").or_else(|| t.find(" ｜ ")) {
        t.truncate(i);
    }
    let t = t.split_whitespace().filter(|w| !w.starts_with('#')).collect::<Vec<_>>().join(" ");
    let mut artist = artist.trim().trim_end_matches(" - Topic").to_string();
    let split = [" - ", " – ", " — "].iter().find_map(|d| t.split_once(d)).map(|(a, b)| (a.to_string(), b.to_string())).or_else(|| {
        // Artist "Song" / Artist 「Song」
        [('"', '"'), ('“', '”'), ('「', '」'), ('『', '』')].iter().find_map(|(o, c)| {
            let a = t.find(*o)?;
            let b = t[a + o.len_utf8()..].find(*c)? + a + o.len_utf8();
            Some((t[..a].to_string(), t[a + o.len_utf8()..b].to_string()))
        })
    });
    let mut song = t.clone();
    if let Some((a, b)) = split {
        if is_version(&b, music_app) {
            song = a;
        } else if !music_app {
            let a = tidy(&a);
            if !a.is_empty() && (artist.is_empty() || !norm(&t).contains(&norm(&artist))) {
                artist = a;
            }
            song = b;
        }
    }
    let artist = tidy(artist.trim_start_matches('@')).trim_end_matches("VEVO").trim().to_string();
    (tidy(&song), artist)
}

/// The artists in a credit, normalized: "Mahalini x Nuca" → ["mahalini", "nuca"].
fn artist_parts(who: &str) -> Vec<String> {
    let mut parts = vec![format!(" {} ", who.to_lowercase())];
    for sep in [",", ";", "&", "/", " x ", " and ", " dan ", " with ", " feat. ", " feat ", " ft. ", " ft "] {
        parts = parts.iter().flat_map(|p| p.split(sep).map(|s| format!(" {} ", s.trim())).collect::<Vec<_>>()).collect();
    }
    parts.iter().map(|p| norm(p)).filter(|p| p.len() >= 2).collect()
}

/// Whether an artist we have names one of a result's: one of ours is one of theirs, or starts it or
/// is started by it ("Dewa19" ~ "Dewa 19 & Ello", "Dewa 19 Official" ~ "Dewa 19", "Mahalini x Nuca"
/// ~ "Mahalini & Nuca"), but "TaylorSwift" is not "Tonight I'm Taylor Swift".
fn artist_fits(who: &str, theirs: &str) -> bool {
    if norm(who).is_empty() {
        return true;
    }
    let theirs = artist_parts(theirs);
    artist_parts(who).iter().any(|a| theirs.iter().any(|b| a == b || (a.len() >= 3 && b.len() >= 3 && (a.starts_with(b.as_str()) || b.starts_with(a.as_str())))))
}

/// Whether a result's name is the song's, once its extras are off: "Kau Yang Kusayang" is
/// "Kau Yang Kusayang (feat. Ello)" and "Kau yang Kusayang feat. Ello", but "Live" isn't "Live Forever".
fn song_fits(song: &str, theirs: &str) -> bool {
    let t = drop_brackets(theirs);
    let t = match [" - ", " – ", " — "].iter().find_map(|d| t.split_once(d)) {
        Some((a, b)) if is_version(b, true) => a.to_string(),
        _ => t.clone(),
    };
    let a = norm(song);
    !a.is_empty() && a == norm(strip_feat(&t))
}

/// The LRCLIB record picked for a track (with synced lyrics), for `lyrics` and the live tests.
/// Every result has to be the same song by the same artist (one as long as the track first), so a
/// wrong guess finds nothing rather than another song or a cover.
pub fn find_lyrics(title: &str, artist: &str, album: &str, dur_ms: Option<i64>) -> Option<Value> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(8))
        .user_agent(concat!("aina/", env!("CARGO_PKG_VERSION"), " (Windows desktop assistant)"))
        .build()
        .ok()?;
    let has_synced = |v: &Value| v["syncedLyrics"].as_str().is_some_and(|s| !parse_lrc(s).is_empty());
    let (track, who) = clean_query(title, artist, album);
    if track.is_empty() {
        return None;
    }
    let secs = dur_ms.filter(|d| *d > 0).map(|d| d as f64 / 1000.0);
    let near = |v: &Value| secs.zip(v["duration"].as_f64()).is_some_and(|(s, d)| (d - s).abs() <= 5.0);
    let named = |v: &Value, song: &str| song_fits(song, v["trackName"].as_str().unwrap_or(""));
    let by = |v: &Value, who: &str| artist_fits(who, v["artistName"].as_str().unwrap_or(""));

    let mut q = vec![("track_name", track.clone()), ("artist_name", who.clone())];
    if !album.is_empty() {
        q.push(("album_name", album.to_string()));
    }
    if let Some(s) = secs {
        q.push(("duration", (s.round() as i64).to_string()));
    }
    if let Ok(r) = client.get(format!("{LRCLIB}/get")).query(&q).send()
        && r.status().is_success()
        && let Ok(v) = r.json::<Value>()
        && has_synced(&v)
        && named(&v, &track)
    {
        return Some(v);
    }
    let search = |q: &[(&str, &str)]| -> Vec<Value> {
        client.get(format!("{LRCLIB}/search")).query(q).send().ok().and_then(|r| r.json::<Value>().ok()).and_then(|v| v.as_array().cloned()).unwrap_or_default()
    };
    // The same song by the same artist (a cover of the same length is another song), preferring one
    // as long as the track.
    let pick = |found: Vec<Value>, song: &str, who: &str| -> Option<Value> {
        let ok: Vec<Value> = found.into_iter().filter(|v| has_synced(v) && named(v, song) && by(v, who)).collect();
        // Best first: the artist named first ("Dewa 19 & Ello" for "Dewa19", not "Tonight I'm Taylor
        // Swift" for "TaylorSwift"), then the same length.
        let lead = |v: &Value| !who.is_empty() && norm(v["artistName"].as_str().unwrap_or("")).starts_with(&norm(who));
        let best = ok.iter().max_by_key(|v| (lead(v), near(v)))?;
        (near(best) || !(who.is_empty() && secs.is_some())).then(|| best.clone())
    };
    let attempt = |song: &str, who: &str| -> Option<Value> {
        pick(search(&[("track_name", song), ("artist_name", who)]), song, who)
            // The artist as a video spells it ("Dewa19", "TaylorSwiftVEVO") may not match LRCLIB's
            // search: the song alone, checked against the artist here.
            .or_else(|| pick(search(&[("q", song)]), song, who))
    };
    if let Some(v) = attempt(&track, &who) {
        return Some(v);
    }
    // "Song - Artist" the other way round (lyric channels do this), never for a music app's title.
    if album.trim().is_empty() && !who.is_empty() && norm(&who) != norm(&track) {
        return attempt(&who, &track);
    }
    None
}

/// Synced lyrics for a track from LRCLIB (free, no account), as (ms, line). None when there are none,
/// only unsynced ones, or no result is surely the same song. Sends only the title, artist, album and
/// length.
pub fn lyrics(title: &str, artist: &str, album: &str, dur_ms: Option<i64>) -> Option<Vec<(u64, String)>> {
    find_lyrics(title, artist, album, dur_ms).and_then(|v| v["syncedLyrics"].as_str().map(parse_lrc)).filter(|l| !l.is_empty())
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
        let q = |t: &str, a: &str, al: &str| clean_query(t, a, al);
        let p = |s: &str, a: &str| (s.to_string(), a.to_string());
        // Video titles (no album).
        assert_eq!(q("gnash - i hate u, i love u (Lyrics) ft. olivia o'brien", "Pizza Music", ""), p("i hate u, i love u", "gnash"));
        assert_eq!(q("Blinding Lights", "The Weeknd", ""), p("Blinding Lights", "The Weeknd"));
        assert_eq!(q("Tulus - Hati-Hati di Jalan [Official Music Video]", "Tulus", ""), p("Hati-Hati di Jalan", "Tulus"));
        assert_eq!(q("Hati-Hati di Jalan", "Tulus - Topic", ""), p("Hati-Hati di Jalan", "Tulus"));
        assert_eq!(q("@Dewa19 Feat Ello - Kau Yang Kusayang", "VIDEO LEGEND", ""), p("Kau Yang Kusayang", "Dewa19"));
        assert_eq!(q("Raisa - Kali Kedua ft Afgan", "", ""), p("Kali Kedua", "Raisa"));
        assert_eq!(q("Left", "Featherweight", ""), p("Left", "Featherweight"));
        assert_eq!(q("Dewa 19 - Kangen | Official Music Video", "Dewa 19 Official", ""), p("Kangen", "Dewa 19"));
        assert_eq!(q("Payung Teduh - Akad (Official Music Video) | HD", "Payung Teduh", ""), p("Akad", "Payung Teduh"));
        assert_eq!(q("Nadin Amizah - Bertaut | Live at Synchronize", "Nadin Amizah", ""), p("Bertaut", "Nadin Amizah"));
        assert_eq!(q("Tulus — Monokrom", "TULUS", ""), p("Monokrom", "TULUS"));
        assert_eq!(q("Dewa 19 \"Kangen\"", "Dewa 19 Official", ""), p("Kangen", "Dewa 19"));
        assert_eq!(q("Tulus - Hati-Hati di Jalan #tulus #lagu", "Tulus", ""), p("Hati-Hati di Jalan", "Tulus"));
        assert_eq!(q("Sheila On 7 - Dan... (Official Lyric Video", "Sheila On 7", ""), p("Dan...", "Sheila On 7"));
        assert_eq!(q("Bernadya - Satu Bulan Official Lyric Video", "Bernadya", ""), p("Satu Bulan", "Bernadya"));
        assert_eq!(q("Mahalini - Sial (Official Music Video) - YouTube", "", ""), p("Sial", "Mahalini"));
        assert_eq!(q("【MV】YOASOBI - アイドル", "Ayase / YOASOBI", ""), p("アイドル", "YOASOBI"));
        assert_eq!(q("Shake It Off", "TaylorSwiftVEVO", ""), p("Shake It Off", "TaylorSwift"));
        // Song and artist the other way round: cleaned as given; the lookup tries both ways.
        assert_eq!(q("Lirik Lagu Kangen - Dewa 19", "Lirik Channel", ""), p("Dewa 19", "Kangen"));
        // A video's "- Live …" is part of the song; only "Remastered" counts as a version there.
        assert_eq!(q("Oasis - Live Forever", "Rhino", ""), p("Live Forever", "Oasis"));
        assert_eq!(q("Dewa 19 - Kangen Live", "Dewa 19", ""), p("Kangen Live", "Dewa 19"));
        assert_eq!(q("Audio", "LSD", ""), p("Audio", "LSD"));
        // A music app's titles (with an album) keep their artist.
        assert_eq!(q("Bohemian Rhapsody - Remastered 2011", "Queen", "A Night at the Opera"), p("Bohemian Rhapsody", "Queen"));
        assert_eq!(q("Kangen - Live", "Dewa 19", "Live Album"), p("Kangen", "Dewa 19"));
        assert_eq!(q("Let It Go - From \"Frozen\"", "Idina Menzel", "Frozen"), p("Let It Go", "Idina Menzel"));
        assert_eq!(q("Pt. 2 - Interlude", "Someone", "Album"), p("Pt. 2 - Interlude", "Someone"));
    }

    #[test]
    fn same_song_only() {
        assert!(song_fits("Kau Yang Kusayang", "Kau Yang Kusayang (feat. Ello)"));
        assert!(song_fits("Kau Yang Kusayang", "Kau yang Kusayang feat. Ello"));
        assert!(song_fits("Bohemian Rhapsody", "Bohemian Rhapsody - Remastered 2011"));
        assert!(song_fits("Dan...", "Dan"));
        assert!(!song_fits("Live", "Live Forever"));
        assert!(!song_fits("Kau Yang Kusayang", "Kau Yang Sangat Kusayang"));
        assert!(artist_fits("Dewa19", "Dewa 19 & Ello"));
        assert!(artist_fits("Mahalini x Nuca", "Mahalini & Nuca"));
        assert!(artist_fits("Dewa 19 Official", "Dewa 19"));
        assert!(artist_fits("TaylorSwift", "Taylor Swift"));
        assert!(artist_fits("Rizky Febian & Mahalini", "Mahalini & Rizky Febian"));
        assert!(!artist_fits("Kangen", "Dewa 19"));
        assert!(!artist_fits("Lirik Channel", "Dewa 19"));
        assert!(!artist_fits("TaylorSwift", "Tonight i'm Taylor Swift"));
        assert!(!artist_fits("TaylorSwift", "Scott Bradlee & Postmodern Jukebox feat. Von Smith"));
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

    // Title shapes through LRCLIB: each must find the song named (by its artist), or nothing at all,
    // never another song. Prints what each one picked.
    #[test]
    #[ignore]
    fn finds_lyrics() {
        // (title, artist, album, length ms, the song and artist it must be; "" = nothing found is fine)
        type Case<'a> = (&'a str, &'a str, &'a str, Option<i64>, &'a str, &'a str);
        let cases: &[Case] = &[
            ("gnash - i hate u, i love u (Lyrics) ft. olivia o'brien", "Pizza Music", "", None, "i hate u, i love u", "gnash"),
            ("@Dewa19 Feat Ello - Kau Yang Kusayang", "VIDEO LEGEND", "", Some(226_000), "Kau Yang Kusayang", "Dewa"),
            ("Dewa 19 - Kangen | Official Music Video", "Dewa 19 Official", "", None, "Kangen", "Dewa"),
            ("Payung Teduh - Akad (Official Music Video) | HD", "Payung Teduh", "", None, "Akad", "Payung Teduh"),
            ("Tulus — Monokrom", "TULUS", "", None, "Monokrom", "Tulus"),
            ("Dewa 19 \"Kangen\"", "Dewa 19 Official", "", None, "Kangen", "Dewa"),
            ("Lirik Lagu Kangen - Dewa 19", "Lirik Channel", "", None, "Kangen", "Dewa"),
            ("Kangen - Dewa 19 (Lirik)", "Lirik Channel", "", None, "Kangen", "Dewa"),
            ("Tulus - Hati-Hati di Jalan #tulus #lagu", "Tulus", "", None, "Hati-Hati di Jalan", "Tulus"),
            ("Sheila On 7 - Dan... (Official Lyric Video", "Sheila On 7", "", None, "Dan", "Sheila"),
            ("Bernadya - Satu Bulan Official Lyric Video", "Bernadya", "", None, "Satu Bulan", "Bernadya"),
            ("Mahalini - Sial (Official Music Video) - YouTube", "", "", None, "Sial", "Mahalini"),
            ("Mahalini x Nuca - Aku Yang Salah", "Mahalini", "", None, "Aku Yang Salah", "Mahalini"),
            // LRCLIB's search for the song alone shows only covers: never one of them.
            ("Shake It Off", "TaylorSwiftVEVO", "", Some(242_000), "Shake It Off", "Taylor Swift"),
            ("Nadin Amizah - Bertaut | Live at Synchronize", "Nadin Amizah", "", None, "Bertaut", "Nadin"),
            ("Bohemian Rhapsody - Remastered 2011", "Queen", "A Night at the Opera", Some(354_000), "Bohemian Rhapsody", "Queen"),
            ("Kangen - Live", "Dewa 19", "Live Album", None, "Kangen", "Dewa"),
            ("Kau Yang Kusayang", "Dewa 19, Ello", "Best Of", None, "Kau Yang Kusayang", "Dewa"),
        ];
        let mut wrong = Vec::new();
        for (title, artist, album, dur, song, by) in cases {
            let v = super::find_lyrics(title, artist, album, *dur);
            let (got_song, got_by) = v.as_ref().map(|v| (v["trackName"].as_str().unwrap_or(""), v["artistName"].as_str().unwrap_or(""))).unwrap_or(("", ""));
            println!("{title:<52} => {}", if v.is_some() { format!("{got_by} — {got_song}") } else { "(nothing)".into() });
            if v.is_some() && !(super::song_fits(song, got_song) && super::norm(got_by).starts_with(&super::norm(by))) {
                wrong.push(format!("{title}: got {got_by} — {got_song}"));
            }
            if v.is_none() && !song.is_empty() {
                wrong.push(format!("{title}: nothing found"));
            }
        }
        assert!(wrong.is_empty(), "{wrong:#?}");
    }
}
