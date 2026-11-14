//! Whether PhitPup crashed, from Windows' own Application log.
//!
//! Windows writes an "Application Error" event whenever a program crashes.
//! PhitPup reads its own ones back, so that the next start can offer to
//! report the crash and "Report a problem" can say where it happened. Only
//! the version, the exception code, the module and the offset in it are
//! taken; the event also names the computer and the user, which are left
//! alone. Nothing is sent anywhere.

use windows_sys::Win32::System::EventLog::{
    EVT_HANDLE, EvtClose, EvtNext, EvtQuery, EvtQueryChannelPath, EvtQueryReverseDirection,
    EvtRender, EvtRenderEventXml,
};

use crate::settings;

/// One crash of PhitPup, as Windows logged it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Crash {
    /// When, in UTC: `2026-11-13T10:19:21.1861130Z`. Sorts as text.
    pub time: String,
    /// `0.4.4.0`
    pub version: String,
    /// `c0000005`
    pub code: String,
    pub module: String,
    /// Hex, without leading zeros.
    pub offset: String,
    /// Which run it was, to pair the two events one crash can leave.
    process: String,
}

impl Crash {
    /// A line for a problem report.
    pub fn describe(&self) -> String {
        format!(
            "{} crashed with 0x{} at {}+0x{} ({} UTC)",
            self.version,
            self.code,
            self.module,
            self.offset,
            self.time.get(..16).unwrap_or(&self.time).replace('T', " "),
        )
    }

    /// Whether it was this version of PhitPup.
    pub fn this_version(&self) -> bool {
        self.version
            .strip_prefix(env!("CARGO_PKG_VERSION"))
            .is_some_and(|rest| rest.is_empty() || rest == ".0")
    }
}

/// PhitPup's crashes in the last 30 days, newest first; at most this many
/// events are read.
const QUERY: &str = "*[System[Provider[@Name='Application Error'] and (EventID=1000) \
                     and TimeCreated[timediff(@SystemTime) <= 2592000000]]] \
                     and *[EventData[Data[@Name='AppName']='phitpup.exe']]";
const READ: usize = 4;

/// The newest crash of PhitPup in the last 30 days.
pub fn newest() -> Option<Crash> {
    pick(events().iter().filter_map(|xml| parse(xml)).collect())
}

/// The newest crash, if it is newer than the last one the person was told
/// about; it then counts as told.
pub fn unseen() -> Option<Crash> {
    let crash = newest()?;
    if settings::last_crash_seen().is_some_and(|seen| seen.as_str() >= crash.time.as_str()) {
        return None;
    }
    settings::set_last_crash_seen(&crash.time);
    Some(crash)
}

/// One crash can log two events: the access violation, then 0xc000041d
/// ("an exception in a callback"). The first says more.
fn pick(crashes: Vec<Crash>) -> Option<Crash> {
    let mut it = crashes.into_iter();
    let newest = it.next()?;
    if newest.code.eq_ignore_ascii_case("c000041d")
        && let Some(earlier) = it.next()
        && earlier.process == newest.process
    {
        return Some(Crash {
            time: newest.time,
            ..earlier
        });
    }
    Some(newest)
}

/// Reads one crash event's XML.
fn parse(xml: &str) -> Option<Crash> {
    let data = |name: &str| -> Option<String> {
        let open = format!("<Data Name='{name}'>");
        let rest = &xml[xml.find(&open)? + open.len()..];
        Some(rest[..rest.find("</Data>")?].to_string())
    };
    let time = {
        let key = "SystemTime='";
        let rest = &xml[xml.find(key)? + key.len()..];
        rest[..rest.find('\'')?].to_string()
    };
    let offset = data("FaultingOffset")?;
    let offset = offset.trim_start_matches('0');
    Some(Crash {
        time,
        version: data("AppVersion")?,
        code: data("ExceptionCode")?,
        module: data("ModuleName")?,
        offset: if offset.is_empty() { "0" } else { offset }.to_string(),
        process: format!(
            "{}/{}",
            data("ProcessId").unwrap_or_default(),
            data("ProcessCreationTime").unwrap_or_default()
        ),
    })
}

