//! Where PhitPup should stay out of the way.
//!
//! [`gate`] answers, for every key, whether PhitPup may act in the window in
//! front:
//! - **Off** (keys pass untouched and nothing is remembered): paused, password
//!   fields, password managers, remote desktop, full-screen windows, and
//!   `off_apps` from the settings.
//! - **Quiet** (Shift+Backspace works, no bubble): terminals and code
//!   editors, and `quiet_apps`.
//!
//! Password fields drawn by the app itself (browsers, Electron) are found
//! with UI Automation on a thread of their own, woken by focus changes, so
//! the keyboard hook never waits on another program.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use windows_sys::Win32::Foundation::{CloseHandle, HWND, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows_sys::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, EVENT_OBJECT_FOCUS, EVENT_SYSTEM_FOREGROUND, GetClassNameW, GetMessageW,
    GetWindowRect, GetWindowThreadProcessId, MSG, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS,
};

use crate::{App, layout, uia};

/// Fully off: they handle secrets, or the keys go to another computer.
const OFF_APPS: &[&str] = &[
    // Password managers
    "keepass.exe",
    "keepassxc.exe",
    "1password.exe",
    "bitwarden.exe",
    "nordpass.exe",
    "enpass.exe",
    "dashlane.exe",
    "roboform.exe",
    "proton pass.exe",
    // Windows sign-in and credential prompts
    "credentialuibroker.exe",
    "logonui.exe",
    "consent.exe",
    // Remote desktop and virtual machines
    "mstsc.exe",
    "msrdc.exe",
    "vmconnect.exe",
    "anydesk.exe",
    "teamviewer.exe",
    "rustdesk.exe",
    "parsecd.exe",
    "vncviewer.exe",
    "vmware.exe",
    "virtualboxvm.exe",
];

/// Bubble off: typing here is mostly commands and code.
const QUIET_APPS: &[&str] = &[
    "windowsterminal.exe",
    "cmd.exe",
    "powershell.exe",
    "pwsh.exe",
    "conhost.exe",
    "openconsole.exe",
    "wezterm-gui.exe",
    "alacritty.exe",
    "mintty.exe",
    "code.exe",
    "code - insiders.exe",
    "cursor.exe",
    "windsurf.exe",
    "antigravity.exe",
    "zed.exe",
    "devenv.exe",
    "sublime_text.exe",
    "idea64.exe",
    "pycharm64.exe",
    "webstorm64.exe",
    "rider64.exe",
    "clion64.exe",
    "goland64.exe",
    "rustrover64.exe",
    "phpstorm64.exe",
    "datagrip64.exe",
    "studio64.exe",
];

