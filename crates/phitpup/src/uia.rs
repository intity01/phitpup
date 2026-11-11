//! The few UI Automation calls PhitPup needs, made through raw COM vtables.
//!
//! UI Automation sees inside apps that draw their own controls (browsers,
//! Electron, password managers), which plain Win32 cannot: whether the focused
//! field is a password field, and where its caret is. Slot numbers come from
//! `UIAutomationClient.h` in the Windows SDK; only these interfaces are used:
//!
//! | interface | slot | method |
//! |---|---|---|
//! | IUnknown | 2 | Release |
//! | IUIAutomation | 8 | GetFocusedElement |
//! | IUIAutomationElement | 14 | GetCurrentPatternAs |
//! | IUIAutomationElement | 35 | get_CurrentIsPassword |
//! | IUIAutomationTextPattern | 5 | GetSelection |
//! | IUIAutomationTextRangeArray | 4 | GetElement |
//! | IUIAutomationTextPattern2 | 10 | GetCaretRange |
//! | IUIAutomationTextRange | 3 | Clone |
//! | IUIAutomationTextRange | 10 | GetBoundingRectangles |
//! | IUIAutomationTextRange | 12 | GetText |
//! | IUIAutomationTextRange | 14 | MoveEndpointByUnit |
//! | IUIAutomationTextRange | 15 | MoveEndpointByRange |
//! | IUIAutomationTextRange | 16 | Select |

use std::ffi::c_void;

use windows_sys::Win32::Foundation::{RECT, SysFreeString, SysStringLen};
use windows_sys::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, SAFEARRAY,
};
use windows_sys::Win32::System::Ole::{
    SafeArrayAccessData, SafeArrayDestroy, SafeArrayGetLBound, SafeArrayGetUBound,
    SafeArrayUnaccessData,
};
use windows_sys::core::{BSTR, GUID, HRESULT};

type Raw = *mut c_void;

const CLSID_CUIAUTOMATION: GUID = GUID::from_u128(0xff48dba4_60ef_4201_aa87_54103eef594e);
const IID_IUIAUTOMATION: GUID = GUID::from_u128(0x30cbe57d_d9d0_452a_ab13_7ac5ac4825ee);
const IID_TEXT_PATTERN2: GUID = GUID::from_u128(0x506a921a_fcc9_409f_b23b_37eb74106872);
const UIA_TEXT_PATTERN2_ID: i32 = 10024;
const IID_TEXT_PATTERN: GUID = GUID::from_u128(0x32eba289_3583_42c9_9c59_3b6d9a1e9b6a);
const UIA_TEXT_PATTERN_ID: i32 = 10014;
const ENDPOINT_START: i32 = 0;
const ENDPOINT_END: i32 = 1;
/// The longest word [`Automation::word_at_caret`] looks for on either side.
const REACH: i32 = 40;
const UNIT_CHARACTER: i32 = 0;

/// An owned COM pointer, released on drop.
struct Com(Raw);

impl Com {
    /// The function in vtable slot `i`, typed as `F`.
    ///
    /// SAFETY: `F` must be the exact signature of that slot.
    unsafe fn slot<F: Copy>(&self, i: usize) -> F {
        // SAFETY: a COM object starts with a pointer to its vtable, an array
        // of function pointers; the caller vouches for slot i's type.
        unsafe {
            let vtbl = *(self.0 as *const *const usize);
            std::mem::transmute_copy(&*vtbl.add(i))
        }
    }

    /// Calls a slot that hands back a new object: `(this, *mut Raw)`.
    fn get(&self, i: usize) -> Option<Com> {
        let mut out: Raw = std::ptr::null_mut();
        // SAFETY: callers only pass slots with this signature.
        let hr = unsafe {
            self.slot::<unsafe extern "system" fn(Raw, *mut Raw) -> HRESULT>(i)(self.0, &mut out)
        };
        (hr >= 0 && !out.is_null()).then_some(Com(out))
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        // SAFETY: slot 2 is IUnknown::Release on every interface.
        unsafe { self.slot::<unsafe extern "system" fn(Raw) -> u32>(2)(self.0) };
    }
}

pub struct Automation(Com);

