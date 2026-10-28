//! The suggestion bubble: a small card under the caret with the fixed word,
//! a TH/EN badge for the layout it switches to, and a "Tab" key hint.
//!
//! It is drawn here, pixel by pixel, into a premultiplied 32-bit bitmap
//! (anti-aliased corners, soft shadow, hairline border, grayscale text from
//! GDI used as a coverage mask) and shown with `UpdateLayeredWindow`, which
//! also fades and moves it without redrawing. It never takes focus or clicks.
//!
//! Motion: it fades in while sliding up a few pixels, fades out when the
//! suggestion goes, and stays put while the caret moves along the same word
//! instead of jumping with every key. It waits for a short pause in typing
//! before first appearing, so fast typing does not make it flicker.
//!
//! It lives on the worker thread; the hook posts
//! [`WM_BUBBLE`](crate::WM_BUBBLE) whenever the suggestion changes.

use std::cell::RefCell;
use std::time::{Duration, Instant};

use phitpup_core::model::Lang;
use windows_sys::Win32::Foundation::{
    ERROR_SUCCESS, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM,
};
use windows_sys::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, ANTIALIASED_QUALITY, BI_RGB, BITMAPINFO, BITMAPINFOHEADER,
    BLENDFUNCTION, ClientToScreen, CreateCompatibleDC, CreateDIBSection, CreateFontW,
    DEFAULT_CHARSET, DIB_RGB_COLORS, DeleteDC, DeleteObject, FW_NORMAL, FW_SEMIBOLD, GdiFlush,
    GetMonitorInfoW, GetTextExtentPoint32W, HBITMAP, HDC, HFONT, MONITOR_DEFAULTTONEAREST,
    MONITORINFO, MonitorFromPoint, SelectObject, SetBkMode, SetTextColor, TRANSPARENT, TextOutW,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GUITHREADINFO, GetCursorPos, GetForegroundWindow,
    GetGUIThreadInfo, GetWindowThreadProcessId, HWND_TOPMOST, KillTimer, RegisterClassExW, SW_HIDE,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SetTimer, SetWindowPos, ShowWindow,
    ULW_ALPHA, UpdateLayeredWindow, WM_TIMER, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

use crate::{APP, uia};

/// Waits for the suggestion to settle before acting on it.
const SETTLE: usize = 1;
/// Drives an animation, about 60 frames a second.
const FRAME: usize = 2;
const FRAME_MS: u32 = 15;

/// A pause in typing this long shows the bubble.
const SHOW_DELAY_MS: u32 = 120;
/// A suggestion that disappears for less than this does not hide it.
const HIDE_DELAY_MS: u32 = 60;
const UPDATE_DELAY_MS: u32 = 25;

const FADE_IN: Duration = Duration::from_millis(150);
const FADE_OUT: Duration = Duration::from_millis(90);
const GLIDE: Duration = Duration::from_millis(110);
/// How far below its place the bubble starts when fading in, at 96 DPI.
const SLIDE_PX: f32 = 6.0;

// Layout at 96 DPI.
const SHADOW: f32 = 16.0;
const RADIUS: f32 = 8.0;
const PAD_X: f32 = 10.0;
const GAP: f32 = 8.0;
const WORD_PT: i32 = 12;
const SMALL_PT: i32 = 8;

type Rgb = [f32; 3];

const fn hex(v: u32) -> Rgb {
    [
        ((v >> 16) & 0xFF) as f32 / 255.0,
        ((v >> 8) & 0xFF) as f32 / 255.0,
        (v & 0xFF) as f32 / 255.0,
    ]
}

/// Colours close to Windows 11 flyouts.
struct Palette {
    card: Rgb,
    text: Rgb,
    hint: Rgb,
    key: Rgb,
    accent: Rgb,
    on_accent: Rgb,
    border: (Rgb, f32),
    shadow: f32,
}

const DARK: Palette = Palette {
    card: hex(0x2C2C2C),
    text: hex(0xFFFFFF),
    hint: hex(0xC8C8C8),
    key: hex(0x3D3D3D),
    accent: hex(0x4C8DF6),
    on_accent: hex(0xFFFFFF),
    border: (hex(0xFFFFFF), 0.10),
    shadow: 0.42,
};

