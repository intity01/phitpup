//! Synthetic input via SendInput. Every event carries [`TAG`] so our own hook
//! can tell it apart from the user's typing.

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
    KEYEVENTF_UNICODE, SendInput, VIRTUAL_KEY, VK_BACK, VK_CONTROL, VK_INSERT, VK_LSHIFT,
    VK_RETURN, VK_RSHIFT,
};

/// `dwExtraInfo` on events we inject ("PPUP").
pub const TAG: usize = 0x5050_5550;

fn key(vk: VIRTUAL_KEY, scan: u16, flags: u32) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: TAG,
            },
        },
    }
}

fn tap(out: &mut Vec<INPUT>, vk: VIRTUAL_KEY) {
    out.push(key(vk, 0, 0));
    out.push(key(vk, 0, KEYEVENTF_KEYUP));
}

/// Types `text`. Line breaks go out as Enter, since a Unicode CR or LF is
/// read differently from app to app.
fn unicode(out: &mut Vec<INPUT>, text: &str) {
    for c in text.chars() {
        match c {
            '\r' => {}
            '\n' => tap(out, VK_RETURN),
            c => {
                let mut units = [0u16; 2];
                for &unit in c.encode_utf16(&mut units).iter() {
                    out.push(key(0, unit, KEYEVENTF_UNICODE));
                    out.push(key(0, unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
                }
            }
        }
    }
}

fn send(inputs: &[INPUT]) {
    // SAFETY: a slice of fully initialised INPUT structs.
    unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            std::mem::size_of::<INPUT>() as i32,
        )
    };
}

/// Erases `delete` characters before the caret and types `insert`, in one
/// SendInput call so the user's own keys cannot land in between.
pub fn replace(delete: usize, insert: &str) {
    let mut inputs = Vec::with_capacity(2 * (delete + insert.len()));
    for _ in 0..delete {
        tap(&mut inputs, VK_BACK);
    }
    unicode(&mut inputs, insert);
    send(&inputs);
}

/// A plain Backspace, for when the hotkey had nothing to fix.
pub fn backspace() {
    replace(1, "");
}

/// Releases Shift if the user is still holding it, so the next shortcut we
/// send is not read as Ctrl+Shift+….
pub fn release_shift() {
    let mut inputs = Vec::new();
    for vk in [VK_LSHIFT, VK_RSHIFT] {
        // SAFETY: plain key-state query.
        if unsafe { GetAsyncKeyState(vk.into()) } < 0 {
            inputs.push(key(vk, 0, KEYEVENTF_KEYUP));
        }
    }
    send(&inputs);
}

/// Ctrl+Insert: copy. Unlike Ctrl+C it never interrupts a terminal.
pub fn copy() {
    let mut inputs = vec![key(VK_CONTROL, 0, 0)];
    tap(&mut inputs, VK_INSERT);
    inputs.push(key(VK_CONTROL, 0, KEYEVENTF_KEYUP));
    send(&inputs);
}
