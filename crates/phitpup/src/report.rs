//! "Report a problem" on the About page: copies a short description of this
//! copy of PhitPup to the clipboard and opens a new GitHub issue, where the
//! person pastes it. Nothing is sent by PhitPup itself, and the text never
//! holds anything typed: version, Windows, keyboard layouts and settings,
//! with the word lists given only as counts.

use std::sync::atomic::{AtomicBool, Ordering};

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::Registry::{
    HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegGetValueW,
};

use crate::{APP, clipboard, i18n, settings, update};

const NEW_ISSUE: &str = "https://github.com/intity01/phitpup/issues/new";

/// Set once the details were copied, so the About page can say so.
pub static COPIED: AtomicBool = AtomicBool::new(false);

/// The button was clicked.
pub fn copy_and_open(owner: HWND) {
    if clipboard::set_text(owner, &details()) {
        COPIED.store(true, Ordering::Relaxed);
    }
    settings::open(NEW_ISSUE);
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

fn windows_value(name: &str) -> Option<String> {
    let key = wide(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion");
    let name = wide(name);
    let mut buf = [0u16; 128];
    let mut size = (buf.len() * 2) as u32;
    // SAFETY: a fixed buffer whose size in bytes is passed along.
    let ok = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buf.as_mut_ptr().cast(),
            &mut size,
        )
    } == 0;
    let end = buf.iter().position(|&u| u == 0).unwrap_or(buf.len());
    ok.then(|| String::from_utf16_lossy(&buf[..end]))
}

fn windows_ubr() -> Option<u32> {
    let key = wide(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion");
    let name = wide("UBR");
    let (mut value, mut size) = (0u32, 4u32);
    // SAFETY: a 4-byte buffer for a DWORD.
    let ok = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            std::ptr::addr_of_mut!(value).cast(),
            &mut size,
        )
    } == 0;
    ok.then_some(value)
}

/// The text that is copied.
pub fn details() -> String {
    let mut out = String::new();
    let mut line = |k: &str, v: String| {
        out.push_str(&format!("- {k}: {v}\n"));
    };
    line(
        "PhitPup",
        format!(
            "{} ({})",
            env!("CARGO_PKG_VERSION"),
            if update::helper().is_some() {
                "installed"
            } else {
                "single file"
            }
        ),
    );
    let mut product = windows_value("ProductName").unwrap_or_default();
    let display = windows_value("DisplayVersion").unwrap_or_default();
    let build = windows_value("CurrentBuild").unwrap_or_default();
    // Windows 11 still calls itself Windows 10 here; the build tells them apart.
    if build.parse::<u32>().is_ok_and(|b| b >= 22000) {
        product = product.replace("Windows 10", "Windows 11");
    }
    let ubr = windows_ubr().map_or_else(String::new, |u| format!(".{u}"));
    line(
        "Windows",
        format!("{product} {display}, build {build}{ubr}"),
    );
    line(
        "Settings language",
        if i18n::thai() { "Thai" } else { "English" }.into(),
    );
    if let Some(app) = APP.get() {
        line(
            "Keyboard layouts",
            format!("English {:08X}, Thai {:08X}", app.english, app.thai),
        );
        line("Paused", app.paused.load(Ordering::Relaxed).to_string());
        let c = app.config.lock().unwrap();
        line("Mode", c.mode.as_str().into());
        line("Fix hotkey", c.hotkey.label());
        line("Accept key", c.accept_key.label());
        line(
            "Switches",
            format!(
                "underline {}, bubble above {}, spelling {}, typos {}, slips {}, double Shift {}, learn {}, off in full screen {}, update reminder {}",
                c.underline,
                c.bubble_above,
                c.spelling,
                c.typos,
                c.slips,
                c.double_shift,
                c.learn,
                c.off_in_fullscreen,
                c.remind_updates
            ),
        );
        line(
            "Programs",
            format!(
                "off: {}; no bubble: {}",
                list(&c.off_apps),
                list(&c.quiet_apps)
            ),
        );
        line(
            "Word lists",
            format!(
                "{} left alone, {} always fixed (words not included)",
                c.ignore_words.len(),
                c.learned_words.len()
            ),
        );
    }
    line(
        "Start with Windows",
        settings::starts_with_windows().to_string(),
    );
    out
}

fn list(apps: &[String]) -> String {
    if apps.is_empty() {
        "none".into()
    } else {
        apps.join(", ")
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn details_name_the_version_and_windows() {
        let d = super::details();
        println!("{d}");
        assert!(d.contains(env!("CARGO_PKG_VERSION")));
        assert!(d.contains("Windows"));
    }
}
