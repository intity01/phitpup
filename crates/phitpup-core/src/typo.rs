//! Slips that have nothing to do with the keyboard layout: a word typed
//! with Caps Lock left on, and the same word typed twice.

/// "hELLO" → "Hello": Caps Lock was on and Shift was held for the first
/// letter. Latin letters only, at least three, the first lowercase and all
/// the others uppercase. Words in all capitals are left alone; those are
/// usually meant.
pub fn caps(word: &str) -> Option<String> {
    let mut chars = word.chars();
    let first = chars.next()?;
    let rest: Vec<char> = chars.collect();
    let slip =
        first.is_ascii_lowercase() && rest.len() >= 2 && rest.iter().all(char::is_ascii_uppercase);
    slip.then(|| {
        std::iter::once(first.to_ascii_uppercase())
            .chain(rest.iter().map(char::to_ascii_lowercase))
            .collect()
    })
}

/// English words that are often right twice in a row ("had had", "that
/// that", "bye bye").
const FINE_TWICE: &[&str] = &[
    "that", "had", "is", "do", "no", "so", "very", "bye", "ha", "haha", "hah", "blah", "bla",
    "knock", "yada", "tut", "pew", "boo", "bang", "hip", "chop", "oh", "yeah", "yes", "go", "now",
    "well", "there", "far", "out", "more", "on", "round", "over", "again", "and",
];

/// "the the" → "the": the last two words before the caret are the same
/// (ignoring case), Latin letters, one space apart. Returns how many
/// characters from the end the pair takes up and what to put instead (the
/// first copy). Thai is left alone: doubled Thai words are often right
/// (ที่ที่, เด็กเด็ก).
pub fn doubled(text: &[char]) -> Option<(usize, String)> {
    let n = text.len();
    let second = text
        .iter()
        .rposition(|c| !c.is_ascii_alphabetic())
        .map_or(0, |i| i + 1);
    if second == n || second == 0 || text[second - 1] != ' ' {
        return None;
    }
    let gap = second - 1;
    let first = text[..gap]
        .iter()
        .rposition(|c| !c.is_ascii_alphabetic())
        .map_or(0, |i| i + 1);
    if first == gap || first > 0 && !text[first - 1].is_whitespace() {
        return None;
    }
    let a: String = text[first..gap].iter().collect();
    let b: String = text[second..].iter().collect();
    let same = a.eq_ignore_ascii_case(&b) && !FINE_TWICE.contains(&a.to_ascii_lowercase().as_str());
    same.then(|| (n - first, a))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_lock_slips() {
        assert_eq!(caps("hELLO").as_deref(), Some("Hello"));
        assert_eq!(caps("tHE").as_deref(), Some("The"));
        for w in ["HELLO", "Hello", "hello", "iPHONE2", "hE", "eBay", "ผิด", ""] {
            assert_eq!(caps(w), None, "{w}");
        }
    }

    #[test]
    fn doubled_words() {
        let t = |s: &str| doubled(&s.chars().collect::<Vec<_>>());
        assert_eq!(t("I saw the the"), Some((7, "the".into())));
        assert_eq!(t("The the"), Some((7, "The".into())));
        assert_eq!(t("a a"), Some((3, "a".into())));
        for s in [
            "the then",
            "the  the",
            "x-the the",
            "had had",
            "bye bye",
            "the",
            "ที่ที่",
            "the the ",
        ] {
            assert_eq!(t(s), None, "{s}");
        }
    }
}
