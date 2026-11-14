//! `cargo xtask soak [hours]`: PhitPup left running for hours (8 unless
//! said) while it is used, to find what only shows after long use: a crash,
//! a leak, or the fix quietly no longer working.
//!
//! Like `cargo xtask e2e` it builds the app, stops your PhitPup, and runs
//! the new build with its own settings (in `target-soak/home`) against
//! Notepad. Then, round after round, it types like a person: a slip fixed
//! with Tab, one left with Esc, one fixed with Shift+Backspace, Thai typed
//! on the Thai layout (which shows the language stamp), and now and then it
//! opens and closes Character Map and PhitPup's settings window. Every
//! round reads the text back to check the fix still works.
//!
//! Every minute it writes PhitPup's memory, handle, GDI and USER object
//! counts to `target-soak/soak.csv`; at the end it says whether any of them
//! kept climbing, and writes `target-soak/soak-report.txt`.
//!
//! It keeps the PC awake and the screen on while it runs. It stops at once
//! if PhitPup closes (and says how), and if the mouse moves or another
//! window comes to the front: someone is using the PC, and nothing should be
//! typed into their window. Start it, leave the PC alone, read the report
//! in the morning. Your PhitPup and clipboard are put back at the end.

use std::os::windows::io::AsRawHandle;

use windows_sys::Win32::Foundation::{HANDLE, POINT};
use windows_sys::Win32::System::Power::{
    ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED, SetThreadExecutionState,
};
use windows_sys::Win32::System::ProcessStatus::{
    K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
};
use windows_sys::Win32::System::Threading::{
    GR_GDIOBJECTS, GR_USEROBJECTS, GetGuiResources, GetProcessHandleCount,
};
use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;

use super::*;

/// The test copy's settings: no learning, so the same slip is offered
/// round after round.
const SOAK_CONFIG: &str = "mode = \"suggest\"\nlearn = false\n";
/// How often PhitPup's counts are written down.
const SAMPLE_EVERY: Duration = Duration::from_secs(60);
/// Counts settle once everything has been used once; growth is measured
/// from then.
const WARMUP_MINUTES: f64 = 15.0;
/// A pause between rounds, like someone reading what they typed.
const BETWEEN_ROUNDS_MS: u64 = 1500;
/// Screenshots of failed rounds kept, at most.
const SHOTS: u64 = 5;

pub fn run(root: &Path, args: &[String]) {
    let hours = match args {
        [] => 8.0,
        [h] => h
            .parse::<f64>()
            .ok()
            .filter(|h| *h > 0.0)
            .unwrap_or_else(|| fail_early("usage: cargo xtask soak [hours]")),
        _ => fail_early("usage: cargo xtask soak [hours]"),
    };
    // SAFETY: plain process setting, so screenshots are full size.
    unsafe { SetProcessDPIAware() };
    if !unlocked() {
        fail_early("the screen is locked; unlock it and run again");
    }
    let exe = build(root);
    let dir = root.join("target-soak");
    let home = dir.join("home");
    let _ = std::fs::remove_dir_all(&home);
    let file = dir.join("soak.txt");
    let ready = std::fs::create_dir_all(&home)
        .and_then(|()| std::fs::write(home.join("config.toml"), SOAK_CONFIG))
        .and_then(|()| std::fs::write(&file, ""));
    if let Err(e) = ready {
        fail_early(&format!("could not set up target-soak: {e}"));
    }

    println!("Soaking PhitPup for {hours} hours. Leave the PC alone: moving the mouse stops it.");
    // SAFETY: plain power request, undone at the end.
    unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED) };
    let mut run = Run {
        restart: stop_running_copy(),
        clipboard: clipboard_text(),
        phitpup: None,
        target: Target::Notepad,
        quiet: false,
        window: None,
        launched: None,
        typed: false,
        stopped: false,
        log: dir.join("phitpup.log"),
        exe,
        dir,
        home,
    };
    // Release builds abort on a panic, so nothing here may panic: finish()
    // has to run to put your PhitPup back.
    let code = match run.start(&file) {
        Ok(()) => soak(&mut run, hours),
        Err(why) => {
            println!("  could not start: {why}");
            1
        }
    };
    run.finish();
    // SAFETY: back to the usual power settings.
    unsafe { SetThreadExecutionState(ES_CONTINUOUS) };
    std::process::exit(code);
}

