//! Low-level keyboard and mouse hooks.
//!
//! The keyboard hook keeps [`Fixer`](phitpup_core::Fixer) in step with what
//! is typed before the caret and catches Shift+Backspace. It must return
//! quickly, so the fix itself runs on the worker thread.
//!
//! In suggest mode it also asks the fixer for a suggestion after every key,
//! and the accept key (Tab), Esc or Shift+Esc act on the bubble while one is
//! shown; a word hidden with Esc three times is not suggested again. Both shortcuts come from the settings. In auto mode, Space after
//! a word the detector is sure about is swallowed and the worker types the
//! fixed word and the space instead.
//!
//! After every key it keeps what the fixer learned from the user's
//! corrections (see [`learn`]). With `learn_auto` on, a word whose fix was
//! taken often is fixed on Space and the bubble says so; with `learn_own`
//! on, a word typed on past often is left alone.
//!
//! Where [`context::gate`] says PhitPup is off (password fields and the
//! like), keys pass untouched and nothing is remembered.

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetKeyState, ToUnicodeEx, VK_BACK, VK_CAPITAL, VK_CONTROL, VK_ESCAPE,
    VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU, VK_PACKET, VK_RCONTROL, VK_RETURN,
    VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT, VK_SPACE,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetForegroundWindow, HC_ACTION, KBDLLHOOKSTRUCT, MSLLHOOKSTRUCT,
    PostThreadMessageW, SetWindowsHookExW, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN, WM_KEYUP,
    WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_RBUTTONDOWN, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_XBUTTONDOWN,
};

use phitpup_core::Kind;
use phitpup_core::config::Mode;
use phitpup_core::hotkey::DoubleTap;

use crate::context::{self, Gate};
use crate::{APP, App, WM_ACCEPT, WM_BUBBLE, WM_FIX, WM_SAVE, bubble, inject, layout};

/// CapsLock as we last saw it; GetKeyState is stale for other threads' input.
static CAPS: AtomicBool = AtomicBool::new(false);
/// Foreground window at the last key; a change means the caret moved.
static LAST_FOREGROUND: AtomicIsize = AtomicIsize::new(0);
/// Esc dismissed the bubble; it stays away until the word ends.
static DISMISSED: AtomicBool = AtomicBool::new(false);
/// The gate at the last key, to log only its changes.
static LAST_GATE: std::sync::Mutex<Gate> = std::sync::Mutex::new(Gate::On);
/// For `double_shift`.
static SHIFT_TAPS: std::sync::Mutex<DoubleTap> = std::sync::Mutex::new(DoubleTap::new());

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

/// The bubble's keep button was clicked: like Shift+Esc, the word stays as
/// typed and is not suggested again. Runs on the worker thread.
pub fn keep_as_typed() {
    let Some(app) = APP.get() else { return };
    let Some(s) = app.suggestion.lock().unwrap().take() else {
        return;
    };
    DISMISSED.store(true, Ordering::Relaxed);
    debug!("keep button: the word will not be suggested again");
    if app.config.lock().unwrap().ignore(&s.word) {
        post(app, WM_SAVE);
    }
    post(app, WM_BUBBLE);
}

/// The notice's undo button was clicked: the same as the hotkey right after
/// the fix. Runs on the worker thread.
pub fn undo_notice() {
    let Some(app) = APP.get() else { return };
    debug!("undo button: undoing the fix made on its own");
    post(app, WM_BUBBLE);
    // SAFETY: posting to the worker thread's queue.
    unsafe { PostThreadMessageW(app.worker, WM_FIX, 1, 0) };
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
        learn(app);
    }
}

