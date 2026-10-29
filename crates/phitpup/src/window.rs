//! The settings window, opened by clicking the tray icon.
//!
//! Five pages: General (on/off, mode, start with Windows, full screen),
//! Programs (where PhitPup is off or shows no bubble), Words (never
//! suggested), Shortcuts and About. Every change is saved to `config.toml` at once
//! and applies right away; the window always shows the running settings, so
//! changes from the tray menu or the file show up here too.
//!
//! Like the bubble it is drawn by hand: shapes are anti-aliased per pixel
//! into a bitmap, then GDI adds ClearType text on top. The look is a red pen
//! on paper, as a teacher corrects homework: cream paper (or dark paper in
//! the dark app theme), hairlines instead of cards, the current page and
//! choices circled in pen, ticks for switches, Mali for the handwriting
//! (see [`fonts`](crate::fonts)). It lives on the tray thread.

use std::cell::RefCell;

use phitpup_core::config::{self, Config, Mode};
use phitpup_core::hotkey::{self, Hotkey, VK_ESCAPE};
use phitpup_core::icon::{self, Tone};
use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Globalization::{SCRIPT_ITEM, SCRIPT_LOGATTR, ScriptBreak, ScriptItemize};
use windows_sys::Win32::Graphics::Dwm::{
    DWMWA_CAPTION_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute,
};
use windows_sys::Win32::Graphics::Gdi::{
    ANTIALIASED_QUALITY, FW_BOLD, GdiFlush, GetTextFaceW, GetTextMetricsW, HBITMAP, HGDIOBJ,
    TEXTMETRICW,
};
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BeginPaint, BitBlt, CLEARTYPE_QUALITY, ClientToScreen,
    CreateCompatibleDC, CreateDIBSection, CreateFontW, DEFAULT_CHARSET, DIB_RGB_COLORS,
    DT_CALCRECT, DT_CENTER, DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX, DT_RIGHT, DT_SINGLELINE, DT_TOP,
    DT_VCENTER, DT_WORDBREAK, DeleteDC, DeleteObject, DrawTextW, EndPaint, FW_NORMAL, FW_SEMIBOLD,
    GetMonitorInfoW, GetTextExtentPoint32W, HDC, HFONT, InvalidateRect, MONITOR_DEFAULTTONEAREST,
    MONITORINFO, MonitorFromPoint, PAINTSTRUCT, SRCCOPY, SelectObject, SetBkMode, SetTextColor,
    TRANSPARENT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::{
    AdjustWindowRectExForDpi, GetDpiForMonitor, GetDpiForSystem, GetDpiForWindow,
    GetSystemMetricsForDpi, MDT_EFFECTIVE_DPI,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent, VK_CONTROL, VK_LCONTROL, VK_LMENU,
    VK_LSHIFT, VK_LWIN, VK_MENU, VK_RCONTROL, VK_RETURN, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT,
    VK_SPACE, VK_TAB,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    EnumWindows, GW_OWNER, GWL_EXSTYLE, GetCaretBlinkTime, GetClientRect, GetCursorPos, GetWindow,
    GetWindowLongPtrW, GetWindowTextLengthW, HICON, HTCLIENT, IDC_ARROW, IDC_IBEAM, IMAGE_ICON,
    IsIconic, IsWindowVisible, KillTimer, LoadCursorW, LoadIconW, LoadImageW, MF_GRAYED, MF_STRING,
    PostMessageW, RegisterClassExW, SM_CXSMICON, SW_RESTORE, SW_SHOW, SWP_NOACTIVATE, SWP_NOZORDER,
    SetCursor, SetForegroundWindow, SetTimer, SetWindowPos, ShowWindow, TPM_LEFTALIGN,
    TPM_RETURNCMD, TPM_TOPALIGN, TrackPopupMenu, WM_CHAR, WM_CLOSE, WM_DESTROY, WM_DPICHANGED,
    WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NULL, WM_PAINT,
    WM_SETCURSOR, WM_SETTINGCHANGE, WM_SYSCHAR, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_TIMER, WNDCLASSEXW,
    WS_CAPTION, WS_EX_TOOLWINDOW, WS_MINIMIZEBOX, WS_OVERLAPPED, WS_SYSMENU,
};

use crate::i18n::{t, thai};
use crate::{APP, bubble, clipboard, context, fonts, settings, tray, update};

// Layout at 96 DPI.
const W: f32 = 860.0;
const H: f32 = 640.0;
const SIDEBAR: f32 = 240.0;
const LEFT: f32 = 264.0;
const RIGHT: f32 = W - 32.0;
const WIDTH: f32 = RIGHT - LEFT;
/// Longest word the Words page accepts.
const MAX_WORD: usize = 40;

const BLINK: usize = 1;
/// From WinUser.h; windows-sys keeps it behind the UI_Controls feature.
const WM_MOUSELEAVE: u32 = 0x02A3;
const STYLE: u32 = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;

// Segoe MDL2 Assets glyphs (on Windows 10 and 11).
const G_SETTINGS: char = '\u{E713}';
const G_APPS: char = '\u{E71D}';
const G_FONT: char = '\u{E8D2}';
const G_KEYBOARD: char = '\u{E765}';
const G_POWER: char = '\u{E7E8}';
const G_FULLSCREEN: char = '\u{E740}';
const G_FILE: char = '\u{E8A7}';
const G_BULB: char = '\u{EA80}';
const G_EDIT: char = '\u{E70F}';
const G_UNDERLINE: char = '\u{E8DC}';
const G_BELL: char = '\u{EA8F}';
const G_UPDATE: char = '\u{E895}';
const G_INFO: char = '\u{E946}';
const G_PERSON: char = '\u{E77B}';
const G_PEOPLE: char = '\u{E716}';
const G_LOCK: char = '\u{E72E}';
const G_DOC: char = '\u{E8A5}';
const G_CLOSE: char = '\u{E711}';
const G_ADD: char = '\u{E710}';
const G_HIDE: char = '\u{ED1A}';
const G_BLOCK: char = '\u{F140}';

type Rgb = [f32; 3];

const fn hex(v: u32) -> Rgb {
    [
        ((v >> 16) & 0xFF) as f32 / 255.0,
        ((v >> 8) & 0xFF) as f32 / 255.0,
        (v & 0xFF) as f32 / 255.0,
    ]
}

/// Paper and a red pen.
struct Palette {
    bg: Rgb,
    card_hover: Rgb,
    border: Rgb,
    text: Rgb,
    sub: Rgb,
    faint: Rgb,
    nav: Rgb,
    control_hover: Rgb,
    control_border: Rgb,
    accent: Rgb,
    key: Rgb,
    key_border: Rgb,
    error: Rgb,
    /// The notebook's margin line beside the page list.
    margin: Rgb,
}

const LIGHT: Palette = Palette {
    bg: hex(0xFBF7F0),
    card_hover: hex(0xF5EFE5),
    border: hex(0xE6DED2),
    text: hex(0x221F1C),
    sub: hex(0x6A625A),
    faint: hex(0x8C847B),
    nav: hex(0xF3ECE1),
    control_hover: hex(0xF5EFE5),
    control_border: hex(0xCFC6BA),
    accent: hex(0xD93A12),
    key: hex(0xFFFDF8),
    key_border: hex(0xCFC6BA),
    error: hex(0xB3261E),
    margin: hex(0xEDC6B8),
};

