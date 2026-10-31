<p align="center"><img src="assets/icon/phitpup-128.png" width="96" alt=""></p>

<h1 align="center">ผิดปุ๊บ PhitPup</h1>

<p align="center">Fixes text typed in the wrong keyboard layout, Thai ⇄ English, on Windows.<br>
แก้ข้อความที่พิมพ์ผิดภาษา ไทย ⇄ อังกฤษ บน Windows</p>

---

Forgot to switch layouts and typed `l;ylfu`? PhitPup sees you meant **สวัสดี** and fixes it:

- **Suggest** (default): a small bubble under the caret; <kbd>Tab</kbd> accepts, <kbd>Esc</kbd> hides,
  <kbd>Shift</kbd>+<kbd>Esc</kbd> (or <kbd>Esc</kbd> three times) never suggests that word again.
  Two switches on the Words page, off unless you turn them on, learn more: a word whose fix you
  take 5 times is then fixed on <kbd>Space</kbd>, and a word you type on past 5 times is left alone.
- **Auto**: <kbd>Space</kbd> fixes a word PhitPup is very sure about.
- **Hotkey**: <kbd>Shift</kbd>+<kbd>Backspace</kbd> fixes the last word (again: one word further back,
  then undo), or the selected text. Works in every mode, and right after a fix it puts the word back.

It also offers to fix Thai typed in an order that looks right but is not (เเ for แ, ํ า for ำ, a tone
mark before its vowel), so search and spell checkers find the word, plus words typed with Caps Lock
left on (hELLO → Hello) and the same word typed twice (the the → the). The settings window is in
Thai or English (following Windows unless you choose) and works from the keyboard. See [what's new](https://intity01.github.io/phitpup/changes.html)
and the [privacy policy](https://intity01.github.io/phitpup/privacy.html).

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
`installer/phitpup.iss`: after the release build and
`cargo run --release -p xtask -- installer-art`,
`iscc installer\phitpup.iss` writes `target\installer\PhitPup-Setup.exe`.

| Path | What |
|---|---|
| `crates/phitpup-core` | Layouts, conversion, detector, fixer, settings, icon. No OS calls; builds for WASM. |
| `crates/phitpup` | The Windows app: keyboard hook, injection, bubble, tray, settings window. |
| `crates/phitpup-update` | `phitpup-update.exe`, the updater: the only networking code, run on request. |
| `crates/phitpup-web` | The core for the website (wasm32, no bindings crate). |
| `installer` | The Inno Setup script for `PhitPup-Setup.exe`. |
| `site/` | The website (GitHub Pages). |
| `xtask` | Layout table generation, model training, icon export. `cargo xtask <task>` in these docs is `cargo run --release -p xtask -- <task>`. |

## Credits

Made by **Intity01** · sukudone.team-dev. Data sources in [THIRD_PARTY.md](THIRD_PARTY.md).

## Code signing policy

Releases are built by GitHub Actions from this repository; see the
[code signing policy](https://intity01.github.io/phitpup/signing.html) for what is signed and by whom.

## License

Free to use for everyone, including at work. From 0.4.0 PhitPup is open source under the
[GNU General Public License version 3](LICENSE): you may use, study, change and share it, and a
changed version you share must come with its source under the same license. Versions 0.3.4 to
0.3.6 were released under the PolyForm Noncommercial License 1.0.0, and 0.3.3 and earlier under
the MIT License. The PhitPup name and logo are not licensed, and official downloads are only on
the website and GitHub releases: see [TRADEMARKS.md](TRADEMARKS.md).

ใช้ฟรีทุกคน รวมถึงใช้ในที่ทำงาน ตั้งแต่รุ่น 0.4.0 เป็นโอเพนซอร์สตามสัญญา GPL รุ่น 3 ใช้ ศึกษา แก้ และแจกต่อได้
ถ้าแจกฉบับที่แก้แล้วต้องเปิดซอร์สด้วยสัญญาเดียวกัน รุ่น 0.3.4–0.3.6 เป็น PolyForm Noncommercial 1.0.0
และรุ่น 0.3.3 ลงไปเป็น MIT
