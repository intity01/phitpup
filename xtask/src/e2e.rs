//! `cargo xtask e2e`: PhitPup against a real app, with real key presses.
//!
//! `cargo xtask e2e` uses Notepad; `cargo xtask e2e edge` a page with a text
//! box in a new Microsoft Edge window; `cargo xtask e2e window "<title>"` a
//! window that is already open (LINE, WordCraft...), typing where its focus
//! is. Add `--quiet` for an app where PhitPup shows no bubble (code editors
//! such as VS Code): then only Shift+Backspace and English are checked.
//!
//! Builds the app, asks a running PhitPup to quit (and starts it again at the
//! end), and starts the new build with its settings in `target-e2e/home`, so
//! yours are left alone. Then it types into Notepad with `SendInput`, which
//! goes through PhitPup's keyboard hook like a person typing, presses Tab,
//! Esc or Shift+Backspace, and reads the text back by copying it. A failed
//! check saves a screenshot to `target-e2e/`.
//!
//! The keyboard and mouse must be left alone while it runs (about a minute),
//! and the screen must not be locked. Your clipboard text is put back.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{CloseHandle, HWND, LPARAM, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC, SRCCOPY, SelectObject,
};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows_sys::Win32::System::StationsAndDesktops::{CloseDesktop, OpenInputDesktop};
use windows_sys::Win32::System::Threading::{
    CreateProcessW, DETACHED_PROCESS, OpenProcess, PROCESS_INFORMATION,
    PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW, STARTUPINFOW,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyboardLayout, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_KEYUP,
    LoadKeyboardLayoutW, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEINPUT, SendInput, VK_BACK,
    VK_CONTROL, VK_DELETE, VK_ESCAPE, VK_F24, VK_SHIFT, VK_TAB, VkKeyScanExW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowW, GetForegroundWindow, GetSystemMetrics, GetWindowRect, GetWindowTextW,
    GetWindowThreadProcessId, IsWindowVisible, PostMessageW, SM_CXSCREEN, SM_CYSCREEN,
    SetCursorPos, SetForegroundWindow, SetProcessDPIAware, WM_CLOSE, WM_INPUTLANGCHANGEREQUEST,
};

/// `WM_APP + 12`, `WM_QUIT_REQUEST` in `crates/phitpup/src/tray.rs`.
const WM_QUIT_REQUEST: u32 = 0x800C;
const TRAY_CLASS: &str = "PhitPupTray";
const SETTINGS_CLASS: &str = "PhitPupSettings";
const CF_UNICODETEXT: u32 = 13;
/// The settings the test copy starts with.
const CONFIG: &str = "mode = \"suggest\"\nlearn = true\n";
const EDGE: &str = r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe";
const PAGE_TITLE: &str = "PhitPup e2e";
/// A text box that has the focus when the page opens.
const PAGE: &str = "<!doctype html><meta charset=utf-8><title>PhitPup e2e</title>\
<textarea autofocus spellcheck=false style=\"width:90vw;height:60vh;font-size:20px\"></textarea>";
/// สวัสดี typed on a US layout.
const SLIP: &str = "l;ylfu";
const HELLO: &str = "\u{0E2A}\u{0E27}\u{0E31}\u{0E2A}\u{0E14}\u{0E35}";

/// Where the keys go.
enum Target {
    Notepad,
    Edge,
    /// The open window whose title contains this.
    Window(String),
}