const LIGHT: Palette = Palette {
    card: hex(0xFBFBFB),
    text: hex(0x1B1B1B),
    hint: hex(0x5C5C5C),
    key: hex(0xEBEBEB),
    accent: hex(0x2563EB),
    on_accent: hex(0xFFFFFF),
    border: (hex(0x000000), 0.10),
    shadow: 0.20,
};

/// Whether apps should use the dark theme (Settings → Personalization →
/// Colors → app mode).
fn dark_theme() -> bool {
    let key: Vec<u16> = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"
        .encode_utf16()
        .chain([0])
        .collect();
    let value: Vec<u16> = "AppsUseLightTheme".encode_utf16().chain([0]).collect();
    let mut light = 1u32;
    let mut len = 4u32;
    // SAFETY: a DWORD read into a u32 with its size.
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&mut light as *mut u32).cast(),
            &mut len,
        )
    };
    r == ERROR_SUCCESS && light == 0
}

/// What the bubble shows; a change means drawing it again.
#[derive(Clone, PartialEq)]
struct Look {
    word: String,
    lang: Lang,
    dpi: u32,
    dark: bool,
}

/// A drawn bubble: premultiplied BGRA pixels plus where the card sits.
struct Image {
    w: i32,
    h: i32,
    px: Vec<u32>,
    /// The card inside the bitmap (the rest is shadow).
    card: RECT,
}

#[derive(Clone, Copy)]
struct Tween {
    from: f32,
    to: f32,
    start: Instant,
    dur: Duration,
}

impl Tween {
    fn still(v: f32) -> Self {
        Self {
            from: v,
            to: v,
            start: Instant::now(),
            dur: Duration::ZERO,
        }
    }

    fn at(&self, now: Instant) -> f32 {
        if self.dur.is_zero() {
            return self.to;
        }
        let t = (now - self.start).as_secs_f32() / self.dur.as_secs_f32();
        let t = t.clamp(0.0, 1.0);
        let ease = 1.0 - (1.0 - t).powi(3); // ease-out cubic
        self.from + (self.to - self.from) * ease
    }

    fn done(&self, now: Instant) -> bool {
        now - self.start >= self.dur
    }

    /// From wherever it is now towards `to`.
    fn towards(&self, to: f32, dur: Duration) -> Self {
        let now = Instant::now();
        Self {
            from: self.at(now),
            to,
            start: now,
            dur,
        }
    }
}

struct Bubble {
    hwnd: HWND,
    look: Option<Look>,
    image: Option<Image>,
    /// A memory DC holding the drawn bitmap, for UpdateLayeredWindow.
    dc: HDC,
    bitmap: HBITMAP,
    x: Tween,
    y: Tween,
    alpha: Tween,
    /// Shown, or fading in; false while fading out or hidden.
    visible: bool,
    /// Window is on screen (also while fading out).
    on_screen: bool,
    /// Caret position the bubble was placed for.
    anchor: POINT,
}