impl Automation {
    /// Joins the multithreaded apartment on this thread and connects to UI
    /// Automation. `None` if it is unavailable.
    pub fn new() -> Option<Self> {
        let mut p: Raw = std::ptr::null_mut();
        // SAFETY: standard COM start-up; the out pointer is checked.
        let hr = unsafe {
            CoInitializeEx(std::ptr::null(), COINIT_MULTITHREADED as u32);
            CoCreateInstance(
                &CLSID_CUIAUTOMATION,
                std::ptr::null_mut(),
                CLSCTX_INPROC_SERVER,
                &IID_IUIAUTOMATION,
                &mut p,
            )
        };
        (hr >= 0 && !p.is_null()).then(|| Automation(Com(p)))
    }

    fn focused(&self) -> Option<Com> {
        self.0.get(8)
    }

    /// Whether the focused element says it is a password field.
    pub fn focus_is_password(&self) -> bool {
        let Some(el) = self.focused() else {
            return false;
        };
        let mut yes = 0i32;
        // SAFETY: slot 35 is get_CurrentIsPassword(this, *mut BOOL).
        let hr = unsafe {
            el.slot::<unsafe extern "system" fn(Raw, *mut i32) -> HRESULT>(35)(el.0, &mut yes)
        };
        hr >= 0 && yes != 0
    }

    /// The caret of the focused text field, in screen pixels.
    pub fn caret(&self) -> Option<RECT> {
        let range = self.caret_range()?;
        if let Some(r) = rects(&range).first() {
            return Some(edge(r, false));
        }
        // An empty range often has no rectangle; take the character before
        // the caret and use its right edge.
        let before = range.get(3)?;
        move_start(&before, -1);
        rects(&before).last().map(|r| edge(r, true))
    }

    /// Where `word`, typed just before the caret, is drawn: one rectangle
    /// per line, in screen pixels. Empty if the app cannot say, or if the
    /// text there is not `word`.
    pub fn word_rects(&self, word: &str) -> Vec<RECT> {
        let Some(range) = self.caret_range().and_then(|r| r.get(3)) else {
            return Vec::new();
        };
        let n = word.chars().count() as i32;
        move_start(&range, -n);
        // Apps count characters differently (a Thai vowel or tone mark can be
        // a character of its own or part of the one before), so step the
        // start until the range holds exactly `word`.
        for _ in 0..=n {
            let Some(text) = text(&range) else { break };
            if text == word {
                return rects(&range).iter().map(rect).collect();
            }
            let step = if text.ends_with(word) {
                1 // took too much
            } else if word.ends_with(text.as_str()) {
                -1 // took too little
            } else {
                break;
            };
            if move_start(&range, step) == 0 {
                break;
            }
        }
        Vec::new()
    }

    /// The character right before `typed`, which was just typed before the
    /// caret. `None` if the app cannot say or the text there is not (yet)
    /// `typed`; `Some(None)` if nothing comes before it.
    pub fn char_before(&self, typed: &str) -> Option<Option<char>> {
        let range = self.caret_range()?.get(3)?;
        move_start(&range, -(typed.chars().count() as i32 + 1));
        let text = text(&range)?;
        Some(text.strip_suffix(typed)?.chars().next_back())
    }

    /// Whether the focused field can say what text is around its caret.
    pub fn reads_text(&self) -> bool {
        self.caret_range().is_some()
    }

    /// The word the caret is in or right after, ready to be selected.
    pub fn word_at_caret(&self) -> Option<WordAt> {
        let caret = self.caret_range()?;
        let boundary = |c: char| c.is_whitespace() || c.is_control() || c == '\u{fffc}';
        let before = caret.get(3)?;
        move_start(&before, -REACH);
        let before = text(&before)?;
        let after = caret.get(3)?;
        move_endpoint(&after, ENDPOINT_END, REACH);
        let after = text(&after)?;
        let left = before.rsplit(boundary).next().unwrap_or_default();
        let right = after.split(boundary).next().unwrap_or_default();
        let word = format!("{left}{right}");
        let reach = REACH as usize;
        if word.is_empty() || left.chars().count() >= reach || right.chars().count() >= reach {
            return None;
        }
        // Stretch a range over exactly that word.
        let range = caret.get(3)?;
        let end = caret.get(3)?;
        if !fit(&range, ENDPOINT_START, left) || !fit(&end, ENDPOINT_END, right) {
            return None;
        }
        // SAFETY: slot 15 is MoveEndpointByRange(this, endpoint, range, endpoint).
        let hr = unsafe {
            range.slot::<unsafe extern "system" fn(Raw, i32, Raw, i32) -> HRESULT>(15)(
                range.0,
                ENDPOINT_END,
                end.0,
                ENDPOINT_END,
            )
        };
        (hr >= 0 && text(&range).as_deref() == Some(word.as_str()))
            .then_some(WordAt { text: word, range })
    }

