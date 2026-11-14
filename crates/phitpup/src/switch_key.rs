//! The key Windows switches between Thai and English with.
//!
//! Older Windows, and many Thai typists, switch with the key above Tab
//! (~, the grave accent). Windows 11 offers it too, but only deep in
//! "Advanced keyboard settings", and starts with Win+Space and Alt+Shift.
//! PhitPup offers to turn it on, and its keyboard hook needs to know when it
//! is on: that key then switches the language instead of typing a character,
//! so the word being typed ends there.
//!
//! The choice is in `HKEY_CURRENT_USER\Keyboard Layout\Toggle`: "Language
//! Hotkey" (and "Hotkey", which older Windows read) is "1" for Left
//! Alt+Shift, "2" for Ctrl+Shift, "3" for none and "4" for the grave accent.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegGetValueW, RegSetKeyValueW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    SPI_SETLANGTOGGLE, SPIF_SENDCHANGE, SystemParametersInfoW,
};

const KEY: &str = r"Keyboard Layout\Toggle";
const LANGUAGE: &str = "Language Hotkey";
const LEGACY: &str = "Hotkey";
const GRAVE: &str = "4";
/// Left Alt+Shift, what Windows starts with.
const ALT_SHIFT: &str = "1";

/// Whether ~ switches the language, as last read. The keyboard hook reads
/// this on every key, so the registry is read on the tray's timer instead.
static GRAVE_ON: AtomicBool = AtomicBool::new(false);
/// What switched the language before PhitPup set ~, to put back.
static BEFORE: Mutex<Option<String>> = Mutex::new(None);

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

fn read(name: &str) -> Option<String> {
    read_at(KEY, name)
}

fn read_at(key: &str, name: &str) -> Option<String> {
    let (key, name) = (wide(key), wide(name));
    let mut buf = [0u16; 16];
    let mut size = std::mem::size_of_val(&buf) as u32;
    // SAFETY: NUL-terminated names; the buffer's size is passed in bytes.
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buf.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if r != ERROR_SUCCESS {
        return None;
    }
    let n = (size as usize / 2).min(buf.len());
    Some(
        String::from_utf16_lossy(&buf[..n])
            .trim_end_matches('\0')
            .trim()
            .to_string(),
    )
}

fn write(name: &str, value: &str) {
    let (key, name, data) = (wide(KEY), wide(name), wide(value));
    // SAFETY: NUL-terminated strings; the data's size is in bytes and
    // includes the terminator.
    unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            REG_SZ,
            data.as_ptr().cast(),
            (data.len() * 2) as u32,
        )
    };
}

/// Which value counts: "Language Hotkey", or the older "Hotkey" without it.
fn chosen(language: Option<String>, legacy: Option<String>) -> Option<String> {
    language.or(legacy)
}

/// Reads the setting again; Windows' own settings may have changed it.
pub fn refresh() {
    let on = chosen(read(LANGUAGE), read(LEGACY)).as_deref() == Some(GRAVE);
    if GRAVE_ON.swap(on, Ordering::Relaxed) != on {
        debug!(
            "the ~ key {} the language",
            if on { "switches" } else { "no longer switches" }
        );
    }
}

/// Whether ~ switches the language.
pub fn grave() -> bool {
    GRAVE_ON.load(Ordering::Relaxed)
}

/// Makes ~ switch the language, or puts back what did before (Alt+Shift if
/// that is not known), and tells Windows to read it again.
pub fn set_grave(on: bool) {
    let value = if on {
        let now = chosen(read(LANGUAGE), read(LEGACY));
        *BEFORE.lock().unwrap() = now.filter(|v| v != GRAVE);
        GRAVE.to_string()
    } else {
        BEFORE
            .lock()
            .unwrap()
            .take()
            .unwrap_or_else(|| ALT_SHIFT.to_string())
    };
    write(LANGUAGE, &value);
    write(LEGACY, &value);
    // SAFETY: SPI_SETLANGTOGGLE takes no parameters; it re-reads the values
    // just written.
    unsafe { SystemParametersInfoW(SPI_SETLANGTOGGLE, 0, std::ptr::null_mut(), SPIF_SENDCHANGE) };
    refresh();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_newer_value_wins() {
        let s = |v: &str| Some(v.to_string());
        assert_eq!(chosen(s("4"), s("1")).as_deref(), Some("4"));
        assert_eq!(chosen(None, s("4")).as_deref(), Some("4"));
        assert_eq!(chosen(None, None), None);
    }

    /// Reads this PC's setting, changing nothing:
    /// `cargo test -p phitpup switch_key -- --ignored --nocapture`.
    #[test]
    #[ignore = "reads this PC's registry"]
    fn reads_this_pc() {
        // Every PC with a keyboard layout has this one.
        assert!(read_at(r"Keyboard Layout\Preload", "1").is_some_and(|v| v.len() == 8));
        println!(
            "Language Hotkey {:?}, Hotkey {:?}",
            read(LANGUAGE),
            read(LEGACY)
        );
        refresh();
        println!("~ switches: {}", grave());
    }
}
