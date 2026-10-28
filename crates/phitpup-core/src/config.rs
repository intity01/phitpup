//! The settings file, `config.toml`.
//!
//! Only a flat subset of TOML is needed: `key = value` lines whose values are
//! strings, booleans or one-line string arrays. Anything else is reported and
//! skipped, so a typo never stops the program; the setting keeps its default.

/// When PhitPup offers fixes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Only on Shift+Backspace.
    Manual,
    /// Also a bubble while typing; Tab accepts it.
    Suggest,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Manual => "manual",
            Mode::Suggest => "suggest",
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
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mode: Mode::Manual,
            off_in_fullscreen: true,
            off_apps: Vec::new(),
            quiet_apps: Vec::new(),
        }
    }
}

/// Written when there is no settings file yet.
pub const TEMPLATE: &str = r#"# ผิดปุ๊บ PhitPup settings. The tray menu saves its changes here too.
# Saved changes apply within a few seconds; no restart needed.

# "manual"  : fix only when you press Shift+Backspace
# "suggest" : also show a bubble while you type; Tab accepts, Esc hides
mode = "manual"

# Turn PhitPup off in windows that cover the whole screen (games, videos).
off_in_fullscreen = true

# Programs where PhitPup does nothing, by exe name, e.g. ["game.exe"].
# Password managers and remote desktop apps are already included,
# and PhitPup is always off in password fields.
off_apps = []

# Programs where only the bubble is off (Shift+Backspace still works).
# Terminals and code editors are already included.
quiet_apps = []
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
                    Some("auto") => {
                        problems.push(format!(
                            "{at}: auto mode is not available yet; using suggest"
                        ));
                        Some(Mode::Suggest)
                    }
                    _ => None,
                }
                .map(|m| c.mode = m),
                "off_in_fullscreen" => boolean(value).map(|b| c.off_in_fullscreen = b),
                "off_apps" => apps(value).map(|a| c.off_apps = a),
                "quiet_apps" => apps(value).map(|a| c.quiet_apps = a),
                _ => {
                    problems.push(format!("{at}: unknown setting `{key}`"));
                    continue;
                }
            };
            if ok.is_none() {
                problems.push(format!("{at}: invalid value for `{key}`"));
            }
        }
        (c, problems)
    }
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

/// A one-line array of exe names, lowercased.
fn apps(v: &str) -> Option<Vec<String>> {
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
            "mode = suggest\noff_in_fullscreen = yes\nhotkey = \"f12\"\n[section]\noff_apps = [game.exe]\n",
        );
        assert_eq!(c, Config::default());
        assert_eq!(problems.len(), 5, "{problems:?}");
        assert!(problems[2].contains("unknown setting `hotkey`"));
    }

    #[test]
    fn auto_falls_back_to_suggest() {
        let (c, problems) = Config::parse("mode = \"auto\"");
        assert_eq!(c.mode, Mode::Suggest);
        assert_eq!(problems.len(), 1);
    }

    #[test]
    fn set_replaces_only_the_value() {
        let text = set(TEMPLATE, "mode", "\"suggest\"");
        assert_eq!(Config::parse(&text).0.mode, Mode::Suggest);
        assert_eq!(text.lines().count(), TEMPLATE.lines().count());
        assert!(text.contains("# \"manual\"  : fix only"), "comments kept");

        let text = set("off_apps = []\n", "mode", "\"suggest\"");
        assert_eq!(text, "off_apps = []\nmode = \"suggest\"\n");
    }
}
