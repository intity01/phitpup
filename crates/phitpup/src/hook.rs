//! Low-level keyboard and mouse hooks.
//!
//! The keyboard hook keeps [`Fixer`](phitpup_core::Fixer) in step with what
//! is typed before the caret and catches Shift+Backspace. It must return
//! quickly, so the fix itself runs on the worker thread.
//!
//! In suggest mode it also asks the fixer for a suggestion after every key,
//! and the accept key (Tab), Esc or Shift+Esc act on the bubble while one is
//! shown. Both shortcuts come from the settings. In auto mode, Space after
//! a word the detector is sure about is swallowed and the worker types the
//! fixed word and the space instead.
//!
//! Where [`context::gate`] says PhitPup is off (password fields and the
//! like), keys pass untouched and nothing is remembered.

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetKeyState, ToUnicodeEx, VK_BACK, VK_CAPITAL, VK_CONTROL, VK_ESCAPE,
    VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU, VK_PACKET, VK_RCONTROL, VK_RMENU,
    VK_RSHIFT, VK_RWIN, VK_SHIFT, VK_SPACE,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetForegroundWindow, HC_ACTION, KBDLLHOOKSTRUCT, PostThreadMessageW,
    SetWindowsHookExW, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN, WM_LBUTTONDOWN, WM_MBUTTONDOWN,
    WM_RBUTTONDOWN, WM_SYSKEYDOWN, WM_XBUTTONDOWN,
};

use phitpup_core::config::Mode;

use crate::context::{self, Gate};
use crate::{APP, App, WM_ACCEPT, WM_BUBBLE, WM_FIX, WM_SAVE, inject, layout};

/// CapsLock as we last saw it; GetKeyState is stale for other threads' input.
static CAPS: AtomicBool = AtomicBool::new(false);
/// Foreground window at the last key; a change means the caret moved.
static LAST_FOREGROUND: AtomicIsize = AtomicIsize::new(0);
/// Esc dismissed the bubble; it stays away until the word ends.
static DISMISSED: AtomicBool = AtomicBool::new(false);
/// The gate at the last key, to log only its changes.
static LAST_GATE: std::sync::Mutex<Gate> = std::sync::Mutex::new(Gate::On);

/// ToUnicodeEx: leave the keyboard state (dead keys) alone. Windows 10 1607+.
const NO_STATE_CHANGE: u32 = 0x4;

pub fn install() -> Result<(), String> {
    // SAFETY: the hook procedures are 'static functions, and this thread runs
    // a message loop for as long as the process lives.
    unsafe {
        CAPS.store(GetKeyState(VK_CAPITAL.into()) & 1 != 0, Ordering::Relaxed);
        let module = GetModuleHandleW(std::ptr::null());
        if SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard), module, 0).is_null() {
            return Err("could not install the keyboard hook".into());
        }
        if SetWindowsHookExW(WH_MOUSE_LL, Some(mouse), module, 0).is_null() {
            return Err("could not install the mouse hook".into());
        }
    }
    Ok(())
}

fn down(vk: u16) -> bool {
    // SAFETY: plain key-state query.
    unsafe { GetAsyncKeyState(vk.into()) < 0 }
}

/// Forgets what was typed and hides the bubble.
pub fn reset(why: &str) {
    if let Some(app) = APP.get() {
        let mut fixer = app.fixer.lock().unwrap();
        if !fixer.is_empty() {
            debug!("forgot what was typed: {why}");
        }
        fixer.reset();
        drop(fixer);
        DISMISSED.store(false, Ordering::Relaxed);
        set_suggestion(app, None);
    }
}

fn post(app: &App, msg: u32) {
    // SAFETY: posting to the worker thread's queue.
    unsafe { PostThreadMessageW(app.worker, msg, 0, 0) };
}

/// Shows `s` in the bubble (or hides it), telling the worker only on change.
fn set_suggestion(app: &App, s: Option<phitpup_core::Suggestion>) {
    let mut current = app.suggestion.lock().unwrap();
    if *current != s {
        *current = s;
        post(app, WM_BUBBLE);
    }
}

/// Asks the fixer whether the word being typed needs fixing.
fn refresh(app: &App, quiet: bool) {
    if quiet || app.mode() == Mode::Manual {
        return;
    }
    let s = app
        .fixer
        .lock()
        .unwrap()
        .suggestion(&app.detector)
        .filter(|s| {
            let word = s.word.to_lowercase();
            !app.config.lock().unwrap().ignore_words.contains(&word)
        });
    if s.is_none() {
        DISMISSED.store(false, Ordering::Relaxed);
    }
    set_suggestion(app, s.filter(|_| !DISMISSED.load(Ordering::Relaxed)));
}