const DARK: Palette = Palette {
    bg: hex(0x1E1B18),
    card_hover: hex(0x2F2A26),
    border: hex(0x3A3530),
    text: hex(0xF3EEE7),
    sub: hex(0xB3AAA0),
    faint: hex(0x8C847B),
    nav: hex(0x2A2622),
    control_hover: hex(0x332E29),
    control_border: hex(0x4A443E),
    accent: hex(0xFF7A52),
    key: hex(0x2A2622),
    key_border: hex(0x4A443E),
    error: hex(0xFF8A80),
    margin: hex(0x4A2A1E),
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Page {
    General,
    Programs,
    Words,
    Shortcuts,
    About,
}

const PAGES: [(Page, char, &str); 5] = [
    (Page::General, G_SETTINGS, "General"),
    (Page::Programs, G_APPS, "Programs"),
    (Page::Words, G_FONT, "Words"),
    (Page::Shortcuts, G_KEYBOARD, "Shortcuts"),
    (Page::About, G_INFO, "About"),
];

/// Who makes PhitPup, for the About page.
const AUTHOR: &str = "Intity01";
const TEAM: &str = "sukudone.team-dev";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Which {
    Fix,
    Accept,
}

/// Something that reacts to a click.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Hit {
    Nav(Page),
    On,
    Mode(Mode),
    Startup,
    Fullscreen,
    OpenFile,
    Remind,
    DoubleShift,
    Underline,
    Spelling,
    Typos,
    Updates,
    Report,
    AddProgram,
    /// Program `i` of [`programs`]: off (true) or no bubble (false).
    Kind(usize, bool),
    RemoveProgram(usize),
    Input,
    AddWord,
    RemoveWord(usize),
    RemoveLearned(usize),
    Learn,
    Change(Which),
    ResetKeys,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct R {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl R {
    const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    fn contains(&self, (x, y): (f32, f32)) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }

    fn inset(&self, d: f32) -> R {
        R::new(self.x + d, self.y + d, self.w - 2.0 * d, self.h - 2.0 * d)
    }

    fn scaled(&self, s: f32) -> RECT {
        RECT {
            left: (self.x * s).round() as i32,
            top: (self.y * s).round() as i32,
            right: ((self.x + self.w) * s).round() as i32,
            bottom: ((self.y + self.h) * s).round() as i32,
        }
    }
}

#[derive(Clone, Copy)]
enum F {
    Title,
    Body,
    Strong,
    Caption,
    Brand,
    Key,
    /// For Thai and for words the user typed.
    Word,
    SmallGlyph,
    /// The red pen's handwriting.
    Pen,
    PenBig,
}

const SANS: &[&str] = &[fonts::SANS, "Leelawadee UI"];
const MEDIUM: &[&str] = &[fonts::SANS_MEDIUM, "Leelawadee UI"];
const PEN: &[&str] = &[fonts::PEN, "Segoe Print"];
const SERIF: &[&str] = &[fonts::SERIF, "Times New Roman"];
const SERIF_THAI: &[&str] = &[fonts::SERIF_THAI, "Leelawadee UI"];
const ICONS: &[&str] = &["Segoe Fluent Icons", "Segoe MDL2 Assets"];
const FW_MEDIUM: u32 = 500;

/// (size in px at 96 DPI, weight, faces to try in order) for each [`F`].
/// The first faces are bundled ([`fonts`]); the others are fallbacks.
const FONTS: [(i32, u32, &[&str]); 10] = [
    (30, FW_BOLD, SERIF),
    (14, FW_NORMAL, SANS),
    (14, FW_MEDIUM, MEDIUM),
    (12, FW_NORMAL, SANS),
    (26, FW_BOLD, SERIF_THAI),
    (12, FW_MEDIUM, MEDIUM),
    (15, FW_NORMAL, SANS),
    (10, FW_NORMAL, ICONS),
    (16, FW_SEMIBOLD, PEN),
    (22, FW_SEMIBOLD, PEN),
];

struct Fonts([HFONT; FONTS.len()]);

impl Fonts {
    fn new(dpi: u32) -> Self {
        fonts::load();
        // SAFETY: a memory DC only used to check which face GDI picked.
        let dc = unsafe { CreateCompatibleDC(std::ptr::null_mut()) };
        let fonts = Fonts(FONTS.map(|(px, weight, faces)| {
            // Thai headings in a Thai serif; the Latin one has no Thai.
            let faces = if faces == SERIF && thai() {
                SERIF_THAI
            } else {
                faces
            };
            let quality = if faces == ICONS {
                ANTIALIASED_QUALITY // no colour fringes on thin icons
            } else {
                CLEARTYPE_QUALITY
            };
            let mut font: HFONT = std::ptr::null_mut();
            for (i, face) in faces.iter().enumerate() {
                if !font.is_null() {
                    // SAFETY: a font we made and did not keep.
                    unsafe { DeleteObject(font) };
                }
                font = create_font(-(px * dpi as i32 / 96), weight, quality, face);
                if i + 1 == faces.len() || picked(dc, font, face) {
                    break;
                }
            }
            font
        }));
        // SAFETY: the DC made above; no font of ours stays selected in it.
        unsafe { DeleteDC(dc) };
        fonts
    }

    fn get(&self, f: F) -> HFONT {
        self.0[f as usize]
    }
}

/// `s` with line breaks where it must wrap to fit `width` pixels in the
/// DC's font. DrawText only wraps at spaces, and Thai puts none between
/// words, so the break points come from Windows' own word breaking
/// (Uniscribe), which knows Thai.
fn wrap(dc: HDC, s: &str, width: i32) -> Vec<u16> {
    let text: Vec<u16> = s.encode_utf16().collect();
    let n = text.len();
    if n == 0 {
        return text;
    }
    // Where a line may start: after a space or a newline, or before a
    // soft break Uniscribe found.
    let mut can_break = vec![false; n + 1];
    // SAFETY: buffers sized for the text; each item is broken within its
    // own range of `text` and `attrs`.
    unsafe {
        let mut items: Vec<SCRIPT_ITEM> = vec![std::mem::zeroed(); n + 1];
        let mut count = 0;
        if ScriptItemize(
            text.as_ptr(),
            n as i32,
            items.len() as i32,
            std::ptr::null(),
            std::ptr::null(),
            items.as_mut_ptr(),
            &mut count,
        ) >= 0
        {
            let mut attrs: Vec<SCRIPT_LOGATTR> = vec![std::mem::zeroed(); n];
            for i in 0..count as usize {
                let (start, end) = (items[i].iCharPos as usize, items[i + 1].iCharPos as usize);
                ScriptBreak(
                    text[start..].as_ptr(),
                    (end - start) as i32,
                    &items[i].a,
                    attrs[start..].as_mut_ptr(),
                );
            }
            for (i, a) in attrs.iter().enumerate() {
                can_break[i] |= a._bitfield & 1 != 0; // fSoftBreak
            }
        }
    }
    for i in 0..n {
        if text[i] == u16::from(b' ') || text[i] == u16::from(b'\n') {
            can_break[i + 1] = true;
        }
    }
    let width_of = |part: &[u16]| {
        let mut size = windows_sys::Win32::Foundation::SIZE { cx: 0, cy: 0 };
        // SAFETY: measuring with the caller's DC and font.
        unsafe { GetTextExtentPoint32W(dc, part.as_ptr(), part.len() as i32, &mut size) };
        size.cx
    };
    let mut out = Vec::with_capacity(n + 8);
    let mut line = 0;
    let mut last_break = None;
    for i in 1..=n {
        if text[i - 1] == u16::from(b'\n') {
            out.extend_from_slice(&text[line..i]);
            line = i;
            last_break = None;
            continue;
        }
        let end = text[line..i]
            .iter()
            .rposition(|&c| c != u16::from(b' '))
            .map_or(line, |p| line + p + 1);
        if width_of(&text[line..end]) > width
            && let Some(b) = last_break.filter(|&b| b > line)
        {
            out.extend_from_slice(&text[line..b]);
            while out.last() == Some(&u16::from(b' ')) {
                out.pop();
            }
            out.push(u16::from(b'\n'));
            line = b;
            last_break = None;
        }
        if can_break[i] {
            last_break = Some(i);
        }
    }
    out.extend_from_slice(&text[line..]);
    out
}

fn create_font(height: i32, weight: u32, quality: u8, face: &str) -> HFONT {
    let face = wide(face);
    // SAFETY: plain font creation; the face name is NUL-terminated.
    unsafe {
        CreateFontW(
            height,
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
            quality.into(),
            0,
            face.as_ptr(),
        )
    }
}

/// Whether GDI gave `font` the face asked for, upright. A missing face is
/// silently replaced, and on some systems a missing upright weight by the
/// italic one.
fn picked(dc: HDC, font: HFONT, face: &str) -> bool {
    // SAFETY: the font is selected only for these queries.
    unsafe {
        let old = SelectObject(dc, font);
        let mut name = [0u16; 64];
        let n = GetTextFaceW(dc, name.len() as i32, name.as_mut_ptr());
        let mut tm: TEXTMETRICW = std::mem::zeroed();
        GetTextMetricsW(dc, &mut tm);
        SelectObject(dc, old);
        let got = String::from_utf16_lossy(&name[..(n.max(1) - 1) as usize]);
        got.eq_ignore_ascii_case(face) && tm.tmItalic == 0
    }
}

impl Drop for Fonts {
    fn drop(&mut self) {
        for f in self.0 {
            // SAFETY: we created these fonts and they are not selected anywhere.
            unsafe { DeleteObject(f) };
        }
    }
}

struct Win {
    hwnd: HWND,
    dpi: u32,
    dark: bool,
    fonts: Fonts,
    page: Page,
    /// Mouse position in layout units, if it is over the window.
    mouse: Option<(f32, f32)>,
    tracking: bool,
    scroll: f32,
    max_scroll: f32,
    /// Clickable areas from the last paint, in drawing order.
    hits: Vec<(R, Hit)>,
    input: Option<R>,
    /// The word being typed on the Words page.
    draft: String,
    focus: bool,
    caret_on: bool,
    /// Listening for a new shortcut.
    capture: Option<Which>,
    /// A message under the control that was just used.
    note: Option<String>,
    /// What the keyboard focus ring is on (Tab moves it).
    kb: Option<Hit>,
}

impl Win {
    fn scale(&self) -> f32 {
        self.dpi as f32 / 96.0
    }

    fn hit_at(&self, p: (f32, f32)) -> Option<Hit> {
        self.hits
            .iter()
            .rev()
            .find(|(r, _)| r.contains(p))
            .map(|&(_, h)| h)
    }
}

thread_local! {
    static WIN: RefCell<Option<Win>> = const { RefCell::new(None) };
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

/// Runs `f` on the window state, unless there is none or it is in use
/// further up the stack (a modal loop re-entering the window procedure).
fn with<T>(f: impl FnOnce(&mut Win) -> T) -> Option<T> {
    WIN.with(|w| w.try_borrow_mut().ok()?.as_mut().map(f))
}

fn hwnd() -> Option<HWND> {
    WIN.with(|w| w.try_borrow().ok()?.as_ref().map(|w| w.hwnd))
}

/// Redraws the window if it is open, to show settings changed elsewhere.
pub fn refresh() {
    if let Some(hwnd) = hwnd() {
        // SAFETY: our own window.
        unsafe { InvalidateRect(hwnd, std::ptr::null(), 0) };
    }
}

/// Closes the window if it is open.
pub fn close() {
    if let Some(hwnd) = hwnd() {
        // SAFETY: our own window, on this thread.
        unsafe { DestroyWindow(hwnd) };
    }
}

/// Opens the window on the screen with the mouse pointer, or brings it to
/// the front if it is already open.
pub fn open() {
    // SAFETY: plain window calls on our own window and class.
    unsafe {
        if let Some(hwnd) = hwnd() {
            if IsIconic(hwnd) != 0 {
                ShowWindow(hwnd, SW_RESTORE);
            }
            SetForegroundWindow(hwnd);
            return;
        }
        let instance = GetModuleHandleW(std::ptr::null());
        let class = wide("PhitPupSettings");
        let icon = LoadIconW(instance, 1 as _);
        let small = LoadImageW(
            instance,
            1 as _,
            IMAGE_ICON,
            GetSystemMetricsForDpi(SM_CXSMICON, GetDpiForSystem()),
            GetSystemMetricsForDpi(SM_CXSMICON, GetDpiForSystem()),
            0,
        ) as HICON;
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            hIcon: icon,
            hIconSm: small,
            hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
            lpszClassName: class.as_ptr(),
            ..std::mem::zeroed()
        };
        RegisterClassExW(&wc); // fails harmlessly once registered

        let mut pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pt);
        let monitor = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        GetMonitorInfoW(monitor, &mut mi);
        let (mut dpi, mut unused) = (96u32, 96u32);
        GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut unused);
        let s = dpi as f32 / 96.0;
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: (W * s).round() as i32,
            bottom: (H * s).round() as i32,
        };
        AdjustWindowRectExForDpi(&mut rect, STYLE, 0, 0, dpi);
        let (ww, wh) = (rect.right - rect.left, rect.bottom - rect.top);
        let work = mi.rcWork;
        let x = work.left + ((work.right - work.left) - ww).max(0) / 2;
        let y = work.top + ((work.bottom - work.top) - wh).max(0) / 2;

        let title = wide(t("PhitPup Settings"));
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            STYLE,
            x,
            y,
            ww,
            wh,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            debug!("could not open the settings window");
            return;
        }
        let dpi = GetDpiForWindow(hwnd).max(96);
        let dark = bubble::dark_theme();
        WIN.with_borrow_mut(|w| {
            *w = Some(Win {
                hwnd,
                dpi,
                dark,
                fonts: Fonts::new(dpi),
                page: Page::General,
                mouse: None,
                tracking: false,
                scroll: 0.0,
                max_scroll: 0.0,
                hits: Vec::new(),
                input: None,
                draft: String::new(),
                focus: false,
                caret_on: true,
                capture: None,
                note: None,
                kb: None,
            })
        });
        debug!("settings window opened");
        theme_frame(hwnd, dark);
        SetTimer(hwnd, BLINK, GetCaretBlinkTime().clamp(300, 1200), None);
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
    }
}

