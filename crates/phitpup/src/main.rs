//! ผิดปุ๊บ (PhitPup): fix text typed with the wrong keyboard layout.
//!
//! Shift+Backspace converts the last word between Thai and English (press
//! again to reach further back, and once more to undo), or the selection if
//! nothing was typed since the caret last moved.
//!
//! With `--suggest`, a bubble under the caret offers the fix while the word is
//! being typed; Tab accepts it and Esc dismisses it.
//!
//! Threads: the main thread owns the keyboard and mouse hooks and runs their
//! message loop; it must answer each key within milliseconds. The worker
//! thread does the slow part (SendInput, the clipboard) when the hook posts
//! [`WM_FIX`] to it. The suggestion bubble also lives on the worker thread.

/// `PHITPUP_DEBUG=1` prints what the program decides to stderr. It never
/// prints what was typed, only counts and reasons.
#[cfg(windows)]
static DEBUG: std::sync::LazyLock<bool> =
    std::sync::LazyLock::new(|| std::env::var_os("PHITPUP_DEBUG").is_some());

macro_rules! debug {
    ($($t:tt)*) => {
        if *crate::DEBUG {
            eprintln!("[phitpup] {}", format_args!($($t)*));
        }
    };
}

#[cfg(windows)]
mod bubble;
#[cfg(windows)]
mod clipboard;
#[cfg(windows)]
mod hook;
#[cfg(windows)]
mod inject;
#[cfg(windows)]
mod layout;

