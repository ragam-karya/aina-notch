// Badi's windows. Each one loads a v1 page from ui-dist (src/ui/*.html) with shim.js injected as
// window.badi. Overlays: one per display, transparent, always on top, click-through and kept out of
// screen captures, like v1's createOverlays().

use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::AppState;

const SHIM: &str = include_str!("shim.js");
// BADI_DEMO=mascot: the overlays also get the frame/memory probe (perf_probe.js, demo.rs).
const PERF_PROBE: &str = include_str!("perf_probe.js");

/// One display's overlay. `origin` is the display's top-left in the overlay coordinate space (logical
/// px, the space v1 called "global DIP"); pages subtract it from every position they receive.
#[derive(Clone, Debug)]
pub struct Overlay {
    pub label: String,
    pub pos: PhysicalPosition<i32>,
    pub size: PhysicalSize<u32>,
    pub scale: f64,
    pub origin: (f64, f64),
    pub primary: bool,
}

impl Overlay {
    pub fn contains(&self, p: PhysicalPosition<f64>) -> bool {
        p.x >= self.pos.x as f64 && p.y >= self.pos.y as f64 && p.x < (self.pos.x + self.size.width as i32) as f64 && p.y < (self.pos.y + self.size.height as i32) as f64
    }

    /// A physical desktop point → this page's CSS px.
    pub fn local(&self, p: PhysicalPosition<f64>) -> (f64, f64) {
        ((p.x - self.pos.x as f64) / self.scale, (p.y - self.pos.y as f64) / self.scale)
    }

    /// The display in the overlay coordinate space, as a logical rect.
    pub fn bounds(&self) -> (f64, f64, f64, f64) {
        (self.origin.0, self.origin.1, self.size.width as f64 / self.scale, self.size.height as f64 / self.scale)
    }
}

// Runs before the page's scripts: gives the page its v1 query string (overlay.html?x=&y=,
// agents.html?slug=) and window.badi.
pub fn init_script(query: &str) -> String {
    let query = serde_json::to_string(query).unwrap_or_else(|_| "\"\"".into());
    format!("(() => {{ const q = {query}; if (q && location.search !== q) history.replaceState(null, '', location.pathname + q); }})();\n{SHIM}")
}

pub fn page(name: &str) -> WebviewUrl {
    WebviewUrl::App(format!("src/ui/{name}").into())
}

/// WebView2's own data (cache, storage) lives in the profile's "webview" folder and is wiped on start
/// and quit, like v1's privacy.js did for Chromium (nothing a page stores outlives a session).
pub fn webview_dir() -> std::path::PathBuf {
    badi_store::data_dir().join("webview")
}

pub fn wipe_webview_data() {
    let _ = std::fs::remove_dir_all(webview_dir());
}

// WebView2's own background work that Badi never uses (component and "variations" downloads, Edge's
// pings and crash uploads), off so an idle Badi stays at 0% CPU. All windows share one environment, so
// they all pass the same arguments; the first three are wry's defaults, which this replaces.
const BROWSER_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --disable-background-networking --disable-component-update --disable-domain-reliability --no-pings --disable-breakpad";

/// Every Badi window starts here, so all of them share the one wiped data folder.
pub fn builder<'a>(app: &'a AppHandle, label: &str, url: WebviewUrl) -> WebviewWindowBuilder<'a, tauri::Wry, AppHandle> {
    WebviewWindowBuilder::new(app, label, url).data_directory(webview_dir()).additional_browser_args(BROWSER_ARGS)
}

// The overlays' memory target: low once Badi has been quiet for QUIET (WebView2 trims what it can
// rebuild: ~430 MB of working set became ~120 MB), normal again the moment something shows.
static MEM_LOW: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static MEM_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
const QUIET: std::time::Duration = std::time::Duration::from_secs(15);

/// Something shows (a turn, a panel, the notch hovered): the overlays get their normal memory back.
pub fn overlays_busy(app: &AppHandle) {
    use std::sync::atomic::Ordering;
    MEM_GEN.fetch_add(1, Ordering::SeqCst);
    if MEM_LOW.swap(false, Ordering::SeqCst) {
        overlay_memory(app, false);
    }
}