/// PhitPup's counts at one moment.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Sample {
    minutes: f64,
    private_mb: f64,
    handles: u32,
    gdi: u32,
    user: u32,
}

fn soak(run: &mut Run, hours: f64) -> i32 {
    let started = Instant::now();
    let end = started + Duration::from_secs_f64(hours * 3600.0);
    let csv = run.dir.join("soak.csv");
    let _ = std::fs::write(&csv, "minutes,private_mb,handles,gdi,user,rounds,failed\n");
    let mut samples = Vec::new();
    let mut next_sample = started;
    let mut mouse = cursor();
    let (mut rounds, mut failed) = (0u64, 0u64);
    let mut stopped = None;

    while Instant::now() < end {
        if !unlocked() {
            // Locked anyway (a policy, or someone): wait, and check again
            // that nobody is at the PC once it is unlocked.
            pause(30_000);
            mouse = cursor();
            continue;
        }
        let now = cursor();
        if moved(mouse, now) {
            stopped = Some(format!(
                "the mouse moved from {mouse:?} to {now:?}: someone is using the PC"
            ));
            break;
        }
        if let Some(how) = closed(run) {
            stopped = Some(how);
            break;
        }
        if Instant::now() >= next_sample {
            if let Some(s) = sample(run, started) {
                let line = format!(
                    "{:.1},{:.1},{},{},{},{rounds},{failed}\n",
                    s.minutes, s.private_mb, s.handles, s.gdi, s.user
                );
                let _ = std::fs::OpenOptions::new()
                    .append(true)
                    .open(&csv)
                    .and_then(|mut f| std::io::Write::write_all(&mut f, line.as_bytes()));
                samples.push(s);
            }
            next_sample += SAMPLE_EVERY;
        }
        rounds += 1;
        if let Err(why) = round(run, rounds) {
            failed += 1;
            println!("  round {rounds}: {why}");
            if failed <= SHOTS {
                screenshot(&run.dir.join(format!("fail-round-{rounds}.bmp")));
            }
            if run.stopped {
                stopped = Some(why);
                break;
            }
        }
        pause(BETWEEN_ROUNDS_MS);
    }
    if let Some(s) = sample(run, started) {
        samples.push(s);
    }
    report(
        run,
        started.elapsed(),
        rounds,
        failed,
        stopped.as_deref(),
        &samples,
    )
}

/// One round of use: always a fixed slip, then something else by turn.
fn round(run: &mut Run, n: u64) -> Result<(), String> {
    let hwnd = run.window.ok_or("the window is not open")?;
    match n % 4 {
        // A sentence ending in a slip, fixed with Tab.
        0 => {
            run.fresh()?;
            type_text("hello world ");
            type_text(SLIP);
            pause(600);
            tap(VK_TAB, &[]);
            pause(500);
            run.expect(&format!("hello world {HELLO}"))?;
        }
        // Left as typed with Esc.
        1 => {
            run.fresh()?;
            type_text(SLIP);
            pause(600);
            tap(VK_ESCAPE, &[]);
            pause(300);
            run.expect(SLIP)?;
        }
        // Fixed with Shift+Backspace.
        2 => {
            run.fresh()?;
            type_text(SLIP);
            pause(300);
            tap(VK_BACK, &[VK_SHIFT]);
            pause(800);
            run.expect(HELLO)?;
        }
        // Typed on the Thai layout, which is right: left alone.
        _ => {
            run.fresh()?;
            thai(hwnd)?;
            pause(300);
            type_text(SLIP);
            type_text(" ");
            pause(600);
            run.expect(&format!("{HELLO} "))?;
        }
    }
    if n.is_multiple_of(10) {
        other_app()?;
        front(hwnd)?;
    }
    if n.is_multiple_of(25) {
        settings_window(run)?;
        front(hwnd)?;
    }
    Ok(())
}

/// Opens Character Map in front of Notepad and closes it again.
fn other_app() -> Result<(), String> {
    let mut child = Command::new("charmap.exe")
        .spawn()
        .map_err(|e| format!("Character Map: {e}"))?;
    let mut hwnd: HWND = std::ptr::null_mut();
    let found = wait_until("Character Map", Duration::from_secs(10), || {
        hwnd = window_titled("Character Map");
        !hwnd.is_null()
    });
    pause(800);
    if !hwnd.is_null() {
        // SAFETY: posting to a window that may have closed is harmless.
        unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0) };
    }
    let _ = wait_until("Character Map to close", Duration::from_secs(5), || {
        matches!(child.try_wait(), Ok(Some(_)))
    });
    let _ = child.kill();
    let _ = child.wait();
    pause(300);
    found
}

