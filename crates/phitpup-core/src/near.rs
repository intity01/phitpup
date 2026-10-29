//! Words typed with a slip of the finger: a key next to the right one, two
//! keys in the wrong order, one too many or one missing (ส้วสดี for สวัสดี,
//! teh for the). The fix offered is the common word ([`crate::model`]'s
//! lexicon) that is closest on the keyboard in use.
//!
//! Text that is still being typed is left alone: the end of the run may be
//! the start of a word, and Thai runs several words together, so only the
//! part after the last place the run splits into known words is looked at.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use crate::layouts::Layout;
use crate::model::{Lang, Model};

/// Edit costs, in tenths of an edit.
const FULL: u32 = 10;
const NEIGHBOUR: u32 = 6;
const SAME_KEY: u32 = 4;
const SWAP: u32 = 6;
/// A slip happening at all, and each tenth of an edit, in tenths of a bit.
const SLIP: u32 = 40;
const PER_TENTH: u32 = 8;
/// How much likelier a fix must be than the text as typed, in tenths of a
/// bit.
const MARGIN: u32 = 20;
/// The longest word in the lexicon is shorter than this, in characters.
const MAX_WORD: usize = 24;

/// Letters as small numbers: Thai U+0E01..=U+0E4E as 0..78; English a-z as
/// 0..26 and the apostrophe as 26.
const LETTERS: usize = 78;

fn index(lang: Lang, c: char) -> Option<u8> {
    match lang {
        Lang::Thai => (c as u32)
            .checked_sub(0x0E01)
            .filter(|&i| (i as usize) < LETTERS)
            .map(|i| i as u8),
        Lang::English => match c {
            'a'..='z' => Some(c as u8 - b'a'),
            '\'' => Some(26),
            _ => None,
        },
    }
}

/// One known word, ready for a quick comparison.
struct Entry {
    chars: Vec<u8>,
    /// Which characters it uses: two words a slip or two apart differ in at
    /// most four of these.
    mask: u128,
    cost: u16,
    word: String,
}