/// Dark or light title bar, coloured like the page.
fn theme_frame(hwnd: HWND, dark: bool) {
    let bg = if dark { DARK.bg } else { LIGHT.bg };
    let on: BOOL = dark.into();
    let caption = colorref(bg);
    // SAFETY: both attributes take a 4-byte value; older Windows ignores them.
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
            (&on as *const BOOL).cast(),
            4,
        );
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_CAPTION_COLOR as u32,
            (&caption as *const u32).cast(),
            4,
        );
    }
}

fn colorref(c: Rgb) -> u32 {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    b(c[0]) | b(c[1]) << 8 | b(c[2]) << 16
}

fn config() -> Config {
    APP.get()
        .map(|a| a.config.lock().unwrap().clone())
        .unwrap_or_default()
}

/// Changes the running settings and saves them.
fn edit(f: impl FnOnce(&mut Config)) {
    let mut c = config();
    f(&mut c);
    settings::apply(c);
}

fn paused() -> bool {
    APP.get()
        .is_some_and(|a| a.paused.load(std::sync::atomic::Ordering::Relaxed))
}

/// The user's programs: off ones first, then the ones with no bubble.
fn programs(c: &Config) -> Vec<(String, bool)> {
    let off = c.off_apps.iter().map(|a| (a.clone(), true));
    off.chain(c.quiet_apps.iter().map(|a| (a.clone(), false)))
        .collect()
}

// ---------------------------------------------------------------- drawing

enum Shape {
    Fill {
        r: R,
        rad: f32,
        c: Rgb,
    },
    /// A line `width` layout units wide just inside the edge.
    Stroke {
        r: R,
        rad: f32,
        width: f32,
        c: Rgb,
    },
    Logo {
        r: R,
        tone: Tone,
    },
    /// A pen stroke through `pts`, `width` wide.
    Pen {
        pts: Vec<(f32, f32)>,
        width: f32,
        c: Rgb,
    },
}

/// A loose hand-drawn loop around `r`, as a pen circles a word: a little
/// more than one turn, starting top left, not quite closing on itself.
fn pen_loop(r: R) -> Vec<(f32, f32)> {
    let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
    let (rx, ry) = (r.w / 2.0 + 14.0, r.h / 2.0 + 5.0);
    let start = -2.3f32;
    let turn = std::f32::consts::TAU + 0.6;
    let n = 64;
    (0..=n)
        .map(|i| {
            let k = i as f32 / n as f32;
            let t = start + turn * k;
            let wobble = 1.0 + 0.035 * (3.0 * t).sin();
            let drift = ry * 0.14 * k; // the second pass runs a little lower
            (
                cx + rx * wobble * t.cos(),
                cy + ry * wobble * t.sin() + drift,
            )
        })
        .collect()
}

/// A tick `size` wide whose long stroke overshoots the top, at (x, y).
fn pen_tick(x: f32, y: f32, size: f32) -> Vec<(f32, f32)> {
    vec![
        (x, y + 0.45 * size),
        (x + 0.35 * size, y + 0.85 * size),
        (x + 1.05 * size, y - 0.2 * size),
    ]
}

struct Text {
    r: R,
    s: Vec<u16>,
    f: F,
    c: Rgb,
    flags: u32,
}

const LINE: u32 = DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX | DT_END_ELLIPSIS;

/// Collects what one paint draws, and where clicks go.
struct Builder<'a> {
    pal: &'static Palette,
    mouse: Option<(f32, f32)>,
    dc: HDC,
    fonts: &'a Fonts,
    scale: f32,
    shapes: Vec<Shape>,
    texts: Vec<Text>,
    hits: Vec<(R, Hit)>,
    input: Option<R>,
    /// Where the page's content ends.
    bottom: f32,
}

impl Builder<'_> {
    fn fill(&mut self, r: R, rad: f32, c: Rgb) {
        self.shapes.push(Shape::Fill { r, rad, c });
    }

    fn stroke(&mut self, r: R, rad: f32, c: Rgb) {
        self.shapes.push(Shape::Stroke {
            r,
            rad,
            width: 1.0,
            c,
        });
    }

    fn text(&mut self, r: R, s: &str, f: F, c: Rgb, flags: u32) {
        self.texts.push(Text {
            r,
            s: s.encode_utf16().collect(),
            f,
            c,
            flags,
        });
    }

    fn glyph(&mut self, r: R, g: char, f: F, c: Rgb) {
        self.text(r, &g.to_string(), f, c, LINE | DT_CENTER);
    }

    /// Makes `r` clickable; true if the mouse is over it.
    fn hit(&mut self, r: R, h: Hit) -> bool {
        self.hits.push((r, h));
        self.mouse.is_some_and(|m| r.contains(m))
    }

    fn width(&self, f: F, s: &str) -> f32 {
        let s: Vec<u16> = s.encode_utf16().collect();
        let mut size = windows_sys::Win32::Foundation::SIZE { cx: 0, cy: 0 };
        // SAFETY: measuring with our own DC and font.
        unsafe {
            SelectObject(self.dc, self.fonts.get(f));
            GetTextExtentPoint32W(self.dc, s.as_ptr(), s.len() as i32, &mut size);
        }
        size.cx as f32 / self.scale
    }

    /// Word-wrapped text from (x, y), `w` wide; returns its height.
    fn para(&mut self, x: f32, y: f32, w: f32, s: &str, f: F, c: Rgb) -> f32 {
        let mut rc = R::new(x, y, w, 1000.0).scaled(self.scale);
        // SAFETY: measuring with our own DC and font.
        unsafe { SelectObject(self.dc, self.fonts.get(f)) };
        let text = wrap(self.dc, s, rc.right - rc.left);
        // SAFETY: measuring with our own DC and font.
        unsafe {
            DrawTextW(
                self.dc,
                text.as_ptr(),
                text.len() as i32,
                &mut rc,
                DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX,
            );
        }
        let h = (rc.bottom - rc.top) as f32 / self.scale + 1.0;
        self.texts.push(Text {
            r: R::new(x, y, w, h),
            s: text,
            f,
            c,
            flags: DT_WORDBREAK | DT_NOPREFIX | DT_LEFT | DT_TOP,
        });
        h
    }

    fn pen(&mut self, pts: Vec<(f32, f32)>, width: f32, c: Rgb) {
        self.shapes.push(Shape::Pen { pts, width, c });
    }

    /// Circles `r` in pen.
    fn circle(&mut self, r: R) {
        let c = self.pal.accent;
        self.pen(pen_loop(r), 1.8, c);
    }

    /// A ruled line under `r`, the paper's version of a card; a soft
    /// highlight while hovered.
    fn card(&mut self, r: R, hovered: bool) {
        let p = self.pal;
        if hovered {
            self.fill(r.inset(1.0), 6.0, p.card_hover);
        }
        self.fill(R::new(r.x, r.y + r.h - 1.0, r.w, 1.0), 0.0, p.border);
    }

    /// A settings card `h` high with an icon, a title and a caption centred
    /// in its top `head` units. With `hit`, the whole card is clickable.
    /// Returns the vertical centre of the head, for the control on the right.
    #[allow(clippy::too_many_arguments)]
    fn row(
        &mut self,
        y: f32,
        h: f32,
        head: f32,
        glyph: Option<char>,
        title: &str,
        caption: &str,
        hit: Option<Hit>,
    ) -> (f32, bool) {
        let p = self.pal;
        let r = R::new(LEFT, y, WIDTH, h);
        let hovered = hit.is_some_and(|hit| self.hit(r, hit));
        self.card(r, hovered);
        let cy = y + head / 2.0;
        let _ = glyph; // paper has no icons
        let x = LEFT + 8.0;
        let tw = WIDTH - 8.0 - 220.0;
        if caption.is_empty() {
            self.text(R::new(x, cy - 10.0, tw, 20.0), title, F::Body, p.text, LINE);
        } else {
            self.text(
                R::new(x, cy - 19.0, tw, 20.0),
                title,
                F::Strong,
                p.text,
                LINE,
            );
            self.text(
                R::new(x, cy + 1.0, tw, 18.0),
                caption,
                F::Caption,
                p.sub,
                LINE,
            );
        }
        (cy, hovered)
    }

    /// A box on the right, ticked in pen when on.
    fn toggle(&mut self, cy: f32, on: bool, hovered: bool) {
        let p = self.pal;
        let r = R::new(RIGHT - 16.0 - 22.0, cy - 11.0, 22.0, 22.0);
        if hovered {
            self.fill(r, 3.0, p.card_hover);
        }
        self.stroke(r, 3.0, p.text);
        self.stroke(r.inset(0.5), 2.5, p.text);
        if on {
            let c = p.accent;
            self.pen(pen_tick(r.x + 2.0, r.y + 1.0, 22.0), 2.6, c);
        }
    }

    fn button(&mut self, r: R, label: &str, glyph: Option<char>, accent: bool, hit: Hit) {
        let p = self.pal;
        let hovered = self.hit(r, hit);
        // The main action is ink on paper; others are drawn outlines.
        let (fill, ink, f) = match (accent, hovered) {
            (true, false) => (p.text, p.bg, F::Strong),
            (true, true) => (mix(p.text, p.accent, 0.35), p.bg, F::Strong),
            (false, false) => (p.bg, p.text, F::Strong),
            (false, true) => (p.control_hover, p.text, F::Strong),
        };
        self.fill(r, 4.0, fill);
        if !accent {
            self.stroke(r, 4.0, p.control_border);
        }
        match glyph {
            Some(g) => {
                let tw = self.width(f, label);
                let x = r.x + (r.w - tw - 22.0) / 2.0;
                self.glyph(R::new(x, r.y, 12.0, r.h), g, F::SmallGlyph, ink);
                self.text(R::new(x + 22.0, r.y, tw + 4.0, r.h), label, f, ink, LINE);
            }
            None => self.text(r, label, f, ink, LINE | DT_CENTER),
        }
    }

    /// Keycaps ending at `right`; returns where they start.
    fn keycaps(&mut self, right: f32, cy: f32, caps: &[String], muted: bool) -> f32 {
        let p = self.pal;
        let widths: Vec<f32> = caps
            .iter()
            .map(|c| (self.width(F::Key, c) + 18.0).max(30.0))
            .collect();
        let total = widths.iter().sum::<f32>() + 6.0 * (caps.len() as f32 - 1.0);
        let mut x = right - total;
        let start = x;
        for (cap, w) in caps.iter().zip(widths) {
            let r = R::new(x, cy - 14.0, w, 28.0);
            self.fill(r, 4.0, p.key);
            self.stroke(r, 4.0, p.key_border);
            let ink = if muted { p.sub } else { p.text };
            self.text(r, cap, F::Key, ink, LINE | DT_CENTER);
            x += w + 6.0;
        }
        start
    }

    fn title(&mut self, y: f32, s: &str) -> f32 {
        self.text(
            R::new(LEFT + 8.0, y, WIDTH, 40.0),
            s,
            F::Title,
            self.pal.text,
            LINE,
        );
        y + 56.0
    }

    fn section(&mut self, y: f32, s: &str) -> f32 {
        self.text(
            R::new(LEFT, y, WIDTH, 20.0),
            s,
            F::Strong,
            self.pal.text,
            LINE,
        );
        y + 28.0
    }
}

