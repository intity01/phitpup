//! What `phitpup-update.exe` and the app agree on: where the newest version
//! is described, how to read that description, and the lines the helper
//! prints for the app to show.
//!
//! The app itself never connects to anything. When the person clicks "Check
//! for updates", it starts the helper, which fetches [`RELEASE_JSON`] (the
//! file the website workflow writes) and, if asked, the installer.

/// Written by `.github/workflows/pages.yml` for each release.
pub const RELEASE_HOST: &str = "intity01.github.io";
pub const RELEASE_JSON: &str = "/phitpup/release.json";
pub const DOWNLOAD_HOST: &str = "github.com";

/// Where the installer for `version` is published.
pub fn setup_path(version: &str) -> String {
    format!("/intity01/phitpup/releases/download/v{version}/PhitPup-Setup.exe")
}

/// The newest release, as `release.json` describes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    /// The installer's SHA-256 (lowercase hex) and size in bytes, absent
    /// for releases made before there was an installer.
    pub setup: Option<(String, u64)>,
}

/// Reads `release.json`. It is our own small, fixed format, so this looks
/// for the few keys it needs instead of parsing JSON in general.
pub fn parse(json: &str) -> Option<Release> {
    let version = string_after(json, "\"version\"")?;
    parts(&version)?;
    let setup = json.find("\"setup\"").and_then(|at| {
        let rest = &json[at..];
        let sha = string_after(rest, "\"sha256\"")?;
        let size = number_after(rest, "\"size\"")?;
        (sha.len() == 64 && sha.bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| (sha.to_ascii_lowercase(), size))
    });
    Some(Release { version, setup })
}

fn value_after<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let rest = &json[json.find(key)? + key.len()..];
    Some(rest.trim_start().strip_prefix(':')?.trim_start())
}

fn string_after(json: &str, key: &str) -> Option<String> {
    let rest = value_after(json, key)?.strip_prefix('"')?;
    Some(rest[..rest.find('"')?].to_string())
}

fn number_after(json: &str, key: &str) -> Option<u64> {
    let rest = value_after(json, key)?;
    let end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

fn parts(version: &str) -> Option<(u32, u32, u32)> {
    let mut it = version.split('.').map(str::parse::<u32>);
    let v = (it.next()?.ok()?, it.next()?.ok()?, it.next()?.ok()?);
    it.next().is_none().then_some(v)
}

/// Whether `latest` is a newer version than `current`.
pub fn newer(latest: &str, current: &str) -> bool {
    match (parts(latest), parts(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

/// One line the helper prints on its standard output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Line {
    /// `check`: this version is the newest.
    Current,
    /// `check`: a newer version with an installer, and its size.
    Available(String, u64),
    /// `install`: bytes downloaded so far, of the total.
    Progress(u64, u64),
    /// `install`: the installer checked out and has been started.
    Started,
    /// Something went wrong; a short English sentence for the person.
    Failed(String),
}

impl Line {
    pub fn read(s: &str) -> Option<Line> {
        let mut words = s.trim().splitn(3, ' ');
        let n = |w: Option<&str>| w.and_then(|w| w.parse::<u64>().ok());
        Some(match words.next()? {
            "current" => Line::Current,
            "available" => Line::Available(words.next()?.to_string(), n(words.next())?),
            "progress" => {
                let (done, total) = (n(words.next()), words.next().and_then(|w| w.parse().ok()));
                Line::Progress(done?, total?)
            }
            "started" => Line::Started,
            "failed" => Line::Failed(s.trim()["failed".len()..].trim().to_string()),
            _ => return None,
        })
    }
}

impl std::fmt::Display for Line {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Line::Current => write!(f, "current"),
            Line::Available(v, size) => write!(f, "available {v} {size}"),
            Line::Progress(done, total) => write!(f, "progress {done} {total}"),
            Line::Started => write!(f, "started"),
            Line::Failed(why) => write!(f, "failed {why}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "10db036757c8ebcd6af7fab09c5a3551c9e0349196f3834da9cc8a06d026f024";

    #[test]
    fn reads_release_json() {
        let json = format!(
            r#"{{"version":"0.3.1","sha256":"{}","size":1143296,"setup":{{"sha256":"{SHA}","size":2507332}}}}"#,
            "7".repeat(64)
        );
        let r = parse(&json).unwrap();
        assert_eq!(r.version, "0.3.1");
        assert_eq!(r.setup, Some((SHA.to_string(), 2_507_332)));
    }

    #[test]
    fn older_releases_have_no_setup() {
        let r = parse(r#"{"version":"0.3.0","sha256":"ab","size":12}"#).unwrap();
        assert_eq!(r.setup, None);
        assert!(parse(r#"{"version":"soon"}"#).is_none());
        assert!(parse("<html>").is_none());
    }

    #[test]
    fn compares_versions_by_number() {
        assert!(newer("0.3.10", "0.3.9"));
        assert!(newer("1.0.0", "0.9.9"));
        assert!(!newer("0.3.1", "0.3.1"));
        assert!(!newer("0.3.0", "0.3.1"));
        assert!(!newer("x", "0.3.1"));
    }

    #[test]
    fn lines_round_trip() {
        for line in [
            Line::Current,
            Line::Available("0.4.0".into(), 2_500_000),
            Line::Progress(10, 20),
            Line::Started,
            Line::Failed("No internet connection.".into()),
        ] {
            assert_eq!(Line::read(&line.to_string()), Some(line));
        }
        assert_eq!(Line::read("hello"), None);
    }
}
