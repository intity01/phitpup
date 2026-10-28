//! Position-based conversion between two layouts.

use std::collections::HashMap;

use crate::Layout;

/// Rewrites text typed in `from` as if the same keys had been pressed in `to`.
///
/// Characters `from` cannot type (space, newlines, emoji, …) pass through
/// unchanged.
#[derive(Clone, Debug)]
pub struct Converter {
    map: HashMap<char, char>,
}

impl Converter {
    pub fn new(from: &Layout, to: &Layout) -> Self {
        let target: HashMap<(u16, bool), char> =
            to.chars().map(|(sc, shift, c)| ((sc, shift), c)).collect();
        // When `from` types a char on two keys (Pattachote has `_` and ั twice),
        // the unshifted key wins: insert all base chars before any shifted ones.
        let mut source: Vec<_> = from.chars().collect();
        source.sort_by_key(|&(_, shift, _)| shift);
        let mut map = HashMap::new();
        for (sc, shift, c) in source {
            if let Some(&t) = target.get(&(sc, shift)) {
                map.entry(c).or_insert(t);
            }
        }
        Self { map }
    }

    /// Whether `c` comes from a key this converter knows.
    pub fn maps(&self, c: char) -> bool {
        self.map.contains_key(&c)
    }

    pub fn convert_char(&self, c: char) -> char {
        self.map.get(&c).copied().unwrap_or(c)
    }

    pub fn convert(&self, text: &str) -> String {
        text.chars().map(|c| self.convert_char(c)).collect()
    }
}
