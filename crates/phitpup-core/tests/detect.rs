use phitpup_core::model::Model;
use phitpup_core::{Detector, Direction, Layout, Thresholds};

fn kedmanee(thresholds: Thresholds) -> Detector {
    let mut d = Detector::new(&Layout::us_qwerty(), &Layout::kedmanee());
    d.thresholds = thresholds;
    d
}

#[test]
fn embedded_model_is_trained() {
    assert!(
        Model::embedded().is_trained(),
        "run `cargo xtask train-model`"
    );
}

#[test]
fn flips_thai_typed_on_english() {
    let d = kedmanee(Thresholds::AUTO);
    assert_eq!(d.convert("สวัสดี", Direction::ToEnglish), "l;ylfu");
    for meant in [
        "สวัสดี",
        "ครับ",
        "ขอบคุณ",
        "ภาษาไทย",
        "อากาศ",
        "โปรแกรม",
        "วันนี้",
        "กินข้าว",
        "ไม่เป็นไร",
    ] {
        let typed = d.convert(meant, Direction::ToEnglish);
        let v = d.judge(&typed).unwrap();
        assert_eq!(v.direction, Direction::ToThai, "{typed}");
        assert!(v.flip, "{typed} → {} margin {:+.2}", v.converted, v.margin);
        assert_eq!(v.converted, meant);
    }
}

#[test]
fn pattachote_common_words_flip() {
    // Their Pattachote key sequences are English-looking ("shy" = ที่), so
    // only the frequency lexicon gets them over the AUTO threshold.
    let mut d = Detector::new(&Layout::us_qwerty(), &Layout::pattachote());
    d.thresholds = Thresholds::AUTO;
    for meant in ["ที่", "การ", "เป็น", "ไม่", "ของ", "สวัสดี"]
    {
        let typed = d.convert(meant, Direction::ToEnglish);
        let v = d.judge(&typed).unwrap();
        assert!(v.flip, "{typed} → {} margin {:+.2}", v.converted, v.margin);
        assert_eq!(v.converted, meant);
    }
}

#[test]
fn flips_english_typed_on_thai() {
    let d = kedmanee(Thresholds::AUTO);
    for (typed, meant) in [
        ("้ำสสน", "hello"),
        ("ไนพสก", "world"),
        ("แนพพำแะ", "correct"),
        ("ะ้ำ", "the"),
    ] {
        let v = d.judge(typed).unwrap();
        assert_eq!(v.direction, Direction::ToEnglish);
        assert!(v.flip, "{typed} → {} margin {:+.2}", v.converted, v.margin);
        assert_eq!(v.converted, meant);
    }
}

#[test]
fn keeps_correct_english() {
    let d = kedmanee(Thresholds::AUTO);
    for w in [
        "hello", "world", "the", "and", "code", "Thailand", "keyboard", "PhitPup", "don't", "OK",
        "email", "JSON",
    ] {
        let v = d.judge(w).unwrap();
        assert!(!v.flip, "{w} → {} margin {:+.2}", v.converted, v.margin);
    }
}

#[test]
fn keeps_correct_thai() {
    let d = kedmanee(Thresholds::AUTO);
    for w in [
        "สวัสดี",
        "ครับ",
        "ภาษาไทย",
        "ที่",
        "การ",
        "เป็น",
        "น้ำ",
        "ค่าาา",
        "โปรแกรม",
        "คอมพิวเตอร์",
    ] {
        let v = d.judge(w).unwrap();
        assert!(!v.flip, "{w} → {} margin {:+.2}", v.converted, v.margin);
    }
}

#[test]
fn numbers_stay() {
    let d = kedmanee(Thresholds::SUGGEST);
    for w in ["2026", "12.5", "10:30", "1/2", "-5%"] {
        let v = d.judge(w).unwrap();
        assert!(v.vetoed && !v.flip, "{w}");
    }
}

#[test]
fn single_english_words_typed_on_thai() {
    let d = kedmanee(Thresholds::SUGGEST);
    for (typed, meant) in [("ร", "i"), ("ฟ", "a")] {
        let v = d.judge(typed).unwrap();
        assert!(v.flip, "{typed}");
        assert_eq!(v.converted, meant);
    }
    assert!(!d.judge("ก").unwrap().flip, "d is not a word");
    assert!(!kedmanee(Thresholds::AUTO).judge("ร").unwrap().flip);
}

#[test]
fn nothing_to_judge() {
    let d = kedmanee(Thresholds::SUGGEST);
    assert!(d.judge("😀").is_none());
    assert!(d.judge("").is_none());
}

#[test]
fn selection_fixes_only_the_wrong_words() {
    let d = kedmanee(Thresholds::AUTO); // selection uses SUGGEST regardless
    assert_eq!(
        d.selection("hello l;ylfu"),
        Some((Direction::ToThai, "hello สวัสดี".into()))
    );
    assert_eq!(
        d.selection("ครับ ้ำสสน\nok"),
        Some((Direction::ToEnglish, "ครับ hello\nok".into()))
    );
    // Whole wrong sentence.
    assert_eq!(
        d.selection("l;ylfu 8iy["),
        Some((Direction::ToThai, "สวัสดี ครับ".into()))
    );
}

#[test]
fn selection_with_nothing_wrong_flips_everything() {
    let d = kedmanee(Thresholds::SUGGEST);
    assert_eq!(d.selection("hello world"), d.manual("hello world"));
    assert_eq!(d.selection(" \n "), None);
}

#[test]
fn manual_flips_by_script() {
    let d = kedmanee(Thresholds::SUGGEST);
    assert_eq!(d.manual("hello"), Some((Direction::ToThai, "้ำสสน".into())));
    assert_eq!(
        d.manual("l;ylfu 8iy["),
        Some((Direction::ToThai, "สวัสดี ครับ".into()))
    );
    assert_eq!(
        d.manual("้ำสสน"),
        Some((Direction::ToEnglish, "hello".into()))
    );
    assert_eq!(d.manual("   "), None);
}

#[test]
fn abbreviations_stay_english() {
    use phitpup_core::Converter;
    use phitpup_core::detect::ABBREVIATIONS;
    assert!(ABBREVIATIONS.is_sorted(), "kept sorted for the lookup");
    let thai: std::collections::HashSet<&str> = Model::embedded()
        .lang(phitpup_core::model::Lang::Thai)
        .lexicon()
        .map(|(w, _)| w)
        .collect();
    for layout in [Layout::kedmanee(), Layout::pattachote()] {
        let d = Detector::new(&Layout::us_qwerty(), &layout);
        let to_th = Converter::new(&Layout::us_qwerty(), &layout);
        for a in ABBREVIATIONS {
            let as_thai = to_th.convert(a);
            assert!(
                !thai.contains(as_thai.as_str()),
                "{a} spells the Thai word {as_thai}"
            );
            for w in [a.to_string(), a.to_uppercase(), format!("{a}s")] {
                for th in [Thresholds::SUGGEST, Thresholds::AUTO] {
                    let v = d.judge_with(&w, th);
                    assert!(!v.is_some_and(|v| v.flip), "{w}");
                }
            }
        }
    }
    // Thai typed on the English layout is still caught.
    let d = kedmanee(Thresholds::SUGGEST);
    assert!(d.judge("l;ylfu").unwrap().flip);
}
