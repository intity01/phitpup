//! "Check for updates" on the About page. PhitPup has no networking code:
//! it runs `phitpup-update.exe`, which the installer puts next to it, and
//! shows the lines that prints (`phitpup_core::update::Line`). Without the
//! helper (the single-file download) the button opens the download page.

use std::io::{BufRead, BufReader};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Mutex;

use phitpup_core::update::Line;

use crate::{settings, tray};

/// What the About page shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    Idle,
    Checking,
    Current,
    /// A newer version and its installer's size.
    Available(String, u64),
    /// Bytes downloaded, of the total.
    Downloading(u64, u64),
    /// The installer is running; it closes PhitPup and starts the new one.
    Installing,
    Failed(String),
}

static STATE: Mutex<State> = Mutex::new(State::Idle);

pub fn state() -> State {
    STATE.lock().map_or(State::Idle, |s| s.clone())
}

pub(crate) fn set(state: State) {
    if let Ok(mut s) = STATE.lock() {
        *s = state;
    }
    tray::refresh_settings();
}

/// The helper, if PhitPup was installed with the installer.
pub fn helper() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.with_file_name("phitpup-update.exe")).filter(|p| p.is_file())
}

/// The button was clicked.
pub fn click() {
    let Some(helper) = helper() else {
        settings::open(settings::RELEASES);
        return;
    };
    match state() {
        State::Idle | State::Current | State::Failed(_) => {
            run(helper, vec!["check".into()], State::Checking)
        }
        State::Available(version, size) => run(
            helper,
            vec!["install".into(), version],
            State::Downloading(0, size),
        ),
        State::Checking | State::Downloading(..) | State::Installing => {}
    }
}

/// Starts the helper and follows its output on a thread of its own.
fn run(helper: PathBuf, args: Vec<String>, busy: State) {
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    set(busy);
    std::thread::spawn(move || {
        let child = Command::new(helper)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(_) => return set(State::Failed("Could not start the updater.".into())),
        };
        let mut last = None;
        if let Some(out) = child.stdout.take() {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                let Some(line) = Line::read(&line) else {
                    continue;
                };
                debug!("updater: {line}");
                set(match &line {
                    Line::Current => State::Current,
                    Line::Available(v, size) => State::Available(v.clone(), *size),
                    Line::Progress(done, total) => State::Downloading(*done, *total),
                    Line::Started => State::Installing,
                    Line::Failed(why) => State::Failed(why.clone()),
                });
                last = Some(line);
            }
        }
        let _ = child.wait();
        if matches!(last, None | Some(Line::Progress(..))) {
            set(State::Failed("The updater stopped unexpectedly.".into()));
        }
    });
}
