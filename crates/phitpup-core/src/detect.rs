//! Decides whether a word was typed with the wrong layout.
//!
//! Manual mode does not need this: the user pressed the hotkey, so the text
//! is flipped toward the other script. Suggest and auto mode do. They compare
//! how Thai and how English the word looks as typed and as converted, and veto
//! any Thai reading that breaks Thai spelling rules.

use crate::model::{Lang, Model};
use crate::{Converter, Layout, thai};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// Latin text that was meant to be Thai.
    ToThai,
    /// Thai text that was meant to be English.
    ToEnglish,
}

/// How sure the detector must be before it suggests or applies a fix.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Thresholds {
    /// Bits per char the Thai reading must beat the English one by.
    pub to_thai: f32,
    /// Bits per char the English reading must beat the Thai one by.
    pub to_english: f32,
    /// Shorter words are never flipped automatically.
    pub min_chars: usize,
}

// Numbers from `cargo xtask eval-detector` on held-out words, Kedmanee
// (Thai recall weighted by word frequency):
//   suggest: Thai recall 99.8% / 0.35% English words misread;
//            English recall 99.2% / 0.04% Thai (by frequency) misread.
//   auto:    Thai recall 95.7% / 0.12%; English recall 97.6% / 0.0005%.
impl Thresholds {
    /// For the suggestion bubble: a wrong guess costs one ignored popup.
    pub const SUGGEST: Thresholds = Thresholds {
        to_thai: 0.5,
        to_english: 0.5,
        min_chars: 2,
    };
    /// For auto-fix on space: a wrong guess rewrites the user's text.
    pub const AUTO: Thresholds = Thresholds {
        to_thai: 1.5,
        to_english: 1.5,
        min_chars: 3,
    };
}

impl Default for Thresholds {
    fn default() -> Self {
        Self::SUGGEST
    }
}

/// Extra Thai cost for text that breaks Thai spelling rules.
const INVALID_THAI_PENALTY: f32 = 4.0;
/// Extra English cost for capitals mid-word ("gLL", "lkEd"), which is what
/// Thai Shift-layer letters turn into when read as English.
const MIXED_CASE_PENALTY: f32 = 2.0;

/// English cost including the capitalisation pattern. lower, Capitalised and
/// UPPER are normal; anything else is not.
fn english_cost(model: &Model, text: &str) -> f32 {
    let letters: Vec<char> = text.chars().filter(char::is_ascii_alphabetic).collect();
    let upper = letters.iter().filter(|c| c.is_ascii_uppercase()).count();
    let normal =
        upper == 0 || upper == letters.len() || (upper == 1 && letters[0].is_ascii_uppercase());
    model.lang(Lang::English).cost(text) + if normal { 0.0 } else { MIXED_CASE_PENALTY }
}

/// Digits with number punctuation (12.5, 10:30, 1/2) stay as typed.
fn looks_numeric(text: &str) -> bool {
    text.chars().any(|c| c.is_ascii_digit())
        && text
            .chars()
            .all(|c| c.is_ascii_digit() || ".,:-/+%".contains(c))
}

#[derive(Clone, Debug, PartialEq)]
pub struct Verdict {
    pub direction: Direction,
    pub converted: String,
    /// Bits per char by which the converted reading wins, after the threshold.
    /// Positive means flip.
    pub margin: f32,
    /// A hard rule rules the flip out: the Thai reading breaks Thai spelling,
    /// or there is nothing but digits and punctuation.
    pub vetoed: bool,
    pub flip: bool,
}

pub struct Detector {
    to_th: Converter,
    to_en: Converter,
    model: &'static Model,
    pub thresholds: Thresholds,
}

impl Detector {
    pub fn new(english: &Layout, thai: &Layout) -> Self {
        Self::with_model(english, thai, Model::embedded())
    }

    pub fn with_model(english: &Layout, thai: &Layout, model: &'static Model) -> Self {
        Self {
            to_th: Converter::new(english, thai),
            to_en: Converter::new(thai, english),
            model,
            thresholds: Thresholds::default(),
        }
    }

    /// Thai text goes to English; Latin text the Thai layout could have typed
    /// goes to Thai.
    pub fn direction(&self, text: &str) -> Option<Direction> {
        if text.chars().any(thai::is_thai) {
            Some(Direction::ToEnglish)
        } else if text.chars().any(|c| self.to_th.maps(c)) {
            Some(Direction::ToThai)
        } else {
            None
        }
    }

    pub fn convert(&self, text: &str, direction: Direction) -> String {
        match direction {
            Direction::ToThai => self.to_th.convert(text),
            Direction::ToEnglish => self.to_en.convert(text),
        }
    }

    /// What the hotkey does: flip toward the other script, no questions asked.
    pub fn manual(&self, text: &str) -> Option<(Direction, String)> {
        let d = self.direction(text)?;
        Some((d, self.convert(text, d)))
    }

    /// What the hotkey does to a selection. A selection often mixes words
    /// that were typed right with ones that were not ("hello l;ylfu"), so
    /// only the words the detector is sure about change, at the strict
    /// [`Thresholds::AUTO`] setting: a short word like "ok" (= นา) stays as
    /// it is. If no word qualifies, the user still asked, so the whole
    /// selection flips like
    /// [`manual`](Self::manual). The direction returned is the last changed
    /// word's: the layout to keep typing in.
    pub fn selection(&self, text: &str) -> Option<(Direction, String)> {
        let mut out = String::with_capacity(text.len());
        let mut last = None;
        let mut word = String::new();
        let mut flush = |word: &mut String, out: &mut String| {
            match self.judge_with(word, Thresholds::AUTO).filter(|v| v.flip) {
                Some(v) => {
                    last = Some(v.direction);
                    out.push_str(&v.converted);
                }
                None => out.push_str(word),
            }
            word.clear();
        };
        for c in text.chars() {
            if c.is_whitespace() {
                flush(&mut word, &mut out);
                out.push(c);
            } else {
                word.push(c);
            }
        }
        flush(&mut word, &mut out);
        match last {
            Some(d) => Some((d, out)),
            None => self.manual(text),
        }
    }

    /// Judges one word (no spaces) for suggest and auto mode.
    pub fn judge(&self, word: &str) -> Option<Verdict> {
        self.judge_with(word, self.thresholds)
    }

    /// Judges one word at `thresholds` instead of the detector's own.
    pub fn judge_with(&self, word: &str, thresholds: Thresholds) -> Option<Verdict> {
        let direction = self.direction(word)?;
        let converted = self.convert(word, direction);
        let th = self.model.lang(Lang::Thai);

        let (margin, allowed) = match direction {
            Direction::ToThai => {
                let margin =
                    english_cost(self.model, word) - th.cost(&converted) - thresholds.to_thai;
                (
                    margin,
                    !looks_numeric(word) && thai::check(&converted).is_ok(),
                )
            }
            Direction::ToEnglish => {
                let penalty = if thai::check(word).is_err() {
                    INVALID_THAI_PENALTY
                } else {
                    0.0
                };
                let margin = th.cost(word) + penalty
                    - english_cost(self.model, &converted)
                    - thresholds.to_english;
                (margin, converted.chars().any(|c| c.is_ascii_alphabetic()))
            }
        };
        let long_enough = word.chars().count() >= thresholds.min_chars;
        Some(Verdict {
            direction,
            converted,
            margin,
            vetoed: !allowed,
            flip: allowed && long_enough && margin > 0.0,
        })
    }
}
