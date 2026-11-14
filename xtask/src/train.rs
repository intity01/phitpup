//! `train-model` and `eval-detector`.

use std::path::Path;

use phitpup_core::model::{Lang, Model, Trainer};
use phitpup_core::{Detector, Direction, Layout, Thresholds, thai};

use crate::corpus::{self, Word};

/// Thai weights are ln(1 + freq), so common words count more without
/// drowning out the long tail; English words weigh 1 each.
const MIN_TRIGRAM_THAI: f64 = 2.0;
const MIN_TRIGRAM_ENGLISH: f64 = 2.0;
/// Most frequent Thai words kept as a lexicon in the shipped model.
pub const THAI_LEXICON: usize = 20_000;

/// Trigrams come from the train split only unless `include_test`. The lexicon
/// always comes from the whole corpus: it stands for "words the user will
/// type often", which in real use are known words, so it is judged by the
/// frequency-weighted numbers.
pub fn build(dir: &Path, include_test: bool, thai_lexicon: usize) -> Model {
    let thai_words = corpus::thai(dir);
    let mut th = Trainer::new(Lang::Thai);
    for w in &thai_words {
        if include_test || !w.test {
            th.add(&w.text, (1.0 + w.freq).ln());
        }
    }
    let mut th = th.finish(MIN_TRIGRAM_THAI);

    let total: f64 = thai_words.iter().map(|w| w.freq).sum();
    let mut by_freq: Vec<&Word> = thai_words.iter().collect();
    by_freq.sort_by(|a, b| b.freq.total_cmp(&a.freq).then_with(|| a.text.cmp(&b.text)));
    th.set_lexicon(
        by_freq
            .iter()
            .take(thai_lexicon)
            .map(|w| (w.text.clone(), w.freq / total)),
    );

    let mut en = Trainer::new(Lang::English);
    for w in corpus::english(dir) {
        if include_test || !w.test {
            en.add(&w.text, 1.0);
        }
    }
    let mut en = en.finish(MIN_TRIGRAM_ENGLISH);
    // Words typed every day that the trigrams alone take for odd English
    // (cpu, pdf, idk): as common as the dictionary's most common words.
    let p = (-ENGLISH_LEXICON_BITS).exp2();
    let mut lexicon: std::collections::HashMap<String, f64> = scowl(dir, ENGLISH_LEXICON_LEVELS)
        .into_iter()
        .map(|(w, bits)| (w, (-bits).exp2()))
        .collect();
    for w in ENGLISH_COMMON {
        lexicon.insert((*w).to_owned(), p);
    }
    en.set_lexicon(lexicon);
    Model::new(th, en)
}

/// SCOWL size levels kept for English slips, and what a word at each level
/// is taken to cost in bits (level 10 holds the most common words).
const ENGLISH_LEVELS: &[(u32, f64)] = &[
    (10, 10.0),
    (20, 12.5),
    (35, 14.5),
    (40, 15.5),
    (50, 16.5),
    // Rarer words: known, so never "fixed", but seldom offered.
    (55, 22.0),
    (60, 22.0),
    (70, 22.0),
];

/// Everyday words that dictionaries leave out, taken as typed on purpose.
const ENGLISH_EXTRA: &[&str] = &[
    "todo",
    "json",
    "url",
    "urls",
    "btw",
    "omg",
    "wifi",
    "imo",
    "imho",
    "pls",
    "plz",
    "lol",
    "idk",
    "tbh",
    "smh",
    "nvm",
    "thx",
    "lgtm",
    "wip",
    "fyi",
    "asap",
    "afaik",
    "brb",
    "gg",
    "covid",
    "tiktok",
    "vlog",
    "vlogs",
    "docker",
    "github",
    "gitlab",
    "repo",
    "repos",
    "config",
    "async",
    "npm",
    "api",
    "apis",
    "app",
    "apps",
    "ui",
    "ux",
    "faq",
    "pdf",
    "ok",
    "okay",
    "emoji",
    "emojis",
    "selfie",
    "selfies",
    "hashtag",
    "hashtags",
    "podcast",
    "podcasts",
    "blog",
    "blogs",
    "login",
    "logout",
    "signup",
    "username",
    "email",
    "emails",
    "online",
    "offline",
    "website",
    "websites",
    "webpage",
    "smartphone",
    "laptop",
    "laptops",
    "gonna",
    "wanna",
    "gotta",
    "yeah",
    "nope",
    "yep",
    "kinda",
    "sorta",
    "dunno",
    "bangkok",
    "pattaya",
    "chiang",
    "mai",
    "phuket",
    "youtube",
    "google",
    "netflix",
    "iphone",
    "thai",
    // Chat.
    "haha",
    "hahaha",
    "hehe",
    "bruh",
    "yeet",
    "irl",
    "hbu",
    "wyd",
    "ngl",
    "tmr",
    "tmrw",
    "ikr",
    "lmk",
    "pfp",
    "bff",
    "dms",
    "rn",
];