    /// The (usually empty) range at the caret of the focused text field.
    fn caret_range(&self) -> Option<Com> {
        let el = self.focused()?;
        let mut tp: Raw = std::ptr::null_mut();
        // SAFETY: slot 14 is GetCurrentPatternAs(this, id, riid, *mut Raw).
        let hr = unsafe {
            el.slot::<unsafe extern "system" fn(Raw, i32, *const GUID, *mut Raw) -> HRESULT>(14)(
                el.0,
                UIA_TEXT_PATTERN2_ID,
                &IID_TEXT_PATTERN2,
                &mut tp,
            )
        };
        if hr < 0 || tp.is_null() {
            return self.empty_selection(&el);
        }
        let tp = Com(tp);
        let mut active = 0i32;
        let mut range: Raw = std::ptr::null_mut();
        // SAFETY: slot 10 is GetCaretRange(this, *mut BOOL, *mut Raw).
        let hr = unsafe {
            tp.slot::<unsafe extern "system" fn(Raw, *mut i32, *mut Raw) -> HRESULT>(10)(
                tp.0,
                &mut active,
                &mut range,
            )
        };
        (hr >= 0 && !range.is_null()).then_some(Com(range))
    }

    /// The caret of a field that only has the older TextPattern (Notepad):
    /// its selection, when that is empty.
    fn empty_selection(&self, el: &Com) -> Option<Com> {
        let mut tp: Raw = std::ptr::null_mut();
        // SAFETY: slot 14 is GetCurrentPatternAs(this, id, riid, *mut Raw).
        let hr = unsafe {
            el.slot::<unsafe extern "system" fn(Raw, i32, *const GUID, *mut Raw) -> HRESULT>(14)(
                el.0,
                UIA_TEXT_PATTERN_ID,
                &IID_TEXT_PATTERN,
                &mut tp,
            )
        };
        if hr < 0 || tp.is_null() {
            return None;
        }
        // Slot 5 is IUIAutomationTextPattern::GetSelection, which hands back
        // an IUIAutomationTextRangeArray.
        let ranges = Com(tp).get(5)?;
        let mut range: Raw = std::ptr::null_mut();
        // SAFETY: slot 4 is IUIAutomationTextRangeArray::GetElement(this, index, *mut Raw).
        let hr = unsafe {
            ranges.slot::<unsafe extern "system" fn(Raw, i32, *mut Raw) -> HRESULT>(4)(
                ranges.0, 0, &mut range,
            )
        };
        if hr < 0 || range.is_null() {
            return None;
        }
        let range = Com(range);
        text(&range).is_some_and(|t| t.is_empty()).then_some(range)
    }
}

/// A word in the focused text field.
pub struct WordAt {
    pub text: String,
    range: Com,
}

impl WordAt {
    /// Selects the word, so that what is typed next replaces it.
    pub fn select(&self) -> bool {
        // SAFETY: slot 16 is Select(this).
        let hr = unsafe {
            self.range
                .slot::<unsafe extern "system" fn(Raw) -> HRESULT>(16)(self.range.0)
        };
        hr >= 0
    }
}

