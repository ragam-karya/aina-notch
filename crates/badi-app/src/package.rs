// Running as an MSIX package (the Microsoft Store build, scripts/build-msix.mjs). Windows then gives the
// app its identity and keeps its registry and AppData writes private to the package, so a few things
// go through the package instead: badi:// is declared in AppxManifest.xml (not written to HKCU),
// "Jalan saat Windows mulai" is the manifest's StartupTask (not the Run key), the taskbar identity is
// the package's own, and the browser extension is copied where a browser can read it.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The StartupTask's id in AppxManifest.xml.
const STARTUP_TASK: &str = "BadiStartup";

pub fn packaged() -> bool {
    static PACKAGED: OnceLock<bool> = OnceLock::new();
    *PACKAGED.get_or_init(|| {
        use ::windows::Win32::Foundation::APPMODEL_ERROR_NO_PACKAGE;
        use ::windows::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName;
        let mut len = 0u32;
        unsafe { GetCurrentPackageFullName(&mut len, None) != APPMODEL_ERROR_NO_PACKAGE }
    })
}

/// Turns the package's StartupTask on or off. Turning it on may be refused when the person switched it
/// off in Windows' Startup settings; only they can turn it back on there.
pub fn set_startup(on: bool) -> Result<(), String> {
    use ::windows::ApplicationModel::{StartupTask, StartupTaskState};
    let task = StartupTask::GetAsync(&STARTUP_TASK.into()).and_then(|op| op.join()).map_err(|e| e.to_string())?;
    if !on {
        return task.Disable().map_err(|e| e.to_string());
    }
    let state = task.RequestEnableAsync().and_then(|op| op.join()).map_err(|e| e.to_string())?;
    if state == StartupTaskState::Enabled || state == StartupTaskState::EnabledByPolicy {
        Ok(())
    } else {
        Err("Windows menolak menyalakannya. Nyalakan aina di Settings Windows → Apps → Startup.".into())
    }
}

/// The extension's folder for "Load unpacked": the package's own folder can't be opened by a browser,
/// so it is copied to Documents\Badi\extension (Documents is not kept private to the package).
pub fn shared_extension(src: &Path) -> Option<PathBuf> {
    let dest = shared_extension_dir();
    copy_dir(src, &dest).ok()?;
    Some(dest)
}

pub fn shared_extension_dir() -> PathBuf {
    badi_agent::files::app_documents().join("extension")
}

fn copy_dir(src: &Path, dest: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let to = dest.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &to)?;
        } else {
            std::fs::copy(entry.path(), to)?;
        }
    }
    Ok(())
}
