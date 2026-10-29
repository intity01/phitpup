//! Embeds the PhitPup icon in the exe, so Explorer, the taskbar and the
//! "start with Windows" list show it. The icon is drawn by
//! `phitpup_core::icon` and written as a compiled resource (`.res`), which
//! MSVC's linker accepts directly; no resource compiler is needed. The same
//! file carries the version details Explorer shows under Properties.
//!
//! It also records when the exe was built (`PHITPUP_BUILT`, Unix seconds),
//! for the update reminder.

fn main() {
    println!("cargo:rerun-if-changed=../phitpup-core/src/icon.rs");
    let built = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    println!("cargo:rustc-env=PHITPUP_BUILT={built}");
    let windows = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    if !(windows && msvc) {
        return;
    }
    use phitpup_core::icon::{Tone, images, res, version_info};
    let version = env!("CARGO_PKG_VERSION");
    let info = version_info(
        version,
        &[
            ("CompanyName", "sukudone.team-dev"),
            (
                "FileDescription",
                "PhitPup - fixes text typed in the wrong keyboard layout",
            ),
            ("FileVersion", version),
            ("InternalName", "phitpup"),
            (
                "LegalCopyright",
                "\u{a9} 2026 Intity01. Free, not for sale (PolyForm Noncommercial 1.0.0).",
            ),
            ("OriginalFilename", "phitpup.exe"),
            ("ProductName", "PhitPup"),
            ("ProductVersion", version),
            ("Comments", "https://intity01.github.io/phitpup/"),
        ],
    );
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("phitpup.res");
    std::fs::write(&out, res(&images(Tone::Active), Some(&info))).expect("write phitpup.res");
    println!("cargo:rustc-link-arg-bins={}", out.display());
}