pub fn run(root: &Path, args: &[String]) {
    let quiet = args.iter().any(|a| a == "--quiet");
    let args: Vec<&str> = args
        .iter()
        .map(String::as_str)
        .filter(|a| *a != "--quiet")
        .collect();
    let target = match args.as_slice() {
        [] | ["notepad"] => Target::Notepad,
        ["edge"] => Target::Edge,
        ["window", title] => Target::Window(title.to_string()),
        _ => fail_early("usage: cargo xtask e2e [notepad | edge | window \"<title>\"] [--quiet]"),
    };
    // SAFETY: plain process setting, so screenshots are full size.
    unsafe { SetProcessDPIAware() };
    if !unlocked() {
        fail_early("the screen is locked; unlock it and run again");
    }
    let exe = build(root);
    let dir = root.join("target-e2e");
    let home = dir.join("home");
    let _ = std::fs::remove_dir_all(&home);
    let file = dir.join("e2e.txt");
    let ready = std::fs::create_dir_all(&home)
        .and_then(|()| std::fs::write(home.join("config.toml"), CONFIG))
        .and_then(|()| std::fs::write(&file, ""));
    if let Err(e) = ready {
        fail_early(&format!("could not set up target-e2e: {e}"));
    }

    println!("Leave the keyboard and mouse alone until this finishes.");
    let mut run = Run {
        restart: stop_running_copy(),
        clipboard: clipboard_text(),
        phitpup: None,
        target,
        quiet,
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
        Ok(()) => run.cases(),
        Err(why) => {
            println!("  could not start: {why}");
            1
        }
    };
    run.finish();
    std::process::exit(code);
}

fn fail_early(why: &str) -> ! {
    eprintln!("e2e: {why}");
    std::process::exit(2);
}

/// Builds the app and returns its exe.
fn build(root: &Path) -> PathBuf {
    let out = Command::new(env!("CARGO"))
        .args([
            "build",
            "--release",
            "-p",
            "phitpup",
            "--message-format=json",
        ])
        .current_dir(root)
        .stderr(Stdio::inherit())
        .output()
        .expect("run cargo build");
    if !out.status.success() {
        fail_early("the build failed");
    }
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines()
        .filter(|l| l.contains("\"name\":\"phitpup\"") && l.contains("\"executable\":\""))
        .filter_map(|l| {
            let start = l.find("\"executable\":\"")? + 14;
            let end = start + l[start..].find('"')?;
            Some(PathBuf::from(l[start..end].replace("\\\\", "\\")))
        })
        .next_back()
        .unwrap_or_else(|| fail_early("cargo did not say where phitpup.exe is"))
}

/// One check: Ok, or what went wrong.
type Case = fn(&mut Run) -> Result<(), String>;

struct Run {
    /// The PhitPup that was running before, to start again at the end.
    restart: Option<PathBuf>,
    clipboard: Option<String>,
    phitpup: Option<Child>,
    target: Target,
    /// No bubble in this app; only the hotkey works.
    quiet: bool,
    /// The window the keys go to.
    window: Option<HWND>,
    /// Kept only so it is not a zombie; the app is closed by its tab.
    launched: Option<Child>,
    /// A case has typed; from then on the window must stay in front.
    typed: bool,
    /// Another window came to the front: the rest are skipped.
    stopped: bool,
    log: PathBuf,
    exe: PathBuf,
    dir: PathBuf,
    home: PathBuf,
}

impl Run {
    fn start(&mut self, file: &Path) -> Result<(), String> {
        let log = std::fs::File::create(&self.log).map_err(|e| format!("the log: {e}"))?;
        let child = Command::new(&self.exe)
            .env("PHITPUP_HOME", &self.home)
            .env("PHITPUP_DEBUG", "1")
            .env_remove("PHITPUP_LANG")
            .stdout(Stdio::null())
            .stderr(log)
            .spawn()
            .map_err(|e| format!("phitpup.exe: {e}"))?;
        self.phitpup = Some(child);
        wait_until("PhitPup to start", Duration::from_secs(10), || {
            !find(TRAY_CLASS).is_null()
        })?;
        // The word lists for slips take a moment.
        std::thread::sleep(Duration::from_secs(3));

        // Not waited for: Windows 11's Notepad may hand the file to a window
        // that is already open and stay running, or keep running itself;
        // Edge too.
        let title = match &self.target {
            Target::Notepad => {
                let child = Command::new("notepad.exe").arg(file).spawn();
                self.launched = Some(child.map_err(|e| format!("Notepad: {e}"))?);
                "e2e.txt".to_string()
            }
            Target::Edge => {
                let page = self.dir.join("e2e.html");
                std::fs::write(&page, PAGE).map_err(|e| format!("e2e.html: {e}"))?;
                // Detached: Edge keeps running in the background.
                let args = format!("--new-window --no-first-run \"{}\"", page.display());
                if !detached(Path::new(EDGE), &args) {
                    return Err("could not start Microsoft Edge".into());
                }
                PAGE_TITLE.to_string()
            }
            Target::Window(title) => title.clone(),
        };
        let mut hwnd: HWND = std::ptr::null_mut();
        wait_until(
            &format!("a window titled {title:?}"),
            Duration::from_secs(15),
            || {
                hwnd = window_titled(&title);
                !hwnd.is_null()
            },
        )?;
        self.window = Some(hwnd);
        std::thread::sleep(Duration::from_millis(1500));
        if matches!(self.target, Target::Edge) {
            // The focus may be in the address bar: click into the text box,
            // which fills the middle of the page.
            front(hwnd)?;
            click_middle(hwnd);
            pause(300);
        }
        Ok(())
    }

