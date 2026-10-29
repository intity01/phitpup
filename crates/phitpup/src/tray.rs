//! The notification-area icon and its menu, on a thread of their own.
//!
//! Left click opens the settings window, which also runs on this thread.
//! The right-click menu has the quick switches: pause, mode, start with
//! Windows, quit. The thread also reloads the settings when the file is
//! edited by hand.

use std::sync::atomic::Ordering;

use phitpup_core::config::Mode;
use phitpup_core::icon::{self, Tone};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, CreateDIBSection, DIB_RGB_COLORS,
    DeleteObject,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::{GetDpiForSystem, GetSystemMetricsForDpi};
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CheckMenuRadioItem, CreateIconIndirect, CreatePopupMenu, CreateWindowExW,
    DefWindowProcW, DestroyIcon, DestroyMenu, DispatchMessageW, GetCursorPos, GetMessageW, HICON,
    ICONINFO, MB_ICONWARNING, MB_OK, MF_BYCOMMAND, MF_CHECKED, MF_SEPARATOR, MF_STRING, MSG,
    MessageBoxW, PostMessageW, PostThreadMessageW, RegisterClassExW, RegisterWindowMessageW,
    SM_CXSMICON, SetForegroundWindow, SetMenuDefaultItem, SetTimer, TPM_RETURNCMD, TPM_RIGHTBUTTON,
    TrackPopupMenu, TranslateMessage, WM_APP, WM_CONTEXTMENU, WM_LBUTTONUP, WM_NULL, WM_QUIT,
    WM_RBUTTONUP, WM_TIMER, WNDCLASSEXW,
};

use windows_sys::Win32::UI::WindowsAndMessaging::{
    ASFW_ANY, AllowSetForegroundWindow, FindWindowW,
};

use crate::i18n::t;
use crate::{APP, hook, settings, window};

const WM_TRAY: u32 = WM_APP + 10;
/// Sent by a second copy of PhitPup: open the settings window.
const WM_OPEN_SETTINGS: u32 = WM_APP + 11;
/// Sent by the installer and uninstaller (installer/phitpup.iss). Not
/// WM_CLOSE: older versions would destroy the window and keep running.
const WM_QUIT_REQUEST: u32 = WM_APP + 12;
/// Redraw the settings window (it lives on the tray thread).
const WM_REFRESH: u32 = WM_APP + 13;
const CLASS: &str = "PhitPupTray";
const RELOAD_TIMER: usize = 1;
/// Checks now and then whether the update reminder is due.
const REMIND_TIMER: usize = 2;
/// The balloon was clicked (`NIN_BALLOONUSERCLICK`).
const NIN_BALLOONUSERCLICK: u32 = 0x0400 + 5;

const DAY: u64 = 24 * 60 * 60;
/// How old this version must be before the reminder shows.
const REMIND_AFTER: u64 = 60 * DAY;
/// How long to wait before showing it again.
const REMIND_EVERY: u64 = 30 * DAY;

const CMD_PAUSE: usize = 1;
// The three modes need consecutive ids for CheckMenuRadioItem.
const CMD_MANUAL: usize = 2;
const CMD_SUGGEST: usize = 3;
const CMD_AUTO: usize = 4;
const CMD_STARTUP: usize = 5;
const CMD_SETTINGS: usize = 6;
const CMD_QUIT: usize = 7;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

struct Tray {
    hwnd: HWND,
    main_thread: u32,
    taskbar_created: u32,
}

thread_local! {
    static TRAY: std::cell::RefCell<Option<Tray>> = const { std::cell::RefCell::new(None) };
}