fn build<'a>(win: &'a Win, c: &Config, dc: HDC) -> Builder<'a> {
    let pal = if win.dark { &DARK } else { &LIGHT };
    let mut b = Builder {
        pal,
        mouse: win.mouse,
        dc,
        fonts: &win.fonts,
        scale: win.scale(),
        shapes: Vec::new(),
        texts: Vec::new(),
        hits: Vec::new(),
        input: None,
        bottom: 0.0,
    };
    let top = 28.0 - win.scroll;
    let bottom = match win.page {
        Page::General => general(&mut b, c, top),
        Page::Programs => programs_page(&mut b, c, top),
        Page::Words => words(&mut b, c, win, top),
        Page::Shortcuts => shortcuts(&mut b, c, win, top),
        Page::About => about(&mut b, c, top),
    };
    b.bottom = bottom + 32.0;
    sidebar(&mut b, win.page);
    // The keyboard focus ring, drawn in pen colour around the focused item.
    if let Some(&(r, _)) = win.kb.and_then(|k| b.hits.iter().find(|&&(_, h)| h == k)) {
        let p = b.pal;
        b.shapes.push(Shape::Stroke {
            r: R::new(r.x - 2.0, r.y - 2.0, r.w + 4.0, r.h + 4.0),
            rad: 6.0,
            width: 2.0,
            c: p.accent,
        });
    }
    b
}

fn sidebar(b: &mut Builder, page: Page) {
    let p = b.pal;
    // The notebook's margin line.
    b.fill(R::new(SIDEBAR - 2.0, 0.0, 2.0, H), 0.0, p.margin);
    b.text(
        R::new(28.0, 22.0, SIDEBAR - 40.0, 36.0),
        "ผิดปุ๊บ",
        F::Brand,
        p.text,
        LINE,
    );
    let name = if paused() {
        t("PhitPup · paused")
    } else {
        "PhitPup"
    };
    b.text(
        R::new(28.0, 58.0, SIDEBAR - 40.0, 18.0),
        name,
        F::Caption,
        p.sub,
        LINE,
    );

    for (i, (pg, _, label)) in PAGES.into_iter().enumerate() {
        let r = R::new(16.0, 104.0 + i as f32 * 46.0, SIDEBAR - 40.0, 38.0);
        let hovered = b.hit(r, Hit::Nav(pg));
        if hovered && pg != page {
            b.fill(r, 6.0, p.nav);
        }
        let x = r.x + 16.0;
        let ink = if pg == page { p.accent } else { p.text };
        let label = t(label);
        b.text(R::new(x, r.y, r.w - 16.0, r.h), label, F::Strong, ink, LINE);
        if pg == page {
            let tw = b.width(F::Strong, label);
            b.circle(R::new(x, r.y + 9.0, tw, 20.0));
        }
    }

    b.text(
        R::new(28.0, H - 66.0, SIDEBAR - 40.0, 24.0),
        "ไม่ส่ง ไม่จำที่พิมพ์",
        F::Pen,
        p.accent,
        LINE,
    );
    let version = t("Version {v}").replace("{v}", env!("CARGO_PKG_VERSION"));
    b.text(
        R::new(28.0, H - 40.0, SIDEBAR - 40.0, 18.0),
        &version,
        F::Caption,
        p.sub,
        LINE,
    );
}

fn general(b: &mut Builder, c: &Config, y: f32) -> f32 {
    let p = b.pal;
    let mut y = b.title(y, t("General"));

    let on = !paused();
    let caption = if on {
        t("Watching for text typed in the wrong layout.")
    } else {
        t("Paused. Every key passes through untouched.")
    };
    let (cy, hovered) = b.row(
        y,
        68.0,
        68.0,
        None,
        t("PhitPup is on"),
        caption,
        Some(Hit::On),
    );
    b.toggle(cy, on, hovered);
    y += 72.0;

    let options = [
        (
            Mode::Suggest,
            t("Suggest while typing").to_string(),
            t("A bubble shows the fix. {key} accepts it, Esc hides it.")
                .replace("{key}", &c.accept_key.label()),
        ),
        (
            Mode::Auto,
            t("Fix automatically when I press Space").to_string(),
            t("Only words it is very sure about. {key} right after undoes it.")
                .replace("{key}", &c.hotkey.label()),
        ),
        (
            Mode::Manual,
            t("Only when I press {key}").replace("{key}", &c.hotkey.label()),
            t("Fixes the last word; press again to reach further back.").to_string(),
        ),
    ];
    let h = 68.0 + 56.0 * options.len() as f32 + 6.0;
    b.row(
        y,
        h,
        68.0,
        Some(G_BULB),
        t("When to offer a fix"),
        t("The hotkey always works."),
        None,
    );
    for (i, (mode, title, caption)) in options.iter().enumerate() {
        let r = R::new(LEFT + 1.0, y + 66.0 + i as f32 * 56.0, WIDTH - 2.0, 54.0);
        let hovered = b.hit(r, Hit::Mode(*mode));
        if hovered {
            b.fill(r.inset(2.0), 6.0, p.card_hover);
        }
        let cy = r.y + r.h / 2.0;
        let chosen = c.mode == *mode;
        let x = LEFT + 40.0;
        let tw = WIDTH - 60.0;
        let ink = if chosen { p.accent } else { p.text };
        b.text(R::new(x, cy - 19.0, tw, 20.0), title, F::Strong, ink, LINE);
        if chosen {
            let w = b.width(F::Strong, title);
            b.circle(R::new(x, cy - 18.0, w, 18.0));
        }
        let x = LEFT + 40.0;
        b.text(
            R::new(x, cy + 3.0, tw, 18.0),
            caption,
            F::Caption,
            p.sub,
            LINE,
        );
    }
    y += h + 4.0;

    let (cy, hovered) = b.row(
        y,
        68.0,
        68.0,
        Some(G_UNDERLINE),
        t("Underline the word instead of a bubble"),
        t("Apps that cannot show it get the bubble."),
        Some(Hit::Underline),
    );
    b.toggle(cy, c.underline, hovered);
    y += 72.0;

    let (cy, hovered) = b.row(
        y,
        68.0,
        68.0,
        Some(G_FONT),
        t("Fix Thai typed in the wrong order"),
        t("Such as เเม่ → แม่, ทํา → ทำ, ก่ี → กี่. Tab accepts."),
        Some(Hit::Spelling),
    );
    b.toggle(cy, c.spelling, hovered);
    y += 72.0;

    let (cy, hovered) = b.row(
        y,
        68.0,
        68.0,
        Some(G_EDIT),
        t("Fix Caps Lock slips and doubled words"),
        t("Such as hELLO \u{2192} Hello and the the \u{2192} the."),
        Some(Hit::Typos),
    );
    b.toggle(cy, c.typos, hovered);
    y += 72.0;

    let startup = settings::starts_with_windows();
    let (cy, hovered) = b.row(
        y,
        68.0,
        68.0,
        Some(G_POWER),
        t("Start with Windows"),
        t("Run PhitPup when you sign in."),
        Some(Hit::Startup),
    );
    b.toggle(cy, startup, hovered);
    y += 72.0;

    let (cy, hovered) = b.row(
        y,
        68.0,
        68.0,
        Some(G_FULLSCREEN),
        t("Turn off in full-screen windows"),
        t("Games, videos and presentations."),
        Some(Hit::Fullscreen),
    );
    b.toggle(cy, c.off_in_fullscreen, hovered);
    y += 72.0;

    let path = r"%APPDATA%\PhitPup\config.toml";
    let (cy, _) = b.row(y, 68.0, 68.0, Some(G_FILE), t("Settings file"), path, None);
    b.button(
        R::new(RIGHT - 20.0 - 96.0, cy - 16.0, 96.0, 32.0),
        t("Open"),
        None,
        false,
        Hit::OpenFile,
    );
    y + 68.0
}

fn programs_page(b: &mut Builder, c: &Config, y: f32) -> f32 {
    let p = b.pal;
    let mut y = b.title(y, t("Programs"));
    y += b.para(
        LEFT,
        y,
        WIDTH,
        t("Choose programs where PhitPup stays out of the way."),
        F::Body,
        p.sub,
    );
    y += 16.0;
    b.text(
        R::new(LEFT, y, 300.0, 32.0),
        t("Your programs"),
        F::Strong,
        p.text,
        LINE,
    );
    b.button(
        R::new(RIGHT - 160.0, y, 160.0, 32.0),
        t("Add a program"),
        Some(G_ADD),
        true,
        Hit::AddProgram,
    );
    y += 44.0;

    let list = programs(c);
    if list.is_empty() {
        b.card(R::new(LEFT, y, WIDTH, 68.0), false);
        b.text(
            R::new(LEFT + 20.0, y, WIDTH - 40.0, 68.0),
            t("Nothing added yet. Add a game or an app where PhitPup should do nothing."),
            F::Body,
            p.sub,
            LINE | DT_CENTER,
        );
        y += 72.0;
    }
    let hotkey = c.hotkey.label();
    for (i, (name, off)) in list.iter().enumerate() {
        let caption = if *off {
            t("Off: PhitPup does nothing here.").to_string()
        } else {
            t("No bubble: only {key} works here.").replace("{key}", &hotkey)
        };
        let glyph = if *off { G_BLOCK } else { G_HIDE };
        let (cy, _) = b.row(y, 64.0, 64.0, Some(glyph), name, &caption, None);

        let x = R::new(RIGHT - 16.0 - 32.0, cy - 16.0, 32.0, 32.0);
        if b.hit(x, Hit::RemoveProgram(i)) {
            b.fill(x, 4.0, p.control_hover);
        }
        b.glyph(x, G_CLOSE, F::SmallGlyph, p.sub);

        let seg = R::new(x.x - 16.0 - 200.0, cy - 16.0, 200.0, 32.0);
        for (j, (label, kind)) in [(t("Off"), true), (t("No bubble"), false)]
            .into_iter()
            .enumerate()
        {
            let half = R::new(seg.x + j as f32 * 100.0, seg.y, 100.0, 32.0);
            let hovered = b.hit(half, Hit::Kind(i, kind));
            let chosen = *off == kind;
            if hovered && !chosen {
                b.fill(half.inset(3.0), 6.0, p.card_hover);
            }
            let ink = if chosen { p.accent } else { p.sub };
            b.text(half, label, F::Strong, ink, LINE | DT_CENTER);
            if chosen {
                let w = b.width(F::Strong, label);
                b.circle(R::new(half.x + (half.w - w) / 2.0, half.y + 7.0, w, 18.0));
            }
        }
        y += 68.0;
    }

    y += 20.0;
    y = b.section(y, t("Built in"));
    y += b.para(
        LEFT,
        y,
        WIDTH,
        t("Always off: password fields, password managers (KeePass, 1Password, Bitwarden and others), Windows sign-in prompts, remote desktop and virtual machines."),
        F::Caption,
        p.sub,
    );
    y += 6.0;
    y += b.para(
        LEFT,
        y,
        WIDTH,
        t("No bubble: terminals and code editors (Windows Terminal, PowerShell, VS Code, Cursor, Visual Studio, JetBrains IDEs and others)."),
        F::Caption,
        p.sub,
    );
    y
}