thread_local! {
    static BUBBLE: RefCell<Option<Bubble>> = const { RefCell::new(None) };
    /// For apps that draw their own caret; created on first use.
    static AUTOMATION: Option<uia::Automation> = uia::Automation::new();
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// Creates the (hidden) bubble window. Call once, on the worker thread.
pub fn create() {
    let class: Vec<u16> = wide("PhitPupBubble").into_iter().chain([0]).collect();
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
    BUBBLE.with_borrow_mut(|b| {
        *b = Some(Bubble {
            hwnd,
            look: None,
            image: None,
            dc,
            bitmap: std::ptr::null_mut(),
            x: Tween::still(0.0),
            y: Tween::still(0.0),
            alpha: Tween::still(0.0),
            visible: false,
            on_screen: false,
            anchor: POINT { x: 0, y: 0 },
        })
    });
}

fn current() -> Option<(String, Lang)> {
    APP.get().and_then(|app| {
        app.suggestion
            .lock()
            .unwrap()
            .as_ref()
            .map(|s| (s.edit.insert.clone(), s.edit.switch_to))
    })
}

/// The suggestion changed: act on it once it settles.
pub fn schedule() {
    let has = current().is_some();
    BUBBLE.with_borrow(|b| {
        let Some(b) = b else { return };
        let ms = match (b.visible, has) {
            (false, true) => SHOW_DELAY_MS,
            (true, false) => HIDE_DELAY_MS,
            _ => UPDATE_DELAY_MS,
        };
        // SAFETY: our own window; re-arming the same id restarts the timer.
        unsafe { SetTimer(b.hwnd, SETTLE, ms, None) };
    });
}

/// Fades the bubble out now (a suggestion was accepted).
pub fn hide() {
    BUBBLE.with_borrow_mut(|b| {
        if let Some(b) = b {
            // SAFETY: our own window.
            unsafe { KillTimer(b.hwnd, SETTLE) };
            fade_out(b);
        }
    });
}

fn settle() {
    match current() {
        Some((word, lang)) => show(word, lang),
        None => hide(),
    }
}

fn fade_out(b: &mut Bubble) {
    if !b.visible {
        return;
    }
    b.visible = false;
    b.alpha = b.alpha.towards(0.0, FADE_OUT);
    start_frames(b);
}

fn start_frames(b: &Bubble) {
    // SAFETY: our own window.
    unsafe { SetTimer(b.hwnd, FRAME, FRAME_MS, None) };
}

fn monitor_of(p: POINT) -> (RECT, u32) {
    // SAFETY: plain queries into zeroed structs.
    unsafe {
        let monitor = MonitorFromPoint(p, MONITOR_DEFAULTTONEAREST);
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        GetMonitorInfoW(monitor, &mut mi);
        let (mut dx, mut dy) = (96u32, 96u32);
        if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dx, &mut dy) < 0 {
            dx = 96;
        }
        (mi.rcWork, dx)
    }
}

fn show(word: String, lang: Lang) {
    let (anchor, caret_h) = caret();
    let (work, dpi) = monitor_of(anchor);
    let scale = dpi as f32 / 96.0;
    let look = Look {
        word,
        lang,
        dpi,
        dark: dark_theme(),
    };

    BUBBLE.with_borrow_mut(|b| {
        let Some(b) = b else { return };
        if b.look.as_ref() != Some(&look) {
            let image = render(&look);
            install(b, &image);
            b.image = Some(image);
            b.look = Some(look);
        }
        let card = b.image.as_ref().unwrap().card;

        // Line the badge up with the caret, card just under it; above it if
        // there is no room below.
        let gap = (4.0 * scale) as i32;
        let x = (anchor.x - card.left).clamp(
            work.left - card.left,
            (work.right - card.right).max(work.left - card.left),
        );
        let mut y = anchor.y + gap - card.top;
        if y + card.bottom > work.bottom {
            y = anchor.y - caret_h - gap - card.bottom;
        }

        let now = Instant::now();
        if !b.visible {
            // Fade in, sliding up into place.
            let slide = SLIDE_PX * scale;
            let from_alpha = if b.on_screen { b.alpha.at(now) } else { 0.0 };
            b.x = Tween::still(x as f32);
            b.y = Tween {
                from: y as f32 + slide,
                to: y as f32,
                start: now,
                dur: FADE_IN,
            };
            b.alpha = Tween {
                from: from_alpha,
                to: 1.0,
                start: now,
                dur: FADE_IN,
            };
            b.visible = true;
            b.anchor = anchor;
            frame(b);
            if !b.on_screen {
                // SAFETY: our own window; shown without taking focus.
                unsafe {
                    SetWindowPos(
                        b.hwnd,
                        HWND_TOPMOST,
                        0,
                        0,
                        0,
                        0,
                        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
                    )
                };
                b.on_screen = true;
            }
            start_frames(b);
            return;
        }

        // Already showing: stay put while the caret moves along the same
        // line inside (or just past) the card; glide otherwise.
        let left = b.x.to as i32 + card.left;
        let right = b.x.to as i32 + card.right;
        let same_line = (anchor.y - b.anchor.y).abs() <= caret_h / 2;
        let near = anchor.x >= left - gap && anchor.x <= right + (40.0 * scale) as i32;
        let fits = right <= work.right;
        if same_line && near && fits {
            frame(b); // the image may have changed size
            return;
        }
        b.anchor = anchor;
        b.x = b.x.towards(x as f32, GLIDE);
        b.y = b.y.towards(y as f32, GLIDE);
        start_frames(b);
    });
}

