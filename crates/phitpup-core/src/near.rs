//! Words typed with a slip of the finger: a key next to the right one, two
//! keys in the wrong order, one too many or one missing (ส้วสดี for สวัสดี,
//! teh for the). The fix offered is the common word ([`crate::model`]'s
//! lexicon) that is closest on the keyboard in use.
//!
//! Text that is still being typed is left alone: the end of the run may be
//! the start of a word, and Thai runs several words together, so only the
//! part after the last place the run splits into known words is looked at.

use std::ops::Range;
use std::sync::{Mutex, OnceLock};

use crate::layouts::Layout;
use crate::model::{Lang, Model, english_words};

/// Edit costs, in tenths of an edit.
const FULL: u32 = 10;
const NEIGHBOUR: u32 = 6;
const SAME_KEY: u32 = 4;
const SWAP: u32 = 4;
/// A slip happening at all, and each tenth of an edit, in tenths of a bit.
const SLIP: u32 = 40;
const PER_TENTH: u32 = 8;
/// How much likelier a fix must be than the text as typed, in tenths of a
/// bit.
const MARGIN: u32 = 20;
/// A word not in the lexicon (a name, slang, jargon), in tenths of a bit:
/// this much, plus `UNKNOWN_PER`/10 of what the letter model says it costs.
const UNKNOWN: u32 = 125;
const UNKNOWN_PER: u32 = 7;
/// How much likelier the best fix must be than the next one, in tenths of a
/// bit.
const CLOSE: u32 = 10;
/// The longest word in the lexicon is shorter than this, in characters.
const MAX_WORD: usize = 24;

/// Letters as small numbers: Thai U+0E01..=U+0E4E as 0..78; English a-z as
/// 0..26 and the apostrophe as 26. Words are compared, sorted and searched as
/// these numbers.
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

fn letter(lang: Lang, i: u8) -> char {
    match (lang, i) {
        (Lang::Thai, _) => char::from_u32(0x0E01 + u32::from(i)).unwrap_or('?'),
        (Lang::English, 26) => '\'',
        (Lang::English, _) => char::from(b'a' + i),
    }
}

/// One known word: its letters are `Near::letters[start..start + len]`.
struct Entry {
    start: u32,
    len: u8,
    /// −log2 of how common it is, in 1/10 bit.
    cost: u16,
    /// Which letters it uses: two words a slip or two apart differ in at
    /// most four of these.
    mask: u128,
}

