//! Keyboard shortcuts, written in the settings file as `"shift+backspace"`.
//!
//! Keys are Windows virtual-key codes. They are plain numbers, so this module
//! still makes no OS calls.

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hotkey {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    /// Virtual-key code of the main (non-modifier) key.
    pub vk: u16,
}

pub const VK_BACK: u16 = 0x08;
pub const VK_TAB: u16 = 0x09;
pub const VK_ESCAPE: u16 = 0x1B;

impl Hotkey {
    /// Fixes the last word: Shift+Backspace.
    pub const FIX: Hotkey = Hotkey::new(false, true, false, VK_BACK);
    /// Accepts the suggestion: Tab.
    pub const ACCEPT: Hotkey = Hotkey::new(false, false, false, VK_TAB);

    pub const fn new(ctrl: bool, shift: bool, alt: bool, vk: u16) -> Self {
        Self {
            ctrl,
            shift,
            alt,
            vk,
        }
    }

    pub fn matches(&self, vk: u16, ctrl: bool, shift: bool, alt: bool) -> bool {
        *self == Hotkey::new(ctrl, shift, alt, vk)
    }

    /// Reads `"ctrl+shift+space"` (any case, spaces allowed around `+`).
    pub fn parse(s: &str) -> Option<Hotkey> {
        let mut key = Hotkey::new(false, false, false, 0);
        let parts: Vec<String> = s.split('+').map(|p| p.trim().to_lowercase()).collect();
        let (main, mods) = parts.split_last()?;
        for m in mods {
            let flag = match m.as_str() {
                "ctrl" | "control" => &mut key.ctrl,
                "shift" => &mut key.shift,
                "alt" => &mut key.alt,
                _ => return None,
            };
            if *flag {
                return None;
            }
            *flag = true;
        }
        key.vk = vk_of(main)?;
        Some(key)
    }

    /// Keycap labels, modifiers first: `["Shift", "Backspace"]`.
    pub fn caps(&self) -> Vec<String> {
        let mut out: Vec<String> = [
            (self.ctrl, "Ctrl"),
            (self.shift, "Shift"),
            (self.alt, "Alt"),
        ]
        .into_iter()
        .filter(|(on, _)| *on)
        .map(|(_, name)| name.to_string())
        .collect();
        out.push(label_of(self.vk));
        out
    }

    /// For menus and hints: `"Shift+Backspace"`.
    pub fn label(&self) -> String {
        self.caps().join("+")
    }

    /// Why this key cannot be a PhitPup shortcut, if it cannot.
    pub fn problem(&self) -> Option<&'static str> {
        if !self.ctrl && !self.alt && types_text(self.vk) {
            Some("That key types a character. Add Ctrl or Alt.")
        } else if *self == Hotkey::new(false, false, false, VK_ESCAPE)
            || *self == Hotkey::new(false, true, false, VK_ESCAPE)
        {
            Some("Esc and Shift+Esc already dismiss the suggestion.")
        } else {
            None
        }
    }
}

/// As the settings file writes it: `"shift+backspace"`.
impl fmt::Display for Hotkey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (on, name) in [
            (self.ctrl, "ctrl+"),
            (self.shift, "shift+"),
            (self.alt, "alt+"),
        ] {
            if on {
                f.write_str(name)?;
            }
        }
        f.write_str(&name_of(self.vk))
    }
}