/// Puts a freshly drawn image into the memory DC.
fn install(b: &mut Bubble, image: &Image) {
    // SAFETY: the new DIB is filled with exactly w*h pixels and selected into
    // our DC, which deselects the previous one before it is deleted.
    unsafe {
        let (bitmap, bits) = dib(b.dc, image.w, image.h);
        std::ptr::copy_nonoverlapping(image.px.as_ptr(), bits, image.px.len());
        SelectObject(b.dc, bitmap);
        if !b.bitmap.is_null() {
            DeleteObject(b.bitmap);
        }
        b.bitmap = bitmap;
    }
}

/// Shows the current image at the animated position and opacity. Returns
/// true once the animation has finished.
fn frame(b: &mut Bubble) -> bool {
    let now = Instant::now();
    let Some(image) = &b.image else { return true };
    let pos = POINT {
        x: b.x.at(now).round() as i32,
        y: b.y.at(now).round() as i32,
    };
    let size = SIZE {
        cx: image.w,
        cy: image.h,
    };
    let src = POINT { x: 0, y: 0 };
    let blend = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: (b.alpha.at(now).clamp(0.0, 1.0) * 255.0).round() as u8,
        AlphaFormat: AC_SRC_ALPHA as u8,
    };
    // SAFETY: our window and DC; all pointers are to locals.
    unsafe {
        UpdateLayeredWindow(
            b.hwnd,
            std::ptr::null_mut(),
            &pos,
            &size,
            b.dc,
            &src,
            0,
            &blend,
            ULW_ALPHA,
        )
    };
    let done = b.x.done(now) && b.y.done(now) && b.alpha.done(now);
    if done && !b.visible && b.on_screen {
        // SAFETY: our own window.
        unsafe { ShowWindow(b.hwnd, SW_HIDE) };
        b.on_screen = false;
    }
    done
}

/// A top-down 32-bit DIB section of `w` x `h` pixels.
///
/// SAFETY: `dc` must be a valid DC; the caller owns the bitmap.
unsafe fn dib(dc: HDC, w: i32, h: i32) -> (HBITMAP, *mut u32) {
    // SAFETY: a zeroed BITMAPINFO with the header filled in.
    unsafe {
        let mut bmi: BITMAPINFO = std::mem::zeroed();
        bmi.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..std::mem::zeroed()
        };
        let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
        let bitmap = CreateDIBSection(dc, &bmi, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
        (bitmap, bits.cast())
    }
}

fn font(pt: i32, weight: u32, dpi: u32) -> HFONT {
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
            ANTIALIASED_QUALITY.into(),
            0,
            face.as_ptr(),
        )
    }
}

#[derive(Clone, Copy)]
struct Box2 {
    l: f32,
    t: f32,
    r: f32,
    b: f32,
}

impl Box2 {
    fn new(l: i32, t: i32, w: i32, h: i32) -> Self {
        Self {
            l: l as f32,
            t: t as f32,
            r: (l + w) as f32,
            b: (t + h) as f32,
        }
    }

