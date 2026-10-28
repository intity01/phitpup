//! ผิดปุ๊บ (PhitPup): fix text typed with the wrong keyboard layout.
//!
//! Shift+Backspace converts the last word between Thai and English (press
//! again to reach further back, and once more to undo), or the selection if
//! nothing was typed since the caret last moved.
//!
//! Threads: the main thread owns the keyboard and mouse hooks and runs their
//! message loop; it must answer each key within milliseconds. The worker
//! thread does the slow part (SendInput, the clipboard) when the hook posts
//! [`WM_FIX`] to it.

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
pub use app::{APP, WM_FIX};

#[cfg(windows)]
mod app {
    use std::sync::{Mutex, OnceLock, mpsc};
    use std::time::{Duration, Instant};

    use phitpup_core::model::{Lang, Model};
    use phitpup_core::{Detector, Direction, Fixer};
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DispatchMessageW, GetMessageW, HWND_MESSAGE, MSG, PM_NOREMOVE,
        PeekMessageW, TranslateMessage, WM_APP,
    };

    use crate::{clipboard, hook, inject, layout};

    /// Posted by the hook to the worker: the user pressed Shift+Backspace.
    pub const WM_FIX: u32 = WM_APP + 1;

    pub struct App {
        pub fixer: Mutex<Fixer>,
        pub detector: Detector,
        pub english: isize,
        pub thai: isize,
        pub worker: u32,
    }

    pub static APP: OnceLock<App> = OnceLock::new();

    pub fn run() -> Result<(), String> {
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
        println!("  Close this window or press Ctrl+C to quit.");

        let app = App {
            fixer: Mutex::new(Fixer::new()),
            detector,
            english: english.hkl,
            thai: thai.hkl,
            worker,
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
        // SAFETY: plain query.
        let _ = tx.send(unsafe { GetCurrentThreadId() });

        // SAFETY: standard message loop on this thread's queue.
        unsafe {
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                if msg.message == WM_FIX && msg.hwnd.is_null() {
                    fix(owner);
                } else {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        }
    }

    fn fix(owner: HWND) {
        let app = APP.get().unwrap();
        let edit = app.fixer.lock().unwrap().hotkey(&app.detector);
        match edit {
            Some(e) => {
                inject::replace(e.delete, &e.insert);
                switch_to(e.switch_to);
            }
            None => {
                if !convert_selection(owner) {
                    inject::backspace(); // nothing to fix: behave like Backspace
                }
            }
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
            return true;
        };
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