/// Keys with a name of their own: (code, settings name, label).
const NAMED: &[(u16, &str, &str)] = &[
    (VK_BACK, "backspace", "Backspace"),
    (VK_TAB, "tab", "Tab"),
    (0x0D, "enter", "Enter"),
    (0x13, "pause", "Pause"),
    (VK_ESCAPE, "esc", "Esc"),
    (0x20, "space", "Space"),
    (0x21, "pageup", "Page Up"),
    (0x22, "pagedown", "Page Down"),
    (0x23, "end", "End"),
    (0x24, "home", "Home"),
    (0x25, "left", "Left"),
    (0x26, "up", "Up"),
    (0x27, "right", "Right"),
    (0x28, "down", "Down"),
    (0x2D, "insert", "Insert"),
    (0x2E, "delete", "Delete"),
    (0x5D, "menu", "Menu"),
    (0x91, "scrolllock", "Scroll Lock"),
    (0xBA, ";", ";"),
    (0xBB, "=", "="),
    (0xBC, ",", ","),
    (0xBD, "-", "-"),
    (0xBE, ".", "."),
    (0xBF, "/", "/"),
    (0xC0, "`", "`"),
    (0xDB, "[", "["),
    (0xDC, "backslash", "\\"),
    (0xDD, "]", "]"),
    (0xDE, "'", "'"),
];

fn vk_of(name: &str) -> Option<u16> {
    if let Some(&(vk, _, _)) = NAMED.iter().find(|(_, n, _)| *n == name) {
        return Some(vk);
    }
    let mut chars = name.chars();
    match (chars.next(), chars.next()) {
        (Some(c @ ('a'..='z' | '0'..='9')), None) => Some(c.to_ascii_uppercase() as u16),
        (Some('f'), Some(_)) => match name[1..].parse::<u16>() {
            Ok(n @ 1..=24) => Some(0x6F + n),
            _ => None,
        },
        _ => None,
    }
}

fn name_of(vk: u16) -> String {
    match vk {
        0x41..=0x5A | 0x30..=0x39 => ((vk as u8) as char).to_ascii_lowercase().to_string(),
        0x70..=0x87 => format!("f{}", vk - 0x6F),
        _ => NAMED
            .iter()
            .find(|(v, _, _)| *v == vk)
            .map_or_else(|| format!("key{vk}"), |(_, n, _)| n.to_string()),
    }
}

fn label_of(vk: u16) -> String {
    match NAMED.iter().find(|(v, _, _)| *v == vk) {
        Some((_, _, label)) => label.to_string(),
        None => name_of(vk).to_uppercase(),
    }
}

/// Whether the key types a character when pressed alone or with Shift.
fn types_text(vk: u16) -> bool {
    matches!(vk, 0x20 | 0x30..=0x39 | 0x41..=0x5A | 0xBA..=0xC0 | 0xDB..=0xDE)
}

/// Whether `vk` can be the main key of a shortcut (modifiers cannot).
pub fn is_main_key(vk: u16) -> bool {
    name_of(vk) != format!("key{vk}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_writes_the_same_text() {
        for s in [
            "shift+backspace",
            "tab",
            "ctrl+shift+space",
            "alt+f12",
            "ctrl+backslash",
            "ctrl+`",
        ] {
            let k = Hotkey::parse(s).unwrap_or_else(|| panic!("{s}"));
            assert_eq!(k.to_string(), s);
        }
        assert_eq!(Hotkey::parse("Shift + BackSpace"), Some(Hotkey::FIX));
        assert_eq!(Hotkey::FIX.label(), "Shift+Backspace");
        assert_eq!(Hotkey::parse("ctrl+k").unwrap().caps(), ["Ctrl", "K"]);
    }

    #[test]
    fn rejects_nonsense() {
        for s in ["", "shift+", "shift+shift+a", "hyper+a", "f25", "f0", "ab"] {
            assert_eq!(Hotkey::parse(s), None, "{s}");
        }
    }

    #[test]
    fn typing_keys_need_ctrl_or_alt() {
        assert!(Hotkey::parse("shift+a").unwrap().problem().is_some());
        assert!(Hotkey::parse("space").unwrap().problem().is_some());
        assert!(Hotkey::parse("shift+esc").unwrap().problem().is_some());
        assert!(Hotkey::parse("ctrl+space").unwrap().problem().is_none());
        assert!(Hotkey::FIX.problem().is_none());
        assert!(Hotkey::ACCEPT.problem().is_none());
        assert!(is_main_key(0x70) && !is_main_key(0x10));
    }
}