#[cfg(windows)]
fn main() {
    if let Err(e) = app::run() {
        eprintln!("phitpup: {e}");
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("phitpup currently runs on Windows only");
    std::process::exit(1);
}

#[cfg(windows)]
pub use app::{APP, App, WM_ACCEPT, WM_BUBBLE, WM_FIX};

#[cfg(windows)]
mod app {
    use std::sync::{Mutex, OnceLock, mpsc};
    use std::time::{Duration, Instant};

    use phitpup_core::model::{Lang, Model};
    use phitpup_core::{Detector, Direction, Edit, Fixer, Suggestion};
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    use windows_sys::Win32::UI::HiDpi::{
        DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DispatchMessageW, GetMessageW, HWND_MESSAGE, MSG, PM_NOREMOVE,
        PeekMessageW, TranslateMessage, WM_APP,
    };

    use crate::{bubble, clipboard, hook, inject, layout};

    /// Posted by the hook to the worker: the user pressed Shift+Backspace.
    pub const WM_FIX: u32 = WM_APP + 1;
    /// Posted by the hook to the worker: the suggestion changed.
    pub const WM_BUBBLE: u32 = WM_APP + 2;
    /// Posted by the hook to the worker: Tab accepted the suggestion, whose
    /// edit is waiting in [`App::accepted`].
    pub const WM_ACCEPT: u32 = WM_APP + 3;

    pub struct App {
        pub fixer: Mutex<Fixer>,
        pub detector: Detector,
        pub english: isize,
        pub thai: isize,
        pub worker: u32,
        /// Suggest mode is on.
        pub suggest: bool,
        /// What the bubble shows, if anything.
        pub suggestion: Mutex<Option<Suggestion>>,
        pub accepted: Mutex<Option<Edit>>,
    }

    pub static APP: OnceLock<App> = OnceLock::new();

    pub fn run() -> Result<(), String> {
        let suggest = std::env::args().skip(1).any(|a| a == "--suggest");
        // SAFETY: plain process setting; the bubble then sizes itself per monitor.
        unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
        let (english, thai) = layout::find_pair()?;
        let detector = Detector::new(&english.layout, &thai.layout);
        if !Model::embedded().is_trained() {
            eprintln!("warning: built without a trained model; only the hotkey works");
        }

        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || worker(tx));
        let worker = rx.recv().map_err(|_| "worker thread failed to start")?;

        println!("ผิดปุ๊บ PhitPup {}", env!("CARGO_PKG_VERSION"));
        println!("  {}  ⇄  {}", english.layout.name(), thai.layout.name());
        println!("  Shift+Backspace: fix the last word (again: one word further, then undo)");
        if suggest {
            println!("  Suggestions on: Tab accepts the bubble, Esc dismisses it");
        } else {
            println!("  Run with --suggest to see fixes while you type");
        }
        println!("  Close this window or press Ctrl+C to quit.");
        debug!("debug output on (counts and reasons only, never the text you type)");

        let app = App {
            fixer: Mutex::new(Fixer::new()),
            detector,
            english: english.hkl,
            thai: thai.hkl,
            worker,
            suggest,
            suggestion: Mutex::new(None),
            accepted: Mutex::new(None),
        };
        APP.set(app).map_err(|_| "already running")?;
        hook::install()?;
        message_loop();
        Ok(())
    }

    fn message_loop() {
        // SAFETY: standard message loop on this thread's queue.
        unsafe {
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }

    fn worker(tx: mpsc::Sender<u32>) {
        // SAFETY: PeekMessage creates this thread's queue before we hand out
        // its id; the message-only window owns the clipboard while we restore it.
        let owner: HWND = unsafe {
            let mut msg: MSG = std::mem::zeroed();
            PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_NOREMOVE);
            let class: Vec<u16> = "STATIC".encode_utf16().chain([0]).collect();
            CreateWindowExW(
                0,
                class.as_ptr(),
                std::ptr::null(),
                0,
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                std::ptr::null_mut(),
                GetModuleHandleW(std::ptr::null()),
                std::ptr::null(),
            )
        };
        bubble::create();
        // SAFETY: plain query.
        let _ = tx.send(unsafe { GetCurrentThreadId() });

        // SAFETY: standard message loop on this thread's queue.
        unsafe {
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                match msg.message {
                    _ if !msg.hwnd.is_null() => {
                        TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                    WM_FIX => fix(owner),
                    WM_BUBBLE => bubble::schedule(),
                    WM_ACCEPT => accept(),
                    _ => {}
                }
            }
        }
    }

    fn fix(owner: HWND) {
        let app = APP.get().unwrap();
        if layout::foreground_is_elevated() {
            debug!(
                "hotkey: the window in front runs as administrator; Windows blocks typing into it"
            );
        }
        let edit = app.fixer.lock().unwrap().hotkey(&app.detector);
        match edit {
            Some(e) => {
                debug!(
                    "hotkey: erase {} chars, type {} chars, switch to {:?}",
                    e.delete,
                    e.insert.chars().count(),
                    e.switch_to
                );
                inject::replace(e.delete, &e.insert);
                switch_to(e.switch_to);
            }
            None => {
                debug!("hotkey: nothing typed since the caret moved; trying the selection");
                if !convert_selection(owner) {
                    debug!("hotkey: no selection either; sent a plain Backspace");
                    inject::backspace(); // nothing to fix: behave like Backspace
                }
            }
        }
    }

    fn accept() {
        bubble::hide();
        let app = APP.get().unwrap();
        if let Some(e) = app.accepted.lock().unwrap().take() {
            debug!(
                "Tab: erase {} chars, type {} chars, switch to {:?}",
                e.delete,
                e.insert.chars().count(),
                e.switch_to
            );
            inject::replace(e.delete, &e.insert);
            switch_to(e.switch_to);
        }
    }

    fn switch_to(lang: Lang) {
        let app = APP.get().unwrap();
        let hkl = match lang {
            Lang::Thai => app.thai,
            Lang::English => app.english,
        };
        if layout::lang_of(layout::foreground()) != Some(lang) {
            layout::activate(hkl);
        }
    }

    /// Copies the selection, converts it and types the result over it.
    /// Returns false if nothing was selected.
    fn convert_selection(owner: HWND) -> bool {
        let app = APP.get().unwrap();
        let saved = clipboard::snapshot(owner);
        let before = clipboard::sequence();
        inject::release_shift();
        inject::copy();
        if !wait(Duration::from_millis(250), || {
            clipboard::sequence() != before
        }) {
            return false;
        }
        let text = clipboard::text(owner);
        clipboard::restore(owner, &saved);

        let Some((direction, converted)) = text.as_deref().and_then(|t| app.detector.selection(t))
        else {
            debug!("selection: copied, but nothing in it to convert");
            return true;
        };
        debug!(
            "selection: typing {} chars over it",
            converted.chars().count()
        );
        inject::replace(0, &converted);
        switch_to(match direction {
            Direction::ToThai => Lang::Thai,
            Direction::ToEnglish => Lang::English,
        });
        true
    }

    /// Polls `done` while still answering messages sent to this thread
    /// (clipboard notifications must not block other apps).
    fn wait(timeout: Duration, mut done: impl FnMut() -> bool) -> bool {
        let start = Instant::now();
        while start.elapsed() < timeout {
            if done() {
                return true;
            }
            // SAFETY: PM_NOREMOVE only runs pending sent messages.
            unsafe {
                let mut msg: MSG = std::mem::zeroed();
                PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_NOREMOVE);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        done()
    }
}