    /// Signed distance from (x, y) to this box with corner radius `rad`:
    /// negative inside, positive outside.
    fn distance(&self, x: f32, y: f32, rad: f32) -> f32 {
        let (cx, cy) = ((self.l + self.r) / 2.0, (self.t + self.b) / 2.0);
        let (hx, hy) = ((self.r - self.l) / 2.0, (self.b - self.t) / 2.0);
        let qx = (x - cx).abs() - (hx - rad);
        let qy = (y - cy).abs() - (hy - rad);
        let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
        outside + qx.max(qy).min(0.0) - rad
    }
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

/// Coverage of a shape at signed distance `d`, anti-aliased over a pixel.
fn coverage(d: f32) -> f32 {
    (0.5 - d).clamp(0.0, 1.0)
}

/// Draws the bubble for `look`.
fn render(look: &Look) -> Image {
    let s = look.dpi as f32 / 96.0;
    let px = |v: f32| (v * s).round() as i32;
    let pal = if look.dark { &DARK } else { &LIGHT };
    let word = wide(&look.word);
    let badge = wide(match look.lang {
        Lang::Thai => "TH",
        Lang::English => "EN",
    });
    let hint = wide("Tab");

    // SAFETY: GDI objects are created, selected, restored and deleted here;
    // the mask DIB holds w*h pixels and is read only after GdiFlush.
    unsafe {
        let dc = CreateCompatibleDC(std::ptr::null_mut());
        let big = font(WORD_PT, FW_NORMAL, look.dpi);
        let small = font(SMALL_PT, FW_SEMIBOLD, look.dpi);
        let measure = |f: HFONT, t: &[u16]| {
            let mut size = SIZE { cx: 0, cy: 0 };
            SelectObject(dc, f);
            GetTextExtentPoint32W(dc, t.as_ptr(), t.len() as i32, &mut size);
            size
        };
        let ws = measure(big, &word);
        let bs = measure(small, &badge);
        let hs = measure(small, &hint);

        let m = px(SHADOW);
        let (pad, gap) = (px(PAD_X), px(GAP));
        let (badge_w, badge_h) = (bs.cx + px(10.0), bs.cy + px(2.0));
        let (key_w, key_h) = (hs.cx + px(12.0), hs.cy + px(4.0));
        let card_h = ws.cy + px(12.0);
        let card_w = pad + badge_w + gap + ws.cx + px(12.0) + key_w + pad;
        let (w, h) = (card_w + 2 * m, card_h + 2 * m);
        let mid = m + card_h / 2;

        let card = Box2::new(m, m, card_w, card_h);
        let badge_box = Box2::new(m + pad, mid - badge_h / 2, badge_w, badge_h);
        let word_x = m + pad + badge_w + gap;
        let key_box = Box2::new(m + card_w - pad - key_w, mid - key_h / 2, key_w, key_h);

        // Text as white-on-black coverage.
        let (mask_bmp, mask) = dib(dc, w, h);
        let old = SelectObject(dc, mask_bmp);
        SetBkMode(dc, TRANSPARENT as i32);
        SetTextColor(dc, 0x00FF_FFFF);
        SelectObject(dc, big);
        TextOutW(
            dc,
            word_x,
            mid - ws.cy / 2,
            word.as_ptr(),
            word.len() as i32,
        );
        SelectObject(dc, small);
        TextOutW(
            dc,
            badge_box.l as i32 + (badge_w - bs.cx) / 2,
            badge_box.t as i32 + (badge_h - bs.cy) / 2,
            badge.as_ptr(),
            badge.len() as i32,
        );
        TextOutW(
            dc,
            key_box.l as i32 + (key_w - hs.cx) / 2,
            key_box.t as i32 + (key_h - hs.cy) / 2,
            hint.as_ptr(),
            hint.len() as i32,
        );
        GdiFlush();
        let mask = std::slice::from_raw_parts(mask, (w * h) as usize).to_vec();
        SelectObject(dc, old);
        DeleteObject(mask_bmp);
        DeleteObject(big);
        DeleteObject(small);
        DeleteDC(dc);

        let radius = RADIUS * s;
        let blur = 12.0 * s;
        let drop = 3.0 * s;
        let shadow_box = Box2 {
            t: card.t + drop,
            b: card.b + drop,
            ..card
        };
        let mut out = vec![0u32; (w * h) as usize];
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) as usize;
                let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);

                let d = card.distance(fx, fy, radius);
                let body = coverage(d);
                let t = (shadow_box.distance(fx, fy, radius) / blur).clamp(0.0, 1.0);
                let shade = pal.shadow * (1.0 - t) * (1.0 - t);
                if body <= 0.0 && shade <= 0.0 {
                    continue;
                }

                let mut c = pal.card;
                let ring = (1.0 - (d + 0.5).abs()).clamp(0.0, 1.0);
                c = mix(c, pal.border.0, ring * pal.border.1);
                let in_badge = coverage(badge_box.distance(fx, fy, badge_h as f32 / 2.0));
                c = mix(c, pal.accent, in_badge);
                let in_key = coverage(key_box.distance(fx, fy, 4.0 * s));
                c = mix(c, pal.key, in_key);
                let ink = (mask[i] & 0xFF) as f32 / 255.0;
                let ink_color = if in_badge > 0.5 {
                    pal.on_accent
                } else if in_key > 0.5 {
                    pal.hint
                } else {
                    pal.text
                };
                c = mix(c, ink_color, ink);