    /// Runs the cases that fit the target; the number that failed.
    fn cases(&mut self) -> i32 {
        let cases: [(&str, Case); 6] = [
            ("Tab accepts the suggestion", |r| {
                r.fresh()?;
                type_text(SLIP);
                r.accept_key(VK_TAB, "accepted:")?;
                r.expect(HELLO)
            }),
            ("Esc leaves the word as typed", |r| {
                r.fresh()?;
                type_text(SLIP);
                pause(500);
                tap(VK_ESCAPE, &[]);
                pause(300);
                r.expect(SLIP)
            }),
            ("Shift+Backspace fixes the last word", |r| {
                r.fresh()?;
                type_text(SLIP);
                pause(300);
                r.accept_key_with(VK_BACK, &[VK_SHIFT], "hotkey: erase")?;
                r.expect(HELLO)
            }),
            ("English is left alone", |r| {
                r.fresh()?;
                type_text("hello world ");
                pause(500);
                r.expect("hello world ")
            }),
            ("Esc three times stops the suggestion", |r| {
                for _ in 0..3 {
                    r.fresh()?;
                    type_text(SLIP);
                    pause(500);
                    tap(VK_ESCAPE, &[]);
                    pause(300);
                }
                pause(1500); // saved by the worker
                let config = std::fs::read_to_string(r.home.join("config.toml"))
                    .map_err(|e| format!("read the test settings: {e}"))?;
                if !config.contains(&format!("ignore_words = [\"{SLIP}\"]")) {
                    return Err(format!("{SLIP} is not in ignore_words"));
                }
                // Nothing to accept: Notepad types a tab. Elsewhere Tab may move
                // the focus out of the text (browsers do), so stop here.
                if !matches!(r.target, Target::Notepad) {
                    return Ok(());
                }
                r.fresh()?;
                type_text(SLIP);
                pause(500);
                tap(VK_TAB, &[]);
                pause(300);
                r.expect(&format!("{SLIP}\t"))
            }),
            ("The language switches at once", |r| r.language()),
        ];
        // In a quiet app only the hotkey works; the language case needs our
        // own settings window, so it runs with Notepad only.
        let wanted = |i: usize| match (&self.target, self.quiet) {
            (_, true) => i == 2 || i == 3,
            (Target::Notepad, false) => true,
            _ => i < 5,
        };
        let cases: Vec<_> = cases
            .into_iter()
            .enumerate()
            .filter(|&(i, _)| wanted(i))
            .collect();
        let mut failed = 0;
        for &(i, (name, case)) in &cases {
            if self.stopped {
                failed += 1;
                println!("  skip  {name}");
                continue;
            }
            match case(self) {
                Ok(()) => println!("  ok    {name}"),
                Err(why) => {
                    failed += 1;
                    let shot = self.dir.join(format!("fail-{}.bmp", i + 1));
                    screenshot(&shot);
                    println!(
                        "  FAIL  {name}: {why}\n        screenshot {}",
                        shot.display()
                    );
                }
            }
        }
        println!(
            "{} of {} passed; PhitPup's log: {}",
            cases.len() as i32 - failed,
            cases.len(),
            self.log.display()
        );
        failed
    }

