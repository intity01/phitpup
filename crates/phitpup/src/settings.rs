//! `%APPDATA%\PhitPup\config.toml`, and the "start with Windows" entry.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::SystemTime;

use phitpup_core::config::{Config, Language, TEMPLATE};
use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SZ, RegCloseKey, RegDeleteValueW,
    RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use crate::{APP, hook};

/// The settings file. `PHITPUP_HOME` puts it, and the files kept beside it,
/// in another folder (`cargo xtask e2e` uses this to leave yours alone).
pub fn path() -> PathBuf {
    if let Some(home) = std::env::var_os("PHITPUP_HOME") {
        return PathBuf::from(home).join("config.toml");
    }
    if let Some(dir) = crate::store::data_dir() {
        return dir.join("config.toml");
    }
    let base = std::env::var_os("APPDATA").map_or_else(|| PathBuf::from("."), PathBuf::from);
    base.join("PhitPup").join("config.toml")
}

/// The page with the newest version.
pub const RELEASES: &str = "https://github.com/intity01/phitpup/releases/latest";

/// Remembers when the update reminder last showed, next to the settings.
fn reminded_path() -> PathBuf {
    path().with_file_name("update-reminder")
}

/// When the update reminder last showed, in Unix seconds.
pub fn last_reminder() -> Option<u64> {
    std::fs::read_to_string(reminded_path())
        .ok()?
        .trim()
        .parse()
        .ok()
}

pub fn set_last_reminder(secs: u64) {
    let _ = std::fs::write(reminded_path(), secs.to_string());
}

/// The page listing what changed in each version.
pub const CHANGES: &str = "https://intity01.github.io/phitpup/changes.html";

/// The version that ran last, kept next to the settings so an update can
/// be announced once.
fn last_version_path() -> PathBuf {
    path().with_file_name("last-version")
}

pub fn last_version() -> Option<String> {
    let v = std::fs::read_to_string(last_version_path()).ok()?;
    Some(v.trim().to_string())
}

pub fn set_last_version(v: &str) {
    let _ = std::fs::write(last_version_path(), v);
}

/// How many words PhitPup has fixed, kept next to the settings. Only the
/// number: never which words.
fn fixed_path() -> PathBuf {
    path().with_file_name("words-fixed")
}

static FIXED: std::sync::OnceLock<std::sync::atomic::AtomicU64> = std::sync::OnceLock::new();

fn fixed_counter() -> &'static std::sync::atomic::AtomicU64 {
    FIXED.get_or_init(|| {
        let n = std::fs::read_to_string(fixed_path())
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);
        std::sync::atomic::AtomicU64::new(n)
    })
}

/// How many words PhitPup has fixed so far.
pub fn words_fixed() -> u64 {
    fixed_counter().load(std::sync::atomic::Ordering::Relaxed)
}

/// One more word (or selection) fixed.
pub fn count_fix() {
    let n = fixed_counter().fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    let _ = std::fs::write(fixed_path(), n.to_string());
}

/// When the newest crash the person was told about happened (as Windows
/// logged it), kept next to the settings so it is offered for reporting
/// once.
fn last_crash_path() -> PathBuf {
    path().with_file_name("last-crash")
}

pub fn last_crash_seen() -> Option<String> {
    let t = std::fs::read_to_string(last_crash_path()).ok()?;
    Some(t.trim().to_string())
}

pub fn set_last_crash_seen(time: &str) {
    let _ = std::fs::write(last_crash_path(), time);
}

/// When the Store build last asked for a review, in Unix seconds, kept
/// next to the settings so it asks once.
fn review_path() -> PathBuf {
    path().with_file_name("review-asked")
}

pub fn review_asked() -> Option<u64> {
    std::fs::read_to_string(review_path())
        .ok()?
        .trim()
        .parse()
        .ok()
}

pub fn set_review_asked(secs: u64) {
    let _ = std::fs::write(review_path(), secs.to_string());
}

pub fn modified() -> Option<SystemTime> {
    std::fs::metadata(path()).and_then(|m| m.modified()).ok()
}

/// The language Setup ran in ("th" or "en"), left by the installer for
/// the first run.
fn setup_language_path() -> PathBuf {
    path().with_file_name("setup-language")
}

/// Reads the settings, writing the commented template first if there are
/// none, and speaks their language from now on. The second value lists
/// problems to show the user.
pub fn load() -> (Config, Vec<String>) {
    let path = path();
    let out = match std::fs::read_to_string(&path) {
        Ok(text) => Config::parse(&text),
        Err(_) => {
            let c = Config {
                language: first_language(),
                ..Config::default()
            };
            let _ = std::fs::create_dir_all(path.parent().unwrap());
            let _ = std::fs::write(&path, c.write_to(TEMPLATE));
            (c, Vec::new())
        }
    };
    *SEEN.lock().unwrap() = modified();
    crate::i18n::set(out.0.language);
    out
}

/// The first run speaks the language Setup was run in, if that is not the
/// Windows one anyway.
fn first_language() -> Language {
    let setup = std::fs::read_to_string(setup_language_path());
    let _ = std::fs::remove_file(setup_language_path());
    match (setup.as_deref().map(str::trim), crate::i18n::windows_thai()) {
        (Ok("th"), false) => Language::Thai,
        (Ok("en"), true) => Language::English,
        _ => Language::Auto,
    }
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
    crate::i18n::set(config.language);
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
    if crate::store::packaged() {
        return crate::store::starts_with_windows();
    }
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
    if crate::store::packaged() {
        return crate::store::set_start_with_windows(on);
    }
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
