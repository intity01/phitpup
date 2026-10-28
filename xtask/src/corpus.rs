//! Loads the training corpora and splits them into train and test words.

use std::path::Path;

use phitpup_core::thai;

pub struct Word {
    pub text: String,
    /// Corpus frequency (Thai) or 1 (English).
    pub freq: f64,
    pub test: bool,
}

/// Deterministic 10% held-out split.
fn is_test(word: &str) -> bool {
    let mut h: u32 = 0x811C_9DC5; // FNV-1a
    for b in word.bytes() {
        h = (h ^ u32::from(b)).wrapping_mul(0x0100_0193);
    }
    h.is_multiple_of(10)
}

fn read(dir: &Path, file: &str) -> String {
    std::fs::read_to_string(dir.join(file))
        .unwrap_or_else(|e| panic!("corpus/{file}: {e} (run `cargo xtask fetch-corpus`)"))
}

/// Thai National Corpus word frequencies, Thai-only words.
pub fn thai(dir: &Path) -> Vec<Word> {
    read(dir, "tnc_freq.txt")
        .lines()
        .filter_map(|line| {
            let (w, f) = line.split_once('\t')?;
            let freq: f64 = f.trim().parse().ok()?;
            (!w.is_empty() && w.chars().all(thai::is_thai)).then(|| Word {
                text: w.to_string(),
                freq,
                test: is_test(w),
            })
        })
        .collect()
}

/// Hunspell en_US words, lowercased, with the common S/D/G suffixes expanded.
pub fn english(dir: &Path) -> Vec<Word> {
    let mut words: Vec<String> = Vec::new();
    for line in read(dir, "en.dic").lines().skip(1) {
        let (stem, flags) = line.split_once('/').unwrap_or((line, ""));
        let stem = stem.to_lowercase();
        if stem.is_empty() || !stem.chars().all(|c| c.is_ascii_lowercase() || c == '\'') {
            continue;
        }
        for flag in flags.chars() {
            if let Some(w) = inflect(&stem, flag) {
                words.push(w);
            }
        }
        words.push(stem);
    }
    words.sort_unstable();
    words.dedup();
    words
        .into_iter()
        .map(|w| {
            let test = is_test(&w);
            Word {
                text: w,
                freq: 1.0,
                test,
            }
        })
        .collect()
}

fn inflect(stem: &str, flag: char) -> Option<String> {
    let last = stem.chars().last()?;
    let before = stem.chars().rev().nth(1);
    let consonant_y = last == 'y' && before.is_some_and(|c| !"aeiou".contains(c));
    let cut = &stem[..stem.len() - 1];
    Some(match flag {
        'S' if consonant_y => format!("{cut}ies"),
        'S' if ["s", "x", "z", "ch", "sh"]
            .iter()
            .any(|e| stem.ends_with(e)) =>
        {
            format!("{stem}es")
        }
        'S' => format!("{stem}s"),
        'D' if last == 'e' => format!("{stem}d"),
        'D' if consonant_y => format!("{cut}ied"),
        'D' => format!("{stem}ed"),
        'G' if last == 'e' && !stem.ends_with("ee") => format!("{cut}ing"),
        'G' => format!("{stem}ing"),
        _ => return None,
    })
}