/// A [`Near`] made for a language and a layout (by name).
type Built = (Lang, String, &'static Near);

pub struct Near {
    lang: Lang,
    /// Every word's letters, one after another.
    letters: Vec<u8>,
    /// The words, sorted by their letters.
    list: Vec<Entry>,
    /// Where the words starting with each letter are in `list`.
    first: Vec<Range<usize>>,
    /// Cost of typing the first letter where the second was meant.
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
        let near = match lang {
            Lang::Thai => Near::new(lang, Model::embedded().lang(lang).lexicon(), layout),
            Lang::English => {
                let words = english_words();
                Near::new(lang, words.iter().map(|(w, c)| (w.as_str(), *c)), layout)
            }
        };
        let near: &'static Near = Box::leak(Box::new(near));
        built.push((lang, layout.name().to_owned(), near));
        near
    }

    pub fn new<'a>(
        lang: Lang,
        words: impl Iterator<Item = (&'a str, u16)>,
        layout: &Layout,
    ) -> Self {
        let mut words: Vec<(Vec<u8>, u16)> = words
            .filter_map(|(w, cost)| {
                let l: Vec<u8> = w.chars().map(|c| index(lang, c)).collect::<Option<_>>()?;
                (!l.is_empty() && l.len() <= MAX_WORD).then_some((l, cost))
            })
            .collect();
        words.sort_unstable();
        words.dedup_by(|a, b| a.0 == b.0);
        let mut letters = Vec::new();
        let list: Vec<Entry> = words
            .iter()
            .map(|(l, cost)| {
                let start = letters.len() as u32;
                letters.extend_from_slice(l);
                Entry {
                    start,
                    len: l.len() as u8,
                    cost: *cost,
                    mask: mask(l),
                }
            })
            .collect();
        let first_of = |e: &Entry| letters[e.start as usize];
        let first = (0..LETTERS as u8)
            .map(|c| {
                list.partition_point(|e| first_of(e) < c)
                    ..list.partition_point(|e| first_of(e) <= c)
            })
            .collect();

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
            letters,
            list,
            first,
            sub,
        }
    }

    fn word(&self, e: &Entry) -> &[u8] {
        &self.letters[e.start as usize..e.start as usize + e.len as usize]
    }

    /// The cost of `w` if it is a known word.
    fn cost(&self, w: &[u8]) -> Option<u16> {
        self.list
            .binary_search_by(|e| self.word(e).cmp(w))
            .ok()
            .map(|i| self.list[i].cost)
    }

    fn spell(&self, w: &[u8]) -> String {
        w.iter().map(|&i| letter(self.lang, i)).collect()
    }

    /// What to replace the end of `run` with: how many characters from the
    /// end, and the word. `None` when the run reads at least as well as known
    /// words as typed, the last one possibly still being typed.
    ///
    /// Readings are compared by cost, in tenths of a bit: the words' costs
    /// (how rare they are) plus, for a fix, the cost of the slips.
    pub fn fix(&self, run: &str) -> Option<(usize, String)> {
        // Capitals are names, acronyms and the like, not slips; English is
        // compared in lowercase, so this also keeps the result's case.
        let run: Vec<u8> = run
            .chars()
            .map(|c| index(self.lang, c))
            .collect::<Option<_>>()?;
        // Chat elongation (ค่ะะ, sooo) is meant.
        if run.len() < 3 || run[run.len() - 1] == run[run.len() - 2] {
            return None;
        }
        // English puts spaces between words, so the run is one word.
        let seg = match self.lang {
            Lang::Thai => self.segments(&run),
            Lang::English => {
                let mut seg = vec![None; run.len() + 1];
                seg[0] = Some(0);
                seg
            }
        };
        // As typed: known words, then the start of one.
        let as_typed = (0..=run.len())
            .filter_map(|i| Some(seg[i]? + self.cheapest_start(&run[i..])?))
            .min();
        // Or known words, then one the lexicon does not have, if its letters
        // read like the language.
        let model = Model::embedded().lang(self.lang);
        let unknown = (0..run.len())
            .filter_map(|i| {
                let tail = &run[i..];
                let bits = model.cost(&self.spell(tail)) * (tail.len() as f32 + 1.0);
                Some(seg[i]? + UNKNOWN + UNKNOWN_PER * (bits * 10.0) as u32 / 10)
            })
            .min();
        let as_typed = as_typed.into_iter().chain(unknown).min();
        // Fixed: known words, then a near word in place of the rest.
        let (cost, n, word) = (0..run.len())
            .filter_map(|i| {
                let tail = &run[i..];
                let (cost, word) = self.closest(tail)?;
                Some((seg[i]? + cost, tail.len(), word))
            })
            .min()?;
        as_typed
            .is_none_or(|t| cost + MARGIN < t)
            .then(|| (n, self.spell(word)))
    }

    /// `seg[i]`: the cheapest split of the first `i` letters into known
    /// words.
    fn segments(&self, run: &[u8]) -> Vec<Option<u32>> {
        let mut seg = vec![None; run.len() + 1];
        seg[0] = Some(0);
        for i in 0..run.len() {
            let Some(base) = seg[i] else { continue };
            for j in i + 1..=run.len().min(i + MAX_WORD) {
                if let Some(c) = self.cost(&run[i..j]) {
                    let total = base + u32::from(c);
                    if seg[j].is_none_or(|t| total < t) {
                        seg[j] = Some(total);
                    }
                }
            }
        }
        seg
    }

    /// The cost of the most common known word that starts with `prefix`; 0
    /// for nothing.
    fn cheapest_start(&self, prefix: &[u8]) -> Option<u32> {
        if prefix.is_empty() {
            return Some(0);
        }
        let i = self.list.partition_point(|e| self.word(e) < prefix);
        self.list[i..]
            .iter()
            .take_while(|e| self.word(e).starts_with(prefix))
            .map(|e| u32::from(e.cost))
            .min()
    }

    /// The known word that best explains `typed` as slips of the finger, and
    /// that reading's cost, if one stands out: one slip for words under
    /// five letters, two from five on. Nothing for a known word.
    fn closest(&self, typed: &[u8]) -> Option<(u32, &[u8])> {
        if typed.len() < 3 || typed.len() > MAX_WORD + 2 || self.cost(typed).is_some() {
            return None;
        }
        let tmask = mask(typed);
        let limit = if typed.len() >= 5 { 2 * FULL } else { FULL };
        // English slips nearly always keep the first letter or swap it with
        // the second, and English has many more words to look at.
        let pool: Vec<&[Entry]> = match self.lang {
            Lang::Thai => vec![&self.list],
            Lang::English => {
                let bucket = |c: u8| &self.list[self.first[c as usize].clone()];
                let mut pool = vec![bucket(typed[0])];
                if typed[1] != typed[0] {
                    pool.push(bucket(typed[1]));
                }
                pool
            }
        };
        let mut found: Vec<(u32, &[u8])> = pool
            .into_iter()
            .flatten()
            .filter(|e| {
                e.len >= 3
                    && usize::from(e.len).abs_diff(typed.len()) <= 2
                    && (e.mask ^ tmask).count_ones() <= 4
            })
            .map(|e| (self.word(e), e.cost))
            .filter(|(w, _)| self.lang == Lang::Thai || w[0] == typed[0] || w[1] == typed[0])
            .filter_map(|(w, cost)| {
                let d = self.distance(typed, w, limit)?;
                Some((u32::from(cost) + SLIP + d * PER_TENTH, w))
            })
            .collect();
        found.sort_unstable();
        let &(cost, w) = found.first()?;
        // Another word about as likely: no way to tell which was meant.
        if found.get(1).is_some_and(|&(c, _)| c < cost + CLOSE) {
            return None;
        }
        Some((cost, w))
    }

    /// Weighted edit distance (with swaps of neighbours), or `None` past
    /// `limit`. Both words are at most `MAX_WORD + 2` letters.
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