unsafe extern "system" fn keyboard(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && matches!(wparam as u32, WM_KEYDOWN | WM_SYSKEYDOWN) {
        // SAFETY: for HC_ACTION, lparam points to a KBDLLHOOKSTRUCT.
        let k = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        if k.dwExtraInfo != inject::TAG && key_down(k) {
            return 1; // swallow
        }
    }
    // SAFETY: forwarding the unchanged arguments.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

/// Returns true to swallow the key.
fn key_down(k: &KBDLLHOOKSTRUCT) -> bool {
    let Some(app) = APP.get() else { return false };
    let vk = k.vkCode as u16;

    // SAFETY: plain query.
    let fg = unsafe { GetForegroundWindow() };
    if LAST_FOREGROUND.swap(fg as isize, Ordering::Relaxed) != fg as isize {
        reset("switched to another window");
    }

    let gate = context::gate(app, fg);
    {
        let mut last = LAST_GATE.lock().unwrap();
        if *last != gate {
            match gate {
                Gate::On => debug!("on"),
                Gate::Quiet(why) => debug!("bubble off: {why}"),
                Gate::Off(why) => debug!("off: {why}"),
            }
            *last = gate;
        }
    }
    if let Gate::Off(why) = gate {
        if vk == VK_CAPITAL {
            CAPS.fetch_xor(true, Ordering::Relaxed);
        }
        reset(why);
        return false;
    }
    let quiet = matches!(gate, Gate::Quiet(_));

    let shift = down(VK_SHIFT);
    let ctrl = down(VK_CONTROL);
    let alt = down(VK_MENU);
    let win = down(VK_LWIN) || down(VK_RWIN);
    let (hotkey, accept) = {
        let c = app.config.lock().unwrap();
        (c.hotkey, c.accept_key)
    };
    let escape = !ctrl && !alt && !win && vk == VK_ESCAPE;
    let accepting = !win && accept.matches(vk, ctrl, shift, alt);

    if escape || accepting {
        let shown = app.suggestion.lock().unwrap().take();
        if let Some(s) = shown {
            post(app, WM_BUBBLE);
            if escape {
                DISMISSED.store(true, Ordering::Relaxed);
                if shift {
                    debug!("Shift+Esc: the word will not be suggested again");
                    let mut c = app.config.lock().unwrap();
                    let word = s.word.to_lowercase();
                    if phitpup_core::config::storable(&word) && !c.ignore_words.contains(&word) {
                        c.ignore_words.push(word);
                        post(app, WM_SAVE);
                    }
                } else {
                    debug!("Esc: suggestion dismissed");
                }
                return true;
            }
            if let Some(edit) = app.fixer.lock().unwrap().accept(&s) {
                *app.accepted.lock().unwrap() = Some(edit);
                post(app, WM_ACCEPT);
                return true;
            }
            debug!("accept key: the suggestion was out of date; the key passed through");
        }
    }

    if vk == VK_SPACE && !ctrl && !alt && !win && !quiet && app.mode() == Mode::Auto {
        let edit = {
            let ignored = app.config.lock().unwrap().ignore_words.clone();
            let skip = |w: &str| ignored.contains(&w.to_lowercase());
            app.fixer.lock().unwrap().auto(&app.detector, " ", skip)
        };
        set_suggestion(app, None);
        DISMISSED.store(false, Ordering::Relaxed);
        if let Some(edit) = edit {
            debug!("Space: the word is fixed automatically");
            *app.accepted.lock().unwrap() = Some(edit);
            post(app, WM_ACCEPT);
            return true;
        }
        return false; // the fixer already recorded the space
    }

    if !win && hotkey.matches(vk, ctrl, shift, alt) {
        debug!(
            "hotkey caught ({} chars remembered)",
            app.fixer.lock().unwrap().text().chars().count()
        );
        set_suggestion(app, None);
        post(app, WM_FIX);
        return true;
    }

    match vk {
        VK_SHIFT | VK_LSHIFT | VK_RSHIFT | VK_CONTROL | VK_LCONTROL | VK_RCONTROL | VK_MENU
        | VK_LMENU | VK_RMENU | VK_LWIN | VK_RWIN => {}
        VK_CAPITAL => {
            CAPS.fetch_xor(true, Ordering::Relaxed);
        }
        VK_BACK if !ctrl && !alt && !win => {
            app.fixer.lock().unwrap().backspace();
            refresh(app, quiet);
        }
        // A character sent as Unicode (IMEs, automation, on-screen keyboards).
        VK_PACKET => match char::from_u32(k.scanCode) {
            Some(c) if !c.is_control() => {
                app.fixer.lock().unwrap().typed(c.encode_utf8(&mut [0; 4]));
                refresh(app, quiet);
            }
            _ => reset("control character"),
        },
        // Shortcuts (and AltGr) do not type into the buffer we mirror.
        _ if ctrl || alt || win => reset("shortcut key"),
        _ => {
            let text = translate(k.vkCode, k.scanCode, shift);
            if text.is_empty() {
                reset("a key that moves the caret or types nothing (arrows, Enter, Tab…)");
            } else {
                app.fixer.lock().unwrap().typed(&text);
                refresh(app, quiet);
            }
        }
    }
    false
}

/// What the key types in the foreground window's current layout.
fn translate(vk: u32, scan: u32, shift: bool) -> String {
    let mut state = [0u8; 256];
    if shift {
        state[VK_SHIFT as usize] = 0x80;
    }
    if CAPS.load(Ordering::Relaxed) {
        state[VK_CAPITAL as usize] = 0x01;
    }
    let mut buf = [0u16; 8];
    // SAFETY: plain buffers; NO_STATE_CHANGE keeps dead-key state intact.
    let n = unsafe {
        ToUnicodeEx(
            vk,
            scan,
            state.as_ptr(),
            buf.as_mut_ptr(),
            8,
            NO_STATE_CHANGE,
            layout::foreground() as _,
        )
    };
    if n <= 0 {
        return String::new();
    }
    let text = String::from_utf16_lossy(&buf[..n as usize]);
    if text.chars().any(char::is_control) {
        String::new()
    } else {
        text
    }
}

unsafe extern "system" fn mouse(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32
        && matches!(
            wparam as u32,
            WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN
        )
    {
        reset("mouse click"); // a click can move the caret anywhere
    }
    // SAFETY: forwarding the unchanged arguments.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}
