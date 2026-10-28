//! The suggestion bubble: a small popup under the caret showing the fixed
//! word and a "Tab" hint.
//!
//! It never takes focus or clicks (WS_EX_NOACTIVATE | WS_EX_TRANSPARENT), so
//! typing goes on in the app underneath. It lives on the worker thread; the
//! hook posts [`WM_BUBBLE`](crate::WM_BUBBLE) whenever the suggestion changes,
//! and a short timer lets the app draw the new character (and move its caret)
//! before the bubble moves.

use std::cell::RefCell;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, CLEARTYPE_QUALITY, ClientToScreen, CreateFontW, CreateRoundRectRgn,
    CreateSolidBrush, DEFAULT_CHARSET, DT_CENTER, DT_SINGLELINE, DT_VCENTER, DeleteObject,
    DrawTextW, EndPaint, FW_NORMAL, FW_SEMIBOLD, FillRect, GetDC, GetMonitorInfoW,
    GetTextExtentPoint32W, HFONT, InvalidateRect, MONITOR_DEFAULTTONEAREST, MONITORINFO,
    MonitorFromPoint, PAINTSTRUCT, ReleaseDC, SelectObject, SetBkMode, SetTextColor, SetWindowRgn,
    TRANSPARENT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GUITHREADINFO, GetCursorPos, GetForegroundWindow,
    GetGUIThreadInfo, GetWindowThreadProcessId, HWND_TOPMOST, KillTimer, LWA_ALPHA,
    RegisterClassExW, SW_HIDE, SWP_NOACTIVATE, SWP_SHOWWINDOW, SetLayeredWindowAttributes,
    SetTimer, SetWindowPos, ShowWindow, WM_PAINT, WM_TIMER, WNDCLASSEXW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

use crate::APP;

const TIMER: usize = 1;
/// Time for the app to draw the key just typed before we look for its caret.
const DELAY_MS: u32 = 30;

const BACKGROUND: u32 = rgb(0x2B, 0x2D, 0x31);
const KEYCAP: u32 = rgb(0x4A, 0x4D, 0x55);
const TEXT: u32 = rgb(0xFF, 0xFF, 0xFF);
const HINT: u32 = rgb(0xC8, 0xCB, 0xD2);

const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    r as u32 | (g as u32) << 8 | (b as u32) << 16
}

struct Bubble {
    hwnd: HWND,
    dpi: u32,
    font: HFONT,
    small: HFONT,
    word: Vec<u16>,
    /// Layout in pixels, computed when the word or DPI changes.
    size: SIZE,
    word_at: RECT,
    key_at: RECT,
}

thread_local! {
    static BUBBLE: RefCell<Option<Bubble>> = const { RefCell::new(None) };
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// Creates the (hidden) bubble window. Call once, on the worker thread.
pub fn create() {
    let class: Vec<u16> = wide("PhitPupBubble").into_iter().chain([0]).collect();
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
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_LAYERED,
            class.as_ptr(),
            std::ptr::null(),
            WS_POPUP,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        );
        SetLayeredWindowAttributes(hwnd, 0, 240, LWA_ALPHA);
        hwnd
    };
    BUBBLE.with_borrow_mut(|b| {
        *b = Some(Bubble {
            hwnd,
            dpi: 0,
            font: std::ptr::null_mut(),
            small: std::ptr::null_mut(),
            word: Vec::new(),
            size: SIZE { cx: 0, cy: 0 },
            word_at: RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            },
            key_at: RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            },
        })
    });
}

/// The suggestion changed: update the bubble shortly.
pub fn schedule() {
    BUBBLE.with_borrow(|b| {
        if let Some(b) = b {
            // SAFETY: our own window; re-arming the same id restarts the timer.
            unsafe { SetTimer(b.hwnd, TIMER, DELAY_MS, None) };
        }
    });
}

/// Hides the bubble right away.
pub fn hide() {
    BUBBLE.with_borrow(|b| {
        if let Some(b) = b {
            // SAFETY: our own window.
            unsafe {
                KillTimer(b.hwnd, TIMER);
                ShowWindow(b.hwnd, SW_HIDE);
            }
        }
    });
}

fn update() {
    let word = APP.get().and_then(|app| {
        app.suggestion
            .lock()
            .unwrap()
            .as_ref()
            .map(|s| s.edit.insert.clone())
    });
    match word {
        Some(w) => show(&w),
        None => hide(),
    }
}

