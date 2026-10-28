//! What the hotkey does to recently typed text.
//!
//! The front end reports every character the user types, plus backspaces and
//! anything that moves the caret (clicks, arrows, focus changes). [`Fixer`]
//! mirrors the text just before the caret, so on the hotkey it can say how many
//! characters to erase and what to type instead, without reading the app.
//!
//! Pressing the hotkey again right away reaches one more word back (up to
//! [`MAX_WORDS`]). Once there is no earlier word, the next press restores the
//! original text, which doubles as undo.

use crate::model::Lang;
use crate::{Detector, Direction};

/// Characters remembered before the caret. Older ones are forgotten.
pub const MAX_CHARS: usize = 300;
/// Words one run of repeated presses can reach.
pub const MAX_WORDS: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    /// Characters (code points) to erase with Backspace.
    pub delete: usize,
    pub insert: String,
    /// Keyboard layout to switch to afterwards, so typing continues correctly.
    pub switch_to: Lang,
}

#[derive(Debug)]
struct Session {
    /// The buffer before the first press of this run.
    original: Vec<char>,
    direction: Direction,
    words: usize,
    /// Where the replaced part starts, in both `original` and the buffer.
    start: usize,
}

#[derive(Debug, Default)]
pub struct Fixer {
    buf: Vec<char>,
    session: Option<Session>,
}

impl Fixer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn typed(&mut self, text: &str) {
        self.session = None;
        self.buf.extend(text.chars());
        if self.buf.len() > MAX_CHARS {
            self.buf.drain(..self.buf.len() - MAX_CHARS);
        }
    }

    pub fn backspace(&mut self) {
        self.session = None;
        self.buf.pop();
    }

    /// The caret moved somewhere we cannot follow.
    pub fn reset(&mut self) {
        self.buf.clear();
        self.session = None;
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    pub fn text(&self) -> String {
        self.buf.iter().collect()
    }

    /// The edit for one hotkey press, or `None` if there is no word to fix.
    pub fn hotkey(&mut self, detector: &Detector) -> Option<Edit> {
        let Some(s) = self.session.take() else {
            let start = word_start(&self.buf, 1)?;
            let tail: String = self.buf[start..].iter().collect();
            let direction = detector.direction(&tail)?;
            let insert = detector.convert(&tail, direction);
            let edit = Edit {
                delete: self.buf.len() - start,
                insert,
                switch_to: target(direction),
            };
            let original = std::mem::take(&mut self.buf);
            self.buf = original[..start]
                .iter()
                .copied()
                .chain(edit.insert.chars())
                .collect();
            self.session = Some(Session {
                original,
                direction,
                words: 1,
                start,
            });
            return Some(edit);
        };

        let words = s.words + 1;
        match word_start(&s.original, words).filter(|_| words <= MAX_WORDS) {
            Some(start) => {
                let tail: String = s.original[start..].iter().collect();
                let insert = detector.convert(&tail, s.direction);
                let edit = Edit {
                    delete: self.buf.len() - start,
                    insert,
                    switch_to: target(s.direction),
                };
                self.buf = s.original[..start]
                    .iter()
                    .copied()
                    .chain(edit.insert.chars())
                    .collect();
                self.session = Some(Session { words, start, ..s });
                Some(edit)
            }
            // Nothing further back: put the original text back.
            None => {
                let edit = Edit {
                    delete: self.buf.len() - s.start,
                    insert: s.original[s.start..].iter().collect(),
                    switch_to: match s.direction {
                        Direction::ToThai => Lang::English,
                        Direction::ToEnglish => Lang::Thai,
                    },
                };
                self.buf = s.original;
                Some(edit)
            }
        }
    }
}

fn target(direction: Direction) -> Lang {
    match direction {
        Direction::ToThai => Lang::Thai,
        Direction::ToEnglish => Lang::English,
    }
}