/// Words typed every day in chat and at work that dictionaries leave out or
/// rank as rare: taken as common English, so that typed on the Thai layout
/// they are fixed, not only left alone when typed right.
const ENGLISH_COMMON: &[&str] = &[
    // Chat.
    "lol",
    "lmao",
    "rofl",
    "omg",
    "btw",
    "idk",
    "tbh",
    "imo",
    "fyi",
    "asap",
    "brb",
    "afk",
    "nvm",
    "thx",
    "pls",
    "plz",
    "ok",
    "okay",
    "yep",
    "yup",
    "nope",
    "sup",
    "bro",
    "dude",
    "haha",
    "hahaha",
    "xd",
    "wtf",
    "dm",
    "dms",
    "ngl",
    "irl",
    "lmk",
    "ikr",
    "tmr",
    "cya",
    "noob",
    "meme",
    "memes",
    "emoji",
    "emojis",
    "selfie",
    "vlog",
    "podcast",
    "hashtag",
    // Work and computers.
    "cpu",
    "gpu",
    "ram",
    "ssd",
    "hdd",
    "usb",
    "hdmi",
    "wifi",
    "vpn",
    "dns",
    "ip",
    "lan",
    "http",
    "https",
    "html",
    "css",
    "js",
    "ts",
    "jsx",
    "tsx",
    "php",
    "sql",
    "api",
    "apis",
    "sdk",
    "ide",
    "cli",
    "gui",
    "os",
    "ios",
    "json",
    "yaml",
    "toml",
    "xml",
    "csv",
    "pdf",
    "png",
    "jpg",
    "jpeg",
    "gif",
    "svg",
    "mp3",
    "mp4",
    "zip",
    "exe",
    "msi",
    "dll",
    "url",
    "urls",
    "git",
    "github",
    "gitlab",
    "npm",
    "repo",
    "repos",
    "config",
    "async",
    "await",
    "admin",
    "login",
    "logout",
    "signup",
    "username",
    "email",
    "emails",
    "inbox",
    "online",
    "offline",
    "website",
    "app",
    "apps",
    "ui",
    "ux",
    "faq",
    "todo",
    "fps",
    "ping",
    "lag",
    "localhost",
    "backend",
    "frontend",
    "devops",
    "deploy",
    "debug",
    "docker",
    "linux",
    "ubuntu",
    "python",
    "javascript",
    "typescript",
    "kotlin",
    "rust",
    "golang",
    // Names typed in lowercase all the time.
    "google",
    "gmail",
    "youtube",
    "facebook",
    "instagram",
    "tiktok",
    "twitter",
    "reddit",
    "discord",
    "zoom",
    "canva",
    "figma",
    "notion",
    "netflix",
    "spotify",
    "shopee",
    "lazada",
    "grab",
    "line",
    "chatgpt",
    "openai",
    "claude",
    "gemini",
    "copilot",
    "chrome",
    "brave",
    "firefox",
    "iphone",
    "ipad",
    "samsung",
    "windows",
    "excel",
    "powerpoint",
    "outlook",
    // Brands and tools.
    "xiaomi",
    "oppo",
    "vivo",
    "huawei",
    "lenovo",
    "nvidia",
    "amd",
    "uber",
    "roblox",
    "dota",
    "csgo",
    "rov",
    "miro",
    "nginx",
    "ruby",
    "zsh",
    "dev",
    "pnpm",
    "regex",
    "vue",
    "svelte",
    "nextjs",
    "nuxt",
    "chatbot",
    "faq",
    "gif",
];