fn show(word: &str) {
    let (anchor, caret_h, target) = caret();
    BUBBLE.with_borrow_mut(|b| {
        let Some(b) = b else { return };
        // SAFETY: plain query; null falls back to the system DPI.
        let dpi = match unsafe { GetDpiForWindow(target) } {
            0 => 96,
            d => d,
        };
        let word = wide(word);
        if dpi != b.dpi || word != b.word {
            b.word = word;
            layout(b, dpi);
        }
        let (w, h) = (b.size.cx, b.size.cy);
        let gap = scale(4, dpi);

        // Below the caret, kept on the caret's monitor; above it if there is
        // no room below.
        // SAFETY: plain queries.
        let work = unsafe {
            let mut mi: MONITORINFO = std::mem::zeroed();
            mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
            GetMonitorInfoW(MonitorFromPoint(anchor, MONITOR_DEFAULTTONEAREST), &mut mi);
            mi.rcWork
        };
        let x = anchor.x.clamp(work.left, (work.right - w).max(work.left));
        let mut y = anchor.y + gap;
        if y + h > work.bottom {
            y = anchor.y - caret_h - gap - h;
        }

        // SAFETY: our own window; the region is handed over to the window.
        unsafe {
            SetWindowRgn(
                b.hwnd,
                CreateRoundRectRgn(0, 0, w + 1, h + 1, scale(8, dpi), scale(8, dpi)),
                1,
            );
            SetWindowPos(
                b.hwnd,
                HWND_TOPMOST,
                x,
                y,
                w,
                h,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            InvalidateRect(b.hwnd, std::ptr::null(), 0);
        }
    });
}

fn scale(px: i32, dpi: u32) -> i32 {
    px * dpi as i32 / 96
}

/// Recreates the fonts for `dpi` and measures the word and the hint.
fn layout(b: &mut Bubble, dpi: u32) {
    if dpi != b.dpi {
        let font = |pt: i32, weight: u32| {
            let face: Vec<u16> = wide("Leelawadee UI").into_iter().chain([0]).collect();
            // SAFETY: plain font creation; the face name is NUL-terminated.
            unsafe {
                CreateFontW(
                    -(pt * dpi as i32 / 72),
                    0,
                    0,
                    0,
                    weight as i32,
                    0,
                    0,
                    0,
                    DEFAULT_CHARSET.into(),
                    0,
                    0,
                    CLEARTYPE_QUALITY.into(),
                    0,
                    face.as_ptr(),
                )
            }
        };
        // SAFETY: the old fonts are no longer selected anywhere.
        unsafe {
            DeleteObject(b.font);
            DeleteObject(b.small);
        }
        b.font = font(11, FW_NORMAL);
        b.small = font(8, FW_SEMIBOLD);
        b.dpi = dpi;
    }

    let hint = wide("Tab");
    // SAFETY: a screen DC, released below; buffers are valid for their lengths.
    let (word, key) = unsafe {
        let dc = GetDC(std::ptr::null_mut());
        let mut word = SIZE { cx: 0, cy: 0 };
        let mut key = SIZE { cx: 0, cy: 0 };
        let old = SelectObject(dc, b.font);
        GetTextExtentPoint32W(dc, b.word.as_ptr(), b.word.len() as i32, &mut word);
        SelectObject(dc, b.small);
        GetTextExtentPoint32W(dc, hint.as_ptr(), hint.len() as i32, &mut key);
        SelectObject(dc, old);
        ReleaseDC(std::ptr::null_mut(), dc);
        (word, key)
    };

    let pad = scale(8, dpi);
    let key_pad = scale(5, dpi);
    let h = word.cy + scale(8, dpi);
    let key_w = key.cx + 2 * key_pad;
    let key_h = key.cy + scale(2, dpi);
    let word_left = pad;
    let key_left = word_left + word.cx + pad;
    b.word_at = RECT {
        left: word_left,
        top: 0,
        right: word_left + word.cx,
        bottom: h,
    };
    b.key_at = RECT {
        left: key_left,
        top: (h - key_h) / 2,
        right: key_left + key_w,
        bottom: (h - key_h) / 2 + key_h,
    };
    b.size = SIZE {
        cx: key_left + key_w + pad - scale(2, dpi),
        cy: h,
    };
}

fn paint(hwnd: HWND) {
    BUBBLE.with_borrow(|b| {
        let Some(b) = b else { return };
        let hint = wide("Tab");
        // SAFETY: standard paint cycle; brushes are deleted after use and the
        // DC's original font is put back.
        unsafe {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let dc = BeginPaint(hwnd, &mut ps);
            let all = RECT {
                left: 0,
                top: 0,
                right: b.size.cx,
                bottom: b.size.cy,
            };
            let bg = CreateSolidBrush(BACKGROUND);
            FillRect(dc, &all, bg);
            DeleteObject(bg);
            let cap = CreateSolidBrush(KEYCAP);
            FillRect(dc, &b.key_at, cap);
            DeleteObject(cap);

            SetBkMode(dc, TRANSPARENT as i32);
            let old = SelectObject(dc, b.font);
            SetTextColor(dc, TEXT);
            let mut r = b.word_at;
            DrawTextW(
                dc,
                b.word.as_ptr(),
                b.word.len() as i32,
                &mut r,
                DT_SINGLELINE | DT_VCENTER,
            );
            SelectObject(dc, b.small);
            SetTextColor(dc, HINT);
            let mut r = b.key_at;
            DrawTextW(
                dc,
                hint.as_ptr(),
                hint.len() as i32,
                &mut r,
                DT_SINGLELINE | DT_VCENTER | DT_CENTER,
            );
            SelectObject(dc, old);
            EndPaint(hwnd, &ps);
        }
    });
}

/// Bottom-left of the caret in screen pixels, the caret's height, and the
/// window it belongs to. Apps that draw their own caret (most browsers and
/// Electron apps) do not report one; the mouse pointer stands in for it.
fn caret() -> (POINT, i32, HWND) {
    // SAFETY: plain queries.
    unsafe {
        let fg = GetForegroundWindow();
        let tid = GetWindowThreadProcessId(fg, std::ptr::null_mut());
        let mut info: GUITHREADINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;
        if GetGUIThreadInfo(tid, &mut info) != 0 && !info.hwndCaret.is_null() {
            let r = info.rcCaret;
            let mut p = POINT {
                x: r.left,
                y: r.bottom,
            };
            ClientToScreen(info.hwndCaret, &mut p);
            return (p, r.bottom - r.top, info.hwndCaret);
        }
        let mut p = POINT { x: 0, y: 0 };
        GetCursorPos(&mut p);
        // Clear the pointer itself.
        (
            POINT {
                x: p.x,
                y: p.y + 16,
            },
            16,
            fg,
        )
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_TIMER if wparam == TIMER => {
            // SAFETY: our own window.
            unsafe { KillTimer(hwnd, TIMER) };
            update();
            0
        }
        WM_PAINT => {
            paint(hwnd);
            0
        }
        // SAFETY: default handling for everything else.
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
