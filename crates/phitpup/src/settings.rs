//! `%APPDATA%\PhitPup\config.toml`, and the "start with Windows" entry.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::SystemTime;

use phitpup_core::config::{Config, TEMPLATE};
use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SZ, RegCloseKey, RegDeleteValueW,
    RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use crate::{APP, hook};

pub fn path() -> PathBuf {
    let base = std::env::var_os("APPDATA").map_or_else(|| PathBuf::from("."), PathBuf::from);
    base.join("PhitPup").join("config.toml")
}

pub fn modified() -> Option<SystemTime> {
    std::fs::metadata(path()).and_then(|m| m.modified()).ok()
}

/// Reads the settings, writing the commented template first if there are
/// none. The second value lists problems to show the user.
pub fn load() -> (Config, Vec<String>) {
    let path = path();
    let out = match std::fs::read_to_string(&path) {
        Ok(text) => Config::parse(&text),
        Err(_) => {
            let _ = std::fs::create_dir_all(path.parent().unwrap());
            let _ = std::fs::write(&path, TEMPLATE);
            (Config::default(), Vec::new())
        }
    };
    *SEEN.lock().unwrap() = modified();
    out
}

/// The file's modification time when we last read or wrote it, so our own
/// saves are not mistaken for edits made by hand.
static SEEN: Mutex<Option<SystemTime>> = Mutex::new(None);

/// Writes `config` into the file, keeping its comments and layout.
pub fn save(config: &Config) {
    let path = path();
    let text = std::fs::read_to_string(&path).unwrap_or_else(|_| TEMPLATE.into());
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    match std::fs::write(&path, config.write_to(&text)) {
        Ok(()) => *SEEN.lock().unwrap() = modified(),
        Err(e) => debug!("could not save settings: {e}"),
    }
}

/// Makes `config` the running settings and saves it.
pub fn apply(config: Config) {
    let Some(app) = APP.get() else { return };
    save(&config);
    *app.config.lock().unwrap() = config;
    crate::context::forget();
    hook::reset("settings changed");
}

/// Reads the file again if it changed since we last read or wrote it.
pub fn changed_on_disk() -> Option<(Config, Vec<String>)> {
    let now = modified();
    let mut seen = SEEN.lock().unwrap();
    if *seen == now {
        return None;
    }
    *seen = now;
    drop(seen);
    Some(load())
}

/// Opens `target` (a web address or a file) with its default program.
pub fn open(target: &str) {
    // SAFETY: NUL-terminated strings.
    unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            wide("open").as_ptr(),
            wide(target).as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
}

/// Opens the settings file in Notepad, creating it first if needed.
pub fn open_file() {
    load();
    let file = wide(&format!("\"{}\"", path().display()));
    // SAFETY: NUL-terminated strings.
    unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            wide("open").as_ptr(),
            wide("notepad.exe").as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
}

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "PhitPup";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

fn run_key(access: u32) -> Option<HKEY> {
    let mut key: HKEY = std::ptr::null_mut();
    // SAFETY: the key name is NUL-terminated; the handle is closed by callers.
    let r = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            wide(RUN_KEY).as_ptr(),
            0,
            access,
            &mut key,
        )
    };
    (r == ERROR_SUCCESS).then_some(key)
}

/// Whether Windows starts PhitPup at sign-in.
pub fn starts_with_windows() -> bool {
    let Some(key) = run_key(KEY_QUERY_VALUE) else {
        return false;
    };
    // SAFETY: a size-only query on an open key, then closed.
    unsafe {
        let r = RegQueryValueExW(
            key,
            wide(RUN_VALUE).as_ptr(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        RegCloseKey(key);
        r == ERROR_SUCCESS
    }
}

/// Adds or removes this exe in the current user's Run key.
pub fn set_start_with_windows(on: bool) {
    let Some(key) = run_key(KEY_SET_VALUE) else {
        return;
    };
    // SAFETY: NUL-terminated strings; the data length is in bytes and
    // includes the terminator; the key is closed afterwards.
    unsafe {
        let name = wide(RUN_VALUE);
        if on {
            if let Ok(exe) = std::env::current_exe() {
                let data = wide(&format!("\"{}\"", exe.display()));
                RegSetValueExW(
                    key,
                    name.as_ptr(),
                    0,
                    REG_SZ,
                    data.as_ptr().cast(),
                    (data.len() * 2) as u32,
                );
            }
        } else {
            RegDeleteValueW(key, name.as_ptr());
        }
        RegCloseKey(key);
    }
}
