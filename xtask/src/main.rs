//! Developer tasks.
//!
//! `cargo xtask gen-layouts` regenerates `crates/phitpup-core/src/layouts/data.rs`
//! from the keyboard layout DLLs that ship with Windows. It reads each DLL's
//! `KbdLayerDescriptor` tables directly, so no layout is loaded into or
//! activated in the user's session.
//!
//! `cargo xtask icons` exports the icon (drawn by `phitpup_core::icon`) to
//! `assets/icon/` for the website and README: an `.ico`, PNGs and a preview.

mod corpus;
mod train;

use std::fmt::Write as _;
use std::path::PathBuf;

use phitpup_core::layouts::{SCANCODES, typeable};

/// (Rust const name, DLL, human name)
const LAYOUTS: &[(&str, &str, &str)] = &[
    ("US_QWERTY", "KBDUS.DLL", "US QWERTY"),
    ("KEDMANEE", "KBDTH0.DLL", "Thai Kedmanee"),
    ("PATTACHOTE", "KBDTH1.DLL", "Thai Pattachote"),
];

/// Training corpora. Downloaded into `corpus/` (not committed); only the model
/// built from them is. (file name, URL, license)
const CORPUS: &[(&str, &str, &str)] = &[
    (
        "tnc_freq.txt",
        "https://raw.githubusercontent.com/PyThaiNLP/pythainlp/dev/pythainlp/corpus/tnc_freq.txt",
        "CC0-1.0 (PyThaiNLP, Thai National Corpus word frequencies)",
    ),
    (
        "en.dic",
        "https://raw.githubusercontent.com/wooorm/dictionaries/main/dictionaries/en/index.dic",
        "SCOWL / Hunspell en, MIT-like (see en.license)",
    ),
    (
        "en.license",
        "https://raw.githubusercontent.com/wooorm/dictionaries/main/dictionaries/en/license",
        "-",
    ),
];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("gen-layouts") => gen_layouts(),
        Some("fetch-corpus") => fetch_corpus(),
        Some("train-model") => train::train(&root()),
        Some("icons") => icons(),
        Some("eval-detector") => {
            let lexicon = match args.get(1).map(String::as_str) {
                Some("--lexicon") => args
                    .get(2)
                    .and_then(|n| n.parse().ok())
                    .expect("--lexicon <words>"),
                _ => train::THAI_LEXICON,
            };
            train::eval(&root(), lexicon)
        }
        _ => {
            eprintln!(
                "usage: cargo xtask <gen-layouts | fetch-corpus | train-model | eval-detector | icons>"
            );
            std::process::exit(2);
        }
    }
}

fn icons() {
    use phitpup_core::icon::{Tone, ico, images, png, render};
    let dir = root().join("assets").join("icon");
    std::fs::create_dir_all(&dir).expect("create assets/icon/");
    let write = |name: &str, data: &[u8]| {
        std::fs::write(dir.join(name), data).expect("write icon file");
        println!("assets/icon/{name}  ({} bytes)", data.len());
    };
    write("phitpup.ico", &ico(&images(Tone::Active)));
    for size in [16, 24, 32, 48, 64, 128, 256, 512] {
        write(
            &format!("phitpup-{size}.png"),
            &png(&render(size, Tone::Active), size),
        );
    }
    write("phitpup-paused-32.png", &png(&render(32, Tone::Paused), 32));
    write("phitpup.svg", SVG.as_bytes());
}

/// The icon for the web, on the same 120-unit grid as `phitpup_core::icon`
/// (the squircle is approximated with curves).
const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 120 120" role="img" aria-label="PhitPup">
  <defs>
    <linearGradient id="bg" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#FF8F5E"/>
      <stop offset="1" stop-color="#FF3B1F"/>
    </linearGradient>
    <clipPath id="tile"><path d="M60 0C108 0 120 12 120 60S108 120 60 120 0 108 0 60 12 0 60 0Z"/></clipPath>
  </defs>
  <g clip-path="url(#tile)">
    <rect width="120" height="120" fill="url(#bg)"/>
    <path d="M40 140V54A22 22 0 0 1 62 32H64A22 22 0 0 1 86 54V62" fill="none" stroke="#fff" stroke-width="15" stroke-linecap="round" stroke-linejoin="round"/>
    <circle cx="76" cy="68" r="9.5" fill="none" stroke="#fff" stroke-width="11"/>
    <rect x="92" y="84" width="9" height="22" rx="4.5" fill="#2A0A05"/>
  </g>
</svg>
"##;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn fetch_corpus() {
    let dir = root().join("corpus");
    std::fs::create_dir_all(&dir).expect("create corpus/");
    for &(file, url, license) in CORPUS {
        let dest = dir.join(file);
        let ok = std::process::Command::new("curl")
            .args([
                "--fail",
                "--location",
                "--silent",
                "--show-error",
                "--output",
            ])
            .arg(&dest)
            .arg(url)
            .status()
            .is_ok_and(|s| s.success());
        assert!(ok, "download failed: {url}");
        let size = std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
        println!("{file:<14} {size:>9} bytes  {license}");
    }
}

