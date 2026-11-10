// Opening apps (from the Start menu list) and URLs/app links for the Brain's custom tools.
// Port of src/brain/apps.js.

use std::path::Path;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use regex::Regex;
use reqwest::Url;
use serde_json::Value;

use crate::files::hidden;

const CACHE: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Clone, PartialEq)]
pub struct App {
    pub name: String,
    pub id: String,
}

static APPS: Mutex<Option<(Instant, Vec<App>)>> = Mutex::new(None);

/// Start menu apps, the same list Windows search uses.
pub fn list_apps() -> Result<Vec<App>, String> {
    if let Some((at, apps)) = APPS.lock().unwrap().as_ref()
        && at.elapsed() < CACHE
        && !apps.is_empty()
    {
        return Ok(apps.clone());
    }
    let out = hidden("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", "[Console]::OutputEncoding=[Text.Encoding]::UTF8; Get-StartApps | ConvertTo-Json -Compress"])
        .output()
        .map_err(|e| format!("Could not list installed apps: {e}"))?;
    let parsed: Value = serde_json::from_slice(&out.stdout).map_err(|e| format!("Could not read the app list: {e}"))?;
    let items = match parsed {
        Value::Array(a) => a,
        v => vec![v],
    };
    let apps: Vec<App> = items
        .iter()
        .filter_map(|a| Some(App { name: a["Name"].as_str()?.to_string(), id: a["AppID"].as_str()?.to_string() }))
        .collect();
    *APPS.lock().unwrap() = Some((Instant::now(), apps.clone()));
    Ok(apps)
}

fn fold(s: &str) -> String {
    let kept: String = s.to_lowercase().chars().map(|c| if c.is_ascii_lowercase() || c.is_ascii_digit() || c == ' ' { c } else { ' ' }).collect();
    kept.split_whitespace().collect::<Vec<_>>().join(" ")
}

static NOT_THE_APP: LazyLock<Regex> = LazyLock::new(|| Regex::new("uninstall|readme|help|documentation|website").unwrap());

/// Ranks apps against what the model asked for.
pub fn match_apps(apps: &[App], query: &str) -> Vec<App> {
    let q = fold(query);
    if q.is_empty() {
        return vec![];
    }
    let words: Vec<&str> = q.split(' ').collect();
    let mut scored: Vec<(f64, &App)> = apps
        .iter()
        .map(|a| {
            let n = fold(&a.name);
            let mut score = if n == q {
                100.0
            } else if n.starts_with(&q) {
                80.0
            } else if n.split(' ').any(|w| w == q) {
                70.0
            } else if n.contains(&q) {
                60.0
            } else {
                let hits = words.iter().filter(|w| n.contains(*w)).count();
                40.0 * hits as f64 / words.len() as f64
            };
            // Prefer the app itself over its uninstaller, help file or website link.
            if NOT_THE_APP.is_match(&n) {
                score -= 30.0;
            }
            (score, a)
        })
        .filter(|(s, _)| *s > 0.0)
        .collect();
    scored.sort_by(|x, y| y.0.total_cmp(&x.0).then(x.1.name.len().cmp(&y.1.name.len())));
    scored.into_iter().map(|(_, a)| a.clone()).collect()
}

pub fn open_app(name: &str) -> Result<String, String> {
    if name.trim().is_empty() {
        return Err("open_app: \"name\" is required.".into());
    }
    let matches = match_apps(&list_apps()?, name);
    let Some(app) = matches.first() else {
        return Err(format!("No installed app matches \"{name}\". It may not be installed; try open_url with its website, or the Start menu."));
    };
    std::process::Command::new("explorer.exe").arg(format!("shell:AppsFolder\\{}", app.id)).spawn().map_err(|e| e.to_string())?;
    // Give the window a moment to appear before Claude looks again.
    std::thread::sleep(Duration::from_millis(1800));
    let others: Vec<&str> = matches.iter().skip(1).take(3).map(|a| a.name.as_str()).collect();
    let more = if others.is_empty() { String::new() } else { format!(" Other matches: {}.", others.join(", ")) };
    Ok(format!("Opened \"{}\".{more} Take a screenshot to see it.", app.name))
}

const URL_SCHEMES: [&str; 5] = ["http", "https", "spotify", "ms-settings", "mailto"];
// Local files open_url may show: pages, documents, images and text. Never anything that runs.
const VIEWABLE: [&str; 11] = ["html", "htm", "pdf", "png", "jpg", "jpeg", "gif", "svg", "webp", "txt", "md"];

