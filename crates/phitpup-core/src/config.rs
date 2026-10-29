//! The settings file, `config.toml`.
//!
//! Only a flat subset of TOML is needed: `key = value` lines whose values are
//! strings, booleans or one-line string arrays. Anything else is reported and
//! skipped, so a typo never stops the program; the setting keeps its default.

use crate::hotkey::Hotkey;

/// When PhitPup offers fixes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Only on Shift+Backspace.
    Manual,
    /// Also a bubble while typing; Tab accepts it.
    Suggest,
    /// Like suggest, and Space fixes the word just typed when the detector
    /// is sure ([`Thresholds::AUTO`](crate::Thresholds::AUTO)).
    Auto,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Manual => "manual",
            Mode::Suggest => "suggest",
            Mode::Auto => "auto",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub mode: Mode,
    /// Turn off in windows that cover the whole screen (games, videos).
    pub off_in_fullscreen: bool,
    /// Programs (exe names) where PhitPup does nothing at all.
    pub off_apps: Vec<String>,
    /// Programs where only the bubble is off; Shift+Backspace still works.
    pub quiet_apps: Vec<String>,
    /// Words (as typed) the bubble never suggests fixing, lowercased.
    pub ignore_words: Vec<String>,
    /// Words (as typed) fixed without asking: the user converted them with
    /// the hotkey and kept the result.
    pub learned_words: Vec<String>,
    /// Learn from corrections: an undone fix adds the word to
    /// `ignore_words`, a kept hotkey fix to `learned_words`.
    pub learn: bool,
    /// Show suggestions as a wavy line under the word instead of a bubble,
    /// in apps that say where the word is.
    pub underline: bool,
    /// Also offer to fix Thai typed in a look-alike order (เเ for แ, ํา for
    /// ำ, a mark typed twice, a tone mark before its vowel).
    pub spelling: bool,
    /// Also offer to fix a word typed with Caps Lock left on (hELLO) and
    /// the same word typed twice (the the).
    pub typos: bool,
    /// Also offer the common word a slip of the finger turned into
    /// something else (ส้วสดี for สวัสดี, teh for the). Suggested only,
    /// never automatic.
    pub slips: bool,
    /// Show the bubble above the line instead of under the caret.
    pub bubble_above: bool,
    /// Tapping Shift twice also fixes the last word.
    pub double_shift: bool,
    /// A tray reminder to look for a new version once this one is old.
    pub remind_updates: bool,
    /// Fixes the last word.
    pub hotkey: Hotkey,
    /// Accepts the suggestion.
    pub accept_key: Hotkey,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mode: Mode::Suggest,
            off_in_fullscreen: true,
            off_apps: Vec::new(),
            quiet_apps: Vec::new(),
            ignore_words: Vec::new(),
            learned_words: Vec::new(),
            learn: true,
            underline: false,
            spelling: true,
            typos: true,
            slips: true,
            bubble_above: false,
            double_shift: false,
            remind_updates: true,
            hotkey: Hotkey::FIX,
            accept_key: Hotkey::ACCEPT,
        }
    }
}

/// Written when there is no settings file yet.
pub const TEMPLATE: &str = r#"# ผิดปุ๊บ PhitPup settings. The tray menu saves its changes here too.
# Saved changes apply within a few seconds; no restart needed.

# "suggest" : show a bubble while you type; Tab accepts, Esc hides
#             (Shift+Backspace works too)
# "manual"  : fix only when you press Shift+Backspace
# "auto"    : like suggest, and Space fixes the word when PhitPup is sure;
#             Shift+Backspace right after puts it back
mode = "suggest"

# Mark the word with a wavy underline instead of the bubble. Apps that do not
# say where the word is (some games and chat apps) still get the bubble.
underline = false

# Also offer to fix Thai typed in the wrong order that looks right:
# เเ for แ, ํ า for ำ, a mark typed twice, a tone mark before its vowel.
spelling = true