/// Badi went quiet: the overlays go low after QUIET, unless something showed meanwhile.
pub fn overlays_quiet(app: &AppHandle) {
    use std::sync::atomic::Ordering;
    let id = MEM_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(QUIET);
        if MEM_GEN.load(Ordering::SeqCst) == id && crate::turn::phase(&app) == "idle" && !MEM_LOW.swap(true, Ordering::SeqCst) {
            overlay_memory(&app, true);
        }
    });
}

/// Hides or shows an overlay window. Hiding the window alone leaves WebView2 thinking it is visible:
/// the page kept drawing at 60 fps (BADI_DEMO=mascot, hideshow), so the controller is told too. Then
/// the page gets visibilitychange, its frame loop and Rive stop, and WebView2 throttles its timers.
pub fn set_overlay_visible(app: &AppHandle, label: &str, visible: bool) {
    let Some(w) = app.get_webview_window(label) else { return };
    if !visible {
        let _ = w.hide();
    }
    let _ = w.with_webview(move |pw| unsafe {
        let _ = pw.controller().SetIsVisible(visible);
    });
    if visible {
        let _ = w.show();
    }
}

fn overlay_memory(app: &AppHandle, low: bool) {
    use webview2_com::Microsoft::Web::WebView2::Win32::*;
    use windows::core::Interface;
    let labels: Vec<String> = app.state::<AppState>().overlays.lock().unwrap().iter().map(|o| o.label.clone()).collect();
    for label in labels {
        let Some(w) = app.get_webview_window(&label) else { continue };
        let _ = w.with_webview(move |pw| unsafe {
            let Ok(core) = pw.controller().CoreWebView2() else { return };
            let Ok(core) = core.cast::<ICoreWebView2_19>() else { return };
            let level = if low { COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW } else { COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL };
            let _ = core.SetMemoryUsageTargetLevel(level);
        });
    }
}

/// The overlay of the display under the cursor (or the primary one).
pub fn overlay_at_cursor(app: &AppHandle) -> Option<Overlay> {
    let cursor = app.cursor_position().ok();
    let state = app.state::<AppState>();
    let overlays = state.overlays.lock().unwrap();
    cursor
        .and_then(|p| overlays.iter().find(|o| o.contains(p)))
        .or_else(|| overlays.iter().find(|o| o.primary))
        .or(overlays.first())
        .cloned()
}

/// Sends a positioned event (a visual, Badi's cursor) to one overlay only. With displays at
/// different scales the overlays' coordinate spaces can overlap, so positions are never broadcast.
pub fn emit_overlay<S: serde::Serialize + Clone>(app: &AppHandle, label: &str, channel: &str, payload: S) {
    use tauri::Emitter;
    let _ = app.emit_to(label, channel, payload);
}

// A signature of the connected displays, to notice one being plugged, unplugged or rescaled.
fn displays(app: &AppHandle) -> Vec<(i32, i32, u32, u32, u64)> {
    app.available_monitors()
        .map(|ms| ms.iter().map(|m| (m.position().x, m.position().y, m.size().width, m.size().height, (m.scale_factor() * 100.0) as u64)).collect())
        .unwrap_or_default()
}

/// Rebuilds the overlays when the displays change (v1 listened to screen display-added/removed/metrics).
pub fn watch_displays(app: AppHandle) {
    std::thread::spawn(move || {
        let mut last = displays(&app);
        loop {
            std::thread::sleep(std::time::Duration::from_secs(2));
            let now = displays(&app);
            if now != last && !now.is_empty() {
                last = now;
                let handle = app.clone();
                let _ = app.run_on_main_thread(move || {
                    let _ = create_overlays(&handle);
                });
            }
        }
    });
}