static DRIVE_PATH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^[a-z]:[\\/]|^\\\\").unwrap());
static HAS_SCHEME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^[a-z][a-z0-9+.-]*:").unwrap());
static HOST_FIRST: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^[^:]+\.[a-z]{2,}(:\d+)?(/|$)").unwrap());

/// A local file the model wants shown (D:\site\index.html or file:///D:/site/index.html) as a file:
/// URL, or None when raw isn't a local path.
fn local_file_url(raw: &str, exists: &dyn Fn(&Path) -> bool) -> Result<Option<String>, String> {
    let file = if raw.to_lowercase().starts_with("file:") {
        Url::parse(raw).ok().and_then(|u| u.to_file_path().ok()).ok_or(format!("open_url: \"{raw}\" is not a valid file link."))?
    } else if DRIVE_PATH.is_match(raw) {
        Path::new(raw).to_path_buf()
    } else {
        return Ok(None);
    };
    let ext = file.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if !VIEWABLE.contains(&ext.as_str()) {
        let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        return Err(format!("open_url: opening \"{name}\" is not allowed; only local pages, documents, images and text files can be opened."));
    }
    if !exists(&file) {
        return Err(format!("open_url: {} does not exist.", file.display()));
    }
    Url::from_file_path(&file).map(|u| Some(u.to_string())).map_err(|_| format!("open_url: \"{raw}\" is not a valid file link."))
}

/// Parses a URL the model wants opened. Scheme-less input is treated as https.
pub fn safe_url(raw: &str) -> Result<String, String> {
    safe_url_with(raw, &|p| p.exists())
}

pub fn safe_url_with(raw: &str, exists: &dyn Fn(&Path) -> bool) -> Result<String, String> {
    let mut text = raw.trim().to_string();
    if text.is_empty() {
        return Err("open_url: \"url\" is required.".into());
    }
    if let Some(local) = local_file_url(&text, exists)? {
        return Ok(local);
    }
    if !HAS_SCHEME.is_match(&text) || HOST_FIRST.is_match(&text) {
        text = format!("https://{text}");
    }
    let url = Url::parse(&text).map_err(|_| format!("open_url: \"{raw}\" is not a valid URL."))?;
    if !URL_SCHEMES.contains(&url.scheme()) {
        return Err(format!("open_url: the \"{}:\" scheme is not allowed. Use http(s), spotify:, ms-settings: or mailto:.", url.scheme()));
    }
    Ok(url.to_string())
}

/// Opens a URL, app link or file with its default app (v1 shell.openExternal / openPath).
#[cfg(windows)]
pub fn shell_open(target: &str) -> Result<(), String> {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::{HSTRING, w};
    let target = visible_path(target);
    let r = unsafe { ShellExecuteW(None, w!("open"), &HSTRING::from(target.as_str()), None, None, SW_SHOWNORMAL) };
    // ShellExecute returns a value above 32 on success.
    if r.0 as isize > 32 { Ok(()) } else { Err(format!("Windows could not open {target}.")) }
}

/// Where other programs find a path. In the MSIX (Store) build, what the app writes under AppData goes
/// to the package's own copy (%LOCALAPPDATA%\Packages\<family>\LocalCache\Roaming or \Local), which
/// only the app sees at the usual place; Explorer and the default apps need the real place. Anything
/// else, and everything outside a package, stays as it is.
pub fn visible_path(target: &str) -> String {
    #[cfg(windows)]
    if let Some(family) = package_family() {
        let env = |k: &str| std::env::var(k).unwrap_or_default();
        if let Some(p) = packaged_copy(target, &family, &env("APPDATA"), &env("LOCALAPPDATA"), &|p: &std::path::Path| p.exists()) {
            return p;
        }
    }
    target.to_string()
}

/// The package's copy of `target` when it lies under AppData and that copy exists.
fn packaged_copy(target: &str, family: &str, appdata: &str, local: &str, exists: &dyn Fn(&std::path::Path) -> bool) -> Option<String> {
    // The rest of `target` after `base` (starting with a backslash), compared without case.
    let after = |base: &str| -> Option<String> {
        let base = base.trim_end_matches('\\');
        let head = target.get(..base.len())?;
        let rest = &target[base.len()..];
        (!base.is_empty() && head.eq_ignore_ascii_case(base) && rest.starts_with('\\') && rest.len() > 1).then(|| rest.to_string())
    };
    let local = local.trim_end_matches('\\');
    if after(&format!(r"{local}\Packages")).is_some() {
        return None; // already the real place
    }
    let cache = format!(r"{local}\Packages\{family}\LocalCache");
    let copy = after(appdata).map(|rest| format!(r"{cache}\Roaming{rest}")).or_else(|| after(local).map(|rest| format!(r"{cache}\Local{rest}")))?;
    exists(std::path::Path::new(&copy)).then_some(copy)
}