# Also offer to fix a word typed with Caps Lock left on (hELLO -> Hello)
# and the same word typed twice (the the -> the).
typos = true

# Also offer the common word that a slip of the finger turned into something
# else (a key next to the right one, two keys swapped, one too many or one
# missing: ส้วสดี -> สวัสดี, teh -> the). Suggested only, never fixed by itself.
slips = true

# Show the bubble above the line, so it does not cover lists that open under
# the text (search suggestions, the address bar).
bubble_above = false

# Turn PhitPup off in windows that cover the whole screen (games, videos).
off_in_fullscreen = true

# Programs where PhitPup does nothing, by exe name, e.g. ["game.exe"].
# Password managers and remote desktop apps are already included,
# and PhitPup is always off in password fields.
off_apps = []

# Programs where only the bubble is off (Shift+Backspace still works).
# Terminals and code editors are already included.
quiet_apps = []

# Words the bubble never offers to fix, as you type them, e.g. ["gdp"].
# Shift+Esc on the bubble adds the word it shows.
ignore_words = []

# Learn from corrections: undoing a fix right away adds the word to
# ignore_words; converting a word with the hotkey and keeping it adds it to
# learned_words, which are then fixed without asking.
learn = true
learned_words = []

# Shortcuts: modifiers ctrl, shift, alt, then a key such as backspace, tab,
# space, enter, insert, f1-f24, a-z.
hotkey = "shift+backspace"
accept_key = "tab"

# Tapping Shift twice quickly also fixes the last word, like hotkey.
double_shift = false

# Once this version is two months old, a tray note now and then suggests
# looking for a newer one. PhitPup never checks online itself.
remind_updates = true
"#;

impl Config {
    /// Reads `text`, keeping the default for anything missing or invalid.
    /// The second value lists what was skipped, for the user to fix.
    pub fn parse(text: &str) -> (Config, Vec<String>) {
        let mut c = Config::default();
        let mut problems = Vec::new();
        for (n, line) in text.lines().enumerate() {
            let line = strip_comment(line).trim();
            if line.is_empty() {
                continue;
            }
            let at = format!("line {}", n + 1);
            let Some((key, value)) = line.split_once('=') else {
                problems.push(format!("{at}: expected key = value"));
                continue;
            };
            let (key, value) = (key.trim(), value.trim());
            let ok = match key {
                "mode" => match string(value).as_deref() {
                    Some("manual") => Some(Mode::Manual),
                    Some("suggest") => Some(Mode::Suggest),
                    Some("auto") => Some(Mode::Auto),
                    _ => None,
                }
                .map(|m| c.mode = m),
                "off_in_fullscreen" => boolean(value).map(|b| c.off_in_fullscreen = b),
                "off_apps" => list(value).map(|a| c.off_apps = a),
                "quiet_apps" => list(value).map(|a| c.quiet_apps = a),
                "ignore_words" => list(value).map(|w| c.ignore_words = w),
                "learned_words" => list(value).map(|w| c.learned_words = w),
                "learn" => boolean(value).map(|b| c.learn = b),
                "underline" => boolean(value).map(|b| c.underline = b),
                "spelling" => boolean(value).map(|b| c.spelling = b),
                "typos" => boolean(value).map(|b| c.typos = b),
                "slips" => boolean(value).map(|b| c.slips = b),
                "bubble_above" => boolean(value).map(|b| c.bubble_above = b),
                "double_shift" => boolean(value).map(|b| c.double_shift = b),
                "remind_updates" => boolean(value).map(|b| c.remind_updates = b),
                "hotkey" => shortcut(value).map(|k| c.hotkey = k),
                "accept_key" => shortcut(value).map(|k| c.accept_key = k),
                _ => {
                    problems.push(format!("{at}: unknown setting `{key}`"));
                    continue;
                }
            };
            if ok.is_none() {
                problems.push(format!("{at}: invalid value for `{key}`"));
            }
        }
        if c.hotkey == c.accept_key {
            problems.push("hotkey and accept_key are the same key; using the defaults".into());
            (c.hotkey, c.accept_key) = (Hotkey::FIX, Hotkey::ACCEPT);
        }
        (c, problems)
    }

