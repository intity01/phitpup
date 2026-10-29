//! Reading a selection through the clipboard without losing what the user
//! had copied: snapshot every format, copy, read the text, put it all back.

use std::ffi::c_void;

use windows_sys::Win32::Foundation::{GlobalFree, HWND};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData,
    GetClipboardSequenceNumber, OpenClipboard, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{
    GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};
use windows_sys::Win32::System::Ole::CF_UNICODETEXT;

/// Formats whose handle is not an HGLOBAL (bitmaps, metafiles, palettes,
/// owner-drawn). Windows synthesises CF_BITMAP from CF_DIB on its own.
const NOT_HGLOBAL: &[u32] = &[2, 3, 9, 14, 0x80, 0x82, 0x83, 0x8E];

pub struct Snapshot(Vec<(u32, Vec<u8>)>);

pub fn sequence() -> u32 {
    // SAFETY: no arguments.
    unsafe { GetClipboardSequenceNumber() }
}

/// Opens the clipboard, retrying briefly while another app holds it.
fn open(owner: HWND) -> bool {
    for _ in 0..10 {
        // SAFETY: owner is our own message-only window.
        if unsafe { OpenClipboard(owner) } != 0 {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    false
}

unsafe fn global_bytes(h: *mut c_void) -> Option<Vec<u8>> {
    unsafe {
        let p = GlobalLock(h).cast::<u8>();
        if p.is_null() {
            return None;
        }
        let bytes = std::slice::from_raw_parts(p, GlobalSize(h)).to_vec();
        GlobalUnlock(h);
        Some(bytes)
    }
}

pub fn snapshot(owner: HWND) -> Snapshot {
    let mut formats = Vec::new();
    if !open(owner) {
        return Snapshot(formats);
    }
    // SAFETY: the clipboard is open; handles are only read while it stays open.
    unsafe {
        let mut f = EnumClipboardFormats(0);
        while f != 0 {
            if !NOT_HGLOBAL.contains(&f) {
                let h = GetClipboardData(f);
                if !h.is_null()
                    && let Some(bytes) = global_bytes(h)
                {
                    formats.push((f, bytes));
                }
            }
            f = EnumClipboardFormats(f);
        }
        CloseClipboard();
    }
    Snapshot(formats)
}

pub fn restore(owner: HWND, snap: &Snapshot) {
    if !open(owner) {
        return;
    }
    // SAFETY: the clipboard is open and owned by `owner`; on success
    // SetClipboardData takes ownership of each allocation.
    unsafe {
        EmptyClipboard();
        for (format, bytes) in &snap.0 {
            let h = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1));
            if h.is_null() {
                continue;
            }
            let p = GlobalLock(h).cast::<u8>();
            if !p.is_null() {
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
                GlobalUnlock(h);
            }
            if SetClipboardData(*format, h).is_null() {
                GlobalFree(h);
            }
        }
        CloseClipboard();
    }
}

/// Puts `s` on the clipboard as text, replacing what was there. Only for
/// an explicit request (the About page's "Report a problem").
pub fn set_text(owner: HWND, s: &str) -> bool {
    let units: Vec<u16> = s.encode_utf16().chain([0]).collect();
    if !open(owner) {
        return false;
    }
    // SAFETY: the clipboard is open and owned by `owner`; on success
    // SetClipboardData takes ownership of the allocation.
    unsafe {
        EmptyClipboard();
        let bytes = units.len() * 2;
        let h = GlobalAlloc(GMEM_MOVEABLE, bytes);
        let mut ok = false;
        if !h.is_null() {
            let p = GlobalLock(h).cast::<u16>();
            if !p.is_null() {
                std::ptr::copy_nonoverlapping(units.as_ptr(), p, units.len());
                GlobalUnlock(h);
                ok = !SetClipboardData(CF_UNICODETEXT.into(), h).is_null();
            }
            if !ok {
                GlobalFree(h);
            }
        }
        CloseClipboard();
        ok
    }
}

pub fn text(owner: HWND) -> Option<String> {
    if !open(owner) {
        return None;
    }
    // SAFETY: the clipboard is open; the handle is read before closing it.
    let text = unsafe {
        let h = GetClipboardData(CF_UNICODETEXT.into());
        let bytes = if h.is_null() { None } else { global_bytes(h) };
        CloseClipboard();
        bytes
    }?;
    let units: Vec<u16> = bytes_to_units(&text);
    let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
    String::from_utf16(&units[..end]).ok()
}

fn bytes_to_units(bytes: &[u8]) -> Vec<u16> {
    bytes
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect()
}
