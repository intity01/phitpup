//! Thai orthography rules.
//!
//! Thai marks and some vowels can only follow certain characters (a tone mark
//! never starts a word, สระอิ needs a consonant before it, and so on). English
//! typed on a Thai layout breaks these rules almost immediately, e.g. "hello" on
//! Kedmanee is "้ำสสน", which starts with a tone mark. That makes the rules a
//! cheap, strong signal in both directions.

/// What kind of character, for the purpose of the rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    /// ก–ฮ, including ฤ and ฦ.
    Consonant,
    /// เ แ โ ใ ไ: written before the consonant they follow in speech.
    Leading,
    /// ะ า ำ
    Following,
    /// ั ิ ี ึ ื ุ ู ฺ
    AboveBelow,
    /// ่ ้ ๊ ๋
    Tone,
    /// ็
    Maitaikhu,
    /// ์
    Thanthakhat,
    /// ํ ๎
    Nikhahit,
    /// ๅ: only after ฤ or ฦ.
    Lakkhangyao,
    /// ๆ ฯ ๏ ฿ Thai digits, ๚ ๛
    Sign,
    /// Anything outside the Thai block.
    Other,
}

pub fn class(c: char) -> Class {
    match c {
        '\u{0E01}'..='\u{0E2E}' => Class::Consonant,
        '\u{0E40}'..='\u{0E44}' => Class::Leading,
        '\u{0E30}' | '\u{0E32}' | '\u{0E33}' => Class::Following,
        '\u{0E31}' | '\u{0E34}'..='\u{0E3A}' => Class::AboveBelow,
        '\u{0E48}'..='\u{0E4B}' => Class::Tone,
        '\u{0E47}' => Class::Maitaikhu,
        '\u{0E4C}' => Class::Thanthakhat,
        '\u{0E4D}' | '\u{0E4E}' => Class::Nikhahit,
        '\u{0E45}' => Class::Lakkhangyao,
        '\u{0E2F}' | '\u{0E3F}' | '\u{0E46}' | '\u{0E4F}'..='\u{0E5B}' => Class::Sign,
        _ => Class::Other,
    }
}

pub fn is_thai(c: char) -> bool {
    class(c) != Class::Other
}

/// The first rule `word` breaks, as (char index, char).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Violation {
    pub at: usize,
    pub ch: char,
}

/// Checks `word` against Thai spelling rules. Non-Thai characters (spaces,
/// punctuation, Latin) reset the context like a word boundary.
pub fn check(word: &str) -> Result<(), Violation> {
    use Class::*;
    let chars: Vec<char> = word.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        let prev = i.checked_sub(1).map(|j| chars[j]);
        let p = prev.map(class).unwrap_or(Other);
        // Chat elongation (นะะะ, ค่าาา, ๆๆ): repeating a following vowel or ๆ
        // is fine once the first one was.
        if prev == Some(c) && matches!(c, '\u{0E30}' | '\u{0E32}' | '\u{0E46}') {
            continue;
        }
        let ok = match class(c) {
            Consonant | Other => true,
            // A leading vowel must be followed by a consonant. เเ is a common
            // way to type แ, so it is allowed too.
            Leading => match chars.get(i + 1).map(|&n| class(n)) {
                Some(Consonant) => true,
                Some(Leading) => c == '\u{0E40}' && chars[i + 1] == '\u{0E40}',
                _ => false,
            },
            AboveBelow | Maitaikhu => p == Consonant,
            Tone => matches!(p, Consonant) || (p == AboveBelow && prev != Some('\u{0E3A}')),
            Thanthakhat => p == Consonant || matches!(prev, Some('\u{0E34}' | '\u{0E38}')),
            Nikhahit => matches!(p, Consonant | Tone),
            Following => match c {
                // ะ: กะ, ก้ะ, เกาะ
                '\u{0E30}' => matches!(p, Consonant | Tone) || prev == Some('\u{0E32}'),
                // า: กา, ก้า, and ํ+า written as two characters
                '\u{0E32}' => matches!(p, Consonant | Tone | Nikhahit),
                // ำ: ทำ, น้ำ
                _ => matches!(p, Consonant | Tone),
            },
            Lakkhangyao => matches!(prev, Some('\u{0E24}' | '\u{0E26}')),
            // ฯ can open a word (ฯพณฯ); ๆ needs something to repeat.
            Sign => c != '\u{0E46}' || i > 0 && p != Other,
        };
        if !ok {
            return Err(Violation { at: i, ch: c });
        }
    }
    Ok(())
}