/// Opens PhitPup's settings window and closes it again.
fn settings_window(run: &Run) -> Result<(), String> {
    // Started again, PhitPup shows the running copy's settings window.
    let mut second = Command::new(&run.exe)
        .env("PHITPUP_HOME", &run.home)
        .spawn()
        .map_err(|e| format!("open the settings window: {e}"))?;
    let mut hwnd: HWND = std::ptr::null_mut();
    let found = wait_until("the settings window", Duration::from_secs(5), || {
        hwnd = find(SETTINGS_CLASS);
        !hwnd.is_null()
    });
    let _ = second.wait();
    pause(1000);
    if !hwnd.is_null() {
        // SAFETY: posting to a window that may have closed is harmless.
        unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0) };
    }
    pause(300);
    found
}

/// Switches `hwnd` to the Thai layout and waits until it has.
fn thai(hwnd: HWND) -> Result<(), String> {
    // SAFETY: plain layout calls; 0x041E is Thai Kedmanee.
    unsafe {
        let th = LoadKeyboardLayoutW(wide("0000041E").as_ptr(), 0);
        for _ in 0..20 {
            let thread = GetWindowThreadProcessId(GetForegroundWindow(), std::ptr::null_mut());
            if GetKeyboardLayout(thread) as usize & 0xFFFF == 0x041E {
                return Ok(());
            }
            PostMessageW(hwnd, WM_INPUTLANGCHANGEREQUEST, 0, th as LPARAM);
            pause(150);
        }
    }
    Err("the window did not switch to the Thai layout".into())
}

/// Whether the pointer went far enough that someone moved it: a nudge of
/// the desk or the pointer settling is not someone at the PC.
fn moved(from: (i32, i32), to: (i32, i32)) -> bool {
    (from.0 - to.0).abs().max((from.1 - to.1).abs()) > 40
}

fn cursor() -> (i32, i32) {
    let mut p = POINT { x: 0, y: 0 };
    // SAFETY: a plain query into a local.
    unsafe { GetCursorPos(&mut p) };
    (p.x, p.y)
}

/// How PhitPup closed, if it did.
fn closed(run: &mut Run) -> Option<String> {
    let child = run.phitpup.as_mut()?;
    match child.try_wait() {
        Ok(Some(status)) => Some(match status.code() {
            Some(code) => format!(
                "PhitPup closed by itself with exit code {:#010x}; see Event Viewer, Application log",
                code as u32
            ),
            None => "PhitPup closed by itself".into(),
        }),
        _ => None,
    }
}

fn sample(run: &Run, started: Instant) -> Option<Sample> {
    let process = run.phitpup.as_ref()?.as_raw_handle() as HANDLE;
    // SAFETY: queries on our own child's handle, into zeroed locals.
    unsafe {
        let mut memory: PROCESS_MEMORY_COUNTERS_EX = std::mem::zeroed();
        memory.cb = size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
        let ok = K32GetProcessMemoryInfo(
            process,
            (&mut memory as *mut PROCESS_MEMORY_COUNTERS_EX).cast::<PROCESS_MEMORY_COUNTERS>(),
            memory.cb,
        );
        if ok == 0 {
            return None;
        }
        let mut handles = 0;
        GetProcessHandleCount(process, &mut handles);
        Some(Sample {
            minutes: started.elapsed().as_secs_f64() / 60.0,
            private_mb: memory.PrivateUsage as f64 / (1024.0 * 1024.0),
            handles,
            gdi: GetGuiResources(process, GR_GDIOBJECTS),
            user: GetGuiResources(process, GR_USEROBJECTS),
        })
    }
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v.get(v.len() / 2).copied().unwrap_or(0.0)
}

