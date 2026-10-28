//! Keyboard layouts as scancode → character tables.
//!
//! The built-in tables in `data.rs` are generated from the Windows layout DLLs
//! (`cargo xtask gen-layouts`). At runtime the Windows front end can also build
//! a [`Layout`] for any installed layout and hand it to the same converter.

mod data;

use std::collections::HashMap;

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