/// The same word with its Thai characters in the right order, if it was
/// typed in a way that looks the same but is not: two เ for แ, ํ and า for ำ,
/// a mark typed twice, or a tone mark before the vowel it sits on. Search,
/// sorting and spell checkers treat those as different words. `None` when
/// nothing needs changing, or when the result would still break the rules.
pub fn respell(word: &str) -> Option<String> {
    use Class::*;
    const E: char = '\u{0E40}'; // เ
    const AE: char = '\u{0E41}'; // แ
    const NIKHAHIT: char = '\u{0E4D}'; // ํ
    const AA: char = '\u{0E32}'; // า
    const AM: char = '\u{0E33}'; // ำ
    const PHINTHU: char = '\u{0E3A}'; // ฺ
    let chars: Vec<char> = word.chars().collect();
    let mut out: Vec<char> = Vec::with_capacity(chars.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if c == E && next == Some(E) {
            out.push(AE);
            i += 2;
            continue;
        }
        if c == NIKHAHIT {
            if next == Some(AA) {
                out.push(AM);
                i += 2;
                continue;
            }
            // น ํ ้ า: the tone mark goes before ำ.
            if next.map(class) == Some(Tone) && chars.get(i + 2) == Some(&AA) {
                out.extend([chars[i + 1], AM]);
                i += 3;
                continue;
            }
        }
        let mark = matches!(
            class(c),
            Leading | AboveBelow | Tone | Maitaikhu | Thanthakhat | Nikhahit
        );
        if mark && out.last() == Some(&c) {
            i += 1; // typed twice
            continue;
        }
        let n = out.len();
        if class(c) == AboveBelow
            && c != PHINTHU
            && n >= 2
            && class(out[n - 1]) == Tone
            && class(out[n - 2]) == Consonant
        {
            out.insert(n - 1, c); // ก่ี → กี่
            i += 1;
            continue;
        }
        out.push(c);
        i += 1;
    }
    let fixed: String = out.into_iter().collect();
    (fixed != word && check(&fixed).is_ok()).then_some(fixed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn respells_look_alike_typing() {
        for (typed, fixed) in [
            ("เเม่", "แม่"),
            ("เเละ", "และ"),
            ("กํา", "กำ"),
            ("ทํางาน", "ทำงาน"),
            ("นํ้า", "น้ำ"),
            ("ไม่่", "ไม่"),
            ("ดีี", "ดี"),
            ("ก่ี", "กี่"),
            ("ที่่", "ที่"),
            ("แแม่", "แม่"),
        ] {
            assert_eq!(respell(typed).as_deref(), Some(fixed), "{typed}");
        }
    }

    #[test]
    fn leaves_correct_and_unclear_words_alone() {
        for w in [
            "แม่",
            "น้ำ",
            "กี่",
            "สวัสดี",
            "นะะะ",
            "ค่าาา",
            "hello",
            "ก่้",
            "้ำสสน",
            "",
        ] {
            assert_eq!(respell(w), None, "{w}");
        }
    }

    #[test]
    fn accepts_real_words() {
        for w in [
            "สวัสดี",
            "ครับ",
            "น้ำ",
            "ทำ",
            "เกาะ",
            "แม่",
            "เเม่",
            "ก็",
            "จันทร์",
            "ศิลป์",
            "ฤๅษี",
            "ไป",
            "ใหม่",
            "ฯพณฯ",
            "เด็กๆ",
            "ปี๊บ",
            "กํา",
            "ภาษาไทย",
            "พระลักษมณ์",
            "ต่ำ",
            "นะะะ",
            "ค่าาา",
            "ดีๆๆ",
        ] {
            assert_eq!(check(w), Ok(()), "{w}");
        }
    }

    #[test]
    fn rejects_english_typed_on_kedmanee() {
        assert!(check("้ำสสน").is_err()); // "hello": starts with a tone mark
        assert!(check("ะ้ำ").is_err()); // "the": starts with ะ
        assert!(check("ัก").is_err());
        assert!(check("กเ").is_err()); // leading vowel at the end
        assert!(check("กิี").is_err()); // two vowels stacked
        assert!(check("ๆก").is_err());
        assert!(check("กาะ").is_ok()); // ะ after า is a real pattern (เกาะ)
        assert!(check("ก่่").is_err()); // doubled tone mark is a typo, not elongation
        assert!(check("ๅ").is_err());
    }

    #[test]
    fn non_thai_resets_context() {
        assert_eq!(check("ครับ."), Ok(()));
        assert!(check("ก ่").is_err());
    }
}
