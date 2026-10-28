//! Character trigram models for Thai and English.
//!
//! Each model gives a cost, in bits per character, for how unlike the language
//! a word looks. The trained tables live in `model.bin`, built by
//! `cargo xtask train-model` and embedded in the binary. Costs are stored as
//! `u8` in units of 1/[`SCALE`] bit: dense unigram and bigram tables, plus a
//! sorted list of the trigrams seen often enough to keep.

use std::collections::HashMap;
use std::sync::OnceLock;

/// Cost units per bit.
pub const SCALE: f32 = 10.0;
/// Extra cost when a trigram is missing and the bigram is used instead.
const BACKOFF: u32 = 15;
const BOUNDARY: usize = 0;
const MAGIC: &[u8; 4] = b"PPM1";

static EMBEDDED: &[u8] = include_bytes!("model.bin");

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Lang {
    Thai,
    English,
}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::Thai, Lang::English];

    /// Symbols: boundary, the letters, then one bucket for everything else.
    pub fn alphabet_size(self) -> usize {
        match self {
            Lang::Thai => 93,    // boundary + U+0E01..=U+0E5B + other
            Lang::English => 29, // boundary + a-z + apostrophe + other
        }
    }

    pub fn index(self, c: char) -> usize {
        match self {
            Lang::Thai => match c {
                '\u{0E01}'..='\u{0E5B}' => c as usize - 0x0E00,
                _ => 92,
            },
            Lang::English => match c.to_ascii_lowercase() {
                l @ 'a'..='z' => (l as u8 - b'a') as usize + 1,
                '\'' => 27,
                _ => 28,
            },
        }
    }

    fn symbols(self, word: &str) -> impl Iterator<Item = usize> + '_ {
        word.chars().map(move |c| self.index(c)).chain([BOUNDARY])
    }
}

#[derive(Clone, Debug)]
pub struct LangModel {
    lang: Lang,
    uni: Vec<u8>,
    bi: Vec<u8>,
    tri_keys: Vec<u32>,
    tri_costs: Vec<u8>,
}

impl LangModel {
    /// Every symbol equally likely. Used before a model has been trained.
    fn uniform(lang: Lang) -> Self {
        let a = lang.alphabet_size();
        let c = ((a as f32).log2() * SCALE).round() as u8;
        Self {
            lang,
            uni: vec![c; a],
            bi: vec![c; a * a],
            tri_keys: vec![],
            tri_costs: vec![],
        }
    }

    pub fn lang(&self) -> Lang {
        self.lang
    }

    pub fn trigram_count(&self) -> usize {
        self.tri_keys.len()
    }

    fn key(&self, a: usize, b: usize, c: usize) -> u32 {
        let n = self.lang.alphabet_size();
        ((a * n + b) * n + c) as u32
    }

    fn step(&self, a: usize, b: usize, c: usize) -> u32 {
        match self.tri_keys.binary_search(&self.key(a, b, c)) {
            Ok(i) => self.tri_costs[i].into(),
            Err(_) => u32::from(self.bi[b * self.lang.alphabet_size() + c]) + BACKOFF,
        }
    }

    /// Average cost in bits per symbol, counting the end of the word.
    pub fn cost(&self, word: &str) -> f32 {
        let (mut a, mut b) = (BOUNDARY, BOUNDARY);
        let (mut total, mut n) = (0u32, 0u32);
        for c in self.lang.symbols(word) {
            total += self.step(a, b, c);
            n += 1;
            (a, b) = (b, c);
        }
        total as f32 / n as f32 / SCALE
    }

    fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.uni);
        out.extend_from_slice(&self.bi);
        out.extend_from_slice(&(self.tri_keys.len() as u32).to_le_bytes());
        for k in &self.tri_keys {
            out.extend_from_slice(&k.to_le_bytes());
        }
        out.extend_from_slice(&self.tri_costs);
    }

    fn read(lang: Lang, bytes: &mut &[u8]) -> Option<Self> {
        let a = lang.alphabet_size();
        let uni = take(bytes, a)?.to_vec();
        let bi = take(bytes, a * a)?.to_vec();
        let n = u32::from_le_bytes(take(bytes, 4)?.try_into().ok()?) as usize;
        let tri_keys = take(bytes, n * 4)?
            .chunks_exact(4)
            .map(|k| u32::from_le_bytes(k.try_into().unwrap()))
            .collect();
        let tri_costs = take(bytes, n)?.to_vec();
        Some(Self {
            lang,
            uni,
            bi,
            tri_keys,
            tri_costs,
        })
    }
}

fn take<'a>(bytes: &mut &'a [u8], n: usize) -> Option<&'a [u8]> {
    if bytes.len() < n {
        return None;
    }
    let (head, tail) = bytes.split_at(n);
    *bytes = tail;
    Some(head)
}