/// What kept climbing: each count from just after the warm-up to the end,
/// as the median of a few samples so one busy moment does not count. None
/// when the run was too short to tell.
fn leaks(samples: &[Sample]) -> Option<Vec<String>> {
    let settled: Vec<&Sample> = samples
        .iter()
        .filter(|s| s.minutes >= WARMUP_MINUTES)
        .collect();
    if settled.len() < 30 {
        return None;
    }
    let n = (settled.len() / 10).max(5);
    let (first, last) = (&settled[..n], &settled[settled.len() - n..]);
    let growth = |f: fn(&Sample) -> f64| {
        median(last.iter().map(|s| f(s)).collect()) - median(first.iter().map(|s| f(s)).collect())
    };
    // What is measured, how, how much growth is a leak, and its unit.
    type Check = (&'static str, fn(&Sample) -> f64, f64, &'static str);
    let checks: [Check; 4] = [
        ("private memory", |s| s.private_mb, 30.0, " MB"),
        ("handles", |s| s.handles as f64, 100.0, ""),
        ("GDI objects", |s| s.gdi as f64, 50.0, ""),
        ("USER objects", |s| s.user as f64, 50.0, ""),
    ];
    Some(
        checks
            .iter()
            .filter_map(|&(name, f, limit, unit)| {
                let g = growth(f);
                (g > limit).then(|| format!("{name} grew by {g:.0}{unit}"))
            })
            .collect(),
    )
}

fn report(
    run: &Run,
    took: Duration,
    rounds: u64,
    failed: u64,
    stopped: Option<&str>,
    samples: &[Sample],
) -> i32 {
    let mut text = format!(
        "Soak test: {:.1} hours, {rounds} rounds, {failed} failed.\n",
        took.as_secs_f64() / 3600.0
    );
    if let Some(why) = stopped {
        text += &format!("Stopped early: {why}\n");
    }
    if let (Some(a), Some(b)) = (samples.first(), samples.last()) {
        text += &format!(
            "PhitPup at the start: {:.1} MB, {} handles, {} GDI, {} USER objects\n\
             PhitPup at the end:   {:.1} MB, {} handles, {} GDI, {} USER objects\n",
            a.private_mb, a.handles, a.gdi, a.user, b.private_mb, b.handles, b.gdi, b.user
        );
    }
    let leaked = leaks(samples);
    match &leaked {
        None => text += "Too short to judge leaks (needs about 45 minutes).\n",
        Some(l) if l.is_empty() => text += "Nothing kept climbing.\n",
        Some(l) => text += &format!("Kept climbing: {}.\n", l.join(", ")),
    }
    // A few failed rounds can be a slow moment; more is PhitPup.
    let too_many = failed * 100 > rounds.max(1);
    let crashed = stopped.is_some_and(|s| s.starts_with("PhitPup closed"));
    let ok = !too_many && !crashed && leaked.as_ref().is_none_or(Vec::is_empty);
    text += if ok { "PASSED\n" } else { "FAILED\n" };
    text += &format!(
        "Counts each minute: {}\nPhitPup's log: {}\n",
        run.dir.join("soak.csv").display(),
        run.log.display()
    );
    print!("{text}");
    let _ = std::fs::write(run.dir.join("soak-report.txt"), &text);
    i32::from(!ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_of(minutes: usize, f: impl Fn(f64) -> (f64, u32)) -> Vec<Sample> {
        (0..minutes)
            .map(|m| {
                let (mb, handles) = f(m as f64);
                Sample {
                    minutes: m as f64,
                    private_mb: mb,
                    handles,
                    gdi: 40,
                    user: 30,
                }
            })
            .collect()
    }

    #[test]
    fn a_nudge_is_not_someone_at_the_pc() {
        assert!(!moved((100, 100), (100, 100)));
        assert!(!moved((100, 100), (130, 90)));
        assert!(moved((100, 100), (300, 100)));
    }

    #[test]
    fn a_steady_climb_is_a_leak_and_a_flat_line_is_not() {
        let flat = run_of(480, |m| (12.0 + (m % 7.0) * 0.1, 300));
        assert_eq!(leaks(&flat), Some(vec![]));
        let climbing = run_of(480, |m| (12.0 + m * 0.2, 300 + m as u32));
        assert_eq!(
            leaks(&climbing).unwrap(),
            ["private memory grew by 84 MB", "handles grew by 419"]
        );
        // Growth during the warm-up is just starting up.
        let warming = run_of(480, |m| (if m < 10.0 { m * 10.0 } else { 100.0 }, 300));
        assert_eq!(leaks(&warming), Some(vec![]));
        assert_eq!(leaks(&run_of(30, |_| (12.0, 300))), None, "too short");
    }
}