/// Start of the `n`-th word from the end. Whitespace after the last word
/// belongs to it, so "l;ylfu " fixes to "สวัสดี ".
fn word_start(chars: &[char], n: usize) -> Option<usize> {
    let mut i = chars.len();
    for _ in 0..n {
        while i > 0 && chars[i - 1].is_whitespace() {
            i -= 1;
        }
        if i == 0 {
            return None;
        }
        while i > 0 && !chars[i - 1].is_whitespace() {
            i -= 1;
        }
    }
    Some(i)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Layout;

    fn det() -> Detector {
        Detector::new(&Layout::us_qwerty(), &Layout::kedmanee())
    }

    /// Applies an edit to `screen` the way the front end would.
    fn apply(screen: &mut String, e: &Edit) {
        for _ in 0..e.delete {
            screen.pop();
        }
        screen.push_str(&e.insert);
    }

    #[test]
    fn fixes_last_word_and_keeps_trailing_space() {
        let d = det();
        let mut f = Fixer::new();
        f.typed("hello l;ylfu ");
        let e = f.hotkey(&d).unwrap();
        assert_eq!(
            e,
            Edit {
                delete: 7,
                insert: "สวัสดี ".into(),
                switch_to: Lang::Thai
            }
        );
        assert_eq!(f.text(), "hello สวัสดี ");
    }

    #[test]
    fn repeat_reaches_back_then_undoes() {
        let d = det();
        let mut f = Fixer::new();
        let mut screen = String::from("ok l;ylfu 8iy[");
        f.typed(&screen);

        let e = f.hotkey(&d).unwrap();
        apply(&mut screen, &e);
        assert_eq!(screen, "ok l;ylfu ครับ");

        let e = f.hotkey(&d).unwrap();
        apply(&mut screen, &e);
        assert_eq!(screen, "ok สวัสดี ครับ");

        let e = f.hotkey(&d).unwrap();
        apply(&mut screen, &e);
        assert_eq!(screen, "นา สวัสดี ครับ");

        // No fourth word: undo everything.
        let e = f.hotkey(&d).unwrap();
        apply(&mut screen, &e);
        assert_eq!(screen, "ok l;ylfu 8iy[");
        assert_eq!(e.switch_to, Lang::English);
        assert_eq!(f.text(), screen);

        // And the next press starts over.
        let e = f.hotkey(&d).unwrap();
        apply(&mut screen, &e);
        assert_eq!(screen, "ok l;ylfu ครับ");
    }

    #[test]
    fn extending_keeps_the_first_direction() {
        let d = det();
        let mut f = Fixer::new();
        let mut screen = String::from("ครับ l;ylfu");
        f.typed(&screen);
        apply(&mut screen, &f.hotkey(&d).unwrap());
        apply(&mut screen, &f.hotkey(&d).unwrap());
        // ครับ is already Thai; converting toward Thai leaves it alone.
        assert_eq!(screen, "ครับ สวัสดี");
    }

    #[test]
    fn thai_to_english() {
        let d = det();
        let mut f = Fixer::new();
        f.typed("้ำสสน");
        let e = f.hotkey(&d).unwrap();
        assert_eq!(e.insert, "hello");
        assert_eq!(e.switch_to, Lang::English);
    }

    #[test]
    fn typing_or_backspace_ends_the_run() {
        let d = det();
        let mut f = Fixer::new();
        f.typed("l;ylfu");
        f.hotkey(&d).unwrap();
        f.typed(" 8iy[");
        let e = f.hotkey(&d).unwrap();
        assert_eq!(e.insert, "ครับ");
        assert_eq!(f.text(), "สวัสดี ครับ");
        f.backspace();
        assert_eq!(f.text(), "สวัสดี ครั"); // one code point, like Backspace
    }

    #[test]
    fn nothing_to_fix() {
        let d = det();
        let mut f = Fixer::new();
        assert_eq!(f.hotkey(&d), None);
        f.typed("   ");
        assert_eq!(f.hotkey(&d), None);
        f.reset();
        assert!(f.is_empty());
    }

    #[test]
    fn buffer_is_capped() {
        let mut f = Fixer::new();
        f.typed(&"a".repeat(MAX_CHARS + 50));
        assert_eq!(f.text().len(), MAX_CHARS);
    }
}
