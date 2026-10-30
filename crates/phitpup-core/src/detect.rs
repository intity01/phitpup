//! Decides whether a word was typed with the wrong layout.
//!
//! Manual mode does not need this: the user pressed the hotkey, so the text
//! is flipped toward the other script. Suggest and auto mode do. They compare
//! how Thai and how English the word looks as typed and as converted, and veto
//! any Thai reading that breaks Thai spelling rules.

use crate::model::{Lang, Model};
use crate::near::Near;
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

/// Abbreviations, and Thai chat words in Latin letters (krub, khob), typed
/// in English on purpose, which the English letter model cannot tell from
/// Thai typed on the English layout ("pdf" reads as ยกด). Left out: those
/// that spell a common Thai word on Kedmanee or Pattachote, such as cd (แก,
/// ลง), pc (แล), fb (ดิ), ok (นา) and kha (นี้).
pub const ABBREVIATIONS: &[&str] = &[
    "afaik", "afk", "ai", "aka", "api", "apk", "asap", "bcc", "bff", "bkk", "brb", "bruh", "bts",
    "btw", "ceo", "covid", "cpu", "css", "csv", "cto", "diy", "dm", "dns", "doc", "docx", "dvd",
    "eta", "etc", "eu", "exe", "faq", "fps", "ftp", "fyi", "gg", "ghz", "gif", "git", "gps", "gpt",
    "gpu", "haha", "hahaha", "hbu", "hdd", "hdmi", "hehe", "heic", "hr", "html", "http", "https",
    "ide", "idk", "ig", "ikr", "imho", "imo", "ios", "iot", "irl", "jpeg", "jpg", "js", "json",
    "khao", "khob", "khun", "khz", "kpi", "krab", "krap", "krub", "kwh", "lan", "lcd", "lgtm",
    "llm", "lmao", "lol", "mah", "mba", "mhz", "mkv", "mp3", "mp4", "mrt", "msi", "nas", "nft",
    "ngl", "npm", "nvm", "okr", "oled", "otp", "pdf", "pfp", "phd", "php", "pls", "plz", "png",
    "ppt", "pptx", "ps5", "qr", "ram", "rgb", "roi", "rom", "sdk", "seo", "smh", "sms", "sql",
    "ssd", "ssh", "svg", "tba", "tbh", "thx", "tmr", "tmrw", "todo", "tsv", "tv", "txt", "ty",
    "uae", "uri", "url", "usa", "usb", "ux", "vpn", "vs", "wan", "webp", "wifi", "wip", "wtf",
    "www", "wyd", "xls", "xlsx", "xml", "yaml", "yeet", "yt", "zip", "zsh",
];

/// Whether `word` is one of the [`ABBREVIATIONS`] or its plural, in
/// lowercase, capitals or Capitalised. Mixed case is not: Shift types other
/// Thai letters, so wyD is ต่ำ, not "wyd".
fn is_abbreviation(word: &str) -> bool {
    let usual = word.chars().skip(1).all(|c| !c.is_ascii_uppercase())
        || word.chars().all(|c| !c.is_ascii_lowercase());
    if !usual {
        return false;
    }
    let w = word.to_ascii_lowercase();
    let known = |w: &str| ABBREVIATIONS.binary_search(&w).is_ok();
    known(&w) || w.strip_suffix('s').is_some_and(known)
}

/// The bubble's shortest word: see [`Detector::judge_typing`].
const TYPING_MIN_CHARS: usize = 3;

/// Extra Thai cost for text that breaks Thai spelling rules.
const INVALID_THAI_PENALTY: f32 = 4.0;
/// Extra English cost for capitals mid-word ("gLL", "lkEd"), which is what
/// Thai Shift-layer letters turn into when read as English.
const MIXED_CASE_PENALTY: f32 = 2.0;

/// English cost including the capitalisation pattern. lower, Capitalised and
/// UPPER are normal; anything else is not.
fn english_cost(model: &Model, text: &str, open: bool) -> f32 {
    let letters: Vec<char> = text.chars().filter(char::is_ascii_alphabetic).collect();
    let upper = letters.iter().filter(|c| c.is_ascii_uppercase()).count();
    let normal =
        upper == 0 || upper == letters.len() || (upper == 1 && letters[0].is_ascii_uppercase());
    let en = model.lang(Lang::English);
    (if open {
        en.cost_open(text)
    } else {
        en.cost(text)
    }) + if normal { 0.0 } else { MIXED_CASE_PENALTY }
}

/// Digits with number punctuation (12.5, 10:30, 1/2) stay as typed.
fn looks_numeric(text: &str) -> bool {
    text.chars().any(|c| c.is_ascii_digit())
        && text
            .chars()
            .all(|c| c.is_ascii_digit() || ".,:-/+%".contains(c))
}