/// The package family name when running as an MSIX package.
#[cfg(windows)]
fn package_family() -> Option<String> {
    use std::sync::OnceLock;
    static FAMILY: OnceLock<Option<String>> = OnceLock::new();
    FAMILY
        .get_or_init(|| unsafe {
            use windows::Win32::Storage::Packaging::Appx::GetCurrentPackageFamilyName;
            use windows::core::PWSTR;
            let mut len = 0u32;
            let _ = GetCurrentPackageFamilyName(&mut len, None);
            if len == 0 {
                return None;
            }
            let mut buf = vec![0u16; len as usize];
            if GetCurrentPackageFamilyName(&mut len, Some(PWSTR(buf.as_mut_ptr()))).is_err() {
                return None;
            }
            Some(String::from_utf16_lossy(&buf[..(len as usize).saturating_sub(1)]))
        })
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packaged_files_under_appdata_open_at_the_package_copy() {
        let (fam, app, loc) = ("FaisalMashuri.Aina_abc123", r"C:\Users\x\AppData\Roaming", r"C:\Users\x\AppData\Local");
        let yes = |_: &std::path::Path| true;
        let no = |_: &std::path::Path| false;
        assert_eq!(
            packaged_copy(r"C:\Users\x\AppData\Roaming\aina\agents\general\output\a.md", fam, app, loc, &yes).as_deref(),
            Some(r"C:\Users\x\AppData\Local\Packages\FaisalMashuri.Aina_abc123\LocalCache\Roaming\aina\agents\general\output\a.md")
        );
        assert_eq!(
            packaged_copy(r"c:\users\x\appdata\local\Temp\r.txt", fam, app, loc, &yes).as_deref(),
            Some(r"C:\Users\x\AppData\Local\Packages\FaisalMashuri.Aina_abc123\LocalCache\Local\Temp\r.txt")
        );
        assert_eq!(packaged_copy(r"C:\Users\x\AppData\Roaming\aina\a.md", fam, app, loc, &no), None); // no copy: the real file
        assert_eq!(packaged_copy(r"C:\Users\x\Documents\aina\a.md", fam, app, loc, &yes), None);
        assert_eq!(packaged_copy(r"C:\Users\x\AppData\RoamingX\a", fam, app, loc, &yes), None);
        assert_eq!(packaged_copy(r"C:\Users\x\AppData\Local\Packages\Other\a", fam, app, loc, &yes), None);
        assert_eq!(packaged_copy("https://example.com", fam, app, loc, &yes), None);
    }

    fn app(name: &str, id: &str) -> App {
        App { name: name.into(), id: id.into() }
    }

    #[test]
    fn match_apps_prefers_the_real_app_over_its_uninstaller() {
        let apps = [app("Uninstall Spotify", "u"), app("Spotify", "SpotifyAB.SpotifyMusic!Spotify"), app("Excel", "x"), app("Spotify Website", "w")];
        assert_eq!(match_apps(&apps, "spotify")[0].id, "SpotifyAB.SpotifyMusic!Spotify");
        assert_eq!(match_apps(&apps, "excel")[0].id, "x");
        assert_eq!(match_apps(&apps, "Microsoft Excel")[0].id, "x");
        assert!(match_apps(&apps, "photoshop").is_empty());
    }

    #[test]
    fn safe_url_allows_web_and_app_links_only() {
        assert_eq!(safe_url("detik.com").unwrap(), "https://detik.com/");
        assert_eq!(safe_url("www.bi.go.id/id/statistik").unwrap(), "https://www.bi.go.id/id/statistik");
        assert_eq!(safe_url("https://example.com/a?b=1").unwrap(), "https://example.com/a?b=1");
        assert_eq!(safe_url("spotify:search:lofi").unwrap(), "spotify:search:lofi");
        assert_eq!(safe_url("ms-settings:colors").unwrap(), "ms-settings:colors");
        assert!(safe_url("file:///C:/Windows/System32").unwrap_err().contains("not allowed"));
        assert!(safe_url("javascript:alert(1)").unwrap_err().contains("not allowed"));
        assert!(safe_url("").unwrap_err().contains("required"));
    }

    #[test]
    fn local_pages_open_as_file_urls() {
        let yes = |_: &Path| true;
        assert_eq!(safe_url_with(r"D:\site\index.html", &yes).unwrap(), "file:///D:/site/index.html");
        assert!(safe_url_with(r"D:\site\run.exe", &yes).unwrap_err().contains("not allowed"));
        assert!(safe_url_with(r"D:\site\gone.html", &|_| false).unwrap_err().contains("does not exist"));
    }
}