pub fn create_overlays(app: &AppHandle) -> tauri::Result<()> {
    let state = app.state::<AppState>();
    let mut overlays = state.overlays.lock().unwrap();
    for o in overlays.drain(..) {
        if let Some(w) = app.get_webview_window(&o.label) {
            let _ = w.destroy();
        }
    }
    let primary = app.primary_monitor()?.map(|m| *m.position());
    // Demos turn capture protection off so the overlay can be screenshotted (same as v1's BADI_DEMO=notch).
    let protect = std::env::var_os("BADI_DEMO").is_none();
    // destroy() finishes later, so a rebuild can't reuse the old labels: each build gets its own.
    static BUILD: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let build = BUILD.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let prefix = if build == 0 { "overlay".to_string() } else { format!("overlay-{build}") };
    for (i, m) in app.available_monitors()?.into_iter().enumerate() {
        let scale = m.scale_factor();
        let pos = *m.position();
        let size = *m.size();
        let o = Overlay {
            label: format!("{prefix}-{i}"),
            pos,
            size,
            scale,
            origin: (pos.x as f64 / scale, pos.y as f64 / scale),
            primary: primary == Some(pos),
        };
        let query = format!("?x={}&y={}", o.origin.0, o.origin.1);
        let mut script = init_script(&query);
        if std::env::var("BADI_DEMO").as_deref() == Ok("mascot") {
            script.push('\n');
            script.push_str(PERF_PROBE);
        }
        let win = builder(app, &o.label, page("overlay.html"))
            .title("aina")
            .transparent(true)
            .decorations(false)
            .resizable(false)
            .skip_taskbar(true)
            .always_on_top(true)
            .shadow(false)
            .focused(false)
            .focusable(false)
            .visible(false)
            .content_protected(protect)
            .initialization_script(script)
            // The overlay only ever shows Badi's own page.
            // (`npm run dev` serves it from Tauri's local dev server, so debug builds allow that too.)
            .on_navigation(|url| {
                url.host_str() == Some("tauri.localhost")
                    || url.scheme() == "tauri"
                    || (cfg!(debug_assertions) && url.scheme() == "http" && matches!(url.host_str(), Some("127.0.0.1" | "localhost")))
            })
            .build()?;
        win.set_position(pos)?;
        win.set_size(size)?;
        // Click-through; the notch asks for clicks while the cursor is on it (island:interactive).
        win.set_ignore_cursor_events(true)?;
        rebuild_on_crash(app, &win);
        reveal_when_ready(app, &o.label, false);
        overlays.push(o);
    }
    // New overlays start at the normal level.
    MEM_LOW.store(false, std::sync::atomic::Ordering::SeqCst);
    let a = app.clone();
    std::thread::spawn(move || overlays_quiet(&a));
    Ok(())
}

// When an overlay's page crashes or hangs, WebView2 puts its own opaque error page in the transparent,
// always-on-top, click-through window: a full-screen "Edge" page with no close button over every app.
// Tauri doesn't handle ProcessFailed, so the overlays are rebuilt here instead (at most every 5 s, so a
// page that keeps crashing can't loop). A GPU process exit is left alone: WebView2 recovers from it.
static LAST_REBUILD: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);

fn rebuild_on_crash(app: &AppHandle, win: &WebviewWindow) {
    use webview2_com::Microsoft::Web::WebView2::Win32::*;
    let (app, label) = (app.clone(), win.label().to_string());
    let _ = win.with_webview(move |pw| unsafe {
        let Ok(core) = pw.controller().CoreWebView2() else { return };
        let handler = webview2_com::ProcessFailedEventHandler::create(Box::new(move |_, args| {
            let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
            if let Some(args) = args {
                let _ = args.ProcessFailedKind(&mut kind);
            }
            if kind == COREWEBVIEW2_PROCESS_FAILED_KIND_GPU_PROCESS_EXITED {
                return Ok(());
            }
            {
                let mut last = LAST_REBUILD.lock().unwrap();
                if last.is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(5)) {
                    return Ok(());
                }
                *last = Some(std::time::Instant::now());
            }
            eprintln!("[windows] {label}: page process failed (kind {}), rebuilding the overlays", kind.0);
            // Not from inside the failing webview's own event: a moment later, on the main thread.
            let app = app.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(300));
                let handle = app.clone();
                let _ = app.run_on_main_thread(move || {
                    if let Err(e) = create_overlays(&handle) {
                        eprintln!("[windows] rebuilding the overlays failed: {e}");
                    }
                });
            });
            Ok(())
        }));
        let mut token = Default::default();
        let _ = core.add_ProcessFailed(&handler, &mut token);
    });
}

fn show(win: &WebviewWindow) {
    let _ = win.unminimize();
    let _ = win.show();
    let _ = win.set_focus();
}

// A new window stays hidden until its page has loaded and drawn (badi_ready), so WebView2's white
// background never flashes on screen; a page that never reports is shown anyway after a moment.
static WAITING: std::sync::Mutex<Vec<(String, bool)>> = std::sync::Mutex::new(Vec::new());
const READY_LIMIT: std::time::Duration = std::time::Duration::from_millis(2500);