fn words(b: &mut Builder, c: &Config, win: &Win, y: f32) -> f32 {
    let p = b.pal;
    let mut y = b.title(y, t("Words"));
    y += b.para(
        LEFT,
        y,
        WIDTH,
        t("Words PhitPup should leave alone, and words it should always fix. Add them as you type them."),
        F::Body,
        p.sub,
    );
    y += 16.0;

    let hotkey = c.hotkey.label();
    let (cy, hovered) = b.row(
        y,
        68.0,
        68.0,
        Some(G_BULB),
        t("Learn from my corrections"),
        t("Remembers the fixes you undo or keep."),
        Some(Hit::Learn),
    );
    b.toggle(cy, c.learn, hovered);
    y += 84.0;

    let field = R::new(LEFT, y, WIDTH - 108.0, 36.0);
    let hovered = b.hit(field, Hit::Input);
    b.input = Some(field);
    let focused = win.focus;
    // A line to write on, not a box.
    if hovered || focused {
        b.fill(field, 4.0, p.card_hover);
    }
    if focused {
        b.fill(
            R::new(field.x + 1.0, field.y + field.h - 2.0, field.w - 2.0, 2.0),
            1.0,
            p.accent,
        );
    } else {
        b.fill(
            R::new(field.x + 1.0, field.y + field.h - 1.0, field.w - 2.0, 1.0),
            0.5,
            p.sub,
        );
    }
    let tx = field.x + 12.0;
    if win.draft.is_empty() {
        let hint = t("A word to leave alone, e.g. gdp");
        b.text(
            R::new(tx, field.y, field.w - 24.0, field.h),
            hint,
            F::Word,
            p.faint,
            LINE,
        );
    } else {
        let draft = &win.draft;
        b.text(
            R::new(tx, field.y, field.w - 24.0, field.h),
            draft,
            F::Word,
            p.text,
            LINE,
        );
    }
    if focused && win.caret_on {
        let cx = tx + b.width(F::Word, &win.draft).min(field.w - 24.0);
        b.fill(R::new(cx, field.y + 9.0, 1.0, 18.0), 0.0, p.text);
    }
    b.button(
        R::new(RIGHT - 96.0, y + 2.0, 96.0, 32.0),
        t("Add"),
        Some(G_ADD),
        true,
        Hit::AddWord,
    );
    y += 44.0;
    if let Some(note) = &win.note {
        b.text(
            R::new(LEFT, y, WIDTH, 18.0),
            note,
            F::Caption,
            p.error,
            LINE,
        );
        y += 24.0;
    }
    y += 12.0;

    let n = c.ignore_words.len();
    y = b.section(y, &t("Leave alone ({n})").replace("{n}", &n.to_string()));
    y = if n == 0 {
        b.para(
            LEFT,
            y,
            WIDTH,
            t("Press Shift+Esc while the bubble shows a word, or type it above."),
            F::Caption,
            p.faint,
        ) + y
    } else {
        chips(b, y, &c.ignore_words, Hit::RemoveWord, false)
    };
    y += 20.0;

    let n = c.learned_words.len();
    y = b.section(y, &t("Always fix ({n})").replace("{n}", &n.to_string()));
    y = if n == 0 {
        let hint = t("Words you convert with {key} and then keep typing after show up here.")
            .replace("{key}", &hotkey);
        b.para(LEFT, y, WIDTH, &hint, F::Caption, p.faint) + y
    } else {
        chips(b, y + 22.0, &c.learned_words, Hit::RemoveLearned, true)
    };
    if !c.ignore_words.is_empty() || !c.learned_words.is_empty() {
        y += 8.0;
        b.text(
            R::new(LEFT, y, WIDTH, 18.0),
            t("Click a word to remove it."),
            F::Caption,
            p.faint,
            LINE,
        );
        y += 18.0;
    }
    y
}

/// What PhitPup turns `word` into, for the Always fix list.
fn fix_of(word: &str) -> Option<String> {
    let fallback;
    let d = match APP.get() {
        Some(app) => &app.detector,
        None => {
            // The window preview runs without the app.
            use phitpup_core::{Detector, Layout};
            fallback = Detector::new(&Layout::us_qwerty(), &Layout::kedmanee());
            &fallback
        }
    };
    Some(d.convert(word, d.direction(word)?))
}

/// Words on the page, each with a ruled line and an x to remove it,
/// wrapping across the page; returns the y below them. With `fixes`, each
/// word is struck through with its fix written above in pen.
fn chips(b: &mut Builder, mut y: f32, words: &[String], hit: fn(usize) -> Hit, fixes: bool) -> f32 {
    let p = b.pal;
    let step = if fixes { 58.0 } else { 40.0 };
    let mut x = LEFT + 8.0;
    for (i, word) in words.iter().enumerate() {
        let w = (b.width(F::Word, word) + 30.0).min(WIDTH);
        if x + w > RIGHT && x > LEFT + 8.0 {
            x = LEFT + 8.0;
            y += step;
        }
        let r = R::new(x, y, w, 32.0);
        let hovered = b.hit(r, hit(i));
        if hovered {
            b.fill(r, 6.0, p.card_hover);
        }
        b.fill(R::new(x, y + 30.0, w, 1.0), 0.0, p.control_border);
        let tw = w - 24.0;
        let text = R::new(x + 4.0, y, tw, 32.0);
        if fixes {
            b.text(text, word, F::Word, p.sub, LINE);
            let ww = b.width(F::Word, word).min(tw);
            b.pen(
                vec![(x + 2.0, y + 17.0), (x + 6.0 + ww, y + 15.5)],
                1.6,
                p.accent,
            );
            if let Some(fix) = fix_of(word) {
                let above = R::new(x + 6.0, y - 28.0, w + 40.0, 30.0);
                b.text(above, &fix, F::Pen, p.accent, LINE);
            }
        } else {
            b.text(text, word, F::Word, p.text, LINE);
        }
        let ink = if hovered { p.accent } else { p.faint };
        b.text(
            R::new(x + w - 18.0, y, 16.0, 32.0),
            "×",
            F::Pen,
            ink,
            LINE | DT_CENTER,
        );
        x += w + 18.0;
    }
    y + step
}

fn about(b: &mut Builder, c: &Config, y: f32) -> f32 {
    let p = b.pal;
    let mut y = y + 8.0;
    b.shapes.push(Shape::Logo {
        r: R::new(LEFT, y, 64.0, 64.0),
        tone: Tone::Active,
    });
    let x = LEFT + 84.0;
    b.text(
        R::new(x, y + 2.0, WIDTH - 84.0, 36.0),
        "PhitPup",
        F::Title,
        p.text,
        LINE,
    );
    b.text(
        R::new(x, y + 38.0, WIDTH - 84.0, 28.0),
        "ผิดปุ๊บ แก้ปั๊บ",
        F::PenBig,
        p.accent,
        LINE,
    );
    y += 88.0;
    y += b.para(
        LEFT,
        y,
        WIDTH,
        t("Fixes text typed in the wrong keyboard layout, Thai and English."),
        F::Body,
        p.text,
    );
    y += 16.0;

    let version = env!("CARGO_PKG_VERSION");
    let rows = [
        (G_PERSON, t("Made by"), AUTHOR),
        (G_PEOPLE, t("Team"), TEAM),
        (G_INFO, t("Version"), version),
        (G_DOC, t("License"), t("Free, not for sale")),
    ];
    for (glyph, title, value) in rows {
        let (cy, _) = b.row(y, 52.0, 52.0, Some(glyph), title, "", None);
        let r = R::new(RIGHT - 20.0 - 260.0, cy - 10.0, 260.0, 20.0);
        b.text(r, value, F::Strong, p.text, LINE | DT_RIGHT);
        y += 56.0;
    }
    let (caption, label, accent) = updates(update::state());
    let (cy, _) = b.row(y, 68.0, 68.0, Some(G_UPDATE), t("Updates"), &caption, None);
    let r = R::new(RIGHT - 20.0 - 170.0, cy - 16.0, 170.0, 32.0);
    b.button(r, &label, None, accent, Hit::Updates);
    y += 72.0;
    let (cy, hovered) = b.row(
        y,
        68.0,
        68.0,
        Some(G_BELL),
        t("Remind me to check"),
        t("Once this version is two months old."),
        Some(Hit::Remind),
    );
    b.toggle(cy, c.remind_updates, hovered);
    y += 72.0;

    let copied = crate::report::COPIED.load(std::sync::atomic::Ordering::Relaxed);
    let caption = if copied {
        t("Copied. Paste it (Ctrl+V) into the report on GitHub.")
    } else {
        t("Copies your version and settings, never what you type, and opens GitHub.")
    };
    let (cy, _) = b.row(
        y,
        68.0,
        68.0,
        Some(G_INFO),
        t("Report a problem"),
        caption,
        None,
    );
    let r = R::new(RIGHT - 20.0 - 170.0, cy - 16.0, 170.0, 32.0);
    b.button(r, t("Copy and report"), None, false, Hit::Report);
    y += 72.0;

    y += 16.0;
    b.glyph(R::new(LEFT, y, 16.0, 20.0), G_LOCK, F::SmallGlyph, p.sub);
    y += b.para(
        LEFT + 24.0,
        y,
        WIDTH - 24.0,
        t("PhitPup works offline: the part that sees your keys has no networking code. Only when you click Check for updates does the separate phitpup-update.exe ask the website for the newest version number and, if you agree, download the installer. What you type is kept in memory only until you click or switch windows, and is never sent anywhere. The only things saved are your settings file, including the words you add, and the date of the last update reminder."),
        F::Caption,
        p.sub,
    );
    y
}

