//! The text around the caret in a plain Win32 edit box.
//!
//! Classic Notepad, the Run box and many older programs use the system's
//! `Edit` control, which says nothing about its text through UI Automation
//! (see [`crate::uia`]) but answers window messages from any program. Rich
//! edit boxes count line breaks differently in their positions and their
//! text, so they are left out.

use windows_sys::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GUITHREADINFO, GetClassNameW, GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId,
    SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_GETTEXT, WM_GETTEXTLENGTH,
};

const EM_GETSEL: u32 = 0x00B0;
const EM_SETSEL: u32 = 0x00B1;
/// Longer texts are left alone: positions come back as 16-bit numbers.
const MAX_UNITS: usize = 30_000;
/// The longest word looked for on either side of the caret.
const REACH: usize = 40;

/// The text of the focused edit box, and where its caret is in it.
pub struct Caret {
    hwnd: HWND,
    text: Vec<u16>,
    at: usize,
}

/// A word in the box, by its place in the text.
pub struct Word {
    hwnd: HWND,
    pub text: String,
    start: usize,
    end: usize,
}

/// Sends `msg` and waits briefly for the answer; `None` if the program is
/// too busy to give one.
fn ask(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> Option<usize> {
    let mut result = 0usize;
    // SAFETY: a plain message with a short timeout; the result is ours.
    let ok = unsafe {
        SendMessageTimeoutW(
            hwnd,
            msg,
            wparam,
            lparam,
            SMTO_ABORTIFHUNG,
            200,
            &mut result,
        )
    };
    (ok != 0).then_some(result)
}

/// The focused control of the window in front, if it is a plain edit box.
fn focused() -> Option<HWND> {
    // SAFETY: plain queries into a zeroed struct and a sized buffer.
    unsafe {
        let tid = GetWindowThreadProcessId(GetForegroundWindow(), std::ptr::null_mut());
        let mut info: GUITHREADINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;
        if GetGUIThreadInfo(tid, &mut info) == 0 || info.hwndFocus.is_null() {
            return None;
        }
        let mut class = [0u16; 16];
        let n = GetClassNameW(info.hwndFocus, class.as_mut_ptr(), class.len() as i32);
        let class = String::from_utf16_lossy(&class[..n.max(0) as usize]);
        class.eq_ignore_ascii_case("Edit").then_some(info.hwndFocus)
    }
}

impl Caret {
    /// The caret of the focused edit box. `None` if the focus is not in
    /// one, something is selected, or the text is very long.
    pub fn focused() -> Option<Caret> {
        let hwnd = focused()?;
        let len = ask(hwnd, WM_GETTEXTLENGTH, 0, 0)?;
        if len > MAX_UNITS {
            return None;
        }
        let sel = ask(hwnd, EM_GETSEL, 0, 0)?;
        let (start, end) = (sel & 0xFFFF, (sel >> 16) & 0xFFFF);
        if start != end {
            return None;
        }
        let mut text = vec![0u16; len + 1];
        // The system copies the text across for WM_GETTEXT.
        let got = ask(hwnd, WM_GETTEXT, text.len(), text.as_mut_ptr() as LPARAM)?;
        text.truncate(got.min(len));
        (start <= text.len()).then_some(Caret {
            hwnd,
            text,
            at: start,
        })
    }

    /// The character right before `typed`, which was just typed before the
    /// caret. `None` if the text there is not (yet) `typed`; `Some(None)`
    /// if nothing comes before it.
    pub fn char_before(&self, typed: &str) -> Option<Option<char>> {
        let before = String::from_utf16_lossy(&self.text[..self.at]);
        Some(before.strip_suffix(typed)?.chars().next_back())
    }

    /// The word the caret is in or right after.
    pub fn word(&self) -> Option<Word> {
        let boundary =
            |u: &u16| char::from_u32(*u as u32).is_none_or(|c| c.is_whitespace() || c.is_control());
        let start = self.text[..self.at]
            .iter()
            .rposition(boundary)
            .map_or(0, |i| i + 1);
        let end = self.text[self.at..]
            .iter()
            .position(boundary)
            .map_or(self.text.len(), |i| self.at + i);
        if start == end || self.at - start >= REACH || end - self.at >= REACH {
            return None;
        }
        Some(Word {
            hwnd: self.hwnd,
            text: String::from_utf16_lossy(&self.text[start..end]),
            start,
            end,
        })
    }
}

impl Word {
    /// Selects the word, so that what is typed next replaces it.
    pub fn select(&self) -> bool {
        ask(self.hwnd, EM_SETSEL, self.start, self.end as LPARAM).is_some()
    }
}