/// Thai letters in short groups ending in dots (ดร., ก.พ., ส.ส.กำ): a Thai
/// abbreviation. As English it would hold " or * (what the Thai dot's key
/// types), which words do not.
fn thai_abbreviation(text: &str) -> bool {
    let groups: Vec<&str> = text.split('.').collect();
    groups.len() > 1
        && groups
            .iter()
            .all(|g| g.chars().count() <= 3 && g.chars().all(thai::is_thai))
        && groups[..groups.len() - 1].iter().all(|g| !g.is_empty())
}

/// Endings after a dot that make a web address or a file name.
const DOT_ENDINGS: &[&str] = &[
    "app", "co", "com", "dev", "edu", "gov", "info", "io", "md", "me", "net", "org", "th",
];

/// Units and the like after a number (5km, 3pm, 4k, 1080p).
const UNITS: &[&str] = &[
    "am", "c", "cm", "fps", "g", "gb", "h", "hr", "hz", "k", "kb", "kg", "km", "m", "mb", "mg",
    "min", "ml", "mm", "ms", "p", "pm", "s", "tb", "x",
];

/// Latin text shaped like English rather than a word: e.g., Mr., U.S.
/// (letters in short groups with dots), google.com, my-file.pdf, john@x,
/// 5km, x64, v1.2.3, and faces such as -_- with hardly a letter. Thai typed
/// on the English layout can take these shapes too (ทำให้ is me.h on
/// Kedmanee), so the caller checks the Thai reading as well.
fn english_shape(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    let letters = |g: &str| !g.is_empty() && g.bytes().all(|b| b.is_ascii_alphabetic());
    let groups: Vec<&str> = lower.split('.').collect();
    let dotted = groups.len() > 1
        && letters(groups[0])
        && groups
            .iter()
            .all(|g| g.is_empty() || (letters(g) && g.len() <= 3))
        && groups
            .iter()
            .skip_while(|g| !g.is_empty())
            .all(|g| g.is_empty());
    let named = groups.len() > 1
        && groups
            .last()
            .is_some_and(|e| DOT_ENDINGS.contains(e) || ABBREVIATIONS.binary_search(e).is_ok())
        && !groups[0].is_empty();
    let hyphened = lower.split('-').count() > 1 && lower.split('-').all(letters);
    let digits = lower.trim_start_matches(|c: char| c.is_ascii_digit() || c == '.');
    let unit = digits.len() < lower.len()
        && lower.starts_with(|c: char| c.is_ascii_digit())
        && UNITS.contains(&digits);
    let numbered = lower.len() >= 2
        && lower.starts_with(|c: char| c.is_ascii_alphabetic())
        && lower[1..].bytes().all(|b| b.is_ascii_digit() || b == b'.')
        && lower[1..].starts_with(|c: char| c.is_ascii_digit());
    dotted
        || named
        || hyphened
        || unit
        || numbered
        || lower.contains('@')
        || lower.starts_with('#')
        || lower.starts_with("~/")
        || lower.contains(":\\")
        || lower.contains("://")
        || lower.starts_with("www.")
        // A face (-_-, :P, ^^): hardly a letter, and the marks faces use.
        // " and * are the Thai dot's key: ส.ว. is l";" on Kedmanee.
        || (lower.bytes().filter(u8::is_ascii_alphabetic).count() < 2
            && lower.bytes().any(|b| b":;=^_<>()".contains(&b))
            && !lower.contains(['"', '*'])
            && lower.len() >= 2)
}

/// 555 is how Thai laughs (5 reads ห้า, "ha"); "ok55" and "lol555" are
/// meant as typed, not นาถถ. No Thai word ends in ถถ.
fn laughs(text: &str) -> bool {
    text.ends_with("55")
}

/// ถถถ: Thai laughter, as 555 is.
fn thai_laughs(text: &str) -> bool {
    text.chars().count() >= 2 && text.chars().all(|c| c == 'ถ')
}