    /// The window in front, empty, on the US layout.
    fn fresh(&mut self) -> Result<(), String> {
        let hwnd = self.window.ok_or("the window is not open")?;
        // Someone clicked elsewhere: stop rather than type into their window.
        // SAFETY: plain query.
        let now = unsafe { GetForegroundWindow() };
        if now != hwnd && self.typed {
            self.stopped = true;
            return Err(
                "another window came to the front; stopped so nothing is typed into it".into(),
            );
        }
        self.typed = true;
        front(hwnd)?;
        tap(b'A' as u16, &[VK_CONTROL]);
        tap(VK_DELETE, &[]);
        english(hwnd)?;
        // Moving the caret makes PhitPup forget what it saw.
        pause(200);
        Ok(())
    }

    /// Presses `vk` and waits for PhitPup to log `line`.
    fn accept_key(&mut self, vk: u16, line: &str) -> Result<(), String> {
        self.accept_key_with(vk, &[], line)
    }

    fn accept_key_with(&mut self, vk: u16, mods: &[u16], line: &str) -> Result<(), String> {
        pause(500);
        let from = log_len(&self.log);
        tap(vk, mods);
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if log_since(&self.log, from).contains(line) {
                pause(400); // the keys PhitPup typed
                return Ok(());
            }
            pause(50);
        }
        Err(format!("PhitPup did not log \"{line}\""))
    }

    /// The text in the window is `want`.
    fn expect(&mut self, want: &str) -> Result<(), String> {
        let got = self.text()?;
        // Word processors capitalise a sentence's first letter themselves.
        let mut chars = want.chars();
        let capitalised: String = chars
            .next()
            .map(|c| c.to_uppercase().chain(chars).collect())
            .unwrap_or_default();
        if got == want || !matches!(self.target, Target::Notepad) && got == capitalised {
            Ok(())
        } else {
            Err(format!("expected {want:?}, got {got:?}"))
        }
    }

    fn text(&mut self) -> Result<String, String> {
        front(self.window.ok_or("the window is not open")?)?;
        set_clipboard_text("");
        tap(b'A' as u16, &[VK_CONTROL]);
        tap(b'C' as u16, &[VK_CONTROL]);
        pause(300);
        Ok(clipboard_text().unwrap_or_default().replace("\r\n", "\n"))
    }

    /// The settings window's title follows `language` in the settings file.
    fn language(&mut self) -> Result<(), String> {
        // Started again, PhitPup shows the running copy's settings window.
        Command::new(&self.exe)
            .env("PHITPUP_HOME", &self.home)
            .spawn()
            .map_err(|e| format!("open the settings window: {e}"))?;
        let mut hwnd: HWND = std::ptr::null_mut();
        let deadline = Instant::now() + Duration::from_secs(5);
        while hwnd.is_null() && Instant::now() < deadline {
            hwnd = find(SETTINGS_CLASS);
            pause(100);
        }
        if hwnd.is_null() {
            return Err("the settings window did not open".into());
        }
        let result = (|| {
            for (lang, title) in [
                (
                    "th",
                    "\u{0E15}\u{0E31}\u{0E49}\u{0E07}\u{0E04}\u{0E48}\u{0E32}\u{0E1C}\u{0E34}\u{0E14}\u{0E1B}\u{0E38}\u{0E4A}\u{0E1A}",
                ),
                ("en", "PhitPup Settings"),
            ] {
                self.set_language(lang)?;
                let deadline = Instant::now() + Duration::from_secs(6);
                while window_text(hwnd) != title {
                    if Instant::now() > deadline {
                        return Err(format!(
                            "language = \"{lang}\": the title is {:?}, not {title:?}",
                            window_text(hwnd)
                        ));
                    }
                    pause(100);
                }
            }
            Ok(())
        })();
        // SAFETY: posting to a window that may have closed is harmless.
        unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0) };
        pause(300);
        result
    }

    fn set_language(&self, lang: &str) -> Result<(), String> {
        let path = self.home.join("config.toml");
        let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let mut lines: Vec<String> = text
            .lines()
            .filter(|l| !l.starts_with("language"))
            .map(String::from)
            .collect();
        lines.push(format!("language = \"{lang}\""));
        std::fs::write(&path, lines.join("\n") + "\n").map_err(|e| e.to_string())
    }

    /// Empties the text; saves and closes Notepad's tab or Edge's window;
    /// stops the test copy and puts back the clipboard and the PhitPup that
    /// was running.
    fn finish(&mut self) {
        if let Some(hwnd) = self.window.take()
            && front(hwnd).is_ok()
        {
            tap(b'A' as u16, &[VK_CONTROL]);
            tap(VK_DELETE, &[]);
            if matches!(self.target, Target::Notepad) {
                tap(b'S' as u16, &[VK_CONTROL]);
                pause(300);
            }
            if !matches!(self.target, Target::Window(_)) {
                tap(b'W' as u16, &[VK_CONTROL]);
                pause(300);
            }
        }
        if let Some(mut child) = self.phitpup.take() {
            quit(find(TRAY_CLASS));
            let deadline = Instant::now() + Duration::from_secs(3);
            while matches!(child.try_wait(), Ok(None)) && Instant::now() < deadline {
                pause(100);
            }
            let _ = child.kill();
        }
        set_clipboard_text(self.clipboard.as_deref().unwrap_or(""));
        if let Some(exe) = &self.restart {
            if detached(exe, "") {
                println!("Started your PhitPup again ({}).", exe.display());
            } else {
                println!("Could not start your PhitPup again: {}", exe.display());
            }
        }
    }
}

