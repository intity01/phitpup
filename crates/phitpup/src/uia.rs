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
//! | IUIAutomationTextPattern2 | 10 | GetCaretRange |
//! | IUIAutomationTextRange | 3 | Clone |
//! | IUIAutomationTextRange | 10 | GetBoundingRectangles |
//! | IUIAutomationTextRange | 12 | GetText |
//! | IUIAutomationTextRange | 14 | MoveEndpointByUnit |

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
const ENDPOINT_START: i32 = 0;
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
            return None;
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
}

/// Moves the start of `range` by `count` characters; returns how far it
/// actually moved.
fn move_start(range: &Com, count: i32) -> i32 {
    let mut moved = 0i32;
    // SAFETY: slot 14 is MoveEndpointByUnit(this, endpoint, unit, count, *mut i32).
    unsafe {
        range.slot::<unsafe extern "system" fn(Raw, i32, i32, i32, *mut i32) -> HRESULT>(14)(
            range.0,
            ENDPOINT_START,
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
            range.0, 64, &mut s,
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