/// SCOWL levels whose words go into the detector's English lexicon, with
/// their cost in bits for the whole word.
const ENGLISH_LEXICON_LEVELS: &[(u32, f64)] = &[(10, 11.0), (20, 13.0)];

/// Plain lowercase words of the given SCOWL levels, each with its level's
/// bits (the lowest level a word is in wins).
fn scowl(corpus: &Path, levels: &[(u32, f64)]) -> Vec<(String, f64)> {
    let dir = corpus.join("scowl/final");
    let mut words: std::collections::HashMap<String, f64> = Default::default();
    for &(level, bits) in levels {
        for list in ["english-words", "american-words"] {
            let path = dir.join(format!("{list}.{level}"));
            let bytes = std::fs::read(&path).unwrap_or_else(|e| {
                panic!("{}: {e} (unpack SCOWL into corpus/scowl)", path.display())
            });
            let text: String = bytes.iter().map(|&b| char::from(b)).collect();
            for w in text.lines() {
                if w.len() >= 2 && w.bytes().all(|b| b.is_ascii_lowercase()) {
                    let e = words.entry(w.to_owned()).or_insert(f64::MAX);
                    *e = e.min(bits);
                }
            }
        }
    }
    words.into_iter().collect()
}

/// What one of [`ENGLISH_COMMON`] costs in the detector's English model,
/// in bits for the whole word.
const ENGLISH_LEXICON_BITS: f64 = 10.0;

/// The cost in bits given to [`ENGLISH_COMMON`]: as the dictionary's
/// everyday words (level 20).
const COMMON_BITS: f64 = 12.5;

/// Writes `en-words.bin` from SCOWL, unpacked into `corpus/scowl/` (from
/// <http://wordlist.aspell.net/>; only its `final/` lists are read).
/// Within a level, a word spelt the usual way (by the English trigrams)
/// counts as a little more common, so `the` beats `ten` for `teh`.
pub fn english_words(root: &Path) {
    let dir = root.join("corpus/scowl/final");
    let model = Model::embedded().lang(Lang::English);
    let mut words: std::collections::HashMap<String, f64> = Default::default();
    for &(level, bits) in ENGLISH_LEVELS {
        for list in ["english-words", "american-words", "english-contractions"] {
            let path = dir.join(format!("{list}.{level}"));
            // Not every level has contractions.
            if list == "english-contractions" && !path.exists() {
                continue;
            }
            let bytes = std::fs::read(&path).unwrap_or_else(|e| {
                panic!("{}: {e} (unpack SCOWL into corpus/scowl)", path.display())
            });
            // Latin-1; only plain lowercase words are kept anyway.
            let text: String = bytes.iter().map(|&b| char::from(b)).collect();
            for w in text.lines() {
                if w.is_empty() || !w.bytes().all(|b| b.is_ascii_lowercase() || b == b'\'') {
                    continue;
                }
                let symbols = (w.len() + 1) as f64;
                let cost = bits + 0.2 * f64::from(model.cost(w)) * symbols;
                let e = words.entry(w.to_owned()).or_insert(f64::MAX);
                *e = e.min(cost);
            }
        }
    }
    let rare = ENGLISH_LEVELS.last().unwrap().1;
    // The abbreviations the detector leaves in English are known words
    // here too, so no slip fix turns "gps" into "gas".
    let abbreviations = phitpup_core::detect::ABBREVIATIONS
        .iter()
        .filter(|w| w.bytes().all(|b| b.is_ascii_lowercase()));
    for w in ENGLISH_EXTRA.iter().chain(abbreviations) {
        words.entry((*w).to_owned()).or_insert(rare);
    }
    for w in ENGLISH_COMMON {
        let symbols = (w.len() + 1) as f64;
        let cost = COMMON_BITS + 0.2 * f64::from(model.cost(w)) * symbols;
        let e = words.entry((*w).to_owned()).or_insert(f64::MAX);
        *e = e.min(cost);
    }
    let mut words: Vec<(String, u16)> = words
        .into_iter()
        .map(|(w, bits)| (w, (bits * 10.0).round() as u16))
        .collect();
    words.sort_unstable();
    let bytes = phitpup_core::model::words_to_bytes(&words);
    let dest = root.join("crates/phitpup-core/src/en-words.bin");
    std::fs::write(&dest, &bytes).expect("write en-words.bin");
    println!(
        "wrote {} ({} KB; {} words)",
        dest.display(),
        bytes.len() / 1024,
        words.len()
    );
}