/// Starts the tray thread. `welcome` shows a first-run balloon; `problems`
/// are settings lines that could not be read.
pub fn start(main_thread: u32, welcome: bool, problems: Vec<String>) {
    std::thread::spawn(move || {
        let class = wide(CLASS);
        // SAFETY: the class name outlives both calls; window_proc is 'static.
        let hwnd = unsafe {
            let instance = GetModuleHandleW(std::ptr::null());
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(window_proc),
                hInstance: instance,
                lpszClassName: class.as_ptr(),
                ..std::mem::zeroed()
            };
            RegisterClassExW(&wc);
            // A hidden top-level window: message-only windows miss the
            // "TaskbarCreated" broadcast sent when Explorer restarts.
            CreateWindowExW(
                0,
                class.as_ptr(),
                std::ptr::null(),
                0,
                0,
                0,
                0,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                instance,
                std::ptr::null(),
            )
        };
        TRAY.with_borrow_mut(|t| {
            *t = Some(Tray {
                hwnd,
                main_thread,
                // SAFETY: plain registration of a well-known name.
                taskbar_created: unsafe { RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()) },
            })
        });
        update_icon(NIM_ADD);
        // Say once that an update happened, and link to what changed.
        let version = env!("CARGO_PKG_VERSION");
        let before = settings::last_version();
        settings::set_last_version(version);
        if !welcome && before.as_deref() != Some(version) {
            balloon(
                &t("PhitPup was updated to {v}").replace("{v}", version),
                t("Click here to see what's new."),
            );
            BALLOON_LINK.set(Some(format!("{}#v{version}", settings::CHANGES)));
        }
        if welcome {
            let (accept, hotkey) = APP.get().map_or_else(Default::default, |a| {
                let c = a.config.lock().unwrap();
                (c.accept_key.label(), c.hotkey.label())
            });
            balloon(
                t("PhitPup is running"),
                &t("Typed in the wrong layout? Press {accept} on the suggestion, or {hotkey}.\nClick this icon for settings.")
                    .replace("{accept}", &accept)
                    .replace("{hotkey}", &hotkey),
            );
        }
        if !problems.is_empty() {
            warn_settings(&problems);
        }
        // SAFETY: our own window; standard message loop.
        unsafe {
            SetTimer(hwnd, RELOAD_TIMER, 2000, None);
            SetTimer(hwnd, REMIND_TIMER, 6 * 60 * 60 * 1000, None);
            if !welcome {
                remind_if_due();
            }
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg); // typing in the settings window
                DispatchMessageW(&msg);
            }
        }
    });
}

/// Asks the copy of PhitPup that is already running to open its settings
/// window. False if it could not be found.
pub fn open_settings_of_running_copy() -> bool {
    // SAFETY: plain window lookup and post; ASFW_ANY lets the running copy
    // bring its window to the front, since this process is in front now.
    unsafe {
        let hwnd = FindWindowW(wide(CLASS).as_ptr(), std::ptr::null());
        if hwnd.is_null() {
            return false;
        }
        AllowSetForegroundWindow(ASFW_ANY);
        PostMessageW(hwnd, WM_OPEN_SETTINGS, 0, 0) != 0
    }
}

/// Redraws the settings window from any thread, e.g. as an update downloads.
pub fn refresh_settings() {
    // SAFETY: plain window lookup and post.
    unsafe {
        let hwnd = FindWindowW(wide(CLASS).as_ptr(), std::ptr::null());
        if !hwnd.is_null() {
            PostMessageW(hwnd, WM_REFRESH, 0, 0);
        }
    }
}

fn hwnd() -> HWND {
    TRAY.with_borrow(|t| t.as_ref().map_or(std::ptr::null_mut(), |t| t.hwnd))
}

fn paused() -> bool {
    APP.get().is_some_and(|a| a.paused.load(Ordering::Relaxed))
}

fn notify_data() -> NOTIFYICONDATAW {
    // SAFETY: an all-zero NOTIFYICONDATAW is valid.
    let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd();
    nid.uID = 1;
    nid
}

fn copy(dst: &mut [u16], s: &str) {
    let w: Vec<u16> = s.encode_utf16().take(dst.len() - 1).collect();
    dst[..w.len()].copy_from_slice(&w);
    dst[w.len()] = 0;
}

/// Adds or refreshes the icon and its tooltip for the paused state.
fn update_icon(action: u32) {
    let paused = paused();
    let mut nid = notify_data();
    nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    nid.uCallbackMessage = WM_TRAY;
    nid.hIcon = draw_icon(if paused { Tone::Paused } else { Tone::Active });
    copy(
        &mut nid.szTip,
        if paused {
            t("PhitPup: paused")
        } else {
            t("PhitPup: fixes text typed in the wrong layout")
        },
    );
    // SAFETY: nid is fully initialised; the shell copies the icon.
    unsafe {
        Shell_NotifyIconW(action, &nid);
        DestroyIcon(nid.hIcon);
    }
}

