<p align="center"><img src="assets/icon/phitpup-128.png" width="96" alt=""></p>

<h1 align="center">ผิดปุ๊บ PhitPup</h1>

<p align="center">Fixes text typed in the wrong keyboard layout, Thai ⇄ English, on Windows.<br>
แก้ข้อความที่พิมพ์ผิดภาษา ไทย ⇄ อังกฤษ บน Windows</p>

---

Forgot to switch layouts and typed `l;ylfu`? PhitPup sees you meant **สวัสดี** and fixes it:

- **Suggest** (default): a small bubble under the caret; <kbd>Tab</kbd> accepts, <kbd>Esc</kbd> hides,
  <kbd>Shift</kbd>+<kbd>Esc</kbd> never suggests that word again.
- **Auto**: <kbd>Space</kbd> fixes a word PhitPup is very sure about.
- **Hotkey**: <kbd>Shift</kbd>+<kbd>Backspace</kbd> fixes the last word (again: one word further back,
  then undo), or the selected text. Works in every mode, and right after a fix it puts the word back.

It switches the keyboard layout for you, supports Kedmanee and Pattachote, and turns itself off in
password fields, password managers, Remote Desktop and full-screen windows. Click the tray icon for
the settings window.

**Private by design:** the app has no networking code; what you type is kept in memory only for the
word in progress. Only the separate `phitpup-update.exe`, which the installer adds, goes online, and
only when you click **Check for updates**: it fetches the version number and, if you agree, the
installer.

## Download

Get `PhitPup-Setup.exe` from [Releases](../../releases/latest) (Windows 10/11, x64). It installs for
your account only, with no admin rights, adds a Start menu entry and can be removed from Windows
Settings. `phitpup.exe` from the same page is the program alone, which runs without installing.
Check either against the published SHA-256. Windows SmartScreen may warn because the program is not
code-signed yet: **More info → Run anyway**.

## Build

Rust (the version in `rust-toolchain.toml`) with the MSVC toolchain.

```
cargo run --release -p phitpup          # the app (PHITPUP_DEBUG=1 for a log of decisions)
cargo test --workspace
cargo build --release -p phitpup-web --target wasm32-unknown-unknown   # the website's try-it box
```

The installer is built with [Inno Setup](https://jrsoftware.org/isinfo.php) 6.5 from
`installer/phitpup.iss`: `iscc installer\phitpup.iss` after the release build writes
`target\installer\PhitPup-Setup.exe`.

| Path | What |
|---|---|
| `crates/phitpup-core` | Layouts, conversion, detector, fixer, settings, icon. No OS calls; builds for WASM. |
| `crates/phitpup` | The Windows app: keyboard hook, injection, bubble, tray, settings window. |
| `crates/phitpup-update` | `phitpup-update.exe`, the updater: the only networking code, run on request. |
| `crates/phitpup-web` | The core for the website (wasm32, no bindings crate). |
| `installer` | The Inno Setup script for `PhitPup-Setup.exe`. |
| `site/` | The website (GitHub Pages). |
| `xtask` | Layout table generation, model training, icon export. |
| `SPEC.md` | Decisions and milestones. |

## Credits

Made by **Intity01** · sukudone.team-dev. MIT license; data sources in [THIRD_PARTY.md](THIRD_PARTY.md).