// ------------------------------------------------------------ windows

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// Starts `exe` with no handles of ours. `Command` passes on every
/// inheritable one, and a PhitPup that keeps running would hold the output
/// pipe of whoever ran us open, so `cargo xtask e2e | ...` never ends.
fn detached(exe: &Path, args: &str) -> bool {
    let mut line = wide(&format!("\"{}\" {args}", exe.display()));
    // SAFETY: a zeroed STARTUPINFOW with its size, a writable command line,
    // and the returned handles closed.
    unsafe {
        let mut si: STARTUPINFOW = std::mem::zeroed();
        si.cb = size_of::<STARTUPINFOW>() as u32;
        let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
        let ok = CreateProcessW(
            std::ptr::null(),
            line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0, // inherit nothing
            DETACHED_PROCESS,
            std::ptr::null(),
            std::ptr::null(),
            &si,
            &mut pi,
        );
        if ok != 0 {
            CloseHandle(pi.hThread);
            CloseHandle(pi.hProcess);
        }
        ok != 0
    }
}

fn find(class: &str) -> HWND {
    // SAFETY: NUL-terminated class name.
    unsafe { FindWindowW(wide(class).as_ptr(), std::ptr::null()) }
}

fn window_text(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    // SAFETY: the buffer's length is passed.
    let n = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// The visible top-level window whose title contains `part`.
fn window_titled(part: &str) -> HWND {
    struct Search<'a>(&'a str, HWND);
    unsafe extern "system" fn each(hwnd: HWND, lparam: LPARAM) -> i32 {
        // SAFETY: lparam is the Search below, alive for the call.
        let s = unsafe { &mut *(lparam as *mut Search) };
        // SAFETY: plain query.
        if unsafe { IsWindowVisible(hwnd) } != 0 && window_text(hwnd).contains(s.0) {
            s.1 = hwnd;
            return 0;
        }
        1
    }
    let mut s = Search(part, std::ptr::null_mut());
    // SAFETY: the callback only runs during the call.
    unsafe { EnumWindows(Some(each), &mut s as *mut Search as LPARAM) };
    s.1
}

fn exe_of(hwnd: HWND) -> Option<PathBuf> {
    if hwnd.is_null() {
        return None;
    }
    let mut pid = 0;
    // SAFETY: plain queries on a handle closed afterwards.
    unsafe {
        GetWindowThreadProcessId(hwnd, &mut pid);
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return None;
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(process, 0, buf.as_mut_ptr(), &mut len);
        CloseHandle(process);
        (ok != 0).then(|| PathBuf::from(String::from_utf16_lossy(&buf[..len as usize])))
    }
}

