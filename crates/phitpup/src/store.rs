//! The Microsoft Store build. The same phitpup.exe runs from the installer,
//! as a single file, or inside the Store's MSIX package; this tells which,
//! and does what the package needs done differently:
//!
//! - updates come from the Store, so there is no update helper or reminder;
//!   "Check for updates" asks the Store's own update service instead
//!   ([`check_update`], [`install_update`]), so a fix reaches people as soon
//!   as the Store has certified it, not whenever Windows next updates apps;
//! - the settings live in the package's own folder (`LocalState`), so they
//!   are removed with the app and Notepad can open them (MSIX would
//!   otherwise redirect writes to `%APPDATA%` where only PhitPup sees them);
//! - "Start with Windows" is the package's `StartupTask`, not the Run key.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use windows::ApplicationModel::{StartupTask, StartupTaskState};
use windows::Services::Store::{
    StoreContext, StorePackageUpdate, StorePackageUpdateState, StorePackageUpdateStatus,
};
use windows::Win32::UI::Shell::IInitializeWithWindow;
use windows::core::{AgileReference, HSTRING, Interface};
use windows_collections::IVectorView;
use windows_future::AsyncOperationProgressHandler;
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
    std::thread::spawn(move || in_mta(f)).join().ok().flatten()
}

/// Runs `f` on this thread, in the multithreaded apartment.
fn in_mta<T>(f: impl FnOnce() -> windows::core::Result<T>) -> Option<T> {
    // SAFETY: paired with CoUninitialize on this thread.
    let ok = unsafe { CoInitializeEx(std::ptr::null(), COINIT_MULTITHREADED as u32) } >= 0;
    let r = f().ok();
    if ok {
        // SAFETY: balances the successful CoInitializeEx above.
        unsafe { CoUninitialize() };
    }
    r
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

/// The Store's update service, with its dialogs owned by `hwnd` (a desktop
/// app has to say which window they belong to).
fn store(hwnd: isize) -> windows::core::Result<StoreContext> {
    let context = StoreContext::GetDefault()?;
    let init: IInitializeWithWindow = context.cast()?;
    // SAFETY: hwnd is the settings window, which stays open while it asks.
    unsafe { init.Initialize(windows::Win32::Foundation::HWND(hwnd as *mut _))? };
    Ok(context)
}

/// What [`check_update`] found, kept for [`install_update`].
#[derive(Clone)]
struct Found {
    context: StoreContext,
    /// The Store's list is not agile on its own, so it is kept behind an
    /// agile reference to cross from the waiting thread to the window's.
    updates: AgileReference<IVectorView<StorePackageUpdate>>,
}

static FOUND: Mutex<Option<Found>> = Mutex::new(None);

/// Asks the Store whether a newer PhitPup is out. `done` gets `Some(None)`
/// if this is the newest, or `None` if the Store could not be asked.
///
/// Call it on the thread of the window `hwnd`: the Store wants its calls
/// started on the thread that owns its dialogs. Only starting happens
/// there; the waiting is on a thread of its own.
pub fn check_update(hwnd: isize, done: impl FnOnce(Option<Option<String>>) + Send + 'static) {
    *FOUND.lock().unwrap_or_else(|e| e.into_inner()) = None;
    let started = store(hwnd).and_then(|context| {
        let op = context.GetAppAndOptionalStorePackageUpdatesAsync()?;
        Ok((context, op))
    });
    let Ok((context, op)) = started else {
        return done(None);
    };
    std::thread::spawn(move || {
        done(in_mta(move || {
            let updates = op.join()?;
            if updates.Size()? == 0 {
                return Ok(None);
            }
            let v = updates.GetAt(0)?.Package()?.Id()?.Version()?;
            let updates = AgileReference::new(&updates)?;
            *FOUND.lock().unwrap_or_else(|e| e.into_inner()) = Some(Found { context, updates });
            Ok(Some(format!("{}.{}.{}", v.Major, v.Minor, v.Build)))
        }))
    });
}

/// How [`install_update`] ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Installed {
    Done,
    /// The person said no in the Store's dialog.
    Declined,
    /// The Store waits for a charger or for Wi-Fi.
    Waiting,
    Failed,
}

/// Has the Store download and install what [`check_update`] found,
/// reporting `(bytes done, bytes in all, installing)` as it goes, then
/// `done`. Installing closes PhitPup; it asks Windows to start it again
/// afterwards. Call it on the window's thread, like [`check_update`].
pub fn install_update(
    progress: impl Fn(u64, u64, bool) + Send + 'static,
    done: impl FnOnce(Installed) + Send + 'static,
) {
    let found = FOUND.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let Some((context, Ok(updates))) = found.map(|f| (f.context, f.updates.resolve())) else {
        return done(Installed::Failed);
    };
    restart_after_update();
    let started = context
        .RequestDownloadAndInstallStorePackageUpdatesAsync(&updates)
        .and_then(|op| {
            op.SetProgress(&AsyncOperationProgressHandler::new(
                move |_, status: windows::core::Ref<StorePackageUpdateStatus>| {
                    if let Some(s) = status.as_ref() {
                        progress(
                            s.PackageBytesDownloaded,
                            s.PackageDownloadSizeInBytes,
                            s.PackageUpdateState == StorePackageUpdateState::Deploying,
                        );
                    }
                    Ok(())
                },
            ))?;
            Ok(op)
        });
    let Ok(op) = started else {
        return done(Installed::Failed);
    };
    std::thread::spawn(move || {
        let state = in_mta(move || op.join()?.OverallState());
        done(match state {
            Some(StorePackageUpdateState::Completed) => Installed::Done,
            Some(StorePackageUpdateState::Canceled) => Installed::Declined,
            Some(
                StorePackageUpdateState::ErrorLowBattery
                | StorePackageUpdateState::ErrorWiFiRecommended
                | StorePackageUpdateState::ErrorWiFiRequired,
            ) => Installed::Waiting,
            _ => Installed::Failed,
        })
    });
}

/// Asks Windows to start PhitPup again once the update has replaced it,
/// but not after a crash, a hang or a restart of the PC (the StartupTask
/// covers that one).
fn restart_after_update() {
    use windows_sys::Win32::System::Recovery::{
        RESTART_NO_CRASH, RESTART_NO_HANG, RESTART_NO_REBOOT, RegisterApplicationRestart,
    };
    // SAFETY: no command line; plain flags.
    unsafe {
        RegisterApplicationRestart(
            std::ptr::null(),
            RESTART_NO_CRASH | RESTART_NO_HANG | RESTART_NO_REBOOT,
        )
    };
}