pub fn reveal_when_ready(app: &AppHandle, label: &str, focus: bool) {
    WAITING.lock().unwrap().push((label.to_string(), focus));
    let (app, label) = (app.clone(), label.to_string());
    std::thread::spawn(move || {
        std::thread::sleep(READY_LIMIT);
        page_ready(&app, &label);
    });
}

/// A page is ready (badi_ready): its window shows now if it was waiting for that.
pub fn page_ready(app: &AppHandle, label: &str) {
    let waiting = {
        let mut w = WAITING.lock().unwrap();
        w.iter().position(|(l, _)| l == label).map(|i| w.remove(i))
    };
    let Some((_, focus)) = waiting else { return };
    // One more frame, so what shows is the painted page.
    std::thread::sleep(std::time::Duration::from_millis(40));
    if let Some(win) = app.get_webview_window(label) {
        if focus {
            show(&win);
        } else {
            let _ = win.show();
        }
    }
}

pub fn open_settings(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("settings") {
        return show(&w);
    }
    match builder(app, "settings", page("settings.html"))
        .title("aina · Settings")
        .inner_size(820.0, 740.0)
        .min_inner_size(480.0, 480.0)
        .visible(false)
        .initialization_script(init_script(""))
        .build()
    {
        Ok(_) => reveal_when_ready(app, "settings", true),
        Err(e) => eprintln!("[windows] settings: {e}"),
    }
}

pub fn open_agents(app: &AppHandle, slug: Option<&str>) {
    if let Some(w) = app.get_webview_window("agents") {
        show(&w);
        // Already open: switch it to that agent.
        if let Some(s) = slug {
            use tauri::Emitter;
            let _ = w.emit("agents:select", s);
        }
        return;
    }
    let query = slug.map(|s| format!("?slug={s}")).unwrap_or_default();
    if builder(app, "agents", page("agents.html"))
        .title("aina · Sprites")
        .inner_size(980.0, 680.0)
        .visible(false)
        .initialization_script(init_script(&query))
        .build()
        .is_ok()
    {
        reveal_when_ready(app, "agents", true);
    }
}

// The "type a question" bar: frameless, a quarter down the display under the cursor, as in v1. It
// closes when it loses focus (and when a question is sent).
pub fn open_ask(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("ask") {
        return show(&w);
    }
    let cursor = app.cursor_position().ok();
    let monitors = app.available_monitors().unwrap_or_default();
    let on_cursor = cursor.and_then(|p| {
        monitors.iter().find(|m| {
            let (pos, size) = (m.position(), m.size());
            p.x >= pos.x as f64 && p.y >= pos.y as f64 && p.x < (pos.x + size.width as i32) as f64 && p.y < (pos.y + size.height as i32) as f64
        })
    });
    let monitor = on_cursor.cloned().or_else(|| app.primary_monitor().ok().flatten());
    let (x, y) = monitor
        .map(|m| {
            let s = m.scale_factor();
            let (mx, my) = (m.position().x as f64 / s, m.position().y as f64 / s);
            let (mw, mh) = (m.size().width as f64 / s, m.size().height as f64 / s);
            (mx + mw / 2.0 - 260.0, my + mh * 0.25)
        })
        .unwrap_or((200.0, 200.0));
    let built = builder(app, "ask", page("ask.html"))
        .title("aina")
        .inner_size(520.0, 64.0)
        .position(x, y)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .initialization_script(init_script(""))
        .build();
    if let Ok(w) = built {
        let handle = w.clone();
        w.on_window_event(move |e| {
            if let tauri::WindowEvent::Focused(false) = e {
                let _ = handle.destroy();
            }
        });
        reveal_when_ready(app, "ask", true);
    }
}

/// Sends an event to every overlay, like v1's broadcast().
pub fn broadcast<S: serde::Serialize + Clone>(app: &AppHandle, channel: &str, payload: S) {
    use tauri::Emitter;
    let labels: Vec<String> = app.state::<AppState>().overlays.lock().unwrap().iter().map(|o| o.label.clone()).collect();
    for label in labels {
        let _ = app.emit_to(label.as_str(), channel, payload.clone());
    }
}