thread_local! {
    /// Where a click on the balloon showing goes, if anywhere.
    static BALLOON_LINK: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// Whether the update reminder should show: this build is old enough, and
/// it has not shown recently. Times in Unix seconds.
fn reminder_due(built: u64, now: u64, last: Option<u64>) -> bool {
    built > 0
        && now.saturating_sub(built) >= REMIND_AFTER
        && last.is_none_or(|t| now.saturating_sub(t) >= REMIND_EVERY)
}

/// Shows the update reminder if it is due. PhitPup cannot know whether a
/// new version is out (it has no networking code; checking is up to the
/// person, on the About page), only how old it is.
fn remind_if_due() {
    let on = APP
        .get()
        .is_some_and(|a| a.config.lock().unwrap().remind_updates);
    let built: u64 = env!("PHITPUP_BUILT").parse().unwrap_or(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    if !on || !reminder_due(built, now, settings::last_reminder()) {
        return;
    }
    settings::set_last_reminder(now);
    let months = now.saturating_sub(built) / (30 * DAY);
    balloon(
        t("A newer PhitPup may be out"),
        &t("You have version {v}, from about {months} months ago. Click here to open the download page.")
            .replace("{v}", env!("CARGO_PKG_VERSION"))
            .replace("{months}", &months.to_string()),
    );
    BALLOON_LINK.set(Some(settings::RELEASES.into()));
}

fn balloon(title: &str, text: &str) {
    BALLOON_LINK.set(None);
    let mut nid = notify_data();
    nid.uFlags = NIF_INFO;
    nid.dwInfoFlags = NIIF_INFO;
    copy(&mut nid.szInfoTitle, title);
    copy(&mut nid.szInfo, text);
    // SAFETY: nid is fully initialised.
    unsafe { Shell_NotifyIconW(NIM_MODIFY, &nid) };
}

fn warn_settings(problems: &[String]) {
    let text = t("Some lines in the settings file could not be read, so their defaults are used:\n{problems}\n\n{file}")
        .replace("{problems}", &problems.join("\n"))
        .replace("{file}", &settings::path().display().to_string());
    // SAFETY: NUL-terminated strings; blocks only this thread.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            wide(&text).as_ptr(),
            wide("PhitPup").as_ptr(),
            MB_OK | MB_ICONWARNING,
        )
    };
}

/// The PhitPup icon at the tray's size for this screen, grey when paused.
fn draw_icon(tone: Tone) -> HICON {
    // SAFETY: plain queries; the DIB holds size*size pixels and both bitmaps
    // are deleted once the icon has copied them.
    unsafe {
        let size = GetSystemMetricsForDpi(SM_CXSMICON, GetDpiForSystem()).max(16);
        let rgba = icon::render(size as u32, tone);
        let mut bmi: BITMAPINFO = std::mem::zeroed();
        bmi.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: size,
            biHeight: -size, // top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..std::mem::zeroed()
        };
        let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
        let color = CreateDIBSection(
            std::ptr::null_mut(),
            &bmi,
            DIB_RGB_COLORS,
            &mut bits,
            std::ptr::null_mut(),
            0,
        );
        let px = std::slice::from_raw_parts_mut(bits as *mut u8, rgba.len());
        for (dst, src) in px.chunks_exact_mut(4).zip(rgba.chunks_exact(4)) {
            dst.copy_from_slice(&[src[2], src[1], src[0], src[3]]); // RGBA → BGRA
        }
        let mask = CreateBitmap(size, size, 1, 1, std::ptr::null());
        let info = ICONINFO {
            fIcon: 1,
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask,
            hbmColor: color,
        };
        let icon = CreateIconIndirect(&info);
        DeleteObject(mask);
        DeleteObject(color);
        icon
    }
}