fn quit(tray: HWND) {
    if !tray.is_null() {
        // SAFETY: posting to PhitPup's tray window.
        unsafe { PostMessageW(tray, WM_QUIT_REQUEST, 0, 0) };
    }
}

/// Asks a running PhitPup to quit; its exe, to start again later.
fn stop_running_copy() -> Option<PathBuf> {
    let tray = find(TRAY_CLASS);
    let exe = exe_of(tray)?;
    println!("Stopping your PhitPup for the test ({}).", exe.display());
    quit(tray);
    if let Err(why) = wait_until("your PhitPup to quit", Duration::from_secs(5), || {
        find(TRAY_CLASS).is_null()
    }) {
        fail_early(&why);
    }
    pause(500);
    Some(exe)
}

fn unlocked() -> bool {
    // SAFETY: a desktop handle closed right away. DESKTOP_SWITCHDESKTOP.
    unsafe {
        let desk = OpenInputDesktop(0, 0, 0x0100);
        if desk.is_null() {
            return false;
        }
        CloseDesktop(desk);
        true
    }
}

/// Brings `hwnd` to the front. Windows lets a process do that only right
/// after a key press, so F24 is tapped first: unlike Alt, it does nothing
/// in the app (Alt would open Notepad's menu shortcuts and eat the next keys).
fn front(hwnd: HWND) -> Result<(), String> {
    // SAFETY: plain window calls.
    unsafe {
        if GetForegroundWindow() == hwnd {
            return Ok(());
        }
        tap(VK_F24, &[]);
        SetForegroundWindow(hwnd);
        pause(300);
        if GetForegroundWindow() != hwnd {
            return Err("could not bring the window to the front".into());
        }
    }
    Ok(())
}

/// Switches `hwnd` to the US layout and waits until it has.
fn english(hwnd: HWND) -> Result<(), String> {
    // SAFETY: plain layout calls; 0x0409 is US English.
    unsafe {
        let us = LoadKeyboardLayoutW(wide("00000409").as_ptr(), 0);
        for _ in 0..20 {
            let thread = GetWindowThreadProcessId(GetForegroundWindow(), std::ptr::null_mut());
            if GetKeyboardLayout(thread) as usize & 0xFFFF == 0x0409 {
                return Ok(());
            }
            PostMessageW(hwnd, WM_INPUTLANGCHANGEREQUEST, 0, us as LPARAM);
            pause(150);
        }
    }
    Err("the window did not switch to the US layout".into())
}

// ------------------------------------------------------------ keys

fn key(vk: u16, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if up { KEYEVENTF_KEYUP } else { 0 },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send(inputs: &[INPUT]) {
    // SAFETY: a slice of initialised INPUTs and its length.
    unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            size_of::<INPUT>() as i32,
        )
    };
}

/// Presses `vk` with `mods` held.
fn tap(vk: u16, mods: &[u16]) {
    let mut inputs: Vec<INPUT> = mods.iter().map(|&m| key(m, false)).collect();
    inputs.push(key(vk, false));
    inputs.push(key(vk, true));
    inputs.extend(mods.iter().rev().map(|&m| key(m, true)));
    send(&inputs);
    pause(30);
}

/// Types `text` key by key as on a US keyboard.
fn type_text(text: &str) {
    // SAFETY: plain layout calls; 0x0409 is US English.
    let us = unsafe { LoadKeyboardLayoutW(wide("00000409").as_ptr(), 0) };
    for c in text.encode_utf16() {
        // SAFETY: plain query.
        let scan = unsafe { VkKeyScanExW(c, us) };
        if scan == -1 {
            eprintln!("e2e: {text:?} has a character the US layout cannot type");
            continue;
        }
        let vk = (scan & 0xFF) as u16;
        let mods: &[u16] = if scan & 0x100 != 0 { &[VK_SHIFT] } else { &[] };
        tap(vk, mods);
        pause(40);
    }
}

