//! `train-model` and `eval-detector`.

use std::path::Path;

use phitpup_core::model::{Lang, Model, Trainer};
use phitpup_core::{Detector, Direction, Layout, thai};

use crate::corpus::{self, Word};

/// Thai weights are ln(1 + freq), so common words count more without
/// drowning out the long tail; English words weigh 1 each.
const MIN_TRIGRAM_THAI: f64 = 2.0;
const MIN_TRIGRAM_ENGLISH: f64 = 2.0;

pub fn build(dir: &Path, include_test: bool) -> Model {
    let mut th = Trainer::new(Lang::Thai);
    for w in corpus::thai(dir) {
        if include_test || !w.test {
            th.add(&w.text, (1.0 + w.freq).ln());
        }
    }
    let mut en = Trainer::new(Lang::English);
    for w in corpus::english(dir) {
        if include_test || !w.test {
            en.add(&w.text, 1.0);
        }
    }
    Model::new(th.finish(MIN_TRIGRAM_THAI), en.finish(MIN_TRIGRAM_ENGLISH))
}

pub fn train(root: &Path) {
    let model = build(&root.join("corpus"), true);
    let bytes = model.to_bytes();
    let dest = root.join("crates/phitpup-core/src/model.bin");
    std::fs::write(&dest, &bytes).expect("write model.bin");
    println!(
        "wrote {} ({} KB; trigrams: thai {}, english {})",
        dest.display(),
        bytes.len() / 1024,
        model.lang(Lang::Thai).trigram_count(),
        model.lang(Lang::English).trigram_count(),
    );
}

/// One test case: what was typed, what the user meant, how often it occurs.
struct Case {
    typed: String,
    meant: String,
    weight: f64,
}

struct Scored {
    typed: String,
    converted: String,
    margin: f32,
    vetoed: bool,
    len: usize,
    weight: f64,
    right_text: bool,
}

pub fn eval(root: &Path) {
    let dir = root.join("corpus");
    // Train without the held-out words so the numbers mean something.
    let model: &'static Model = Box::leak(Box::new(build(&dir, false)));
    let thai_words: Vec<Word> = corpus::thai(&dir).into_iter().filter(|w| w.test).collect();
    let english_words: Vec<Word> = corpus::english(&dir)
        .into_iter()
        .filter(|w| w.test)
        .collect();

    let bad: Vec<&str> = thai_words
        .iter()
        .filter(|w| thai::check(&w.text).is_err())
        .map(|w| w.text.as_str())
        .collect();
    let bad_weight: f64 = thai_words
        .iter()
        .filter(|w| thai::check(&w.text).is_err())
        .map(|w| w.freq)
        .sum();
    let all_weight: f64 = thai_words.iter().map(|w| w.freq).sum();
    println!(
        "Thai rules reject {} of {} held-out words ({:.3}% by frequency), e.g. {:?}",
        bad.len(),
        thai_words.len(),
        100.0 * bad_weight / all_weight,
        &bad[..bad.len().min(12)]
    );

    let en = Layout::us_qwerty();
    for th in [Layout::kedmanee(), Layout::pattachote()] {
        let det = Detector::with_model(&en, &th, model);
        println!("\n== {} ==", th.name());

        // Thai meant, typed on the English layout / English meant, typed on Thai.
        let mut thai_on_en = Vec::new();
        let mut thai_ok = Vec::new();
        for w in &thai_words {
            let typed = det.convert(&w.text, Direction::ToEnglish);
            if det.convert(&typed, Direction::ToThai) == w.text {
                thai_on_en.push(Case {
                    typed,
                    meant: w.text.clone(),
                    weight: w.freq,
                });
            }
            thai_ok.push(Case {
                typed: w.text.clone(),
                meant: w.text.clone(),
                weight: w.freq,
            });
        }
        let mut en_on_th = Vec::new();
        let mut en_ok = Vec::new();
        for w in &english_words {
            let typed = det.convert(&w.text, Direction::ToThai);
            if det.convert(&typed, Direction::ToEnglish) == w.text {
                en_on_th.push(Case {
                    typed,
                    meant: w.text.clone(),
                    weight: 1.0,
                });
            }
            en_ok.push(Case {
                typed: w.text.clone(),
                meant: w.text.clone(),
                weight: 1.0,
            });
        }

        let score = |cases: &[Case]| -> Vec<Scored> {
            cases
                .iter()
                .filter_map(|c| {
                    let v = det.judge(&c.typed)?;
                    Some(Scored {
                        typed: c.typed.clone(),
                        right_text: v.converted == c.meant,
                        converted: v.converted,
                        margin: v.margin,
                        vetoed: v.vetoed,
                        len: c.typed.chars().count(),
                        weight: c.weight,
                    })
                })
                .collect()
        };

        report("ToThai", &score(&thai_on_en), &score(&en_ok));
        report("ToEnglish", &score(&en_on_th), &score(&thai_ok));
    }
}

/// Recall on `positives` and false-positive rate on `negatives` across thresholds.
fn report(name: &str, positives: &[Scored], negatives: &[Scored]) {
    println!(
        "{name}: {} positives, {} negatives",
        positives.len(),
        negatives.len()
    );
    println!("  min_len  thr   recall  recall(freq)  false+   false+(freq)");
    for min_len in [2usize, 3, 4] {
        for t in [0.0f32, 0.5, 1.0, 1.5, 2.0, 3.0] {
            let pass = |s: &&Scored| !s.vetoed && s.len >= min_len && s.margin > t;
            let rate = |set: &[Scored], need_right: bool, weighted: bool| -> f64 {
                let w = |s: &Scored| if weighted { s.weight } else { 1.0 };
                let total: f64 = set.iter().map(w).sum();
                let hit: f64 = set
                    .iter()
                    .filter(pass)
                    .filter(|s| !need_right || s.right_text)
                    .map(w)
                    .sum();
                100.0 * hit / total.max(1e-9) + 0.0
            };
            println!(
                "  {min_len:>7}  {t:>3.1}  {:>6.2}%  {:>11.2}%  {:>6.3}%  {:>11.4}%",
                rate(positives, true, false),
                rate(positives, true, true),
                rate(negatives, false, false),
                rate(negatives, false, true),
            );
        }
    }

    // The most costly mistakes at the auto-mode setting.
    let (min_len, t) = (3, 1.5);
    let mut missed: Vec<&Scored> = positives
        .iter()
        .filter(|s| s.len >= min_len && (s.vetoed || s.margin <= t || !s.right_text))
        .collect();
    missed.sort_by(|a, b| b.weight.total_cmp(&a.weight));
    let mut wrong: Vec<&Scored> = negatives
        .iter()
        .filter(|s| s.len >= min_len && !s.vetoed && s.margin > t)
        .collect();
    wrong.sort_by(|a, b| {
        b.weight
            .total_cmp(&a.weight)
            .then(b.margin.total_cmp(&a.margin))
    });
    let show = |v: &[&Scored]| -> String {
        v.iter()
            .take(15)
            .map(|s| {
                format!(
                    "{}→{} ({:+.1}{})",
                    s.typed,
                    s.converted,
                    s.margin,
                    if s.vetoed { " veto" } else { "" }
                )
            })
            .collect::<Vec<_>>()
            .join("  ")
    };
    println!("  missed:  {}", show(&missed));
    println!("  false+:  {}", show(&wrong));
}