/// A [`Near`] made for a language and a layout (by name).
type Built = (Lang, String, &'static Near);

pub struct Near {
    lang: Lang,
    /// Word → cost (−log2 frequency, 1/10 bit).
    words: HashMap<String, u16>,
    list: Vec<Entry>,
    /// Sorted, to ask whether a word starts with what was typed.
    sorted: Vec<String>,
    /// Cost of typing the first character where the second was meant.
    sub: Vec<[u8; LETTERS]>,
}

impl Near {
    /// The embedded model's words in `lang`, with the keys of `layout`;
    /// built the first time each pair is asked for.
    pub fn shared(lang: Lang, layout: &Layout) -> &'static Near {
        static BUILT: OnceLock<Mutex<Vec<Built>>> = OnceLock::new();
        let mut built = BUILT.get_or_init(Default::default).lock().unwrap();
        if let Some(&(_, _, near)) = built
            .iter()
            .find(|(l, name, _)| *l == lang && name == layout.name())
        {
            return near;
        }
        let near: &'static Near = Box::leak(Box::new(Near::new(
            lang,
            Model::embedded().lang(lang).lexicon(),
            layout,
        )));
        built.push((lang, layout.name().to_owned(), near));
        near
    }

    pub fn new<'a>(
        lang: Lang,
        words: impl Iterator<Item = (&'a str, u16)>,
        layout: &Layout,
    ) -> Self {
        let words: HashMap<String, u16> = words.map(|(w, c)| (w.to_owned(), c)).collect();
        let list = words
            .iter()
            .filter_map(|(w, &cost)| {
                let chars: Vec<u8> = w.chars().map(|c| index(lang, c)).collect::<Option<_>>()?;
                (chars.len() >= 3 && chars.len() <= MAX_WORD).then(|| Entry {
                    mask: mask(&chars),
                    chars,
                    cost,
                    word: w.clone(),
                })
            })
            .collect();
        let mut sorted: Vec<String> = words.keys().cloned().collect();
        sorted.sort_unstable();
        let mut keys = [None; LETTERS];
        for (sc, _, c) in layout.chars() {
            // English words are kept in lowercase; A sits where a does.
            let c = c.to_ascii_lowercase();
            if let (Some(i), Some((row, x))) = (index(lang, c), position(sc)) {
                keys[i as usize] = Some((row, x, sc));
            }
        }
        let sub = (0..LETTERS)
            .map(|a| {
                let mut row = [FULL as u8; LETTERS];
                for (b, cost) in row.iter_mut().enumerate() {
                    *cost = match (keys[a], keys[b]) {
                        _ if a == b => 0,
                        (Some((r1, x1, k1)), Some((r2, x2, k2))) => {
                            if k1 == k2 {
                                SAME_KEY as u8
                            } else if (r1 - r2).abs() <= 1 && (x1 - x2).abs() <= 1.0 {
                                NEIGHBOUR as u8
                            } else {
                                FULL as u8
                            }
                        }
                        _ => FULL as u8,
                    };
                }
                row
            })
            .collect();
        Self {
            lang,
            words,
            list,
            sorted,
            sub,
        }
    }

    /// What to replace the end of `run` with: how many characters from the
    /// end, and the word. `None` when the run reads at least as well as known
    /// words as typed, the last one possibly still being typed.
    ///
    /// Readings are compared by cost, in tenths of a bit: the words' costs
    /// (how rare they are) plus, for a fix, the cost of the slips.
    pub fn fix(&self, run: &str) -> Option<(usize, String)> {
        let chars: Vec<char> = run.chars().collect();
        // Capitals are names, acronyms and the like, not slips; English is
        // compared in lowercase, so this also keeps the result's case.
        if chars.len() < 3 || !chars.iter().all(|&c| index(self.lang, c).is_some()) {
            return None;
        }
        // Chat elongation (ค่ะะ, sooo) is meant.
        if chars[chars.len() - 1] == chars[chars.len() - 2] {
            return None;
        }
        let seg = self.segments(&chars);
        // As typed: known words, then the start of one.
        let as_typed = (0..=chars.len())
            .filter_map(|i| Some(seg[i]? + self.cheapest_start(&chars[i..])?))
            .min();
        // Fixed: known words, then a near word in place of the rest.
        let (cost, n, word) = (0..chars.len())
            .filter_map(|i| {
                let tail = &chars[i..];
                let (cost, word) = self.closest(tail)?;
                Some((seg[i]? + cost, tail.len(), word))
            })
            .min()?;
        as_typed
            .is_none_or(|t| cost + MARGIN < t)
            .then_some((n, word))
    }

    /// `seg[i]`: the cheapest split of the first `i` characters into known
    /// words.
    fn segments(&self, chars: &[char]) -> Vec<Option<u32>> {
        let mut seg = vec![None; chars.len() + 1];
        seg[0] = Some(0);
        for i in 0..chars.len() {
            let Some(base) = seg[i] else { continue };
            let mut s = String::new();
            for j in i..chars.len().min(i + MAX_WORD) {
                s.push(chars[j]);
                if let Some(&c) = self.words.get(&s) {
                    let total = base + u32::from(c);
                    if seg[j + 1].is_none_or(|t| total < t) {
                        seg[j + 1] = Some(total);
                    }
                }
            }
        }
        seg
    }

    /// The cost of the most common known word that starts with `chars`; 0
    /// for nothing.
    fn cheapest_start(&self, chars: &[char]) -> Option<u32> {
        if chars.is_empty() {
            return Some(0);
        }
        let s: String = chars.iter().collect();
        let i = self.sorted.partition_point(|w| w.as_str() < s.as_str());
        self.sorted[i..]
            .iter()
            .take_while(|w| w.starts_with(&s))
            .map(|w| u32::from(self.words[w]))
            .min()
    }

    /// The known word that best explains `typed` as slips of the finger, and
    /// that reading's cost, if one stands out: one slip for words under
    /// five characters, two from five on. Nothing for a known word.
    fn closest(&self, typed: &[char]) -> Option<(u32, String)> {
        if typed.len() < 3 || typed.len() > MAX_WORD + 2 {
            return None;
        }
        let s: String = typed.iter().collect();
        if self.words.contains_key(&s) {
            return None;
        }
        let typed: Vec<u8> = typed
            .iter()
            .map(|&c| index(self.lang, c))
            .collect::<Option<_>>()?;
        let tmask = mask(&typed);
        let limit = if typed.len() >= 5 { 2 * FULL } else { FULL };
        let mut found: Vec<(u32, &str)> = self
            .list
            .iter()
            .filter(|e| {
                e.chars.len().abs_diff(typed.len()) <= 2 && (e.mask ^ tmask).count_ones() <= 4
            })
            .filter_map(|e| {
                let d = self.distance(&typed, &e.chars, limit)?;
                Some((u32::from(e.cost) + SLIP + d * PER_TENTH, e.word.as_str()))
            })
            .collect();
        found.sort_unstable();
        let &(cost, w) = found.first()?;
        // Another word about as likely: no way to tell which was meant.
        if found.get(1).is_some_and(|&(c, _)| c < cost + MARGIN) {
            return None;
        }
        Some((cost, w.to_owned()))
    }

    /// Weighted edit distance (with swaps of neighbours), or `None` past
    /// `limit`. Both words are at most `MAX_WORD + 2` characters.
    fn distance(&self, a: &[u8], b: &[u8], limit: u32) -> Option<u32> {
        const W: usize = MAX_WORD + 3;
        let (n, m) = (a.len(), b.len());
        // Two rows back and one row back.
        let mut rows = [[0u32; W]; 2];
        for (j, v) in rows[1][..=m].iter_mut().enumerate() {
            *v = j as u32 * FULL;
        }
        for i in 1..=n {
            let mut cur = [0u32; W];
            cur[0] = i as u32 * FULL;
            let mut row_min = cur[0];
            for j in 1..=m {
                let prev = &rows[1];
                let sub = u32::from(self.sub[a[i - 1] as usize][b[j - 1] as usize]);
                let mut v = (prev[j - 1] + sub)
                    .min(prev[j] + FULL)
                    .min(cur[j - 1] + FULL);
                // Two keys in the wrong order, one of them possibly the
                // key next to the right one (ส้ว for สวั).
                if i > 1 && j > 1 && a[i - 1] == b[j - 2] {
                    let near = u32::from(self.sub[a[i - 2] as usize][b[j - 1] as usize]);
                    if near < FULL {
                        v = v.min(rows[0][j - 2] + SWAP + near);
                    }
                }
                cur[j] = v;
                row_min = row_min.min(v);
            }
            rows[0] = rows[1];
            rows[1] = cur;
            if row_min > limit {
                return None;
            }
        }
        let d = rows[1][m];
        (d <= limit).then_some(d)
    }
}