fn pause(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

fn wait_until(what: &str, limit: Duration, mut done: impl FnMut() -> bool) -> Result<(), String> {
    let deadline = Instant::now() + limit;
    while !done() {
        if Instant::now() > deadline {
            return Err(format!("timed out waiting for {what}"));
        }
        pause(100);
    }
    Ok(())
}

/// Clicks the middle of `hwnd`.
fn click_middle(hwnd: HWND) {
    let mut r = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    // SAFETY: plain window and input calls.
    unsafe {
        GetWindowRect(hwnd, &mut r);
        SetCursorPos((r.left + r.right) / 2, (r.top + r.bottom) / 2);
    }
    let button = |flags| INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    send(&[button(MOUSEEVENTF_LEFTDOWN), button(MOUSEEVENTF_LEFTUP)]);
}

fn log_len(log: &Path) -> usize {
    std::fs::metadata(log).map_or(0, |m| m.len() as usize)
}

fn log_since(log: &Path, from: usize) -> String {
    let bytes = std::fs::read(log).unwrap_or_default();
    String::from_utf8_lossy(bytes.get(from..).unwrap_or_default()).into_owned()
}

// ------------------------------------------------------------ clipboard

fn clipboard_text() -> Option<String> {
    // SAFETY: the clipboard is opened and closed here; the data is read
    // while locked, up to its NUL.
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return None;
        }
        let data = GetClipboardData(CF_UNICODETEXT);
        let text = if data.is_null() {
            None
        } else {
            let p = GlobalLock(data) as *const u16;
            if p.is_null() {
                None
            } else {
                let mut n = 0;
                while *p.add(n) != 0 {
                    n += 1;
                }
                let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, n));
                GlobalUnlock(data);
                Some(s)
            }
        };
        CloseClipboard();
        text
    }
}

fn set_clipboard_text(text: &str) {
    let w = wide(text);
    // SAFETY: the clipboard is opened and closed here; the memory is
    // filled while locked and then belongs to the clipboard.
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return;
        }
        EmptyClipboard();
        let mem = GlobalAlloc(GMEM_MOVEABLE, w.len() * 2);
        if !mem.is_null() {
            let p = GlobalLock(mem) as *mut u16;
            if !p.is_null() {
                std::ptr::copy_nonoverlapping(w.as_ptr(), p, w.len());
                GlobalUnlock(mem);
                SetClipboardData(CF_UNICODETEXT, mem);
            }
        }
        CloseClipboard();
    }
}

// ------------------------------------------------------------ screenshot

/// The whole primary screen as a 24-bit BMP.
fn screenshot(path: &Path) {
    // SAFETY: GDI objects made and freed here; the buffer is sized for
    // 32-bit rows of the screen's width and height.
    unsafe {
        let (w, h) = (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN));
        let screen = GetDC(std::ptr::null_mut());
        let mem = CreateCompatibleDC(screen);
        let bmp = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(mem, bmp);
        BitBlt(mem, 0, 0, w, h, screen, 0, 0, SRCCOPY);
        let mut info: BITMAPINFO = std::mem::zeroed();
        info.bmiHeader = BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: h, // bottom-up, as BMP files are
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..std::mem::zeroed()
        };
        let mut px = vec![0u8; (w * h * 4) as usize];
        GetDIBits(
            mem,
            bmp,
            0,
            h as u32,
            px.as_mut_ptr().cast(),
            &mut info,
            DIB_RGB_COLORS,
        );
        SelectObject(mem, old);
        DeleteObject(bmp);
        DeleteDC(mem);
        ReleaseDC(std::ptr::null_mut(), screen);

        let size = 54 + px.len();
        let mut out = Vec::with_capacity(size);
        out.extend(b"BM");
        out.extend((size as u32).to_le_bytes());
        out.extend([0u8; 4]);
        out.extend(54u32.to_le_bytes());
        out.extend(40u32.to_le_bytes());
        out.extend(w.to_le_bytes());
        out.extend(h.to_le_bytes());
        out.extend(1u16.to_le_bytes());
        out.extend(32u16.to_le_bytes());
        out.extend([0u8; 24]);
        out.extend(px);
        let _ = std::fs::write(path, out);
    }
}
