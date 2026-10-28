//! The notification-area icon and its menu, on a thread of their own.
//!
//! Left click pauses or resumes. The menu switches the mode, the "start with
//! Windows" entry, opens the settings file, and quits. The thread also
//! reloads the settings when the file changes.

use std::sync::atomic::Ordering;
use std::time::SystemTime;

use phitpup_core::config::Mode;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CLEARTYPE_QUALITY, CreateBitmap, CreateCompatibleDC,
    CreateDIBSection, CreateFontW, CreateSolidBrush, DEFAULT_CHARSET, DIB_RGB_COLORS, DT_CENTER,
    DT_SINGLELINE, DT_VCENTER, DeleteDC, DeleteObject, DrawTextW, FW_BOLD, GetStockObject,
    NULL_PEN, RoundRect, SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::{GetDpiForSystem, GetSystemMetricsForDpi};
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW, Shell_NotifyIconW, ShellExecuteW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CheckMenuRadioItem, CreateIconIndirect, CreatePopupMenu, CreateWindowExW,
    DefWindowProcW, DestroyIcon, DestroyMenu, DispatchMessageW, GetCursorPos, GetMessageW, HICON,
    ICONINFO, MB_ICONWARNING, MB_OK, MF_BYCOMMAND, MF_CHECKED, MF_GRAYED, MF_SEPARATOR, MF_STRING,
    MSG, MessageBoxW, PostMessageW, PostThreadMessageW, RegisterClassExW, RegisterWindowMessageW,
    SM_CXSMICON, SW_SHOWNORMAL, SetForegroundWindow, SetTimer, TPM_RETURNCMD, TPM_RIGHTBUTTON,
    TrackPopupMenu, WM_APP, WM_CONTEXTMENU, WM_LBUTTONUP, WM_NULL, WM_QUIT, WM_RBUTTONUP, WM_TIMER,
    WNDCLASSEXW,
};

use crate::{APP, hook, settings};

const WM_TRAY: u32 = WM_APP + 10;
const RELOAD_TIMER: usize = 1;

const CMD_PAUSE: usize = 1;
const CMD_MANUAL: usize = 2;
const CMD_SUGGEST: usize = 3;
const CMD_STARTUP: usize = 4;
const CMD_SETTINGS: usize = 5;
const CMD_QUIT: usize = 6;

const ACTIVE: u32 = rgb(0x25, 0x63, 0xEB);
const PAUSED: u32 = rgb(0x6B, 0x72, 0x80);

const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    r as u32 | (g as u32) << 8 | (b as u32) << 16
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

struct Tray {
    hwnd: HWND,
    main_thread: u32,
    taskbar_created: u32,
    modified: Option<SystemTime>,
}

thread_local! {
    static TRAY: std::cell::RefCell<Option<Tray>> = const { std::cell::RefCell::new(None) };
}

/// Starts the tray thread. `welcome` shows a first-run balloon; `problems`
/// are settings lines that could not be read.
pub fn start(main_thread: u32, welcome: bool, problems: Vec<String>) {
    std::thread::spawn(move || {
        let class = wide("PhitPupTray");
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
                modified: settings::modified(),
            })
        });
        update_icon(NIM_ADD);
        if welcome {
            balloon(
                "PhitPup is running",
                "Typed in the wrong layout? Press Tab on the suggestion, or Shift+Backspace.\nRight-click this icon for options.",
            );
        }
        if !problems.is_empty() {
            warn_settings(&problems);
        }
        // SAFETY: our own window; standard message loop.
        unsafe {
            SetTimer(hwnd, RELOAD_TIMER, 2000, None);
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                DispatchMessageW(&msg);
            }
        }
    });
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
    nid.hIcon = draw_icon(if paused { PAUSED } else { ACTIVE });
    copy(
        &mut nid.szTip,
        if paused {
            "PhitPup: paused (click to resume)"
        } else {
            "PhitPup: fixes text typed in the wrong layout"
        },
    );
    // SAFETY: nid is fully initialised; the shell copies the icon.
    unsafe {
        Shell_NotifyIconW(action, &nid);
        DestroyIcon(nid.hIcon);
    }
}

fn balloon(title: &str, text: &str) {
    let mut nid = notify_data();
    nid.uFlags = NIF_INFO;
    nid.dwInfoFlags = NIIF_INFO;
    copy(&mut nid.szInfoTitle, title);
    copy(&mut nid.szInfo, text);
    // SAFETY: nid is fully initialised.
    unsafe { Shell_NotifyIconW(NIM_MODIFY, &nid) };
}