/// Moves one end of the empty `range` until it holds exactly `want`, the
/// text on that side of it. Apps count characters differently (see
/// [`Automation::word_rects`]), so the end is stepped until the text fits.
fn fit(range: &Com, endpoint: i32, want: &str) -> bool {
    let n = want.chars().count() as i32;
    if n == 0 {
        return true;
    }
    let out = if endpoint == ENDPOINT_START { -1 } else { 1 };
    move_endpoint(range, endpoint, out * n);
    for _ in 0..=n {
        let Some(text) = text(range) else {
            return false;
        };
        if text == want {
            return true;
        }
        let (too_much, too_little) = if endpoint == ENDPOINT_START {
            (text.ends_with(want), want.ends_with(text.as_str()))
        } else {
            (text.starts_with(want), want.starts_with(text.as_str()))
        };
        let step = if too_much {
            -out
        } else if too_little {
            out
        } else {
            return false;
        };
        if move_endpoint(range, endpoint, step) == 0 {
            return false;
        }
    }
    false
}

/// Moves the start of `range` by `count` characters; returns how far it
/// actually moved.
fn move_start(range: &Com, count: i32) -> i32 {
    move_endpoint(range, ENDPOINT_START, count)
}

fn move_endpoint(range: &Com, endpoint: i32, count: i32) -> i32 {
    let mut moved = 0i32;
    // SAFETY: slot 14 is MoveEndpointByUnit(this, endpoint, unit, count, *mut i32).
    unsafe {
        range.slot::<unsafe extern "system" fn(Raw, i32, i32, i32, *mut i32) -> HRESULT>(14)(
            range.0,
            endpoint,
            UNIT_CHARACTER,
            count,
            &mut moved,
        )
    };
    moved
}

/// The text in `range`.
fn text(range: &Com) -> Option<String> {
    let mut s: BSTR = std::ptr::null();
    // SAFETY: slot 12 is GetText(this, maxLength, *mut BSTR); the string is
    // copied and then freed.
    unsafe {
        let hr = range.slot::<unsafe extern "system" fn(Raw, i32, *mut BSTR) -> HRESULT>(12)(
            range.0, 128, &mut s,
        );
        if hr < 0 || s.is_null() {
            return None;
        }
        let units = std::slice::from_raw_parts(s, SysStringLen(s) as usize);
        let text = String::from_utf16_lossy(units);
        SysFreeString(s);
        Some(text)
    }
}

/// [left, top, width, height] as a RECT.
fn rect(r: &[f64; 4]) -> RECT {
    let [left, top, width, height] = r.map(|v| v.round() as i32);
    RECT {
        left,
        top,
        right: left + width,
        bottom: top + height,
    }
}

/// A one-pixel-wide caret at the left or right edge of `r`.
fn edge(r: &[f64; 4], right: bool) -> RECT {
    let [left, top, width, height] = r.map(|v| v.round() as i32);
    let x = if right { left + width } else { left };
    RECT {
        left: x,
        top,
        right: x + 1,
        bottom: top + height,
    }
}

/// The range's bounding rectangles as [left, top, width, height].
fn rects(range: &Com) -> Vec<[f64; 4]> {
    let mut sa: *mut SAFEARRAY = std::ptr::null_mut();
    // SAFETY: slot 10 is GetBoundingRectangles(this, *mut *mut SAFEARRAY);
    // the array holds doubles and is destroyed after copying.
    unsafe {
        let hr = range.slot::<unsafe extern "system" fn(Raw, *mut *mut SAFEARRAY) -> HRESULT>(10)(
            range.0, &mut sa,
        );
        if hr < 0 || sa.is_null() {
            return Vec::new();
        }
        let (mut lo, mut hi) = (0i32, -1i32);
        SafeArrayGetLBound(sa, 1, &mut lo);
        SafeArrayGetUBound(sa, 1, &mut hi);
        let n = (hi - lo + 1).max(0) as usize;
        let mut data: *mut c_void = std::ptr::null_mut();
        let mut out = Vec::new();
        if n >= 4 && SafeArrayAccessData(sa, &mut data) >= 0 {
            let v = std::slice::from_raw_parts(data as *const f64, n);
            out = v
                .chunks_exact(4)
                .map(|c| [c[0], c[1], c[2], c[3]])
                .filter(|r| r[3] > 0.0)
                .collect();
            SafeArrayUnaccessData(sa);
        }
        SafeArrayDestroy(sa);
        out
    }
}
