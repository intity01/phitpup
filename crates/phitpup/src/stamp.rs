//! The language stamp: a small TH or EN by the caret for a moment when the
//! keyboard language changes, so the next word goes in the language meant.
//! The same pill says "Fixed · TH" when a word was fixed on its own.
//!
//! It lives on the worker thread beside the bubble, whose drawing it
//! borrows. A timer looks at the foreground window's layout a few times a
//! second; the stamp shows when the layout changed in the same window (not
//! when another window comes to the front with its own), only where the
//! app says where its caret is, never over the bubble, and not where
//! PhitPup is off (password fields, games, programs turned off), and not
//! for PhitPup's own switch after a fix. It sits beside the caret on its
//! line, and goes as soon as typing goes on. It never takes focus or
//! clicks: the mouse goes through it.

use std::cell::{Cell, RefCell};
use std::time::{Duration, Instant};

use phitpup_core::model::Lang;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BLENDFUNCTION, CreateCompatibleDC, DeleteObject, HBITMAP, HDC,
    SelectObject,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetForegroundWindow, HWND_TOPMOST, KillTimer, MA_NOACTIVATE,
    RegisterClassExW, SW_HIDE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SetTimer,
    SetWindowPos, ShowWindow, ULW_ALPHA, UpdateLayeredWindow, WM_MOUSEACTIVATE, WM_TIMER,
    WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT, WS_POPUP,
};

use crate::context::{self, Gate};
use crate::i18n::t;
use crate::{APP, bubble, layout};

/// Looks at the layout.
const POLL: usize = 1;
const POLL_MS: u32 = 120;
/// Drives the fade.
const FRAME: usize = 2;
const FRAME_MS: u32 = 15;

const FADE_IN: Duration = Duration::from_millis(120);
const HOLD: Duration = Duration::from_millis(700);
const FADE_OUT: Duration = Duration::from_millis(200);
/// Typing on fades it out, but not before it could be read.
const AT_LEAST: Duration = Duration::from_millis(350);
/// After PhitPup switches the language itself, no stamp for that.
const OWN_SWITCH: Duration = Duration::from_millis(1500);

struct Stamp {
    hwnd: HWND,
    dc: HDC,
    bitmap: HBITMAP,
    /// The foreground window and its language at the last look.
    seen: (isize, Option<Lang>),
    /// What is drawn: the text, DPI and theme.
    look: Option<(String, u32, bool)>,
    size: SIZE,
    /// Where the pill starts in the bitmap (the rest is shadow).
    inset: POINT,
    shown: Option<Instant>,
    /// The last input when it showed; new input fades it out.
    input: u32,
}

thread_local! {
    static STAMP: RefCell<Option<Stamp>> = const { RefCell::new(None) };
    /// When PhitPup last switched the language itself.
    static OWN: Cell<Option<Instant>> = const { Cell::new(None) };
}

/// Creates the (hidden) stamp window and starts looking. Call once, on the
/// worker thread.
pub fn create() {
    let class: Vec<u16> = "PhitPupStamp".encode_utf16().chain([0]).collect();
    // SAFETY: the class name outlives both calls; window_proc is 'static.
    let (hwnd, dc) = unsafe {
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
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED | WS_EX_TRANSPARENT,
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
        SetTimer(hwnd, POLL, POLL_MS, None);
        (hwnd, CreateCompatibleDC(std::ptr::null_mut()))
    };
    STAMP.with_borrow_mut(|s| {
        *s = Some(Stamp {
            hwnd,
            dc,
            bitmap: std::ptr::null_mut(),
            seen: (0, None),
            look: None,
            size: SIZE { cx: 0, cy: 0 },
            inset: POINT { x: 0, y: 0 },
            shown: None,
            input: 0,
        })
    });
}

/// Whether a look at the layout should show the stamp: the same window as
/// last time, now in another language it knows.
fn changed(before: (isize, Option<Lang>), now: (isize, Option<Lang>)) -> bool {
    before.0 == now.0 && now.1.is_some() && before.1.is_some() && before.1 != now.1
}