/// Capitals inside a brand or unit (TikTok, KBank, MHz): it starts with a
/// capital, has another one later, and ends in lowercase. Thai typed in the
/// wrong layout gets capitals from its Shift letters, but rarely in this
/// shape.
fn camel_case(text: &str) -> bool {
    let b = text.as_bytes();
    b.len() >= 3
        && b.iter().all(u8::is_ascii_alphabetic)
        && b[0].is_ascii_uppercase()
        && b[1..].iter().any(u8::is_ascii_uppercase)
        && b[b.len() - 1].is_ascii_lowercase()
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
    /// Kept for [`Detector::near`].
    english: Layout,
    thai: Layout,
    model: &'static Model,
    /// The Thai lexicon, sorted, to see what a word being typed could become.
    thai_words: Vec<String>,
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
            english: english.clone(),
            thai: thai.clone(),
            model,
            thai_words: {
                let mut w: Vec<String> = model
                    .lang(Lang::Thai)
                    .lexicon()
                    .map(|(w, _)| w.to_owned())
                    .collect();
                w.sort_unstable();
                w
            },
            thresholds: Thresholds::default(),
        }
    }

    /// Slips of the finger in `lang`, judged on that language's keyboard.
    pub fn near(&self, lang: Lang) -> &'static Near {
        Near::shared(
            lang,
            match lang {
                Lang::Thai => &self.thai,
                Lang::English => &self.english,
            },
        )
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
        self.judge_inner(word, thresholds, false)
    }

    /// Judges a word that is still being typed, for the bubble. Its end has
    /// not come yet: กั and ประเ are fine Thai so far, though neither could
    /// end a word. Two letters are too few to tell the languages apart (ทำ
    /// reads as "me", "gi" as เร), so the bubble waits for a third, as auto
    /// mode does.
    pub fn judge_typing(&self, word: &str, thresholds: Thresholds) -> Option<Verdict> {
        self.judge_inner(word, thresholds, true)
    }

    /// Whether some common Thai word starts with `text`.
    fn starts_thai_word(&self, text: &str) -> bool {
        let i = self.thai_words.partition_point(|w| w.as_str() < text);
        self.thai_words.get(i).is_some_and(|w| w.starts_with(text))
    }

    fn judge_inner(&self, word: &str, thresholds: Thresholds, open: bool) -> Option<Verdict> {
        let direction = self.direction(word)?;
        let converted = self.convert(word, direction);
        let th = self.model.lang(Lang::Thai);
        let th_cost = |t: &str| if open { th.cost_open(t) } else { th.cost(t) };
        let en_cost = |t: &str| english_cost(self.model, t, open);
        // A leading vowel waits for its consonant while the word is typed.
        let check = |t: &str| {
            let t = if open {
                t.trim_end_matches(thai::is_leading)
            } else {
                t
            };
            thai::check(t).is_ok()
        };

        let (margin, allowed) = match direction {
            Direction::ToThai => {
                let margin = en_cost(word) - th_cost(&converted) - thresholds.to_thai;
                // A Thai word in an English shape is still Thai (me.h = ทำให้).
                // While typing, "my-" and "google." are on their way to such
                // a shape when the part before the mark is English.
                let stem = word.trim_end_matches(['.', '-', '\'']);
                let on_its_way = open
                    && stem.len() < word.len()
                    && stem.len() >= 2
                    && !self
                        .judge_inner(stem, thresholds, open)
                        .is_some_and(|v| v.flip);
                let shaped =
                    (english_shape(word) || on_its_way) && !self.starts_thai_word(&converted);
                (
                    margin,
                    !looks_numeric(word)
                        && !shaped
                        && !is_abbreviation(word)
                        && !laughs(word)
                        && !camel_case(word)
                        && check(&converted),
                )
            }
            Direction::ToEnglish => {
                let penalty = if check(word) {
                    0.0
                } else {
                    INVALID_THAI_PENALTY
                };
                let margin = th_cost(word) + penalty - en_cost(&converted) - thresholds.to_english;
                // หรื is on its way to หรือ, whatever "sin" looks like.
                let thai_so_far = open && self.starts_thai_word(word);
                // ๆ and ฯ belong to the word before ("ดี ๆเสนอ"), and look
                // odd at either end of a run: the rest must read as English
                // too. "question" typed on Kedmanee starts with ๆ (q).
                let rest = word.trim_start_matches(['ๆ', 'ฯ']);
                let rest = rest.trim_end_matches(['ๆ', 'ฯ']);
                let marks_alone = rest.len() < word.len()
                    && !rest.is_empty()
                    && !self
                        .judge_inner(rest, thresholds, open)
                        .is_some_and(|v| v.flip);
                (
                    margin,
                    !thai_so_far
                        && !marks_alone
                        && !thai_abbreviation(word)
                        && !thai_laughs(word)
                        && converted.chars().any(|c| c.is_ascii_alphabetic()),
                )
            }
        };
        let n = word.chars().count();
        let min_chars = if open {
            thresholds.min_chars.max(TYPING_MIN_CHARS)
        } else {
            thresholds.min_chars
        };
        // "I" and "a" are words on their own, and a lone Thai letter is not:
        // ร or ฟ by itself is that word typed in the wrong layout.
        let english_letter = !open
            && direction == Direction::ToEnglish
            && n == 1
            && thresholds.min_chars <= 2
            && matches!(converted.as_str(), "i" | "I" | "a" | "A");
        Some(Verdict {
            direction,
            converted,
            margin,
            vetoed: !allowed,
            flip: allowed && (english_letter || (n >= min_chars && margin > 0.0)),
        })
    }
}