fn mask(chars: &[u8]) -> u128 {
    chars.iter().fold(0, |m, &c| m | 1 << c)
}

/// Where a key sits: its row and how far along, in key widths, counting the
/// usual stagger.
fn position(sc: u16) -> Option<(i32, f32)> {
    let (row, first, offset) = match sc {
        0x02..=0x0D => (0, 0x02, 1.0),
        0x10..=0x1B => (1, 0x10, 1.5),
        0x2B => return Some((1, 1.5 + 12.0)),
        0x1E..=0x28 => (2, 0x1E, 1.75),
        0x2C..=0x35 => (3, 0x2C, 2.25),
        0x29 => return Some((0, 0.0)),
        _ => return None,
    };
    Some((row, offset + f32::from(sc - first)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thai() -> &'static Near {
        Near::shared(Lang::Thai, &Layout::kedmanee())
    }

    fn fix(run: &str) -> Option<String> {
        thai().fix(run).map(|(_, w)| w)
    }

    #[test]
    fn fixes_slips() {
        assert_eq!(fix("ส้วสดี").as_deref(), Some("สวัสดี"));
        assert_eq!(fix("สวัสดร").as_deref(), Some("สวัสดี"));
        assert_eq!(fix("โปรแกรน").as_deref(), Some("โปรแกรม"));
    }

    #[test]
    fn fixes_only_the_end_of_a_run() {
        let (n, w) = thai().fix("วันนี้ส้วสดี").unwrap();
        assert_eq!((n, w.as_str()), (6, "สวัสดี"));
    }

    #[test]
    fn leaves_words_alone() {
        for run in [
            "สวัสดี",
            "สวัสดีครับ",
            "ประเทศไทย",
            "คอมพิว", // still typing
            "คอมพิวเตอ",
            "ค่ะะ",
            "ส้ว", // ส้วม is on its way
            "กข",
            "hello",
        ] {
            assert_eq!(fix(run), None, "{run}");
        }
    }

    #[test]
    fn neighbours_follow_the_layout() {
        // ั and ้ are next to each other on Kedmanee but not on Pattachote,
        // so a Pattachote typist's ส้วสดี is a bigger slip.
        let ked = Near::shared(Lang::Thai, &Layout::kedmanee());
        let pat = Near::shared(Lang::Thai, &Layout::pattachote());
        let [a, b] = ['\u{0E31}', '\u{0E49}'].map(|c| index(Lang::Thai, c).unwrap() as usize);
        assert!(ked.sub[a][b] < pat.sub[a][b]);
    }
}

#[cfg(test)]
mod quality {
    /// How often fixes fire on real words and are right on slips:
    /// `cargo test -p phitpup-core --release quality -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn quality() {
        use crate::layouts::Layout;
        use crate::model::Lang;
        for (lang, layout) in [
            (Lang::Thai, Layout::kedmanee()),
            (Lang::English, Layout::us_qwerty()),
        ] {
            println!("== {lang:?}");
            run(super::Near::shared(lang, &layout));
        }
    }

    fn run(n: &super::Near) {
        let mut words: Vec<(&String, u16)> = n.words.iter().map(|(w, c)| (w, *c)).collect();
        words.sort_by_key(|x| x.1);
        let t = std::time::Instant::now();
        let mut fired = 0;
        for (w, _) in &words {
            if let Some(f) = n.fix(w) {
                fired += 1;
                if fired < 25 {
                    println!("{w} -> {f:?}");
                }
            }
        }
        println!("single: {fired}/{} in {:?}", words.len(), t.elapsed());
        let mut fired = 0;
        let mut total = 0;
        let t = std::time::Instant::now();
        for i in 0..2000 {
            let a = words[(i * 7) % 3000].0;
            let b = words[(i * 13 + 5) % 6000].0;
            let run = format!("{a}{b}");
            total += 1;
            if let Some(f) = n.fix(&run) {
                fired += 1;
                if fired < 25 {
                    println!("{run} -> {f:?}");
                }
            }
        }
        println!("pairs: {fired}/{total} in {:?}", t.elapsed());
        // One slip in each of the 3000 most common words of 4+ characters.
        let (mut right, mut wrong, mut none) = (0, 0, 0);
        for (k, (w, _)) in words
            .iter()
            .filter(|(w, _)| w.chars().count() >= 4)
            .take(3000)
            .enumerate()
        {
            let mut c: Vec<char> = w.chars().collect();
            let i = 1 + k % (c.len() - 2);
            match k % 3 {
                0 => c.swap(i, i + 1),
                1 => {
                    c.remove(i);
                }
                _ => {
                    let x = c[i];
                    c.insert(i, x);
                }
            }
            let typo: String = c.iter().collect();
            match n.fix(&typo) {
                Some((_, f)) if &f == *w => right += 1,
                Some(_) => wrong += 1,
                None => none += 1,
            }
        }
        println!("slips: right {right} wrong {wrong} none {none}");
    }
}