/// The Updates row's caption, button label and whether the button is the
/// main action.
fn updates(state: update::State) -> (String, String, bool) {
    use update::State;
    let mb = |bytes: u64| format!("{:.1} MB", bytes as f64 / 1_048_576.0);
    match state {
        State::Idle if update::helper().is_none() => (
            t("Opens the download page in your browser.").into(),
            t("Check for updates").into(),
            false,
        ),
        State::Idle => (
            t("Checks for a new version and installs it for you.").into(),
            t("Check for updates").into(),
            false,
        ),
        State::Checking => (
            t("Checking\u{2026}").into(),
            t("Checking\u{2026}").into(),
            false,
        ),
        State::Current => (
            t("You have the newest version.").into(),
            t("Check again").into(),
            false,
        ),
        State::Available(v, size) => (
            t("Version {v} is out ({size}).")
                .replace("{v}", &v)
                .replace("{size}", &mb(size)),
            t("Install {v}").replace("{v}", &v),
            true,
        ),
        State::Downloading(done, total) => {
            let pct = (done * 100).checked_div(total).unwrap_or(0);
            (
                t("Downloading\u{2026} {pct}%").replace("{pct}", &pct.to_string()),
                t("Downloading\u{2026}").into(),
                false,
            )
        }
        State::Installing => (
            t("Installing. PhitPup will start again by itself.").into(),
            t("Installing\u{2026}").into(),
            false,
        ),
        State::Failed(why) => (crate::i18n::t_dyn(&why), t("Try again").into(), false),
    }
}

fn shortcuts(b: &mut Builder, c: &Config, win: &Win, y: f32) -> f32 {
    let p = b.pal;
    let mut y = b.title(y, t("Shortcuts"));

    let changeable = [
        (
            Which::Fix,
            t("Fix the last word"),
            t("Press again to go one more word back."),
            c.hotkey,
        ),
        (
            Which::Accept,
            t("Accept the suggestion"),
            t("While the bubble is showing."),
            c.accept_key,
        ),
    ];
    for (which, title, caption, key) in changeable {
        let listening = win.capture == Some(which);
        let caption = if listening {
            t("Press the new shortcut, or Esc to cancel.")
        } else {
            caption
        };
        let hit = Some(Hit::Change(which));
        let (cy, hovered) = b.row(y, 68.0, 68.0, Some(G_KEYBOARD), title, caption, hit);
        let ink = if hovered || listening { p.text } else { p.sub };
        b.glyph(
            R::new(RIGHT - 36.0, cy - 10.0, 16.0, 20.0),
            G_EDIT,
            F::SmallGlyph,
            ink,
        );
        let right = RIGHT - 52.0;
        if listening {
            let r = R::new(right - 160.0, cy - 14.0, 160.0, 28.0);
            b.stroke(r, 4.0, p.accent);
            b.stroke(r.inset(1.0), 3.0, p.accent);
            b.text(
                r,
                t("Press a shortcut\u{2026}"),
                F::Strong,
                p.accent,
                LINE | DT_CENTER,
            );
        } else {
            b.keycaps(right, cy, &key.caps(), false);
        }
        y += 72.0;
    }

    let (cy, hovered) = b.row(
        y,
        68.0,
        68.0,
        Some(G_KEYBOARD),
        t("Also fix when I tap Shift twice"),
        t("Two quick taps, nothing in between."),
        Some(Hit::DoubleShift),
    );
    b.toggle(cy, c.double_shift, hovered);
    y += 72.0;

    let fixed = [
        (
            t("Hide the suggestion"),
            t("Until you start the next word."),
            vec!["Esc"],
        ),
        (
            t("Never suggest this word"),
            t("Adds it to Words."),
            vec!["Shift", "Esc"],
        ),
    ];
    for (title, caption, caps) in fixed {
        let (cy, _) = b.row(y, 68.0, 68.0, Some(G_KEYBOARD), title, caption, None);
        let caps: Vec<String> = caps.into_iter().map(String::from).collect();
        b.keycaps(RIGHT - 20.0, cy, &caps, true);
        y += 72.0;
    }

    if let Some(note) = &win.note {
        y += 4.0;
        b.text(
            R::new(LEFT, y, WIDTH, 18.0),
            note,
            F::Caption,
            p.error,
            LINE,
        );
        y += 24.0;
    }
    if (c.hotkey, c.accept_key) != (Hotkey::FIX, Hotkey::ACCEPT) {
        y += 12.0;
        let r = R::new(LEFT, y, 160.0, 32.0);
        b.button(r, t("Reset to defaults"), None, false, Hit::ResetKeys);
        y += 32.0;
    }
    y
}

// ---------------------------------------------------------------- pixels

fn unpack(p: u32) -> Rgb {
    [
        ((p >> 16) & 0xFF) as f32 / 255.0,
        ((p >> 8) & 0xFF) as f32 / 255.0,
        (p & 0xFF) as f32 / 255.0,
    ]
}