const QUIET_CLASSES: &[&str] = &["ConsoleWindowClass", "CASCADIA_HOSTING_WINDOW_CLASS"];
/// The desktop and taskbar cover the screen but are not full-screen apps.
const SHELL_CLASSES: &[&str] = &["Progman", "WorkerW", "Shell_TrayWnd"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate {
    On,
    Quiet(&'static str),
    Off(&'static str),
}

/// The verdict for the foreground window's program, worked out once per
/// window switch.
struct Window {
    hwnd: isize,
    verdict: Gate,
    shell: bool,
}

static WINDOW: Mutex<Window> = Mutex::new(Window {
    hwnd: 0,
    verdict: Gate::On,
    shell: false,
});
/// UI Automation says the focused element is a password field.
static PASSWORD: AtomicBool = AtomicBool::new(false);

/// Forget the cached verdict (settings changed).
pub fn forget() {
    WINDOW.lock().unwrap().hwnd = 0;
}

/// May PhitPup act on keys going to `fg` right now?
pub fn gate(app: &App, fg: HWND) -> Gate {
    if app.paused.load(Ordering::Relaxed) {
        return Gate::Off("paused");
    }
    let (verdict, shell) = {
        let mut w = WINDOW.lock().unwrap();
        if w.hwnd != fg as isize {
            let class = class_of(fg);
            *w = Window {
                hwnd: fg as isize,
                verdict: if pid_of(fg) == std::process::id() {
                    Gate::Off("PhitPup's own window")
                } else {
                    judge(app, &exe_of(fg), &class)
                },
                shell: SHELL_CLASSES.contains(&class.as_str()),
            };
        }
        (w.verdict, w.shell)
    };
    if let Gate::Off(_) = verdict {
        return verdict;
    }
    if PASSWORD.load(Ordering::Relaxed) || layout::focus_is_password() {
        return Gate::Off("password field");
    }
    if !shell && app.config.lock().unwrap().off_in_fullscreen && fullscreen(fg) {
        return Gate::Off("full-screen window");
    }
    verdict
}

fn judge(app: &App, exe: &str, class: &str) -> Gate {
    let config = app.config.lock().unwrap();
    // Names in the settings may leave out ".exe".
    let named = |a: &str| a == exe || exe.strip_suffix(".exe") == Some(a);
    if OFF_APPS.iter().any(|a| named(a)) || config.off_apps.iter().any(|a| named(a)) {
        Gate::Off("this program is on the off list")
    } else if QUIET_CLASSES.contains(&class)
        || QUIET_APPS.iter().any(|a| named(a))
        || config.quiet_apps.iter().any(|a| named(a))
    {
        Gate::Quiet("terminal or code editor")
    } else {
        Gate::On
    }
}

fn pid_of(hwnd: HWND) -> u32 {
    let mut pid = 0u32;
    // SAFETY: plain query.
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    pid
}

/// The lowercased file name of the program that owns `hwnd`.
pub fn exe_of(hwnd: HWND) -> String {
    exe_of_process(pid_of(hwnd))
}

/// The lowercased file name of process `pid`'s program.
pub fn exe_of_process(pid: u32) -> String {
    // SAFETY: the process handle is checked and closed; the buffer is sized.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return String::new();
        }
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let ok =
            QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len);
        CloseHandle(process);
        if ok == 0 {
            return String::new();
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        path.rsplit('\\').next().unwrap_or_default().to_lowercase()
    }
}

fn class_of(hwnd: HWND) -> String {
    let mut buf = [0u16; 128];
    // SAFETY: plain query into a sized buffer.
    let n = unsafe { GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// Whether `hwnd` covers its whole monitor.
fn fullscreen(hwnd: HWND) -> bool {
    // SAFETY: plain queries into zeroed structs.
    unsafe {
        let mut r: RECT = std::mem::zeroed();
        if GetWindowRect(hwnd, &mut r) == 0 {
            return false;
        }
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut mi) == 0 {
            return false;
        }
        let m = mi.rcMonitor;
        r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom
    }
}

thread_local! {
    static AUTOMATION: Option<uia::Automation> = uia::Automation::new();
}

/// Starts the thread that follows keyboard focus for password fields.
pub fn watch_focus() {
    std::thread::spawn(|| {
        if AUTOMATION.with(|a| a.is_none()) {
            debug!("UI Automation is unavailable; only standard password boxes are detected");
            return;
        }
        // SAFETY: out-of-context hooks call back on this thread's message
        // loop, which runs for the life of the process.
        unsafe {
            for event in [EVENT_SYSTEM_FOREGROUND, EVENT_OBJECT_FOCUS] {
                SetWinEventHook(
                    event,
                    event,
                    std::ptr::null_mut(),
                    Some(focus_changed),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
                );
            }
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                DispatchMessageW(&msg);
            }
        }
    });
}

unsafe extern "system" fn focus_changed(
    _hook: HWINEVENTHOOK,
    _event: u32,
    _hwnd: HWND,
    _object: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    // Until we know, assume the new focus is a password field.
    let was = PASSWORD.swap(true, Ordering::Relaxed);
    let now = AUTOMATION.with(|a| a.as_ref().is_some_and(uia::Automation::focus_is_password));
    PASSWORD.store(now, Ordering::Relaxed);
    if was != now {
        debug!(
            "focus {} a password field",
            if now { "moved into" } else { "left" }
        );
    }
}