/// Keeps what the fixer learned from the user's corrections, if learning is
/// on: an undone fix means leave that word alone; a hotkey fix the user kept
/// means fix that word without asking next time. Saved by the worker.
pub fn learn(app: &App) {
    let (learned, rejected) = {
        let mut f = app.fixer.lock().unwrap();
        (f.take_learned(), f.take_rejected())
    };
    if learned.is_none() && rejected.is_none() {
        return;
    }
    let mut c = app.config.lock().unwrap();
    let mut changed = false;
    if let Some(word) = &rejected
        && c.unauto(word)
    {
        debug!("a fix made on its own was undone: that word will be offered again");
        post(app, WM_SAVE);
        return;
    }
    if !c.learn {
        return;
    }
    if let Some(word) = rejected {
        debug!("a fix was undone: that word will be left alone");
        changed |= c.ignore(&word);
    }
    if let Some(word) = learned {
        debug!("a hotkey fix was kept: that word will be fixed without asking");
        changed |= c.learn_word(&word);
    }
    if changed {
        post(app, WM_SAVE);
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
    if bubble::end_notice() {
        post(app, WM_BUBBLE);
    }
    if quiet || app.mode() == Mode::Manual {
        return;
    }
    let config = app.config.lock().unwrap().clone();
    let mut fixer = app.fixer.lock().unwrap();
    (fixer.spelling, fixer.typos, fixer.slips) = (config.spelling, config.typos, config.slips);
    let s = fixer
        .suggestion(&app.detector, &config.learned_words)
        .filter(|s| !config.leaves_alone(&s.word));
    drop(fixer);
    if s.is_none() {
        DISMISSED.store(false, Ordering::Relaxed);
    }
    set_suggestion(app, s.filter(|_| !DISMISSED.load(Ordering::Relaxed)));
}

unsafe extern "system" fn keyboard(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && matches!(wparam as u32, WM_KEYDOWN | WM_SYSKEYDOWN) {
        // SAFETY: for HC_ACTION, lparam points to a KBDLLHOOKSTRUCT.
        let k = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        if k.dwExtraInfo != inject::TAG {
            SHIFT_TAPS
                .lock()
                .unwrap()
                .key_down(is_shift(k.vkCode), k.time);
            let swallow = key_down(k);
            if let Some(app) = APP.get() {
                learn(app);
            }
            if swallow {
                return 1;
            }
        }
    } else if code == HC_ACTION as i32 && matches!(wparam as u32, WM_KEYUP | WM_SYSKEYUP) {
        // SAFETY: for HC_ACTION, lparam points to a KBDLLHOOKSTRUCT.
        let k = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        let tapped = k.dwExtraInfo != inject::TAG
            && SHIFT_TAPS
                .lock()
                .unwrap()
                .key_up(is_shift(k.vkCode), k.time);
        if tapped {
            double_shift();
        }
    }
    // SAFETY: forwarding the unchanged arguments.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

fn is_shift(vk: u32) -> bool {
    matches!(vk as u16, VK_SHIFT | VK_LSHIFT | VK_RSHIFT)
}

/// Shift tapped twice: the same as the hotkey, if turned on. Unlike the
/// hotkey, it never falls back to a plain Backspace.
fn double_shift() {
    let Some(app) = APP.get() else { return };
    if !app.config.lock().unwrap().double_shift {
        return;
    }
    // SAFETY: plain query.
    let fg = unsafe { GetForegroundWindow() };
    if matches!(context::gate(app, fg), Gate::Off(_)) {
        return;
    }
    debug!("Shift tapped twice: fixing like the hotkey");
    set_suggestion(app, None);
    // SAFETY: posting to the worker thread's queue.
    unsafe { PostThreadMessageW(app.worker, WM_FIX, 1, 0) };
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
                    if app.config.lock().unwrap().ignore(&s.word) {
                        post(app, WM_SAVE);
                    }
                } else {
                    debug!("Esc: suggestion dismissed");
                    let mut c = app.config.lock().unwrap();
                    if c.learn && c.dismiss(&s.word) {
                        if c.ignore_words.contains(&s.word.to_lowercase()) {
                            debug!("Esc for the third time: the word will not be suggested again");
                        }
                        post(app, WM_SAVE);
                    }
                }
                return true;
            }
            let layout = s.kind == Kind::Layout;
            if app.config.lock().unwrap().accepted(&s.word, layout) {
                post(app, WM_SAVE);
            }
            if let Some(edit) = app.fixer.lock().unwrap().accept(&s) {
                *app.accepted.lock().unwrap() = Some(edit);
                post(app, WM_ACCEPT);
                return true;
            }
            debug!("accept key: the suggestion was out of date; the key passed through");
        }
    }

    // The word the bubble offers to fix, as Space or Enter ends it.
    let plain = !ctrl && !alt && !win;
    let offered = if plain && matches!(vk, VK_SPACE | VK_RETURN) {
        app.suggestion.lock().unwrap().clone()
    } else {
        None
    };
    // A word whose fix the user always takes (learn_auto).
    let learned_fix = offered.as_ref().is_some_and(|s| {
        s.kind == Kind::Layout
            && (app.config.lock().unwrap().auto_words).contains(&s.word.to_lowercase())
    });
    let auto = app.mode() == Mode::Auto;

    if vk == VK_SPACE && plain && !quiet && (auto || learned_fix) {
        let edit = {
            let config = app.config.lock().unwrap().clone();
            let mut learned = config.learned_words.clone();
            learned.extend(config.auto_words.iter().cloned());
            let skip = |w: &str| {
                config.leaves_alone(w) || (!auto && !config.auto_words.contains(&w.to_lowercase()))
            };
            let mut fixer = app.fixer.lock().unwrap();
            (fixer.spelling, fixer.typos, fixer.slips) =
                (config.spelling, config.typos, config.slips);
            fixer.auto(&app.detector, " ", skip, &learned)
        };
        set_suggestion(app, None);
        DISMISSED.store(false, Ordering::Relaxed);
        if let Some(edit) = edit {
            if learned_fix && !auto {
                debug!("Space: a word whose fix is always taken is fixed on its own");
                bubble::notice(edit.insert.trim_end().to_string(), edit.switch_to);
                post(app, WM_BUBBLE);
            } else {
                debug!("Space: the word is fixed automatically");
            }
            *app.accepted.lock().unwrap() = Some(edit);
            post(app, WM_ACCEPT);
            return true;
        }
        typed_past(app, offered.as_ref());
        return false; // the fixer already recorded the space
    }
    typed_past(app, offered.as_ref());

    if !win && hotkey.matches(vk, ctrl, shift, alt) {
        debug!(
            "hotkey caught ({} chars remembered)",
            app.fixer.lock().unwrap().text().chars().count()
        );
        set_suggestion(app, None);
        if bubble::end_notice() {
            post(app, WM_BUBBLE);
        }
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

/// The user ended the word `offered` was for without taking its fix: with
/// `learn_own`, count it towards their own words.
fn typed_past(app: &App, offered: Option<&phitpup_core::Suggestion>) {
    let Some(s) = offered else { return };
    let mut c = app.config.lock().unwrap();
    if c.passed(&s.word) {
        if c.leaves_alone(&s.word) {
            debug!("a word typed on past often is now one of the user's own");
        }
        post(app, WM_SAVE);
    }
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
        // SAFETY: for HC_ACTION, lparam points to an MSLLHOOKSTRUCT.
        let m = unsafe { &*(lparam as *const MSLLHOOKSTRUCT) };
        // A click on the bubble's button must not forget the word it is for.
        if !bubble::under(m.pt.x, m.pt.y) {
            reset("mouse click"); // a click can move the caret anywhere
        }
    }
    // SAFETY: forwarding the unchanged arguments.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}