fn pack(c: Rgb) -> u32 {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    b(c[0]) << 16 | b(c[1]) << 8 | b(c[2])
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

/// Signed distance from (x, y) to rounded box `r` (negative inside).
fn distance(r: &R, x: f32, y: f32, rad: f32) -> f32 {
    let rad = rad.min(r.w / 2.0).min(r.h / 2.0);
    let qx = (x - (r.x + r.w / 2.0)).abs() - (r.w / 2.0 - rad);
    let qy = (y - (r.y + r.h / 2.0)).abs() - (r.h / 2.0 - rad);
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    outside + qx.max(qy).min(0.0) - rad
}

fn coverage(d: f32) -> f32 {
    (0.5 - d).clamp(0.0, 1.0)
}

/// Paints a rounded box (or, with `band`, a line that wide inside its edge)
/// in device pixels.
fn paint_box(px: &mut [u32], w: i32, h: i32, r: R, rad: f32, c: Rgb, band: Option<f32>) {
    let x0 = (r.x.floor() as i32 - 1).max(0);
    let y0 = (r.y.floor() as i32 - 1).max(0);
    let x1 = ((r.x + r.w).ceil() as i32 + 1).min(w);
    let y1 = ((r.y + r.h).ceil() as i32 + 1).min(h);
    for y in y0..y1 {
        for x in x0..x1 {
            let d = distance(&r, x as f32 + 0.5, y as f32 + 0.5, rad);
            let cov = match band {
                None => coverage(d),
                Some(bw) => coverage(d) - coverage(d + bw),
            };
            if cov > 0.0 {
                let i = (y * w + x) as usize;
                px[i] = pack(mix(unpack(px[i]), c, cov));
            }
        }
    }
}

/// Paints a round-ended line through `pts`, in device pixels.
fn paint_pen(px: &mut [u32], w: i32, h: i32, pts: &[(f32, f32)], width: f32, c: Rgb) {
    let half = width / 2.0;
    for seg in pts.windows(2) {
        let ((ax, ay), (bx, by)) = (seg[0], seg[1]);
        let x0 = ((ax.min(bx) - half - 1.0).floor() as i32).max(0);
        let x1 = ((ax.max(bx) + half + 1.0).ceil() as i32).min(w);
        let y0 = ((ay.min(by) - half - 1.0).floor() as i32).max(0);
        let y1 = ((ay.max(by) + half + 1.0).ceil() as i32).min(h);
        let (dx, dy) = (bx - ax, by - ay);
        let len2 = (dx * dx + dy * dy).max(1e-6);
        for y in y0..y1 {
            for x in x0..x1 {
                let (px_, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let t = (((px_ - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0);
                let (qx, qy) = (ax + t * dx - px_, ay + t * dy - py);
                let cov = coverage((qx * qx + qy * qy).sqrt() - half);
                if cov > 0.0 {
                    let i = (y * w + x) as usize;
                    // Overlapping segments must not darken the joints: keep
                    // the strongest coverage so far by mixing towards it only.
                    let old = unpack(px[i]);
                    let target = mix(old, c, cov);
                    let closer = |o: f32, t: f32, c: f32| (c - t).abs() < (c - o).abs();
                    if closer(old[0], target[0], c[0]) || closer(old[1], target[1], c[1]) {
                        px[i] = pack(target);
                    }
                }
            }
        }
    }
}

fn paint_logo(px: &mut [u32], w: i32, h: i32, r: R, tone: Tone) {
    let size = r.w.round().max(8.0) as u32;
    let rgba = icon::render(size, tone);
    let (ox, oy) = (r.x.round() as i32, r.y.round() as i32);
    for j in 0..size as i32 {
        for i in 0..size as i32 {
            let (x, y) = (ox + i, oy + j);
            if x < 0 || y < 0 || x >= w || y >= h {
                continue;
            }
            let s = &rgba[((j * size as i32 + i) * 4) as usize..][..4];
            let a = s[3] as f32 / 255.0;
            if a > 0.0 {
                let c = [
                    s[0] as f32 / 255.0,
                    s[1] as f32 / 255.0,
                    s[2] as f32 / 255.0,
                ];
                let k = (y * w + x) as usize;
                px[k] = pack(mix(unpack(px[k]), c, a));
            }
        }
    }
}

/// A top-down 32-bit DIB of `w` x `h` pixels selected into a new memory DC.
/// Returns the DC, the bitmap, the DC's previous bitmap and the pixels.
///
/// SAFETY: the caller selects the previous bitmap back and deletes the DC
/// and the bitmap, after which the pixels must not be used.
unsafe fn canvas(like: HDC, w: i32, h: i32) -> (HDC, HBITMAP, HGDIOBJ, &'static mut [u32]) {
    // SAFETY: a zeroed BITMAPINFO with the header filled in.
    unsafe {
        let mem = CreateCompatibleDC(like);
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
        let bmp = CreateDIBSection(
            mem,
            &bmi,
            DIB_RGB_COLORS,
            &mut bits,
            std::ptr::null_mut(),
            0,
        );
        let old = SelectObject(mem, bmp);
        let px = std::slice::from_raw_parts_mut(bits as *mut u32, (w * h) as usize);
        (mem, bmp, old, px)
    }
}

/// Draws the current page of `win` with settings `c` into `px`, the w x h
/// pixels of the bitmap selected into `mem`, and remembers where clicks go.
fn draw(win: &mut Win, c: &Config, mem: HDC, px: &mut [u32], w: i32, h: i32) {
    let ui = build(win, c, mem);
    let s = ui.scale;
    px.fill(pack(ui.pal.bg));
    for shape in &ui.shapes {
        match *shape {
            Shape::Fill { r, rad, c } => {
                let r = R::new(r.x * s, r.y * s, r.w * s, r.h * s);
                paint_box(px, w, h, r, rad * s, c, None);
            }
            Shape::Stroke { r, rad, width, c } => {
                let r = R::new(r.x * s, r.y * s, r.w * s, r.h * s);
                let band = (width * s).floor().max(1.0);
                paint_box(px, w, h, r, rad * s, c, Some(band));
            }
            Shape::Logo { r, tone } => {
                let r = R::new(r.x * s, r.y * s, r.w * s, r.h * s);
                paint_logo(px, w, h, r, tone);
            }
            Shape::Pen { ref pts, width, c } => {
                let pts: Vec<(f32, f32)> = pts.iter().map(|&(x, y)| (x * s, y * s)).collect();
                paint_pen(px, w, h, &pts, width * s, c);
            }
        }
    }
    // SAFETY: GDI text into our own DC, after the pixel writes above.
    unsafe {
        SetBkMode(mem, TRANSPARENT as i32);
        for t in &ui.texts {
            SelectObject(mem, win.fonts.get(t.f));
            SetTextColor(mem, colorref(t.c));
            let mut r = t.r.scaled(s);
            DrawTextW(mem, t.s.as_ptr(), t.s.len() as i32, &mut r, t.flags);
        }
        GdiFlush();
    }
    let (bottom, input, hits) = (ui.bottom, ui.input, ui.hits);
    win.max_scroll = (bottom + win.scroll - h as f32 / s).max(0.0);
    win.input = input;
    win.hits = hits;
}

fn paint(hwnd: HWND) {
    // SAFETY: standard paint cycle; every GDI object made here is restored
    // and deleted before returning.
    unsafe {
        let mut ps: PAINTSTRUCT = std::mem::zeroed();
        let hdc = BeginPaint(hwnd, &mut ps);
        let mut rc: RECT = std::mem::zeroed();
        GetClientRect(hwnd, &mut rc);
        let (w, h) = (rc.right.max(1), rc.bottom.max(1));
        let c = config();
        with(|win| {
            let (mem, bmp, old, px) = canvas(hdc, w, h);
            draw(win, &c, mem, px, w, h);
            BitBlt(hdc, 0, 0, w, h, mem, 0, 0, SRCCOPY);
            SelectObject(mem, old);
            DeleteObject(bmp);
            DeleteDC(mem);
        });
        EndPaint(hwnd, &ps);
    }
}

// ---------------------------------------------------------------- actions

fn invalidate(hwnd: HWND) {
    // SAFETY: our own window.
    unsafe { InvalidateRect(hwnd, std::ptr::null(), 0) };
}

fn click(hwnd: HWND, hit: Option<Hit>) {
    with(|w| {
        w.focus = w.page == Page::Words && matches!(hit, Some(Hit::Input | Hit::AddWord));
        w.caret_on = true;
        if !matches!(hit, Some(Hit::Change(_))) {
            w.capture = None;
        }
        if !matches!(hit, Some(Hit::AddWord)) {
            w.note = None;
        }
    });
    let Some(hit) = hit else {
        invalidate(hwnd);
        return;
    };
    match hit {
        Hit::Nav(page) => {
            with(|w| {
                w.page = page;
                w.scroll = 0.0;
                w.focus = page == Page::Words;
            });
        }
        Hit::On => tray::toggle_pause(),
        Hit::Mode(mode) => edit(|c| c.mode = mode),
        Hit::Underline => edit(|c| c.underline = !c.underline),
        Hit::Spelling => edit(|c| c.spelling = !c.spelling),
        Hit::Typos => edit(|c| c.typos = !c.typos),
        Hit::DoubleShift => edit(|c| c.double_shift = !c.double_shift),
        Hit::Startup => settings::set_start_with_windows(!settings::starts_with_windows()),
        Hit::Fullscreen => edit(|c| c.off_in_fullscreen = !c.off_in_fullscreen),
        Hit::OpenFile => settings::open_file(),
        // phitpup-update.exe does the checking: PhitPup has no networking code.
        Hit::Updates => update::click(),
        Hit::Report => crate::report::copy_and_open(hwnd),
        Hit::Remind => edit(|c| c.remind_updates = !c.remind_updates),
        Hit::AddProgram => add_program(hwnd),
        Hit::Kind(i, off) => edit(|c| {
            let mut list = programs(c);
            if let Some(item) = list.get_mut(i) {
                item.1 = off;
            }
            set_programs(c, list);
        }),
        Hit::RemoveProgram(i) => edit(|c| {
            let mut list = programs(c);
            if i < list.len() {
                list.remove(i);
            }
            set_programs(c, list);
        }),
        Hit::Input => {}
        Hit::AddWord => add_word(),
        Hit::Learn => edit(|c| c.learn = !c.learn),
        Hit::RemoveLearned(i) => edit(|c| {
            if i < c.learned_words.len() {
                c.learned_words.remove(i);
            }
        }),
        Hit::RemoveWord(i) => edit(|c| {
            if i < c.ignore_words.len() {
                c.ignore_words.remove(i);
            }
        }),
        Hit::Change(which) => {
            with(|w| {
                w.capture = if w.capture == Some(which) {
                    None
                } else {
                    Some(which)
                };
                w.note = None;
            });
        }
        Hit::ResetKeys => edit(|c| (c.hotkey, c.accept_key) = (Hotkey::FIX, Hotkey::ACCEPT)),
    }
    invalidate(hwnd);
}

fn set_programs(c: &mut Config, list: Vec<(String, bool)>) {
    c.off_apps = list.iter().filter(|p| p.1).map(|p| p.0.clone()).collect();
    c.quiet_apps = list.into_iter().filter(|p| !p.1).map(|p| p.0).collect();
}

fn add_word() {
    let Some(draft) = with(|w| std::mem::take(&mut w.draft)) else {
        return;
    };
    let word = draft.trim().to_lowercase();
    let c = config();
    let note = if word.is_empty() {
        Some(t("Type a word first.").to_string())
    } else if word.contains(char::is_whitespace) {
        Some(t("One word at a time, without spaces.").to_string())
    } else if !config::storable(&word) {
        Some(t("Quotes and backslashes can't be saved.").to_string())
    } else if c.ignore_words.contains(&word) {
        Some(t("\u{201C}{word}\u{201D} is already in the list.").replace("{word}", &word))
    } else {
        None
    };
    match note {
        Some(note) => {
            with(|w| {
                w.note = Some(note);
                w.draft = draft;
            });
        }
        None => {
            edit(|c| {
                c.ignore(&word);
            });
            with(|w| w.note = None);
        }
    }
}

fn add_program(hwnd: HWND) {
    let Some((r, s)) = with(|w| {
        let r = w.hits.iter().find(|(_, h)| *h == Hit::AddProgram)?.0;
        Some((r, w.scale()))
    })
    .flatten() else {
        return;
    };
    let c = config();
    let listed: Vec<String> = programs(&c).into_iter().map(|p| p.0).collect();
    let names: Vec<String> = running_programs()
        .into_iter()
        .filter(|n| !listed.contains(n))
        .collect();
    // SAFETY: the menu is built, shown and destroyed here; strings are
    // NUL-terminated and copied by AppendMenu.
    let cmd = unsafe {
        let menu = CreatePopupMenu();
        if names.is_empty() {
            AppendMenuW(
                menu,
                MF_STRING | MF_GRAYED,
                0,
                wide(t("No other programs are open")).as_ptr(),
            );
        }
        for (i, n) in names.iter().enumerate() {
            AppendMenuW(menu, MF_STRING, i + 1, wide(n).as_ptr());
        }
        let mut pt = POINT {
            x: (r.x * s).round() as i32,
            y: ((r.y + r.h + 4.0) * s).round() as i32,
        };
        ClientToScreen(hwnd, &mut pt);
        let cmd = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_LEFTALIGN | TPM_TOPALIGN,
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
    if let Some(name) = cmd.checked_sub(1).and_then(|i| names.get(i)) {
        edit(|c| c.off_apps.push(name.clone()));
    }
}

/// Exe names of programs with a window on the taskbar, sorted, without
/// PhitPup itself and Windows' own hosts.
fn running_programs() -> Vec<String> {
    const SKIP: &[&str] = &[
        "applicationframehost.exe",
        "explorer.exe",
        "searchhost.exe",
        "shellexperiencehost.exe",
        "startmenuexperiencehost.exe",
        "textinputhost.exe",
        "systemsettings.exe",
    ];
    unsafe extern "system" fn each(hwnd: HWND, list: LPARAM) -> BOOL {
        // SAFETY: `list` is the Vec passed to EnumWindows below; the rest are
        // plain queries.
        unsafe {
            let shown = IsWindowVisible(hwnd) != 0
                && GetWindow(hwnd, GW_OWNER).is_null()
                && GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW == 0
                && GetWindowTextLengthW(hwnd) > 0;
            if shown {
                (*(list as *mut Vec<HWND>)).push(hwnd);
            }
        }
        1
    }
    let mut windows: Vec<HWND> = Vec::new();
    // SAFETY: the callback only runs during this call.
    unsafe { EnumWindows(Some(each), &mut windows as *mut Vec<HWND> as LPARAM) };
    let own = context::exe_of_process(std::process::id());
    let mut names: Vec<String> = windows
        .into_iter()
        .map(context::exe_of)
        .filter(|n| !n.is_empty() && *n != own && !SKIP.contains(&n.as_str()))
        .collect();
    names.sort();
    names.dedup();
    names
}

/// Takes the pressed key as the new shortcut. Returns false to let Windows
/// handle the key.
/// Tab and Shift+Tab move a focus ring through everything that can be
/// clicked (the pages first), Enter or Space click it, Esc hides the ring.
/// `typing` is true while the Words box takes the keys.
fn navigate(hwnd: HWND, vk: u16, typing: bool) -> bool {
    // SAFETY: plain key-state query for this thread's input.
    let shift = unsafe { GetKeyState(VK_SHIFT.into()) < 0 };
    match vk {
        VK_TAB => {
            with(|w| {
                let mut order: Vec<(R, Hit)> = Vec::new();
                let nav = w.hits.iter().filter(|(_, h)| matches!(h, Hit::Nav(_)));
                let rest = w.hits.iter().filter(|(_, h)| !matches!(h, Hit::Nav(_)));
                for &(r, h) in nav.chain(rest) {
                    if !order.iter().any(|&(_, o)| o == h) {
                        order.push((r, h));
                    }
                }
                let n = order.len();
                if n == 0 {
                    return;
                }
                let at = w.kb.and_then(|k| order.iter().position(|&(_, h)| h == k));
                let next = match (at, shift) {
                    (None, false) => 0,
                    (None, true) => n - 1,
                    (Some(i), false) => (i + 1) % n,
                    (Some(i), true) => (i + n - 1) % n,
                };
                let (r, h) = order[next];
                w.kb = Some(h);
                w.focus = h == Hit::Input;
                // Bring it into view; the pages in the sidebar never scroll.
                if !matches!(h, Hit::Nav(_)) {
                    if r.y < 16.0 {
                        w.scroll = (w.scroll - (16.0 - r.y)).max(0.0);
                    } else if r.y + r.h > H - 16.0 {
                        w.scroll = (w.scroll + r.y + r.h - (H - 16.0)).min(w.max_scroll);
                    }
                }
            });
            invalidate(hwnd);
            true
        }
        VK_RETURN | VK_SPACE if !typing => match with(|w| w.kb).flatten() {
            Some(h) => {
                click(hwnd, Some(h));
                with(|w| w.kb = Some(h));
                invalidate(hwnd);
                true
            }
            None => false,
        },
        VK_ESCAPE if with(|w| w.kb.take()).flatten().is_some() => {
            invalidate(hwnd);
            true
        }
        _ => false,
    }
}

fn key_down(hwnd: HWND, vk: u16) -> bool {
    let Some((capture, focus)) = with(|w| (w.capture, w.focus && w.page == Page::Words)) else {
        return false;
    };
    let Some(which) = capture else {
        if focus && vk == VK_ESCAPE {
            with(|w| w.focus = false);
            invalidate(hwnd);
            return true;
        }
        return navigate(hwnd, vk, focus);
    };
    let modifier = [
        VK_SHIFT,
        VK_LSHIFT,
        VK_RSHIFT,
        VK_CONTROL,
        VK_LCONTROL,
        VK_RCONTROL,
        VK_MENU,
        VK_LMENU,
        VK_RMENU,
        VK_LWIN,
        VK_RWIN,
    ];
    if modifier.contains(&vk) {
        return true;
    }
    // SAFETY: plain key-state queries for this thread's input.
    let held = |k: u16| unsafe { GetKeyState(k.into()) < 0 };
    let key = Hotkey::new(held(VK_CONTROL), held(VK_SHIFT), held(VK_MENU), vk);
    let c = config();
    let other = match which {
        Which::Fix => c.accept_key,
        Which::Accept => c.hotkey,
    };
    let note = if key == Hotkey::new(false, false, false, VK_ESCAPE) {
        with(|w| w.capture = None);
        None
    } else if held(VK_LWIN) || held(VK_RWIN) {
        Some(t("Shortcuts with the Windows key belong to Windows.").to_string())
    } else if !hotkey::is_main_key(vk) {
        Some(t("That key can't be used. Try a letter, F-key, Space, Tab or Backspace.").to_string())
    } else if let Some(problem) = key.problem() {
        Some(t(problem).to_string())
    } else if key == other {
        Some(t("{key} is already used for the other shortcut.").replace("{key}", &key.label()))
    } else {
        edit(|c| match which {
            Which::Fix => c.hotkey = key,
            Which::Accept => c.accept_key = key,
        });
        with(|w| w.capture = None);
        None
    };
    with(|w| w.note = note);
    invalidate(hwnd);
    true
}

/// Typing into the Words field.
fn char_typed(hwnd: HWND, ch: u32) {
    let typing = with(|w| w.focus && w.page == Page::Words && w.capture.is_none());
    if typing != Some(true) {
        return;
    }
    match ch {
        0x08 => {
            with(|w| w.draft.pop());
        }
        0x0D => add_word(),
        0x16 => {
            let pasted = clipboard::text(hwnd).unwrap_or_default();
            let word: String = pasted.lines().next().unwrap_or("").trim().into();
            with(|w| {
                w.draft.push_str(&word);
                w.draft = w.draft.chars().take(MAX_WORD).collect();
            });
        }
        _ => {
            if let Some(c) = char::from_u32(ch).filter(|c| !c.is_control()) {
                with(|w| {
                    if w.draft.chars().count() < MAX_WORD {
                        w.draft.push(c);
                    }
                });
            }
        }
    }
    with(|w| {
        w.caret_on = true;
        if ch != 0x0D {
            w.note = None;
        }
    });
    invalidate(hwnd);
}

fn mouse_at(lparam: LPARAM, scale: f32) -> (f32, f32) {
    let x = (lparam & 0xFFFF) as i16 as f32;
    let y = ((lparam >> 16) & 0xFFFF) as i16 as f32;
    (x / scale, y / scale)
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let capturing = || with(|w| w.capture.is_some()) == Some(true);
    match msg {
        WM_PAINT => {
            paint(hwnd);
            0
        }
        WM_ERASEBKGND => 1,
        WM_MOUSEMOVE => {
            let changed = with(|w| {
                let p = mouse_at(lparam, w.scale());
                let before = w.mouse.and_then(|m| w.hit_at(m));
                w.mouse = Some(p);
                if !w.tracking {
                    w.tracking = true;
                    let mut t = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    // SAFETY: plain request for WM_MOUSELEAVE.
                    unsafe { TrackMouseEvent(&mut t) };
                }
                before != w.hit_at(p)
            });
            if changed == Some(true) {
                invalidate(hwnd);
            }
            0
        }
        WM_MOUSELEAVE => {
            with(|w| {
                w.mouse = None;
                w.tracking = false;
            });
            invalidate(hwnd);
            0
        }
        WM_LBUTTONDOWN => {
            let hit = with(|w| {
                let p = mouse_at(lparam, w.scale());
                w.mouse = Some(p);
                w.kb = None; // the ring is for the keyboard
                w.hit_at(p)
            })
            .flatten();
            click(hwnd, hit);
            0
        }
        WM_MOUSEWHEEL => {
            let delta = ((wparam >> 16) & 0xFFFF) as i16 as f32;
            with(|w| w.scroll = (w.scroll - delta / 120.0 * 64.0).clamp(0.0, w.max_scroll));
            invalidate(hwnd);
            0
        }
        WM_SETCURSOR if (lparam & 0xFFFF) as u32 == HTCLIENT => {
            let over_input = with(|w| match (w.mouse, w.input) {
                (Some(m), Some(r)) => r.contains(m),
                _ => false,
            });
            if over_input == Some(true) {
                // SAFETY: a system cursor.
                unsafe { SetCursor(LoadCursorW(std::ptr::null_mut(), IDC_IBEAM)) };
                return 1;
            }
            // SAFETY: default handling (the class cursor).
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            if key_down(hwnd, wparam as u16) {
                return 0;
            }
            // SAFETY: default handling.
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        // Alt would open the window menu while a shortcut is being set.
        WM_SYSKEYUP | WM_SYSCHAR if capturing() => 0,
        WM_CHAR => {
            char_typed(hwnd, wparam as u32);
            0
        }
        WM_TIMER if wparam == BLINK => {
            let blink = with(|w| {
                w.caret_on = !w.caret_on;
                w.focus && w.page == Page::Words
            });
            if let Some(true) = blink {
                invalidate(hwnd);
            }
            0
        }
        WM_DPICHANGED => {
            let dpi = ((wparam >> 16) & 0xFFFF) as u32;
            with(|w| {
                w.dpi = dpi.max(96);
                w.fonts = Fonts::new(w.dpi);
            });
            // SAFETY: Windows passes the suggested rectangle in lparam.
            unsafe {
                let r = &*(lparam as *const RECT);
                SetWindowPos(
                    hwnd,
                    std::ptr::null_mut(),
                    r.left,
                    r.top,
                    r.right - r.left,
                    r.bottom - r.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            invalidate(hwnd);
            0
        }
        WM_SETTINGCHANGE => {
            let dark = bubble::dark_theme();
            if with(|w| std::mem::replace(&mut w.dark, dark) != dark) == Some(true) {
                theme_frame(hwnd, dark);
                invalidate(hwnd);
            }
            0
        }
        WM_CLOSE => {
            // SAFETY: our own window.
            unsafe { DestroyWindow(hwnd) };
            0
        }
        WM_DESTROY => {
            // SAFETY: our own timer.
            unsafe { KillTimer(hwnd, BLINK) };
            WIN.with(|w| {
                if let Ok(mut w) = w.try_borrow_mut() {
                    *w = None;
                }
            });
            0
        }
        // SAFETY: default handling for everything else.
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes every page, light and dark, at 100% and 150%, as BMP files into
    /// `PHITPUP_PREVIEW_DIR`, to look at without a live desktop:
    /// `cargo test -p phitpup window_preview -- --ignored`
    #[test]
    #[ignore]
    fn window_preview() {
        let dir = std::path::PathBuf::from(std::env::var("PHITPUP_PREVIEW_DIR").unwrap());
        let c = Config {
            off_apps: vec!["eldenring.exe".into()],
            quiet_apps: vec!["obsidian.exe".into()],
            ignore_words: ["gdp", "mvp", "lcd", "แหก", "rust", "phitpup"]
                .map(String::from)
                .to_vec(),
            learned_words: vec!["muj".into(), "ok".into()],
            ..Config::default()
        };
        // PHITPUP_PREVIEW_UPDATE=available|downloading|failed shows that
        // state on the About page.
        match std::env::var("PHITPUP_PREVIEW_UPDATE").as_deref() {
            Ok("available") => update::set(update::State::Available("0.4.0".into(), 2_507_332)),
            Ok("downloading") => update::set(update::State::Downloading(1_100_000, 2_507_332)),
            Ok("failed") => update::set(update::State::Failed(
                "Could not connect. Check your internet connection.".into(),
            )),
            _ => {}
        }
        for dark in [false, true] {
            for dpi in [96, 144] {
                for (page, _, name) in PAGES {
                    let words = page == Page::Words;
                    let mut win = Win {
                        hwnd: std::ptr::null_mut(),
                        dpi,
                        dark,
                        fonts: Fonts::new(dpi),
                        page,
                        mouse: Some((400.0, 120.0)),
                        tracking: false,
                        scroll: 0.0,
                        max_scroll: 0.0,
                        hits: Vec::new(),
                        input: None,
                        draft: if words { "wtf".into() } else { String::new() },
                        focus: words,
                        caret_on: true,
                        capture: (page == Page::Shortcuts).then_some(Which::Accept),
                        note: None,
                        kb: (page == Page::General).then_some(Hit::Startup),
                    };
                    let s = dpi as f32 / 96.0;
                    let (w, h) = ((W * s).round() as i32, (H * s).round() as i32);
                    let theme = if dark { "dark" } else { "light" };
                    let file = dir.join(format!("window-{name}-{theme}-{dpi}.bmp"));
                    // SAFETY: a memory DC and DIB made and freed here.
                    unsafe {
                        let (mem, bmp, old, px) = canvas(std::ptr::null_mut(), w, h);
                        draw(&mut win, &c, mem, px, w, h);
                        write_bmp(&file, px, w, h);
                        SelectObject(mem, old);
                        DeleteObject(bmp);
                        DeleteDC(mem);
                    }
                }
            }
        }
    }

    /// 24-bit bottom-up BMP.
    fn write_bmp(path: &std::path::Path, px: &[u32], w: i32, h: i32) {
        let row = ((w * 3 + 3) / 4 * 4) as usize;
        let size = 54 + row * h as usize;
        let mut out = Vec::with_capacity(size);
        out.extend(b"BM");
        out.extend((size as u32).to_le_bytes());
        out.extend([0u8; 4]);
        out.extend(54u32.to_le_bytes());
        out.extend(40u32.to_le_bytes());
        out.extend(w.to_le_bytes());
        out.extend(h.to_le_bytes());
        out.extend(1u16.to_le_bytes());
        out.extend(24u16.to_le_bytes());
        out.extend([0u8; 24]);
        for y in (0..h).rev() {
            let start = out.len();
            for x in 0..w {
                let p = px[(y * w + x) as usize];
                out.extend([p as u8, (p >> 8) as u8, (p >> 16) as u8]);
            }
            out.resize(start + row, 0);
        }
        std::fs::write(path, out).unwrap();
    }
}