                // Card over its shadow, premultiplied.
                let a = body + shade * (1.0 - body);
                let to8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
                out[i] = to8(c[2] * body)
                    | to8(c[1] * body) << 8
                    | to8(c[0] * body) << 16
                    | to8(a) << 24;
            }
        }
        Image {
            w,
            h,
            px: out,
            card: RECT {
                left: m,
                top: m,
                right: m + card_w,
                bottom: m + card_h,
            },
        }
    }
}

/// Bottom-left of the caret in screen pixels, and its height. Apps that draw
/// their own caret (browsers, Electron) are asked through UI Automation; if
/// that fails too, the mouse pointer stands in for it.
fn caret() -> (POINT, i32) {
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
            return (p, r.bottom - r.top);
        }
        if let Some(r) = AUTOMATION.with(|a| a.as_ref().and_then(uia::Automation::caret)) {
            return (
                POINT {
                    x: r.left,
                    y: r.bottom,
                },
                r.bottom - r.top,
            );
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
        WM_TIMER if wparam == SETTLE => {
            // SAFETY: our own window.
            unsafe { KillTimer(hwnd, SETTLE) };
            settle();
            0
        }
        WM_TIMER if wparam == FRAME => {
            let done = BUBBLE.with_borrow_mut(|b| b.as_mut().is_none_or(frame));
            if done {
                // SAFETY: our own window.
                unsafe { KillTimer(hwnd, FRAME) };
            }
            0
        }
        // SAFETY: default handling for everything else.
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes the bubble over light and dark backgrounds as BMP files into
    /// `PHITPUP_PREVIEW_DIR`, to look at without a live desktop:
    /// `cargo test -p phitpup preview -- --ignored`
    #[test]
    #[ignore]
    fn preview() {
        let dir = std::path::PathBuf::from(std::env::var("PHITPUP_PREVIEW_DIR").unwrap());
        for (dark, bg) in [(false, 0xF3F3F3u32), (true, 0x202020)] {
            for dpi in [96, 144] {
                for (word, lang) in [("สวัสดี", Lang::Thai), ("hello", Lang::English)] {
                    let look = Look {
                        word: word.into(),
                        lang,
                        dpi,
                        dark,
                    };
                    let img = render(&look);
                    let name = format!(
                        "bubble-{}-{}-{dpi}.bmp",
                        if dark { "dark" } else { "light" },
                        if lang == Lang::Thai { "th" } else { "en" }
                    );
                    write_bmp(&dir.join(name), &img, hex(bg));
                }
            }
        }
    }

    /// The image composited over `bg`, as a 24-bit bottom-up BMP.
    fn write_bmp(path: &std::path::Path, img: &Image, bg: Rgb) {
        let row = (img.w * 3 + 3) & !3;
        let size = 54 + row * img.h;
        let mut f = Vec::with_capacity(size as usize);
        f.extend(b"BM");
        f.extend((size as u32).to_le_bytes());
        f.extend([0u8; 4]);
        f.extend(54u32.to_le_bytes());
        f.extend(40u32.to_le_bytes());
        f.extend(img.w.to_le_bytes());
        f.extend(img.h.to_le_bytes());
        f.extend(1u16.to_le_bytes());
        f.extend(24u16.to_le_bytes());
        f.extend([0u8; 24]);
        for y in (0..img.h).rev() {
            for x in 0..img.w {
                let p = img.px[(y * img.w + x) as usize];
                let a = (p >> 24) as f32 / 255.0;
                for (ch, shift) in [(2, 0), (1, 8), (0, 16)] {
                    let v = ((p >> shift) & 0xFF) as f32 / 255.0 + bg[ch] * (1.0 - a);
                    f.push((v.clamp(0.0, 1.0) * 255.0).round() as u8);
                }
            }
            f.resize(f.len() + (row - img.w * 3) as usize, 0);
        }
        std::fs::write(path, f).unwrap();
    }
}
