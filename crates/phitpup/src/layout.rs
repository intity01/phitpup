//! The user's installed keyboard layouts.

use phitpup_core::Layout;
use phitpup_core::layouts::{Key, SCANCODES, typeable};
use phitpup_core::model::Lang;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyboardLayout, GetKeyboardLayoutList, HKL, MAPVK_VSC_TO_VK, MapVirtualKeyExW, ToUnicodeEx,
    VK_SHIFT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GUITHREADINFO, GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId, PostMessageW,
    WM_INPUTLANGCHANGEREQUEST,
};

const LANG_THAI: usize = 0x041E;
const LANG_ENGLISH_PRIMARY: usize = 0x09;
const LANG_EN_US: usize = 0x0409;
/// ToUnicodeEx: leave the keyboard state (dead keys) alone. Windows 10 1607+.
const NO_STATE_CHANGE: u32 = 0x4;

/// An installed layout: its handle and its character table.
pub struct Installed {
    pub hkl: isize,
    pub layout: Layout,
}

fn lang_id(hkl: isize) -> usize {
    hkl as usize & 0xFFFF
}

/// The English and Thai layouts to convert between, preferring US English.
pub fn find_pair() -> Result<(Installed, Installed), String> {
    // SAFETY: two-call pattern; the buffer is sized by the first call.
    let hkls: Vec<isize> = unsafe {
        let n = GetKeyboardLayoutList(0, std::ptr::null_mut());
        let mut list: Vec<HKL> = vec![std::ptr::null_mut(); n.max(0) as usize];
        let n = GetKeyboardLayoutList(n, list.as_mut_ptr());
        list.truncate(n.max(0) as usize);
        list.into_iter().map(|h| h as isize).collect()
    };
    let english = hkls
        .iter()
        .find(|&&h| lang_id(h) == LANG_EN_US)
        .or_else(|| {
            hkls.iter()
                .find(|&&h| lang_id(h) & 0x3FF == LANG_ENGLISH_PRIMARY)
        })
        .copied()
        .ok_or("no English keyboard layout is installed")?;
    let thai = hkls
        .iter()
        .find(|&&h| lang_id(h) == LANG_THAI)
        .copied()
        .ok_or(
            "no Thai keyboard layout is installed (Settings → Time & language → Language & region)",
        )?;
    Ok((read(english, "English"), read(thai, "Thai")))
}

/// Builds a character table by asking Windows what each key types.
pub fn read(hkl: isize, name: &str) -> Installed {
    let hkl_ptr = hkl as HKL;
    let keys = SCANCODES
        .iter()
        .map(|&sc| {
            let at = |shift: bool| -> Option<char> {
                let mut state = [0u8; 256];
                if shift {
                    state[VK_SHIFT as usize] = 0x80;
                }
                let mut buf = [0u16; 8];
                // SAFETY: plain buffers; NO_STATE_CHANGE keeps dead-key state intact.
                let n = unsafe {
                    let vk = MapVirtualKeyExW(sc.into(), MAPVK_VSC_TO_VK, hkl_ptr);
                    ToUnicodeEx(
                        vk,
                        sc.into(),
                        state.as_ptr(),
                        buf.as_mut_ptr(),
                        8,
                        NO_STATE_CHANGE,
                        hkl_ptr,
                    )
                };
                (n == 1)
                    .then(|| char::from_u32(buf[0].into()))
                    .flatten()
                    .and_then(typeable)
            };
            Key {
                sc,
                base: at(false),
                shift: at(true),
            }
        })
        .collect();
    Installed {
        hkl,
        layout: Layout::new(format!("{name} ({:08X})", hkl as usize as u32), keys),
    }
}

/// The layout of the thread that owns the foreground window.
pub fn foreground() -> isize {
    // SAFETY: plain queries on the foreground window.
    unsafe {
        let tid = GetWindowThreadProcessId(GetForegroundWindow(), std::ptr::null_mut());
        GetKeyboardLayout(tid) as isize
    }
}

pub fn lang_of(hkl: isize) -> Option<Lang> {
    match lang_id(hkl) {
        LANG_THAI => Some(Lang::Thai),
        id if id & 0x3FF == LANG_ENGLISH_PRIMARY => Some(Lang::English),
        _ => None,
    }
}

/// Whether the foreground window belongs to a process running as
/// administrator. Windows blocks a normal process from typing into it.
pub fn foreground_is_elevated() -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::Security::{
        GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    // SAFETY: handles are checked and closed; the token query writes into a
    // correctly sized struct.
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(GetForegroundWindow(), &mut pid);
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return false;
        }
        let mut token = std::ptr::null_mut();
        let mut elevated = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut len = 0u32;
        let ok = OpenProcessToken(process, TOKEN_QUERY, &mut token) != 0
            && GetTokenInformation(
                token,
                TokenElevation,
                (&mut elevated as *mut TOKEN_ELEVATION).cast(),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut len,
            ) != 0;
        if !token.is_null() {
            CloseHandle(token);
        }
        CloseHandle(process);
        ok && elevated.TokenIsElevated != 0
    }
}

/// Whether keyboard focus is in a standard password box. Fields drawn by the
/// app itself (browsers, most password managers) need UI Automation, which
/// comes with the self-disable rules.
pub fn focus_is_password() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        ES_PASSWORD, GWL_STYLE, GetClassNameW, GetWindowLongW,
    };
    // SAFETY: plain queries into stack buffers.
    unsafe {
        let tid = GetWindowThreadProcessId(GetForegroundWindow(), std::ptr::null_mut());
        let mut info: GUITHREADINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;
        if GetGUIThreadInfo(tid, &mut info) == 0 || info.hwndFocus.is_null() {
            return false;
        }
        let mut class = [0u16; 64];
        let n = GetClassNameW(info.hwndFocus, class.as_mut_ptr(), class.len() as i32);
        let class = String::from_utf16_lossy(&class[..n.max(0) as usize]).to_ascii_lowercase();
        class.contains("edit") && GetWindowLongW(info.hwndFocus, GWL_STYLE) & ES_PASSWORD != 0
    }
}

/// Asks the focused window to switch to `hkl`, like picking it in the tray.
pub fn activate(hkl: isize) {
    // SAFETY: plain queries and a posted message.
    unsafe {
        let fg = GetForegroundWindow();
        let tid = GetWindowThreadProcessId(fg, std::ptr::null_mut());
        let mut info: GUITHREADINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;
        let target = if GetGUIThreadInfo(tid, &mut info) != 0 && !info.hwndFocus.is_null() {
            info.hwndFocus
        } else {
            fg
        };
        PostMessageW(target, WM_INPUTLANGCHANGEREQUEST, 0, hkl);
    }
}