#[derive(Clone, Debug)]
pub struct Model {
    thai: LangModel,
    english: LangModel,
}

impl Model {
    /// The model embedded at build time, or a uniform one if none was trained.
    pub fn embedded() -> &'static Model {
        static MODEL: OnceLock<Model> = OnceLock::new();
        MODEL.get_or_init(|| Model::from_bytes(EMBEDDED).unwrap_or_else(Model::uniform))
    }

    pub fn uniform() -> Self {
        Self {
            thai: LangModel::uniform(Lang::Thai),
            english: LangModel::uniform(Lang::English),
        }
    }

    pub fn new(thai: LangModel, english: LangModel) -> Self {
        assert!(thai.lang == Lang::Thai && english.lang == Lang::English);
        Self { thai, english }
    }

    pub fn is_trained(&self) -> bool {
        self.thai.trigram_count() > 0 && self.english.trigram_count() > 0
    }

    pub fn lang(&self, lang: Lang) -> &LangModel {
        match lang {
            Lang::Thai => &self.thai,
            Lang::English => &self.english,
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        self.thai.write(&mut out);
        self.english.write(&mut out);
        out
    }

    pub fn from_bytes(mut bytes: &[u8]) -> Option<Self> {
        if take(&mut bytes, 4)? != MAGIC {
            return None;
        }
        let thai = LangModel::read(Lang::Thai, &mut bytes)?;
        let english = LangModel::read(Lang::English, &mut bytes)?;
        bytes.is_empty().then_some(Self { thai, english })
    }
}

/// Accumulates weighted words and turns them into a [`LangModel`].
pub struct Trainer {
    lang: Lang,
    uni: Vec<f64>,
    bi: Vec<f64>,
    tri: HashMap<u32, f64>,
}

impl Trainer {
    pub fn new(lang: Lang) -> Self {
        let a = lang.alphabet_size();
        Self {
            lang,
            uni: vec![0.0; a],
            bi: vec![0.0; a * a],
            tri: HashMap::new(),
        }
    }

    pub fn add(&mut self, word: &str, weight: f64) {
        let n = self.lang.alphabet_size();
        let (mut a, mut b) = (BOUNDARY, BOUNDARY);
        for c in self.lang.symbols(word) {
            self.uni[c] += weight;
            self.bi[b * n + c] += weight;
            *self.tri.entry(((a * n + b) * n + c) as u32).or_default() += weight;
            (a, b) = (b, c);
        }
    }

    /// Keeps trigrams whose weight is at least `min_trigram`.
    pub fn finish(self, min_trigram: f64) -> LangModel {
        let n = self.lang.alphabet_size();
        let cost = |p: f64| (-p.log2() * SCALE as f64).round().clamp(0.0, 255.0) as u8;

        let total: f64 = self.uni.iter().sum();
        let p_uni: Vec<f64> = self
            .uni
            .iter()
            .map(|&u| (u + 0.5) / (total + 0.5 * n as f64))
            .collect();

        // Bigrams, smoothed toward the unigram distribution.
        const K: f64 = 2.0;
        let mut bi = vec![0u8; n * n];
        for b in 0..n {
            let ctx: f64 = self.bi[b * n..(b + 1) * n].iter().sum();
            for c in 0..n {
                bi[b * n + c] = cost((self.bi[b * n + c] + K * p_uni[c]) / (ctx + K));
            }
        }

        let mut ctx: HashMap<u32, f64> = HashMap::new();
        for (&k, &w) in &self.tri {
            *ctx.entry(k / n as u32).or_default() += w;
        }
        let mut tri: Vec<(u32, u8)> = self
            .tri
            .iter()
            .filter(|&(_, &w)| w >= min_trigram)
            .map(|(&k, &w)| (k, cost(w / ctx[&(k / n as u32)])))
            .collect();
        tri.sort_unstable_by_key(|&(k, _)| k);

        LangModel {
            lang: self.lang,
            uni: p_uni.into_iter().map(cost).collect(),
            bi,
            tri_keys: tri.iter().map(|&(k, _)| k).collect(),
            tri_costs: tri.iter().map(|&(_, c)| c).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_round_trip() {
        let mut t = Trainer::new(Lang::English);
        for w in ["hello", "help", "yellow"] {
            t.add(w, 1.0);
        }
        let mut th = Trainer::new(Lang::Thai);
        th.add("สวัสดี", 1.0);
        let m = Model::new(th.finish(1.0), t.finish(1.0));
        let back = Model::from_bytes(&m.to_bytes()).unwrap();
        assert_eq!(back.to_bytes(), m.to_bytes());
        let en = back.lang(Lang::English);
        assert!(en.cost("hello") < en.cost("qzxv"));
    }

    #[test]
    fn rejects_bad_bytes() {
        assert!(Model::from_bytes(b"").is_none());
        assert!(Model::from_bytes(b"PPM1\0").is_none());
    }
}