fn mask(letters: &[u8]) -> u128 {
    letters.iter().fold(0, |m, &c| m | 1 << c)
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
    fn english_slips() {
        let english = Near::shared(Lang::English, &Layout::us_qwerty());
        let fix = |w: &str| english.fix(w).map(|(_, w)| w);
        assert_eq!(fix("teh").as_deref(), Some("the"));
        assert_eq!(fix("recieve").as_deref(), Some("receive"));
        assert_eq!(fix("becuase").as_deref(), Some("because"));
        assert_eq!(fix("wrok").as_deref(), Some("work"));
        for w in [
            "the",
            "receive",
            "Teh",
            "NASA",
            "sooo",
            "ok",
            "สวัสดี",
            "github",
            "json",
            "btw",
            "wifi",
            "youtube",
        ] {
            assert_eq!(fix(w), None, "{w}");
        }
    }

    #[test]
    fn leaves_unknown_words_alone() {
        // Names and slang the lexicon lacks, but whose letters read fine.
        let english = Near::shared(Lang::English, &Layout::us_qwerty());
        for w in [
            "ubuntu",
            "kubernetes",
            "nginx",
            "shopee",
            "minecraft",
            "laravel",
        ] {
            assert_eq!(english.fix(w), None, "{w}");
        }
        for w in [
            "นอยด์",
            "แคปชั่น",
            "แฟนด้อม",
            "จุงเบย",
            "สตอรี่",
            "ติ๊กต็อก",
            "โอปป้า",
            "ไลค์",
        ] {
            assert_eq!(thai().fix(w), None, "{w}");
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
    use super::Near;
    use crate::layouts::Layout;
    use crate::model::Lang;

    /// How often fixes fire on real words and are right on slips:
    /// `cargo test -p phitpup-core --release quality -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn quality() {
        for (lang, layout) in [
            (Lang::Thai, Layout::kedmanee()),
            (Lang::English, Layout::us_qwerty()),
        ] {
            println!("== {lang:?}");
            run(Near::shared(lang, &layout));
        }
    }

    fn run(n: &Near) {
        let mut words: Vec<(String, u16)> = n
            .list
            .iter()
            .map(|e| (n.spell(n.word(e)), e.cost))
            .collect();
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
        let t = std::time::Instant::now();
        for i in 0..2000 {
            let run = format!(
                "{}{}",
                words[(i * 7) % 3000].0,
                words[(i * 13 + 5) % 6000].0
            );
            if n.fix(&run).is_some() {
                fired += 1;
            }
        }
        println!("pairs: {fired}/2000 in {:?}", t.elapsed());
        // One slip in each of the 3000 most common words of 4+ letters.
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
                Some((_, f)) if &f == w => right += 1,
                Some(_) => wrong += 1,
                None => none += 1,
            }
        }
        println!("slips: right {right} wrong {wrong} none {none}");
    }
}
