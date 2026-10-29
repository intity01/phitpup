//! The underline: a wavy coral line under a word typed in the wrong layout,
//! shown instead of the bubble when `underline = true`.
//!
//! The app has to say where the word is drawn (UI Automation, see
//! [`uia::Automation::word_rects`](crate::uia::Automation::word_rects));
//! where it cannot, the bubble shows as before. Like the bubble it is a
//! layered window that never takes focus or clicks, and it lives on the
//! worker thread.

use std::cell::RefCell;

use windows_sys::Win32::Foundation::{HWND, POINT, RECT, SIZE};
use windows_sys::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BLENDFUNCTION, CreateCompatibleDC, DeleteObject, HBITMAP, HDC,
    SelectObject,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, HWND_TOPMOST, RegisterClassExW, SW_HIDE, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SetWindowPos, ShowWindow, ULW_ALPHA,
    UpdateLayeredWindow, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

use crate::bubble;

// The wave at 96 DPI.
const AMPLITUDE: f32 = 1.25;
const WAVELENGTH: f32 = 5.0;
const THICKNESS: f32 = 1.3;

struct Line {
    hwnd: HWND,
    dc: HDC,
    bitmap: HBITMAP,
    /// What is on screen now, to skip redrawing the same line.
    shown: Option<(Vec<[i32; 4]>, u32, bool)>,
}

thread_local! {
    static LINE: RefCell<Option<Line>> = const { RefCell::new(None) };
}

/// Creates the (hidden) underline window. Call once, on the worker thread.
pub fn create() {
    let class: Vec<u16> = "PhitPupUnderline".encode_utf16().chain([0]).collect();
    // SAFETY: the class name outlives both calls; DefWindowProcW is 'static.
    let (hwnd, dc) = unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(DefWindowProcW),
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
        (hwnd, CreateCompatibleDC(std::ptr::null_mut()))
    };
    LINE.with_borrow_mut(|l| {
        *l = Some(Line {
            hwnd,
            dc,
            bitmap: std::ptr::null_mut(),
            shown: None,
        })
    });
}

/// Underlines `rects` (screen pixels, one per line of the word).
pub fn show(rects: &[RECT], dpi: u32, dark: bool) {
    LINE.with_borrow_mut(|l| {
        let Some(l) = l else { return };
        let edges = rects.iter().map(|r| [r.left, r.top, r.right, r.bottom]);
        let key = (edges.collect(), dpi, dark);
        if l.shown.as_ref() == Some(&key) {
            return;
        }
        let image = render(rects, dpi as f32 / 96.0, dark);
        // SAFETY: the new DIB gets exactly w*h pixels and is selected into
        // our DC, which deselects the previous one before it is deleted; the
        // window and DC are ours and every pointer is to a local.
        unsafe {
            let (bitmap, bits) = bubble::dib(l.dc, image.w, image.h);
            std::ptr::copy_nonoverlapping(image.px.as_ptr(), bits, image.px.len());
            SelectObject(l.dc, bitmap);
            if !l.bitmap.is_null() {
                DeleteObject(l.bitmap);
            }
            l.bitmap = bitmap;
            let size = SIZE {
                cx: image.w,
                cy: image.h,
            };
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let src = POINT { x: 0, y: 0 };
            UpdateLayeredWindow(
                l.hwnd,
                std::ptr::null_mut(),
                &image.at,
                &size,
                l.dc,
                &src,
                0,
                &blend,
                ULW_ALPHA,
            );
            if l.shown.is_none() {
                SetWindowPos(
                    l.hwnd,
                    HWND_TOPMOST,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
            }
        }
        l.shown = Some(key);
    });
}

pub fn hide() {
    LINE.with_borrow_mut(|l| {
        if let Some(l) = l
            && l.shown.take().is_some()
        {
            // SAFETY: our own window.
            unsafe { ShowWindow(l.hwnd, SW_HIDE) };
        }
    });
}

struct Image {
    /// Top-left on screen.
    at: POINT,
    w: i32,
    h: i32,
    /// Premultiplied BGRA, top-down.
    px: Vec<u32>,
}

/// A wave just under the bottom of each rectangle, in the red pen.
fn render(rects: &[RECT], scale: f32, dark: bool) -> Image {
    let amp = AMPLITUDE * scale;
    let len = WAVELENGTH * scale;
    let half = THICKNESS * scale / 2.0;
    let band = (2.0 * (amp + half) + 2.0).ceil() as i32;
    let left = rects.iter().map(|r| r.left).min().unwrap_or(0);
    let right = rects.iter().map(|r| r.right).max().unwrap_or(0);
    let top = rects.iter().map(|r| r.bottom).min().unwrap_or(0);
    let bottom = rects.iter().map(|r| r.bottom).max().unwrap_or(0) + band;
    let (w, h) = ((right - left).max(1), (bottom - top).max(1));
    let [r, g, b] = if dark {
        [0xFF, 0x7A, 0x52]
    } else {
        [0xD9, 0x3A, 0x12]
    };
    let mut px = vec![0u32; (w * h) as usize];
    let k = std::f32::consts::TAU / len;
    for rect in rects {
        let mid = (rect.bottom - top) as f32 + band as f32 / 2.0;
        for x in rect.left - left..rect.right - left {
            let t = (x - (rect.left - left)) as f32 + 0.5;
            let wave = mid + amp * (k * t).sin();
            // Distance across the curve, not straight down.
            let slope = amp * k * (k * t).cos();
            let stretch = (1.0 + slope * slope).sqrt();
            for y in 0..h {
                let d = ((y as f32 + 0.5) - wave).abs() / stretch;
                let a = (half + 0.5 - d).clamp(0.0, 1.0);
                if a > 0.0 {
                    let p = &mut px[(y * w + x) as usize];
                    let a = a.max(((*p >> 24) as f32) / 255.0);
                    let c = |v: u32| ((v as f32 * a).round() as u32).min(255);
                    *p = ((a * 255.0).round() as u32) << 24 | c(r) << 16 | c(g) << 8 | c(b);
                }
            }
        }
    }
    Image {
        at: POINT { x: left, y: top },
        w,
        h,
        px,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wave_covers_each_line_and_nothing_else() {
        let rects = [
            RECT {
                left: 100,
                top: 10,
                right: 160,
                bottom: 30,
            },
            RECT {
                left: 20,
                top: 30,
                right: 50,
                bottom: 50,
            },
        ];
        let img = render(&rects, 1.0, false);
        assert_eq!((img.at.x, img.at.y, img.w), (20, 30, 140));
        let column_has_ink = |x: i32| (0..img.h).any(|y| img.px[(y * img.w + x) as usize] != 0);
        assert!(column_has_ink(0) && column_has_ink(139));
        assert!(!column_has_ink(31 + 20), "between the two lines");
    }
}
