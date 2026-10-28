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
//!
//! In suggest mode the front end asks for a [`Suggestion`] after every key
//! and shows it beside the caret; Tab hands it back to [`Fixer::accept`].
//! In auto mode a space after a word goes to [`Fixer::auto`] instead of
//! [`Fixer::typed`]. Either way, Shift+Backspace right after puts the word
//! back as it was typed.

use crate::model::Lang;
use crate::{Detector, Direction, Thresholds};

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

/// A fix offered for the word being typed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suggestion {
    /// The word as typed, so a stale suggestion is never applied.
    pub word: String,
    pub edit: Edit,
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

/// What an accepted suggestion replaced, while it can still be undone.
#[derive(Debug)]
struct Undo {
    original: Vec<char>,
    start: usize,
    back_to: Lang,
}

#[derive(Debug, Default)]
pub struct Fixer {
    buf: Vec<char>,
    session: Option<Session>,
    undo: Option<Undo>,
}

impl Fixer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn typed(&mut self, text: &str) {
        self.session = None;
        self.undo = None;
        self.buf.extend(text.chars());
        if self.buf.len() > MAX_CHARS {
            self.buf.drain(..self.buf.len() - MAX_CHARS);
        }
    }

    pub fn backspace(&mut self) {
        self.session = None;
        self.undo = None;
        self.buf.pop();
    }

    /// The caret moved somewhere we cannot follow.
    pub fn reset(&mut self) {
        self.buf.clear();
        self.session = None;
        self.undo = None;
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    pub fn text(&self) -> String {
        self.buf.iter().collect()
    }

    /// The word right before the caret, if the detector (at its own
    /// thresholds) says it was typed with the wrong layout. Nothing while the
    /// caret sits after whitespace: the word is finished.
    pub fn suggestion(&self, detector: &Detector) -> Option<Suggestion> {
        if self.buf.last().is_none_or(|c| c.is_whitespace()) {
            return None;
        }
        let start = word_start(&self.buf, 1)?;
        let word: String = self.buf[start..].iter().collect();
        let v = detector.judge(&word).filter(|v| v.flip)?;
        Some(Suggestion {
            edit: Edit {
                delete: self.buf.len() - start,
                insert: v.converted,
                switch_to: target(v.direction),
            },
            word,
        })
    }

    /// Applies an accepted suggestion. Returns `None` if the text before the
    /// caret no longer ends with that word.
    pub fn accept(&mut self, s: &Suggestion) -> Option<Edit> {
        let word: Vec<char> = s.word.chars().collect();
        let start = self.buf.len().checked_sub(word.len())?;
        let fresh =
            self.buf[start..] == word[..] && (start == 0 || self.buf[start - 1].is_whitespace());
        if !fresh {
            return None;
        }
        self.session = None;
        self.undo = Some(Undo {
            original: self.buf.clone(),
            start,
            back_to: match s.edit.switch_to {
                Lang::Thai => Lang::English,
                Lang::English => Lang::Thai,
            },
        });
        self.buf.truncate(start);
        self.buf.extend(s.edit.insert.chars());
        Some(s.edit.clone())
    }

    /// Auto mode: `sep` (a space) was just typed after a word. If the
    /// detector is sure at [`Thresholds::AUTO`] that the word was typed in
    /// the wrong layout, and `skip` does not exempt it, returns the edit that
    /// replaces the word and types `sep` after it; the caller then must not
    /// let the key through. Otherwise records `sep` like [`typed`](Self::typed)
    /// and returns `None`.
    pub fn auto(
        &mut self,
        detector: &Detector,
        sep: &str,
        skip: impl Fn(&str) -> bool,
    ) -> Option<Edit> {
        let fix = (|| {
            if self.buf.last().is_none_or(|c| c.is_whitespace()) {
                return None;
            }
            let start = word_start(&self.buf, 1)?;
            let word: String = self.buf[start..].iter().collect();
            if skip(&word) {
                return None;
            }
            let v = detector
                .judge_with(&word, Thresholds::AUTO)
                .filter(|v| v.flip)?;
            Some((start, v))
        })();
        let Some((start, v)) = fix else {
            self.typed(sep);
            return None;
        };
        let switch_to = target(v.direction);
        let edit = Edit {
            delete: self.buf.len() - start,
            insert: v.converted + sep,
            switch_to,
        };
        let mut original = std::mem::take(&mut self.buf);
        original.extend(sep.chars());
        self.buf = original[..start]
            .iter()
            .copied()
            .chain(edit.insert.chars())
            .collect();
        self.session = None;
        self.undo = Some(Undo {
            original,
            start,
            back_to: match switch_to {
                Lang::Thai => Lang::English,
                Lang::English => Lang::Thai,
            },
        });
        Some(edit)
    }

    /// The edit for one hotkey press, or `None` if there is no word to fix.
    pub fn hotkey(&mut self, detector: &Detector) -> Option<Edit> {
        if let Some(u) = self.undo.take() {
            let edit = Edit {
                delete: self.buf.len() - u.start,
                insert: u.original[u.start..].iter().collect(),
                switch_to: u.back_to,
            };
            self.buf = u.original;
            return Some(edit);
        }
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
    fn auto_fixes_on_space_and_hotkey_undoes() {
        let d = det();
        let mut f = Fixer::new();
        let mut screen = String::from("hello l;ylfu");
        f.typed(&screen);
        let e = f.auto(&d, " ", |_| false).unwrap();
        assert_eq!(e.delete, 6);
        assert_eq!(e.switch_to, Lang::Thai);
        apply(&mut screen, &e);
        assert_eq!(screen, "hello สวัสดี ");
        assert_eq!(f.text(), screen);

        let e = f.hotkey(&d).unwrap();
        apply(&mut screen, &e);
        assert_eq!(screen, "hello l;ylfu ");
        assert_eq!(e.switch_to, Lang::English);
        assert_eq!(f.text(), screen);
    }

    #[test]
    fn auto_leaves_right_short_and_skipped_words() {
        let d = det();
        for (typed, skip) in [
            ("hello", ""),
            ("ok", ""),
            ("l;ylfu", "l;ylfu"),
            ("สวัสดี", ""),
        ] {
            let mut f = Fixer::new();
            f.typed(typed);
            assert_eq!(f.auto(&d, " ", |w| w == skip), None, "{typed}");
            assert_eq!(f.text(), format!("{typed} "), "the space is recorded");
        }
        let mut f = Fixer::new();
        f.typed("l;ylfu ");
        assert_eq!(f.auto(&d, " ", |_| false), None, "after a space: no word");
    }

    #[test]
    fn typing_after_auto_ends_the_undo() {
        let d = det();
        let mut f = Fixer::new();
        f.typed("l;ylfu");
        f.auto(&d, " ", |_| false).unwrap();
        f.typed("ok");
        // The hotkey now fixes the new word instead of undoing.
        assert_eq!(f.hotkey(&d).unwrap().insert, "นา");
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
    fn suggests_while_typing_and_accepts() {
        let d = det();
        let mut f = Fixer::new();
        f.typed("hello l");
        assert_eq!(f.suggestion(&d), None, "too early to tell");
        f.typed(";ylfu");
        let s = f.suggestion(&d).unwrap();
        assert_eq!(s.word, "l;ylfu");
        assert_eq!(
            s.edit,
            Edit {
                delete: 6,
                insert: "สวัสดี".into(),
                switch_to: Lang::Thai
            }
        );
        assert_eq!(f.accept(&s), Some(s.edit.clone()));
        assert_eq!(f.text(), "hello สวัสดี");
    }

    #[test]
    fn no_suggestion_for_right_words_or_after_space() {
        let d = det();
        let mut f = Fixer::new();
        f.typed("hello");
        assert_eq!(f.suggestion(&d), None);
        f.reset();
        f.typed("l;ylfu ");
        assert_eq!(f.suggestion(&d), None);
    }

    #[test]
    fn stale_suggestion_is_refused() {
        let d = det();
        let mut f = Fixer::new();
        f.typed("l;ylfu");
        let s = f.suggestion(&d).unwrap();
        f.typed("x");
        assert_eq!(f.accept(&s), None);
        f.backspace();
        f.reset();
        f.typed("al;ylfu");
        assert_eq!(f.accept(&s), None, "word must start at a boundary");
    }

    #[test]
    fn hotkey_right_after_accept_undoes_it() {
        let d = det();
        let mut f = Fixer::new();
        let mut screen = String::from("hello l;ylfu");
        f.typed(&screen);
        let s = f.suggestion(&d).unwrap();
        apply(&mut screen, &f.accept(&s).unwrap());
        assert_eq!(screen, "hello สวัสดี");

        let e = f.hotkey(&d).unwrap();
        apply(&mut screen, &e);
        assert_eq!(screen, "hello l;ylfu");
        assert_eq!(e.switch_to, Lang::English);
        assert_eq!(f.text(), screen);

        // Typing after Tab keeps the fix: the hotkey works as usual again.
        f.typed(" ");
        let e = f.hotkey(&d).unwrap();
        assert_eq!(e.insert, "สวัสดี ");
    }

    #[test]
    fn buffer_is_capped() {
        let mut f = Fixer::new();
        f.typed(&"a".repeat(MAX_CHARS + 50));
        assert_eq!(f.text().len(), MAX_CHARS);
    }
}
