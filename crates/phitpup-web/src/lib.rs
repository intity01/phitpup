//! PhitPup's core for the website's "try it" box, built for
//! `wasm32-unknown-unknown`.
//!
//! A small C-style interface instead of a bindings crate: the page asks
//! [`alloc`] for room, writes the text there as UTF-8, calls one of the
//! functions below with a layout number (0 Kedmanee, 1 Pattachote), and reads
//! the answer from [`out_ptr`] with the returned length. Answers are
//! `"th\t<text>"` or `"en\t<text>"` (the layout to switch to, then the fixed
//! text), or empty when there is nothing to fix.
//!
//! Everything runs in the visitor's browser; the page sends nothing anywhere.

use std::cell::RefCell;

use phitpup_core::{Detector, Direction, Layout, Thresholds, Verdict};

thread_local! {
    static DETECTORS: [Detector; 2] = [
        Detector::new(&Layout::us_qwerty(), &Layout::kedmanee()),
        Detector::new(&Layout::us_qwerty(), &Layout::pattachote()),
    ];
    static OUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

/// Room for `len` bytes of input, freed by [`dealloc`].
#[unsafe(no_mangle)]
pub extern "C" fn alloc(len: usize) -> *mut u8 {
    let mut buf = Vec::<u8>::with_capacity(len.max(1));
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

/// # Safety
/// `ptr` and `len` must come from one call to [`alloc`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dealloc(ptr: *mut u8, len: usize) {
    // SAFETY: the caller hands back exactly what `alloc` gave out.
    drop(unsafe { Vec::from_raw_parts(ptr, 0, len.max(1)) });
}

/// Where the last answer starts; valid until the next call.
#[unsafe(no_mangle)]
pub extern "C" fn out_ptr() -> *const u8 {
    OUT.with_borrow(|o| o.as_ptr())
}

/// The bubble: the word being typed, judged like suggest mode.
///
/// # Safety
/// `ptr` must point to `len` bytes of UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn suggest(layout: u32, ptr: *const u8, len: usize) -> usize {
    // SAFETY: as documented.
    let word = unsafe { text(ptr, len) };
    answer(layout, |d| verdict(d.judge_with(word, Thresholds::SUGGEST)))
}

/// Auto mode: the word just finished with Space, at the strict thresholds.
///
/// # Safety
/// `ptr` must point to `len` bytes of UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn auto(layout: u32, ptr: *const u8, len: usize) -> usize {
    // SAFETY: as documented.
    let word = unsafe { text(ptr, len) };
    answer(layout, |d| verdict(d.judge_with(word, Thresholds::AUTO)))
}

/// The hotkey on the last word: flips it whatever the detector thinks.
///
/// # Safety
/// `ptr` must point to `len` bytes of UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn manual(layout: u32, ptr: *const u8, len: usize) -> usize {
    // SAFETY: as documented.
    let word = unsafe { text(ptr, len) };
    answer(layout, |d| d.manual(word))
}

/// The hotkey on a selection: fixes the words it is sure about.
///
/// # Safety
/// `ptr` must point to `len` bytes of UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn selection(layout: u32, ptr: *const u8, len: usize) -> usize {
    // SAFETY: as documented.
    let text = unsafe { text(ptr, len) };
    answer(layout, |d| d.selection(text))
}

/// # Safety
/// `ptr` must point to `len` bytes of UTF-8 (or be anything when `len` is 0).
unsafe fn text<'a>(ptr: *const u8, len: usize) -> &'a str {
    if len == 0 {
        return "";
    }
    // SAFETY: the caller promises `len` readable bytes of UTF-8.
    unsafe { std::str::from_utf8(std::slice::from_raw_parts(ptr, len)).unwrap_or("") }
}

fn verdict(v: Option<Verdict>) -> Option<(Direction, String)> {
    v.filter(|v| v.flip).map(|v| (v.direction, v.converted))
}

/// Runs `f` with the chosen layout's detector and stores its answer.
fn answer(layout: u32, f: impl FnOnce(&Detector) -> Option<(Direction, String)>) -> usize {
    let fixed = DETECTORS.with(|d| f(&d[(layout as usize).min(1)]));
    let s = match fixed {
        Some((Direction::ToThai, t)) => format!("th\t{t}"),
        Some((Direction::ToEnglish, t)) => format!("en\t{t}"),
        None => String::new(),
    };
    OUT.with_borrow_mut(|o| {
        *o = s.into_bytes();
        o.len()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(f: unsafe extern "C" fn(u32, *const u8, usize) -> usize, s: &str) -> String {
        // SAFETY: a valid UTF-8 slice; the answer is read before the next call.
        unsafe {
            let n = f(0, s.as_ptr(), s.len());
            String::from_utf8(std::slice::from_raw_parts(out_ptr(), n).to_vec()).unwrap()
        }
    }

    #[test]
    fn answers() {
        assert_eq!(call(suggest, "l;ylfu"), "th\tสวัสดี");
        assert_eq!(call(suggest, "hello"), "");
        assert_eq!(call(auto, "้ำสสน"), "en\thello");
        assert_eq!(call(manual, "ok"), "th\tนา");
        assert_eq!(call(selection, "hello l;ylfu"), "th\thello สวัสดี");
        assert_eq!(call(suggest, ""), "");
    }
}