/// The newest matching events, as XML.
fn events() -> Vec<String> {
    struct Handle(EVT_HANDLE);
    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: an open handle from EvtQuery or EvtNext.
            unsafe { EvtClose(self.0) };
        }
    }
    let wide = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
    let (channel, query) = (wide("Application"), wide(QUERY));
    // SAFETY: both strings are NUL-terminated and outlive the call.
    let results = unsafe {
        EvtQuery(
            0,
            channel.as_ptr(),
            query.as_ptr(),
            EvtQueryChannelPath | EvtQueryReverseDirection,
        )
    };
    if results == 0 {
        return Vec::new();
    }
    let results = Handle(results);
    let mut raw = [0 as EVT_HANDLE; READ];
    let mut got = 0u32;
    // SAFETY: room for READ handles; each returned one is closed below.
    let ok = unsafe { EvtNext(results.0, READ as u32, raw.as_mut_ptr(), 1000, 0, &mut got) };
    if ok == 0 {
        return Vec::new();
    }
    raw[..got as usize]
        .iter()
        .filter_map(|&h| {
            let event = Handle(h);
            let mut buf = vec![0u16; 4096];
            let (mut used, mut props) = (0u32, 0u32);
            // SAFETY: the buffer's size is passed in bytes; on success `used`
            // bytes of UTF-16 XML were written.
            let ok = unsafe {
                EvtRender(
                    0,
                    event.0,
                    EvtRenderEventXml,
                    (buf.len() * 2) as u32,
                    buf.as_mut_ptr().cast(),
                    &mut used,
                    &mut props,
                )
            };
            let n = (used as usize / 2).min(buf.len());
            (ok != 0).then(|| {
                String::from_utf16_lossy(&buf[..n])
                    .trim_end_matches('\0')
                    .to_string()
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape of a real event (computer and user replaced).
    fn event(time: &str, code: &str, pid: &str) -> String {
        format!(
            "<Event><System><Provider Name='Application Error'/><EventID>1000</EventID>\
             <TimeCreated SystemTime='{time}'/><Computer>PC</Computer>\
             <Security UserID='S-1-5-21-0'/></System><EventData>\
             <Data Name='AppName'>phitpup.exe</Data><Data Name='AppVersion'>0.4.3.0</Data>\
             <Data Name='ModuleName'>phitpup.exe</Data><Data Name='ExceptionCode'>{code}</Data>\
             <Data Name='FaultingOffset'>000000000000d6b8</Data><Data Name='ProcessId'>{pid}</Data>\
             <Data Name='ProcessCreationTime'>0x1dd7357948fd4c3</Data>\
             <Data Name='AppPath'>C:\\Users\\someone\\phitpup.exe</Data></EventData></Event>"
        )
    }

    #[test]
    fn reads_a_crash_event() {
        let c = parse(&event("2026-11-13T10:19:21.1861130Z", "c0000005", "0x8018")).unwrap();
        assert_eq!(c.version, "0.4.3.0");
        assert_eq!(c.code, "c0000005");
        assert_eq!(c.offset, "d6b8");
        assert_eq!(
            c.describe(),
            "0.4.3.0 crashed with 0xc0000005 at phitpup.exe+0xd6b8 (2026-11-13 10:19 UTC)"
        );
        // Only these fields are kept: no path, computer or user.
        assert!(!format!("{c:?}").contains("someone"));
    }

    #[test]
    fn prefers_the_access_violation_over_the_callback_event() {
        let newer = parse(&event("2026-11-13T10:19:21Z", "c000041d", "0x8018")).unwrap();
        let first = parse(&event("2026-11-13T10:19:20Z", "c0000005", "0x8018")).unwrap();
        let c = pick(vec![newer.clone(), first]).unwrap();
        assert_eq!(
            (c.code.as_str(), c.time.as_str()),
            ("c0000005", "2026-11-13T10:19:21Z")
        );
        // A different run is not paired.
        let other = parse(&event("2026-11-12T01:00:00Z", "c0000005", "0x1")).unwrap();
        assert_eq!(pick(vec![newer, other]).unwrap().code, "c000041d");
    }
}

#[cfg(test)]
mod live {
    /// Reads this PC's real log: `cargo test -p phitpup crash::live -- --ignored --nocapture`.
    #[test]
    #[ignore = "reads this PC's event log"]
    fn newest_on_this_pc() {
        println!("{:?}", super::newest().map(|c| c.describe()));
    }
}
