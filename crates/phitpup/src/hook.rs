//! Low-level keyboard and mouse hooks.
//!
//! The keyboard hook keeps [`Fixer`](phitpup_core::Fixer) in step with what
//! is typed before the caret and catches Shift+Backspace. It must return
//! quickly, so the fix itself runs on the worker thread.

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetKeyState, ToUnicodeEx, VK_BACK, VK_CAPITAL, VK_CONTROL, VK_LCONTROL,
    VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU, VK_PACKET, VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN,
    VK_SHIFT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetForegroundWindow, HC_ACTION, KBDLLHOOKSTRUCT, PostThreadMessageW,
    SetWindowsHookExW, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN, WM_LBUTTONDOWN, WM_MBUTTONDOWN,
    WM_RBUTTONDOWN, WM_SYSKEYDOWN, WM_XBUTTONDOWN,
};

use crate::{APP, WM_FIX, inject, layout};

/// CapsLock as we last saw it; GetKeyState is stale for other threads' input.
static CAPS: AtomicBool = AtomicBool::new(false);
/// Foreground window at the last key; a change means the caret moved.
static LAST_FOREGROUND: AtomicIsize = AtomicIsize::new(0);

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

fn reset() {
    if let Some(app) = APP.get() {
        app.fixer.lock().unwrap().reset();
    }
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
    let fg = unsafe { GetForegroundWindow() } as isize;
    if LAST_FOREGROUND.swap(fg, Ordering::Relaxed) != fg {
        reset();
    }

    let shift = down(VK_SHIFT);
    let ctrl = down(VK_CONTROL);
    let alt = down(VK_MENU);
    let win = down(VK_LWIN) || down(VK_RWIN);

    match vk {
        VK_SHIFT | VK_LSHIFT | VK_RSHIFT | VK_CONTROL | VK_LCONTROL | VK_RCONTROL | VK_MENU
        | VK_LMENU | VK_RMENU | VK_LWIN | VK_RWIN => {}
        VK_CAPITAL => {
            CAPS.fetch_xor(true, Ordering::Relaxed);
        }
        VK_BACK if shift && !ctrl && !alt && !win => {
            // SAFETY: posting to the worker thread's queue.
            unsafe { PostThreadMessageW(app.worker, WM_FIX, 0, 0) };
            return true;
        }
        VK_BACK if !ctrl && !alt && !win => app.fixer.lock().unwrap().backspace(),
        // A character sent as Unicode (IMEs, automation, on-screen keyboards).
        VK_PACKET => match char::from_u32(k.scanCode) {
            Some(c) if !c.is_control() => {
                app.fixer.lock().unwrap().typed(c.encode_utf8(&mut [0; 4]))
            }
            _ => reset(),
        },
        // Shortcuts (and AltGr) do not type into the buffer we mirror.
        _ if ctrl || alt || win => reset(),
        _ => {
            let text = translate(k.vkCode, k.scanCode, shift);
            let mut fixer = app.fixer.lock().unwrap();
            if text.is_empty() {
                fixer.reset(); // arrows, Enter, Tab, Esc, F-keys…
            } else {
                fixer.typed(&text);
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
        reset(); // a click can move the caret anywhere
    }
    // SAFETY: forwarding the unchanged arguments.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}