    /// Returns `text` with every setting replaced by this config's value.
    pub fn write_to(&self, text: &str) -> String {
        let quoted = |s: &str| format!("\"{s}\"");
        let array = |v: &[String]| {
            format!(
                "[{}]",
                v.iter().map(|s| quoted(s)).collect::<Vec<_>>().join(", ")
            )
        };
        let mut t = set(text, "mode", &quoted(self.mode.as_str()));
        t = set(&t, "underline", &self.underline.to_string());
        t = set(&t, "spelling", &self.spelling.to_string());
        t = set(&t, "typos", &self.typos.to_string());
        t = set(&t, "slips", &self.slips.to_string());
        t = set(&t, "bubble_above", &self.bubble_above.to_string());
        t = set(&t, "off_in_fullscreen", &self.off_in_fullscreen.to_string());
        t = set(&t, "off_apps", &array(&self.off_apps));
        t = set(&t, "quiet_apps", &array(&self.quiet_apps));
        t = set(&t, "ignore_words", &array(&self.ignore_words));
        t = set(&t, "learn", &self.learn.to_string());
        t = set(&t, "learned_words", &array(&self.learned_words));
        t = set(&t, "hotkey", &quoted(&self.hotkey.to_string()));
        t = set(&t, "double_shift", &self.double_shift.to_string());
        t = set(&t, "remind_updates", &self.remind_updates.to_string());
        set(&t, "accept_key", &quoted(&self.accept_key.to_string()))
    }
}

impl Config {
    /// Leave `word` alone from now on (it moves out of the learned words).
    /// False if it was already there or cannot be stored.
    pub fn ignore(&mut self, word: &str) -> bool {
        let word = word.to_lowercase();
        self.learned_words.retain(|w| *w != word);
        if !storable(&word) || self.ignore_words.contains(&word) {
            return false;
        }
        self.ignore_words.push(word);
        true
    }

    /// Fix `word` without asking from now on (it moves out of the ignored
    /// words). False if it was already there or cannot be stored.
    pub fn learn_word(&mut self, word: &str) -> bool {
        let word = word.to_lowercase();
        self.ignore_words.retain(|w| *w != word);
        if !storable(&word) || word.chars().count() < 2 || self.learned_words.contains(&word) {
            return false;
        }
        self.learned_words.push(word);
        true
    }
}

/// Whether `s` can be stored in a settings list.
pub fn storable(s: &str) -> bool {
    !s.trim().is_empty() && !s.contains(['"', '\\', '\n', '\r'])
}

/// Returns `text` with `key` set to `value` (a TOML literal such as
/// `"suggest"`), leaving comments and other lines alone. Appends the key if
/// it is missing.
pub fn set(text: &str, key: &str, value: &str) -> String {
    let mut out = String::with_capacity(text.len() + key.len() + value.len() + 4);
    let mut found = false;
    for line in text.lines() {
        let is_key = strip_comment(line)
            .split_once('=')
            .is_some_and(|(k, _)| k.trim() == key);
        if is_key && !found {
            found = true;
            out.push_str(&format!("{key} = {value}"));
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    if !found {
        out.push_str(&format!("{key} = {value}\n"));
    }
    out
}

/// The part of `line` before a `#` that is not inside a string.
fn strip_comment(line: &str) -> &str {
    let mut quoted = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => quoted = !quoted,
            '#' if !quoted => return &line[..i],
            _ => {}
        }
    }
    line
}

fn string(v: &str) -> Option<String> {
    let inner = v.strip_prefix('"')?.strip_suffix('"')?;
    (!inner.contains('"')).then(|| inner.to_string())
}