/// PhitPup is switching the language for a fix: that needs no stamp. Call
/// on the worker thread.
pub fn own_switch() {
    OWN.set(Some(Instant::now()));
}

/// A word was just fixed on its own into `lang`: say so beside the caret.
/// Call on the worker thread, after the fix was typed.
pub fn fixed(lang: Lang) {
    OWN.set(Some(Instant::now()));
    let Some((caret, caret_h)) = bubble::caret_found() else {
        return;
    };
    show(format!("{} · {}", t("Fixed"), badge(lang)), caret, caret_h);
}

fn badge(lang: Lang) -> &'static str {
    match lang {
        Lang::Thai => "TH",
        Lang::English => "EN",
    }
}

fn last_input() -> u32 {
    let mut info = LASTINPUTINFO {
        cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    // SAFETY: a sized struct on the stack.
    unsafe { GetLastInputInfo(&mut info) };
    info.dwTime
}

/// Where the pill goes: just right of the caret, centred on its line
/// (`caret` is the caret's bottom), kept on the screen.
fn place(caret: POINT, caret_h: i32, pill: SIZE, gap: i32, work: RECT) -> POINT {
    let x = (caret.x + gap).clamp(work.left, (work.right - pill.cx).max(work.left));
    let y = (caret.y - caret_h / 2 - pill.cy / 2)
        .clamp(work.top, (work.bottom - pill.cy).max(work.top));
    POINT { x, y }
}

/// How opaque the stamp is `t` after it showed; `None` once it is gone.
fn alpha(t: Duration) -> Option<f32> {
    if t < FADE_IN {
        Some(t.as_secs_f32() / FADE_IN.as_secs_f32())
    } else if t < FADE_IN + HOLD {
        Some(1.0)
    } else if t < FADE_IN + HOLD + FADE_OUT {
        Some(1.0 - (t - FADE_IN - HOLD).as_secs_f32() / FADE_OUT.as_secs_f32())
    } else {
        None
    }
}

fn poll() {
    // SAFETY: plain query.
    let fg = unsafe { GetForegroundWindow() };
    let now = (fg as isize, layout::lang_of(layout::foreground()));
    let before = STAMP.with_borrow_mut(|s| {
        s.as_mut()
            .map(|s| std::mem::replace(&mut s.seen, now))
            .unwrap_or(now)
    });
    if !changed(before, now) {
        return;
    }
    if OWN.get().is_some_and(|at| at.elapsed() < OWN_SWITCH) {
        return;
    }
    let Some(app) = APP.get() else { return };
    if !app.config.lock().unwrap().show_language || bubble::showing() {
        return;
    }
    if let Gate::Off(_) = context::gate(app, fg) {
        return;
    }
    let Some((caret, caret_h)) = bubble::caret_found() else {
        return;
    };
    show(badge(now.1.unwrap()).to_string(), caret, caret_h);
}

fn show(text: String, caret: POINT, caret_h: i32) {
    let (work, dpi) = bubble::monitor_of(caret);
    let dark = bubble::dark_theme();
    STAMP.with_borrow_mut(|s| {
        let Some(s) = s else { return };
        let look = Some((text, dpi, dark));
        if s.look != look {
            let (w, h, px, pill) = bubble::render_stamp(&look.as_ref().unwrap().0, dpi, dark);
            // SAFETY: the new DIB is filled with exactly w*h pixels and
            // selected into our DC, which deselects the old one first.
            unsafe {
                let (bitmap, bits) = bubble::dib(s.dc, w, h);
                std::ptr::copy_nonoverlapping(px.as_ptr(), bits, px.len());
                SelectObject(s.dc, bitmap);
                if !s.bitmap.is_null() {
                    DeleteObject(s.bitmap);
                }
                s.bitmap = bitmap;
            }
            s.size = SIZE { cx: w, cy: h };
            s.look = look;
            s.inset = POINT {
                x: pill.left,
                y: pill.top,
            };
        }
        // The shadow's margin is the same on every side.
        let pill = RECT {
            left: s.inset.x,
            top: s.inset.y,
            right: s.size.cx - s.inset.x,
            bottom: s.size.cy - s.inset.y,
        };
        let gap = (4 * dpi / 96) as i32;
        let size = SIZE {
            cx: pill.right - pill.left,
            cy: pill.bottom - pill.top,
        };
        let POINT { x, y } = place(caret, caret_h, size, gap, work);
        let at = POINT {
            x: x - pill.left,
            y: y - pill.top,
        };
        s.shown = Some(Instant::now());
        s.input = last_input();
        frame(s, Some(at));
        // SAFETY: our own window; shown without taking focus.
        unsafe {
            SetWindowPos(
                s.hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            SetTimer(s.hwnd, FRAME, FRAME_MS, None);
        }
    });
}

/// Shows the stamp at `at` (or where it is) with the fade's opacity.
/// Returns false once it is gone.
fn frame(s: &mut Stamp, at: Option<POINT>) -> bool {
    // Typing went on: start fading out now, if it is not already.
    if let Some(shown) = s.shown {
        let t = shown.elapsed();
        if t >= AT_LEAST && t < FADE_IN + HOLD && last_input() != s.input {
            s.shown = Instant::now().checked_sub(FADE_IN + HOLD);
        }
    }
    let Some(a) = s.shown.and_then(|t| alpha(t.elapsed())) else {
        s.shown = None;
        // SAFETY: our own window.
        unsafe { ShowWindow(s.hwnd, SW_HIDE) };
        return false;
    };
    let blend = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: (a.clamp(0.0, 1.0) * 255.0).round() as u8,
        AlphaFormat: AC_SRC_ALPHA as u8,
    };
    let src = POINT { x: 0, y: 0 };
    // SAFETY: our window and DC; all pointers are to locals or null.
    unsafe {
        UpdateLayeredWindow(
            s.hwnd,
            std::ptr::null_mut(),
            at.as_ref().map_or(std::ptr::null(), |p| p),
            &s.size,
            s.dc,
            &src,
            0,
            &blend,
            ULW_ALPHA,
        )
    };
    true
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_TIMER if wparam == POLL => {
            poll();
            0
        }
        WM_TIMER if wparam == FRAME => {
            let still = STAMP.with_borrow_mut(|s| s.as_mut().is_some_and(|s| frame(s, None)));
            if !still {
                // SAFETY: our own window.
                unsafe { KillTimer(hwnd, FRAME) };
            }
            0
        }
        WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
        // SAFETY: default handling for everything else.
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shows_only_for_a_switch_in_the_same_window() {
        let (th, en) = (Some(Lang::Thai), Some(Lang::English));
        assert!(changed((1, en), (1, th)));
        assert!(
            !changed((1, en), (2, th)),
            "another window came to the front"
        );
        assert!(!changed((1, en), (1, en)), "no change");
        assert!(
            !changed((1, None), (1, th)),
            "from a language PhitPup does not know"
        );
        assert!(
            !changed((1, en), (1, None)),
            "to a language PhitPup does not know"
        );
    }

    #[test]
    fn sits_right_of_the_caret_on_its_line() {
        let work = RECT {
            left: 0,
            top: 0,
            right: 1000,
            bottom: 800,
        };
        let pill = SIZE { cx: 40, cy: 20 };
        // A 30px caret whose bottom is at y=400: its line's middle is 385.
        let at = place(POINT { x: 100, y: 400 }, 30, pill, 4, work);
        assert_eq!((at.x, at.y), (104, 375));
        // At the right edge it stays on the screen.
        let at = place(POINT { x: 990, y: 400 }, 30, pill, 4, work);
        assert_eq!(at.x, 960);
    }

    #[test]
    fn fades_in_holds_and_fades_out() {
        assert_eq!(alpha(Duration::ZERO), Some(0.0));
        assert_eq!(alpha(FADE_IN + HOLD / 2), Some(1.0));
        let late = alpha(FADE_IN + HOLD + FADE_OUT / 2).unwrap();
        assert!(late > 0.4 && late < 0.6);
        assert_eq!(alpha(FADE_IN + HOLD + FADE_OUT), None);
    }
}