fn warn_settings(problems: &[String]) {
    let text = format!(
        "Some lines in the settings file could not be read, so their defaults are used:\n{}\n\n{}",
        settings::path().display(),
        problems.join("\n")
    );
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

/// A small rounded square in `color` with a white ผ.
fn draw_icon(color: u32) -> HICON {
    // SAFETY: GDI objects are created, selected, restored and deleted here;
    // the DIB's pixel memory is `size * size` u32s, owned by the bitmap.
    unsafe {
        let dpi = GetDpiForSystem();
        let size = GetSystemMetricsForDpi(SM_CXSMICON, dpi).max(16);
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
        let color_bmp = CreateDIBSection(
            std::ptr::null_mut(),
            &bmi,
            DIB_RGB_COLORS,
            &mut bits,
            std::ptr::null_mut(),
            0,
        );
        let dc = CreateCompatibleDC(std::ptr::null_mut());
        let old_bmp = SelectObject(dc, color_bmp);

        let brush = CreateSolidBrush(color);
        let old_brush = SelectObject(dc, brush);
        let old_pen = SelectObject(dc, GetStockObject(NULL_PEN));
        let r = size / 4;
        RoundRect(dc, 0, 0, size + 1, size + 1, r, r);

        let face = wide("Leelawadee UI");
        let font = CreateFontW(
            -(size * 9 / 10),
            0,
            0,
            0,
            FW_BOLD as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET.into(),
            0,
            0,
            CLEARTYPE_QUALITY.into(),
            0,
            face.as_ptr(),
        );
        let old_font = SelectObject(dc, font);
        SetBkMode(dc, TRANSPARENT as i32);
        SetTextColor(dc, rgb(0xFF, 0xFF, 0xFF));
        let mut rect = RECT {
            left: 0,
            top: -size / 12,
            right: size,
            bottom: size,
        };
        let text: Vec<u16> = "ผ".encode_utf16().collect();
        DrawTextW(
            dc,
            text.as_ptr(),
            text.len() as i32,
            &mut rect,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        );

        SelectObject(dc, old_font);
        SelectObject(dc, old_pen);
        SelectObject(dc, old_brush);
        SelectObject(dc, old_bmp);
        DeleteObject(font);
        DeleteObject(brush);
        DeleteDC(dc);

        // GDI leaves alpha at 0: make everything it drew opaque.
        let px = std::slice::from_raw_parts_mut(bits as *mut u32, (size * size) as usize);
        for p in px {
            if *p & 0x00FF_FFFF != 0 {
                *p |= 0xFF00_0000;
            }
        }
        let mask = CreateBitmap(size, size, 1, 1, std::ptr::null());
        let info = ICONINFO {
            fIcon: 1,
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask,
            hbmColor: color_bmp,
        };
        let icon = CreateIconIndirect(&info);
        DeleteObject(mask);
        DeleteObject(color_bmp);
        icon
    }
}

fn show_menu(hwnd: HWND) {
    let Some(app) = APP.get() else { return };
    let mode = app.config.lock().unwrap().mode;
    let startup = settings::starts_with_windows();
    let checked = |on: bool| if on { MF_CHECKED } else { 0 };
    // SAFETY: the menu is built, shown and destroyed here; strings are
    // NUL-terminated and copied by AppendMenu.
    let cmd = unsafe {
        let menu = CreatePopupMenu();
        let add = |flags: u32, id: usize, text: &str| {
            AppendMenuW(menu, flags, id, wide(text).as_ptr());
        };
        add(
            MF_STRING | MF_GRAYED,
            0,
            &format!("PhitPup {}", env!("CARGO_PKG_VERSION")),
        );
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        add(MF_STRING | checked(paused()), CMD_PAUSE, "Pause");
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        add(MF_STRING, CMD_SUGGEST, "Suggest while typing\tTab");
        add(MF_STRING, CMD_MANUAL, "Fix on hotkey only\tShift+Backspace");
        let current = if mode == Mode::Suggest {
            CMD_SUGGEST
        } else {
            CMD_MANUAL
        };
        CheckMenuRadioItem(
            menu,
            CMD_MANUAL as u32,
            CMD_SUGGEST as u32,
            current as u32,
            MF_BYCOMMAND,
        );
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        add(
            MF_STRING | checked(startup),
            CMD_STARTUP,
            "Start with Windows",
        );
        add(MF_STRING, CMD_SETTINGS, "Edit settings file…");
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        add(MF_STRING, CMD_QUIT, "Quit PhitPup");

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
        CMD_MANUAL | CMD_SUGGEST => {
            let mode = if cmd == CMD_SUGGEST {
                Mode::Suggest
            } else {
                Mode::Manual
            };
            app.config.lock().unwrap().mode = mode;
            settings::save_mode(mode);
            TRAY.with_borrow_mut(|t| {
                if let Some(t) = t {
                    t.modified = settings::modified();
                }
            });
            hook::reset("mode changed");
            debug!("mode: {}", mode.as_str());
        }
        CMD_STARTUP => settings::set_start_with_windows(!startup),
        CMD_SETTINGS => {
            settings::load(); // creates the file if it is missing
            let file = wide(&format!("\"{}\"", settings::path().display()));
            // SAFETY: NUL-terminated strings.
            unsafe {
                ShellExecuteW(
                    std::ptr::null_mut(),
                    wide("open").as_ptr(),
                    wide("notepad.exe").as_ptr(),
                    file.as_ptr(),
                    std::ptr::null(),
                    SW_SHOWNORMAL,
                )
            };
        }
        CMD_QUIT => quit(),
        _ => {}
    }
}

fn toggle_pause() {
    let Some(app) = APP.get() else { return };
    let now = !app.paused.load(Ordering::Relaxed);
    app.paused.store(now, Ordering::Relaxed);
    hook::reset(if now { "paused" } else { "resumed" });
    update_icon(NIM_MODIFY);
}

fn quit() {
    let nid = notify_data();
    // SAFETY: removing our own icon, then ending the main message loop.
    unsafe {
        Shell_NotifyIconW(NIM_DELETE, &nid);
        let main = TRAY.with_borrow(|t| t.as_ref().map_or(0, |t| t.main_thread));
        PostThreadMessageW(main, WM_QUIT, 0, 0);
    }
}

/// Applies the settings file if it changed since we last looked.
fn reload_if_changed() {
    let now = settings::modified();
    let changed = TRAY.with_borrow_mut(|t| {
        let t = t.as_mut()?;
        (t.modified != now).then(|| t.modified = now)
    });
    if changed.is_none() {
        return;
    }
    let Some(app) = APP.get() else { return };
    let (config, problems) = settings::load();
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
        WM_TRAY => {
            match lparam as u32 {
                WM_LBUTTONUP => toggle_pause(),
                WM_RBUTTONUP | WM_CONTEXTMENU => show_menu(hwnd),
                _ => {}
            }
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
