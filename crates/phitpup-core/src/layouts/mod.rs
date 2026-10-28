//! Keyboard layouts as scancode → character tables.
//!
//! The built-in tables in `data.rs` are generated from the Windows layout DLLs
//! (`cargo xtask gen-layouts`). At runtime the Windows front end can also build
//! a [`Layout`] for any installed layout and hand it to the same converter.

mod data;

use std::collections::HashMap;

/// The 47 character keys of a US ANSI board, by scancode, in row order. The
/// built-in tables and layouts read from the OS cover exactly these keys.
pub const SCANCODES: &[u16] = &[
    0x29, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, //
    0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1A, 0x1B, 0x2B, //
    0x1E, 0x1F, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, //
    0x2C, 0x2D, 0x2E, 0x2F, 0x30, 0x31, 0x32, 0x33, 0x34, 0x35,
];

/// Drops what a layout reports for a key that is not a printable glyph:
/// space, control characters and private-use glyphs (KBDTH1 puts U+F8C7 on
/// the `\` key).
pub fn typeable(c: char) -> Option<char> {
    (c > ' ' && c != '\u{7F}' && !('\u{E000}'..='\u{F8FF}').contains(&c)).then_some(c)
}

/// One physical key: its scancode and what it types with and without Shift.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    pub sc: u16,
    pub base: Option<char>,
    pub shift: Option<char>,
}

#[derive(Clone, Debug)]
pub struct Layout {
    name: String,
    keys: Vec<Key>,
}

impl Layout {
    pub fn new(name: impl Into<String>, keys: Vec<Key>) -> Self {
        Self {
            name: name.into(),
            keys,
        }
    }

    pub fn us_qwerty() -> Self {
        Self::new("US QWERTY", data::US_QWERTY.to_vec())
    }

    pub fn kedmanee() -> Self {
        Self::new("Thai Kedmanee", data::KEDMANEE.to_vec())
    }

    pub fn pattachote() -> Self {
        Self::new("Thai Pattachote", data::PATTACHOTE.to_vec())
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn keys(&self) -> &[Key] {
        &self.keys
    }

    /// Every character the layout types, as `(scancode, shift, char)`.
    pub fn chars(&self) -> impl Iterator<Item = (u16, bool, char)> + '_ {
        self.keys.iter().flat_map(|k| {
            [(false, k.base), (true, k.shift)]
                .into_iter()
                .filter_map(move |(shift, c)| c.map(|c| (k.sc, shift, c)))
        })
    }

    /// Characters that more than one key types (Pattachote: `_` and ั).
    /// Converting those out of this layout is ambiguous; [`crate::Converter`]
    /// resolves it in favour of the unshifted key.
    pub fn duplicates(&self) -> Vec<char> {
        let mut seen: HashMap<char, usize> = HashMap::new();
        for (_, _, c) in self.chars() {
            *seen.entry(c).or_default() += 1;
        }
        let mut dups: Vec<char> = seen
            .into_iter()
            .filter(|&(_, n)| n > 1)
            .map(|(c, _)| c)
            .collect();
        dups.sort_unstable();
        dups
    }
}
