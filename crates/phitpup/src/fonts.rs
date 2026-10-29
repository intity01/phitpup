//! The fonts PhitPup draws with, bundled in the exe (SIL Open Font License,
//! see `assets/fonts`): IBM Plex Sans Thai for text, Mali for the red pen's
//! handwriting, Noto Serif Thai for Thai headings. They are added for this
//! process only; nothing is installed on the system.

use std::sync::Once;

use windows_sys::Win32::Graphics::Gdi::AddFontMemResourceEx;

/// Family names, as GDI knows them once loaded.
pub const SANS: &str = "IBM Plex Sans Thai";
pub const SANS_MEDIUM: &str = "IBM Plex Sans Thai Medium";
pub const PEN: &str = "Mali SemiBold";
pub const SERIF_THAI: &str = "Noto Serif Thai";
/// For Latin headings: Noto Serif Thai has no Latin letters. Ships with
/// Windows.
pub const SERIF: &str = "Georgia";

const FILES: [&[u8]; 4] = [
    include_bytes!("../../../assets/fonts/IBMPlexSansThai-Regular.ttf"),
    include_bytes!("../../../assets/fonts/IBMPlexSansThai-Medium.ttf"),
    include_bytes!("../../../assets/fonts/Mali-SemiBold.ttf"),
    include_bytes!("../../../assets/fonts/NotoSerifThai-Bold.ttf"),
];

/// Makes the bundled fonts available to this process. Safe to call often.
pub fn load() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        for data in FILES {
            let mut n = 0u32;
            // SAFETY: GDI copies the font data; the slice is 'static anyway.
            // It writes the font count through the last pointer.
            unsafe {
                AddFontMemResourceEx(
                    data.as_ptr().cast(),
                    data.len() as u32,
                    std::ptr::null(),
                    std::ptr::addr_of_mut!(n),
                )
            };
        }
    });
}