fn show_menu(hwnd: HWND) {
    let Some(app) = APP.get() else { return };
    let (mode, hotkey, accept) = {
        let c = app.config.lock().unwrap();
        (c.mode, c.hotkey.label(), c.accept_key.label())
    };
    let startup = settings::starts_with_windows();
    let checked = |on: bool| if on { MF_CHECKED } else { 0 };
    // SAFETY: the menu is built, shown and destroyed here; strings are
    // NUL-terminated and copied by AppendMenu.
    let cmd = unsafe {
        let menu = CreatePopupMenu();
        let add = |flags: u32, id: usize, text: &str| {
            AppendMenuW(menu, flags, id, wide(text).as_ptr());
        };
        add(MF_STRING, CMD_SETTINGS, t("Settings\u{2026}"));
        SetMenuDefaultItem(menu, CMD_SETTINGS as u32, 0);
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        add(MF_STRING | checked(paused()), CMD_PAUSE, t("Pause"));
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        add(
            MF_STRING,
            CMD_SUGGEST,
            &t("Suggest while typing\t{accept}").replace("{accept}", &accept),
        );
        add(MF_STRING, CMD_AUTO, t("Fix automatically\tSpace"));
        add(
            MF_STRING,
            CMD_MANUAL,
            &t("Fix on hotkey only\t{hotkey}").replace("{hotkey}", &hotkey),
        );
        let current = match mode {
            Mode::Manual => CMD_MANUAL,
            Mode::Suggest => CMD_SUGGEST,
            Mode::Auto => CMD_AUTO,
        };
        CheckMenuRadioItem(
            menu,
            CMD_MANUAL as u32,
            CMD_AUTO as u32,
            current as u32,
            MF_BYCOMMAND,
        );
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        add(
            MF_STRING | checked(startup),
            CMD_STARTUP,
            t("Start with Windows"),
        );
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        add(MF_STRING, CMD_QUIT, t("Quit PhitPup"));

        // The menu only closes on an outside click if our window is in front.
        let mut pt = std::mem::zeroed();
        GetCursorPos(&mut pt);
        SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON,
            pt.x,
            pt.y,
            0,
            hwnd,
            std::ptr::null(),
        );
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(menu);
        cmd as usize
    };

    match cmd {
        CMD_PAUSE => toggle_pause(),
        CMD_MANUAL | CMD_SUGGEST | CMD_AUTO => {
            let mode = match cmd {
                CMD_SUGGEST => Mode::Suggest,
                CMD_AUTO => Mode::Auto,
                _ => Mode::Manual,
            };
            let mut c = app.config.lock().unwrap().clone();
            c.mode = mode;
            settings::apply(c);
            debug!("mode: {}", mode.as_str());
        }
        CMD_STARTUP => settings::set_start_with_windows(!startup),
        CMD_SETTINGS => window::open(),
        CMD_QUIT => quit(),
        _ => {}
    }
    window::refresh();
}

pub fn toggle_pause() {
    let Some(app) = APP.get() else { return };
    let now = !app.paused.load(Ordering::Relaxed);
    app.paused.store(now, Ordering::Relaxed);
    hook::reset(if now { "paused" } else { "resumed" });
    update_icon(NIM_MODIFY);
    window::refresh();
}

fn quit() {
    window::close();
    let nid = notify_data();
    // SAFETY: removing our own icon, then ending the main message loop.
    unsafe {
        Shell_NotifyIconW(NIM_DELETE, &nid);
        let main = TRAY.with_borrow(|t| t.as_ref().map_or(0, |t| t.main_thread));
        PostThreadMessageW(main, WM_QUIT, 0, 0);
    }
}

/// Applies the settings file if it was edited since we last read or wrote
/// it, and redraws the settings window (Shift+Esc may have added a word).
fn reload_if_changed() {
    window::refresh();
    let Some((config, problems)) = settings::changed_on_disk() else {
        return;
    };
    let Some(app) = APP.get() else { return };
    debug!("settings reloaded ({} problems)", problems.len());
    *app.config.lock().unwrap() = config;
    crate::context::forget();
    hook::reset("settings changed");
    if !problems.is_empty() {
        warn_settings(&problems);
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_OPEN_SETTINGS => {
            debug!("another copy asked for the settings window");
            window::open();
            0
        }
        WM_TRAY => {
            match lparam as u32 {
                WM_LBUTTONUP => window::open(),
                NIN_BALLOONUSERCLICK => {
                    if let Some(link) = BALLOON_LINK.with_borrow(Clone::clone) {
                        settings::open(&link);
                    }
                }
                WM_RBUTTONUP | WM_CONTEXTMENU => show_menu(hwnd),
                _ => {}
            }
            0
        }
        WM_REFRESH => {
            window::refresh();
            0
        }
        WM_QUIT_REQUEST => {
            debug!("asked to quit");
            quit();
            0
        }
        WM_TIMER if wparam == REMIND_TIMER => {
            remind_if_due();
            0
        }
        WM_TIMER if wparam == RELOAD_TIMER => {
            reload_if_changed();
            0
        }
        _ if msg != 0
            && msg == TRAY.with_borrow(|t| t.as_ref().map_or(0, |t| t.taskbar_created)) =>
        {
            update_icon(NIM_ADD); // Explorer restarted
            0
        }
        // SAFETY: default handling for everything else.
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_reminder_timing() {
        let built = 1_000 * DAY;
        assert!(!reminder_due(built, built + 59 * DAY, None), "too new");
        assert!(reminder_due(built, built + 61 * DAY, None));
        let last = Some(built + 61 * DAY);
        assert!(!reminder_due(built, built + 80 * DAY, last), "shown lately");
        assert!(reminder_due(built, built + 92 * DAY, last));
        assert!(!reminder_due(0, built, None), "unknown build time");
    }
}
