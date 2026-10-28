use phitpup_core::{Converter, Layout};

fn en_th(th: &Layout) -> (Converter, Converter) {
    let en = Layout::us_qwerty();
    (Converter::new(&en, th), Converter::new(th, &en))
}

#[test]
fn tables_cover_all_47_keys() {
    for layout in [
        Layout::us_qwerty(),
        Layout::kedmanee(),
        Layout::pattachote(),
    ] {
        assert_eq!(layout.keys().len(), 47, "{}", layout.name());
        let missing: Vec<_> = layout
            .keys()
            .iter()
            .filter(|k| k.base.is_none())
            .map(|k| k.sc)
            .collect();
        // Pattachote's `\` key has no standard Unicode character on Windows.
        let expected: &[u16] = if layout.name() == "Thai Pattachote" {
            &[0x2B]
        } else {
            &[]
        };
        assert_eq!(missing, expected, "{}", layout.name());
    }
}

#[test]
fn duplicate_characters_are_known() {
    assert!(Layout::us_qwerty().duplicates().is_empty());
    assert!(Layout::kedmanee().duplicates().is_empty());
    assert_eq!(Layout::pattachote().duplicates(), vec!['_', '\u{0E31}']);
}

#[test]
fn kedmanee_known_words() {
    let (to_th, to_en) = en_th(&Layout::kedmanee());
    assert_eq!(to_th.convert("l;ylfu"), "สวัสดี");
    assert_eq!(to_th.convert("8iy["), "ครับ");
    assert_eq!(to_th.convert("l;ylfu8iy["), "สวัสดีครับ");
    assert_eq!(to_th.convert("wx]'"), "ไปลง");
    assert_eq!(to_en.convert("แนพพำแะ"), "correct");
    assert_eq!(to_en.convert("้ำสสน"), "hello");
    assert_eq!(to_en.convert("ไนพสก ้ำสสน"), "world hello");
}

#[test]
fn pattachote_known_words() {
    let (to_th, to_en) = en_th(&Layout::pattachote());
    // ส=m ว=o ั=g ส=m ด=u ี=h
    assert_eq!(to_th.convert("mogmuh"), "สวัสดี");
    assert_eq!(to_en.convert("สวัสดี"), "mogmuh");
}

#[test]
fn round_trip_every_key() {
    let en = Layout::us_qwerty();
    for th in [Layout::kedmanee(), Layout::pattachote()] {
        let (to_th, to_en) = en_th(&th);
        let dups = th.duplicates();
        for (sc, shift, c) in en.chars() {
            let t = to_th.convert_char(c);
            // Keys whose Thai char also lives on another key can't come back.
            if dups.contains(&t) || !to_th.maps(c) {
                continue;
            }
            assert_eq!(
                to_en.convert_char(t),
                c,
                "{} sc={sc:#04x} shift={shift}",
                th.name()
            );
        }
        for (sc, shift, c) in th.chars() {
            if dups.contains(&c) {
                continue;
            }
            let e = to_en.convert_char(c);
            assert_eq!(
                to_th.convert_char(e),
                c,
                "{} sc={sc:#04x} shift={shift}",
                th.name()
            );
        }
    }
}

#[test]
fn round_trip_sentence() {
    let (to_th, to_en) = en_th(&Layout::kedmanee());
    let text = "The quick brown fox, jumps over 13 lazy dogs!";
    assert_eq!(to_en.convert(&to_th.convert(text)), text);
}

#[test]
fn duplicate_prefers_unshifted_key() {
    let (_, to_en) = en_th(&Layout::pattachote());
    assert_eq!(to_en.convert_char('_'), '`'); // not Shift+7 '&'
    assert_eq!(to_en.convert_char('\u{0E31}'), 'g'); // not Shift+B 'B'
}

#[test]
fn unmapped_characters_pass_through() {
    let (to_th, to_en) = en_th(&Layout::kedmanee());
    for s in [" ", "\n", "\t", "😀", "é"] {
        assert_eq!(to_th.convert(s), s);
        assert_eq!(to_en.convert(s), s);
    }
    assert_eq!(to_th.convert("l;ylfu 😀\nl;ylfu"), "สวัสดี 😀\nสวัสดี");
}

#[test]
fn runtime_layout_from_keys() {
    use phitpup_core::Key;
    let tiny = Layout::new(
        "tiny",
        vec![Key {
            sc: 0x10,
            base: Some('ก'),
            shift: None,
        }],
    );
    let c = Converter::new(&Layout::us_qwerty(), &tiny);
    assert_eq!(c.convert("qQw"), "กQw");
}