fn gen_layouts() {
    let mut out = String::new();
    out.push_str(
        "// @generated by `cargo xtask gen-layouts` from the Windows layout DLLs. Do not edit.\n\
         // Columns: scancode, base, shift. The trailing comment shows the glyphs.\n\n\
         use super::Key;\n\n\
         const fn k(sc: u16, base: Option<char>, shift: Option<char>) -> Key {\n    \
         Key { sc, base, shift }\n}\n",
    );

    for &(name, dll, human) in LAYOUTS {
        let table = kbd::dump(dll).unwrap_or_else(|e| panic!("{dll}: {e}"));
        writeln!(
            out,
            "\n/// {human} ({dll})\npub(super) static {name}: &[Key] = &["
        )
        .unwrap();
        for &sc in SCANCODES {
            let [base, shift] = table.get(&sc).copied().unwrap_or([None, None]);
            let line = format!("    k(0x{sc:02X}, {}, {}),", lit(base), lit(shift));
            writeln!(out, "{line:<52}// {} {}", glyph(base), glyph(shift)).unwrap();
        }
        out.push_str("];\n");
    }

    let dest: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "..",
        "crates",
        "phitpup-core",
        "src",
        "layouts",
        "data.rs",
    ]
    .iter()
    .collect();
    std::fs::write(&dest, out).expect("write data.rs");
    println!("wrote {}", dest.display());
}

fn lit(c: Option<char>) -> String {
    match c {
        None => "None".into(),
        Some('\'') => r"Some('\'')".into(),
        Some('\\') => r"Some('\\')".into(),
        Some(c) if c.is_ascii_graphic() => format!("Some('{c}')"),
        Some(c) => format!("Some('\\u{{{:04X}}}')", c as u32),
    }
}

/// Thai combining marks get a dotted circle so the comment stays readable.
fn glyph(c: Option<char>) -> String {
    match c {
        None => "·".into(),
        Some(c @ ('\u{0E31}' | '\u{0E34}'..='\u{0E3A}' | '\u{0E47}'..='\u{0E4E}')) => {
            format!("\u{25CC}{c}")
        }
        Some(c) => c.to_string(),
    }
}

#[cfg(windows)]
mod kbd {
    //! Minimal reader for the `KBDTABLES` structure from the Windows DDK's kbd.h.

    use std::collections::HashMap;
    use std::ffi::c_void;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LoadLibraryW(name: *const u16) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
        fn FreeLibrary(module: *mut c_void) -> i32;
    }

    #[repr(C)]
    struct KbdTables {
        char_modifiers: *const Modifiers,
        vk_to_wchar_table: *const VkToWcharTable,
        dead_key: *const c_void,
        key_names: *const c_void,
        key_names_ext: *const c_void,
        key_names_dead: *const c_void,
        vsc_to_vk: *const u16,
        max_vsc_to_vk: u8,
    }

    #[repr(C)]
    struct Modifiers {
        vk_to_bit: *const c_void,
        max_mod_bits: u16,
        mod_number: [u8; 0], // variable length, indexed by modifier bits
    }

    #[repr(C)]
    struct VkToWcharTable {
        rows: *const u8,
        n_modifications: u8,
        cb_size: u8,
    }

    const KBDSHIFT: usize = 1;
    const WCH_NONE: u16 = 0xF000;
    const WCH_DEAD: u16 = 0xF001;
    const WCH_LGTR: u16 = 0xF002;

    /// Scancode -> [base, shift] characters for one layout DLL.
    pub fn dump(dll: &str) -> Result<HashMap<u16, [Option<char>; 2]>, String> {
        let wide: Vec<u16> = dll.encode_utf16().chain([0]).collect();
        // SAFETY: layout DLLs export a single descriptor function and have no
        // DllMain side effects; all pointers below come from that descriptor and
        // stay valid until FreeLibrary.
        unsafe {
            let module = LoadLibraryW(wide.as_ptr());
            if module.is_null() {
                return Err("LoadLibraryW failed".into());
            }
            let result = read(module);
            FreeLibrary(module);
            result
        }
    }

    unsafe fn read(module: *mut c_void) -> Result<HashMap<u16, [Option<char>; 2]>, String> {
        unsafe {
            let proc = GetProcAddress(module, c"KbdLayerDescriptor".as_ptr().cast());
            if proc.is_null() {
                return Err("no KbdLayerDescriptor export".into());
            }
            let descriptor: extern "system" fn() -> *const KbdTables = std::mem::transmute(proc);
            let tables = &*descriptor();

            let mod_number = (*tables.char_modifiers).mod_number.as_ptr();
            let columns = [*mod_number, *mod_number.add(KBDSHIFT)];

            let mut by_vk: HashMap<u8, [Option<char>; 2]> = HashMap::new();
            let mut t = tables.vk_to_wchar_table;
            while !(*t).rows.is_null() {
                let VkToWcharTable {
                    rows,
                    n_modifications,
                    cb_size,
                } = *t;
                let mut row = rows;
                while *row != 0 {
                    let vk = *row;
                    // 0xFF rows carry dead-key characters for the row above.
                    if vk != 0xFF && !by_vk.contains_key(&vk) {
                        let at = |col: u8| -> Option<char> {
                            if col >= n_modifications {
                                return None;
                            }
                            let w = row.add(2 + 2 * col as usize).cast::<u16>().read_unaligned();
                            match w {
                                WCH_NONE | WCH_DEAD | WCH_LGTR => None,
                                w => char::from_u32(w.into()).and_then(super::typeable),
                            }
                        };
                        by_vk.insert(vk, [at(columns[0]), at(columns[1])]);
                    }
                    row = row.add(cb_size.into());
                }
                t = t.add(1);
            }

            let mut by_sc = HashMap::new();
            for sc in 0..tables.max_vsc_to_vk as u16 {
                let vk = (*tables.vsc_to_vk.add(sc.into()) & 0xFF) as u8;
                if let Some(&chars) = by_vk.get(&vk) {
                    by_sc.insert(sc, chars);
                }
            }
            Ok(by_sc)
        }
    }
}

#[cfg(not(windows))]
mod kbd {
    use std::collections::HashMap;

    pub fn dump(_dll: &str) -> Result<HashMap<u16, [Option<char>; 2]>, String> {
        Err("gen-layouts reads Windows layout DLLs and only runs on Windows".into())
    }
}
