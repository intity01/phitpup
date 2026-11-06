//! The Microsoft Store build. The same phitpup.exe runs from the installer,
//! as a single file, or inside the Store's MSIX package; this tells which,
//! and does what the package needs done differently:
//!
//! - updates come from the Store, so there is no update helper or reminder;
//! - the settings live in the package's own folder (`LocalState`), so they
//!   are removed with the app and Notepad can open them (MSIX would
//!   otherwise redirect writes to `%APPDATA%` where only PhitPup sees them);
//! - "Start with Windows" is the package's `StartupTask`, not the Run key.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use windows::ApplicationModel::{StartupTask, StartupTaskState};
use windows::core::HSTRING;
use windows_sys::Win32::Foundation::ERROR_INSUFFICIENT_BUFFER;
use windows_sys::Win32::Storage::Packaging::Appx::GetCurrentPackageFamilyName;
use windows_sys::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};

/// The StartupTask's id in `msix/AppxManifest.xml`.
const TASK: &str = "PhitPupStartup";

/// The Store page; `ms-windows-store://pdp/?ProductId=` once it has an id.
pub const STORE_UPDATES: &str = "ms-windows-store://downloadsandupdates";

/// The package family name, if PhitPup runs from its MSIX package.
pub fn family_name() -> Option<&'static str> {
    static NAME: OnceLock<Option<String>> = OnceLock::new();
    NAME.get_or_init(|| {
        let mut len = 0u32;
        // SAFETY: a size query with no buffer, then a call with one that big.
        unsafe {
            if GetCurrentPackageFamilyName(&mut len, std::ptr::null_mut())
                != ERROR_INSUFFICIENT_BUFFER
            {
                return None; // APPMODEL_ERROR_NO_PACKAGE: not packaged
            }
            let mut buf = vec![0u16; len as usize];
            if GetCurrentPackageFamilyName(&mut len, buf.as_mut_ptr()) != 0 {
                return None;
            }
            buf.truncate(len.saturating_sub(1) as usize);
            Some(String::from_utf16_lossy(&buf))
        }
    })
    .as_deref()
}

pub fn packaged() -> bool {
    family_name().is_some()
}

/// `%LOCALAPPDATA%\Packages\<family>\LocalState`, when packaged.
pub fn data_dir() -> Option<PathBuf> {
    let family = family_name()?;
    let local = std::env::var_os("LOCALAPPDATA")?;
    Some(
        PathBuf::from(local)
            .join("Packages")
            .join(family)
            .join("LocalState"),
    )
}

/// Runs a WinRT call on its own thread in the multithreaded apartment, so
/// waiting on it never blocks the window's message loop's apartment.
fn on_mta<T: Send + 'static>(
    f: impl FnOnce() -> windows::core::Result<T> + Send + 'static,
) -> Option<T> {
    std::thread::spawn(move || {
        // SAFETY: paired with CoUninitialize on this thread.
        let ok = unsafe { CoInitializeEx(std::ptr::null(), COINIT_MULTITHREADED as u32) } >= 0;
        let r = f().ok();
        if ok {
            // SAFETY: balances the successful CoInitializeEx above.
            unsafe { CoUninitialize() };
        }
        r
    })
    .join()
    .ok()
    .flatten()
}

fn task() -> windows::core::Result<StartupTask> {
    StartupTask::GetAsync(&HSTRING::from(TASK))?.join()
}

/// The last StartupTask answer and when it was asked. The settings window
/// asks on every paint, so it is kept for a moment rather than asking WinRT
/// each time.
static STARTUP: Mutex<Option<(Instant, bool)>> = Mutex::new(None);

/// Whether the StartupTask is on.
pub fn starts_with_windows() -> bool {
    let mut cache = STARTUP.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, on)) = *cache
        && at.elapsed() < Duration::from_secs(2)
    {
        return on;
    }
    let on = on_mta(|| task()?.State())
        .is_some_and(|s| s == StartupTaskState::Enabled || s == StartupTaskState::EnabledByPolicy);
    *cache = Some((Instant::now(), on));
    on
}

/// Turns the StartupTask on or off. Once the person has turned PhitPup off
/// in Task Manager or Settings, only they can turn it back on there, so
/// that case opens Settings' Startup apps page.
pub fn set_start_with_windows(on: bool) {
    let state = on_mta(move || {
        let t = task()?;
        if on {
            t.RequestEnableAsync()?.join()
        } else {
            t.Disable()?;
            t.State()
        }
    });
    *STARTUP.lock().unwrap_or_else(|e| e.into_inner()) = None;
    if on && state == Some(StartupTaskState::DisabledByUser) {
        crate::settings::open("ms-settings:startupapps");
    }
}
