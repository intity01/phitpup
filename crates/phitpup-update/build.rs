//! Gives phitpup-update.exe the PhitPup icon and its own version details,
//! the same way crates/phitpup/build.rs does for the app.

fn main() {
    println!("cargo:rerun-if-changed=../phitpup-core/src/icon.rs");
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
                "PhitPup updater - downloads and installs new PhitPup versions when asked",
            ),
            ("FileVersion", version),
            ("InternalName", "phitpup-update"),
            (
                "LegalCopyright",
                "\u{a9} 2026 Intity01. Free, not for sale (PolyForm Noncommercial 1.0.0).",
            ),
            ("OriginalFilename", "phitpup-update.exe"),
            ("ProductName", "PhitPup"),
            ("ProductVersion", version),
            ("Comments", "https://intity01.github.io/phitpup/"),
        ],
    );
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("update.res");
    std::fs::write(&out, res(&images(Tone::Active), Some(&info))).expect("write update.res");
    println!("cargo:rustc-link-arg-bins={}", out.display());
}