fn boolean(v: &str) -> Option<bool> {
    match v {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn shortcut(v: &str) -> Option<Hotkey> {
    Hotkey::parse(&string(v)?).filter(|k| k.problem().is_none())
}

/// A one-line array of strings (exe names, words), lowercased.
fn list(v: &str) -> Option<Vec<String>> {
    let inner = v.strip_prefix('[')?.strip_suffix(']')?;
    inner
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| string(s).map(|s| s.to_lowercase()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_is_the_default() {
        assert_eq!(Config::parse(TEMPLATE), (Config::default(), vec![]));
    }

    #[test]
    fn reads_every_setting() {
        let (c, problems) = Config::parse(
            "mode = \"suggest\" # bubble\noff_in_fullscreen=false\n\
             off_apps = [\"Game.exe\", \"x # y.exe\",]\nquiet_apps = []\n",
        );
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(c.mode, Mode::Suggest);
        assert!(!c.off_in_fullscreen);
        assert_eq!(c.off_apps, ["game.exe", "x # y.exe"]);
        assert!(c.quiet_apps.is_empty());
    }

    #[test]
    fn bad_lines_keep_defaults_and_are_reported() {
        let (c, problems) = Config::parse(
            "mode = suggest\noff_in_fullscreen = yes\ncolour = \"red\"\n[section]\noff_apps = [game.exe]\n",
        );
        assert_eq!(c, Config::default());
        assert_eq!(problems.len(), 5, "{problems:?}");
        assert!(problems[2].contains("unknown setting `colour`"));
    }

    #[test]
    fn reads_keys_and_words() {
        let (c, problems) = Config::parse(
            "hotkey = \"ctrl+shift+space\"
accept_key = \"right\"
ignore_words = [\"GDP\", \"แหก\"]
",
        );
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(c.hotkey, Hotkey::new(true, true, false, 0x20));
        assert_eq!(c.accept_key.vk, 0x27);
        assert_eq!(c.ignore_words, ["gdp", "แหก"]);

        let (c, problems) = Config::parse(
            "hotkey = \"shift+a\"
accept_key = \"shift+backspace\"",
        );
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert_eq!((c.hotkey, c.accept_key), (Hotkey::FIX, Hotkey::ACCEPT));
    }

    #[test]
    fn write_to_round_trips() {
        let c = Config {
            mode: Mode::Manual,
            off_in_fullscreen: false,
            off_apps: vec!["game.exe".into()],
            quiet_apps: vec!["notes.exe".into(), "x # y.exe".into()],
            ignore_words: vec!["gdp".into()],
            learned_words: vec!["muj".into()],
            learn: false,
            underline: true,
            spelling: false,
            typos: false,
            slips: false,
            bubble_above: true,
            double_shift: true,
            remind_updates: false,
            hotkey: Hotkey::parse("ctrl+backslash").unwrap(),
            accept_key: Hotkey::ACCEPT,
        };
        let text = c.write_to(TEMPLATE);
        assert_eq!(Config::parse(&text), (c, vec![]));
        assert_eq!(text.lines().count(), TEMPLATE.lines().count());
        assert!(!storable("a\"b") && !storable(" ") && storable("แหก"));
    }

    #[test]
    fn reads_auto_mode() {
        let (c, problems) = Config::parse("mode = \"auto\"");
        assert_eq!(c.mode, Mode::Auto);
        assert!(problems.is_empty());
        let text = Config {
            mode: Mode::Auto,
            ..Config::default()
        }
        .write_to(TEMPLATE);
        assert_eq!(Config::parse(&text).0.mode, Mode::Auto);
    }

    #[test]
    fn set_replaces_only_the_value() {
        let text = set(TEMPLATE, "mode", "\"manual\"");
        assert_eq!(Config::parse(&text).0.mode, Mode::Manual);
        assert_eq!(text.lines().count(), TEMPLATE.lines().count());
        assert!(text.contains("# \"manual\"  : fix only"), "comments kept");

        let text = set("off_apps = []\n", "mode", "\"suggest\"");
        assert_eq!(text, "off_apps = []\nmode = \"suggest\"\n");
    }
}