pub fn train(root: &Path) {
    let model = build(&root.join("corpus"), true, THAI_LEXICON);
    let bytes = model.to_bytes();
    let dest = root.join("crates/phitpup-core/src/model.bin");
    std::fs::write(&dest, &bytes).expect("write model.bin");
    println!(
        "wrote {} ({} KB; trigrams: thai {}, english {}; thai lexicon {})",
        dest.display(),
        bytes.len() / 1024,
        model.lang(Lang::Thai).trigram_count(),
        model.lang(Lang::English).trigram_count(),
        model.lang(Lang::Thai).lexicon_len(),
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

pub fn eval(root: &Path, thai_lexicon: usize) {
    println!("Thai lexicon: {thai_lexicon} words");
    let dir = root.join("corpus");
    // Train without the held-out words so the numbers mean something.
    let model: &'static Model = Box::leak(Box::new(build(&dir, false, thai_lexicon)));
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
        let mut det = Detector::with_model(&en, &th, model);
        // Raw margins; `report` applies the thresholds it sweeps.
        det.thresholds = Thresholds {
            to_thai: 0.0,
            to_english: 0.0,
            min_chars: 0,
        };
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
        midword(&det, &thai_on_en, &en_ok, "ToThai");
        midword(&det, &en_on_th, &thai_ok, "ToEnglish");
    }
}

/// Switching partway through a word: for each setting, how many words typed
/// in the wrong layout are fixed before they end (with the right text so
/// far), and how many typed right would be switched at some point.
fn midword(det: &Detector, positives: &[Case], negatives: &[Case], name: &str) {
    let first_flip = |typed: &str, t: Thresholds| -> Option<(usize, String)> {
        let chars: Vec<char> = typed.chars().collect();
        (1..=chars.len()).find_map(|n| {
            let p: String = chars[..n].iter().collect();
            det.judge_typing(&p, t)
                .filter(|v| v.flip)
                .map(|v| (n, v.converted))
        })
    };
    println!("{name} mid-word: min  thr   fixed  fixed(freq)  wrong  wrong(freq)");
    for min_chars in [3usize, 4, 5] {
        for thr in [1.5f32, 2.0, 2.5, 3.0, 4.0] {
            let t = Thresholds {
                to_thai: thr,
                to_english: thr,
                min_chars,
            };
            let (mut hit, mut hit_w, mut all_w) = (0usize, 0.0, 0.0);
            for c in positives {
                all_w += c.weight;
                if let Some((n, conv)) = first_flip(&c.typed, t)
                    && c.meant.chars().take(n).collect::<String>() == conv
                {
                    hit += 1;
                    hit_w += c.weight;
                }
            }
            let (mut bad, mut bad_w, mut neg_w) = (0usize, 0.0, 0.0);
            let mut examples = Vec::new();
            for c in negatives {
                neg_w += c.weight;
                if let Some((n, conv)) = first_flip(&c.typed, t) {
                    bad += 1;
                    bad_w += c.weight;
                    if examples.len() < 8 {
                        let p: String = c.typed.chars().take(n).collect();
                        examples.push(format!("{}:{p}→{conv}", c.typed));
                    }
                }
            }
            println!(
                "              {min_chars}  {thr:.1}  {:6.2}%  {:6.2}%   {:6.3}%  {:7.4}%  {}",
                100.0 * hit as f64 / positives.len() as f64,
                100.0 * hit_w / all_w,
                100.0 * bad as f64 / negatives.len() as f64,
                100.0 * bad_w / neg_w,
                examples.join(" ")
            );
        }
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
