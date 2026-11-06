//! Developer tasks.
//!
//! `cargo xtask gen-layouts` regenerates `crates/phitpup-core/src/layouts/data.rs`
//! from the keyboard layout DLLs that ship with Windows. It reads each DLL's
//! `KbdLayerDescriptor` tables directly, so no layout is loaded into or
//! activated in the user's session.
//!
//! `cargo xtask icons` exports the icon (drawn by `phitpup_core::icon`) to
//! `assets/icon/` for the website and README: an `.ico`, PNGs and a preview.
//!
//! `cargo xtask e2e` checks the built app in a real Notepad with real key
//! presses (see `e2e.rs`).
//!
//! `cargo xtask installer-art` draws the installer's side and corner images
//! (red pen on ruled paper, as in the app) into `target/installer-art/`, where
//! `installer/phitpup.iss` picks them up. They are not committed.

mod corpus;
#[cfg(windows)]
mod e2e;
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
        Some("english-words") => train::english_words(&root()),
        Some("icons") => icons(),
        Some("installer-art") => installer_art(),
        Some("msix") => msix(&args[1..]),
        #[cfg(windows)]
        Some("e2e") => e2e::run(&root(), &args[1..]),
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
                "usage: cargo xtask <gen-layouts | fetch-corpus | train-model | english-words | eval-detector | icons | installer-art | msix | e2e>"
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

/// The Microsoft Store package, `<target>/msix/PhitPup.msix`, from
/// `<target>/release/phitpup.exe` (or `--exe <path>`) and
/// `msix/AppxManifest.xml`. The identity comes from Partner Center through
/// `PHITPUP_STORE_NAME`, `PHITPUP_STORE_PUBLISHER` (`CN=...`) and
/// `PHITPUP_STORE_PUBLISHER_NAME`; without them it gets test values, for a
/// package signed with a local test certificate. The Store signs the real
/// one. phitpup-update.exe is left out: the Store does the updating.
fn msix(args: &[String]) {
    use phitpup_core::icon::{Tone, png_wh, render};
    let root = root();
    let exe = match args {
        [flag, path] if flag == "--exe" => PathBuf::from(path),
        [] => target_dir().join("release").join("phitpup.exe"),
        _ => {
            eprintln!("usage: cargo xtask msix [--exe <phitpup.exe>]");
            std::process::exit(2);
        }
    };
    assert!(
        exe.is_file(),
        "{} is missing: cargo build --release -p phitpup",
        exe.display()
    );
    let out = target_dir().join("msix");
    let layout = out.join("layout");
    let _ = std::fs::remove_dir_all(&layout);
    std::fs::create_dir_all(layout.join("Assets")).expect("create target/msix/layout/");
    std::fs::copy(&exe, layout.join("phitpup.exe")).expect("copy phitpup.exe");

    // The icon centred on a transparent w × h canvas, `icon` pixels wide.
    let art = |name: &str, w: u32, h: u32, icon: u32| {
        let src = render(icon, Tone::Active);
        let mut px = vec![0u8; (w * h * 4) as usize];
        let (ox, oy) = ((w - icon) / 2, (h - icon) / 2);
        for y in 0..icon {
            for x in 0..icon {
                let s = ((y * icon + x) * 4) as usize;
                let d = (((y + oy) * w + x + ox) * 4) as usize;
                px[d..d + 4].copy_from_slice(&src[s..s + 4]);
            }
        }
        std::fs::write(layout.join("Assets").join(name), png_wh(&px, w, h))
            .expect("write package image");
    };
    // Scale 100/200/400% for tiles; target sizes for the taskbar and Start.
    for (scale, f) in [(100, 1), (200, 2), (400, 4)] {
        art(
            &format!("Square150x150Logo.scale-{scale}.png"),
            150 * f,
            150 * f,
            100 * f,
        );
        art(
            &format!("Wide310x150Logo.scale-{scale}.png"),
            310 * f,
            150 * f,
            100 * f,
        );
        art(
            &format!("Square44x44Logo.scale-{scale}.png"),
            44 * f,
            44 * f,
            44 * f,
        );
        art(
            &format!("StoreLogo.scale-{scale}.png"),
            50 * f,
            50 * f,
            50 * f,
        );
    }
    for size in [16, 24, 32, 48, 256] {
        art(
            &format!("Square44x44Logo.targetsize-{size}.png"),
            size,
            size,
            size,
        );
        art(
            &format!("Square44x44Logo.targetsize-{size}_altform-unplated.png"),
            size,
            size,
            size,
        );
    }

    let var = |k: &str, test: &str| std::env::var(k).unwrap_or_else(|_| test.into());
    let version = format!("{}.0", env!("CARGO_PKG_VERSION"));
    let manifest = std::fs::read_to_string(root.join("msix").join("AppxManifest.xml"))
        .expect("read msix/AppxManifest.xml")
        .replace("{{NAME}}", &var("PHITPUP_STORE_NAME", "PhitPup.Test"))
        .replace(
            "{{PUBLISHER}}",
            &var("PHITPUP_STORE_PUBLISHER", "CN=PhitPup Test"),
        )
        .replace(
            "{{PUBLISHER_NAME}}",
            &var("PHITPUP_STORE_PUBLISHER_NAME", "PhitPup Test"),
        )
        .replace("{{VERSION}}", &version);
    std::fs::write(layout.join("AppxManifest.xml"), manifest).expect("write AppxManifest.xml");

    // makeappx.exe from the newest Windows SDK, or MAKEAPPX.
    let makeappx = std::env::var_os("MAKEAPPX").map(PathBuf::from).or_else(|| {
        let bin = PathBuf::from(r"C:\Program Files (x86)\Windows Kits\10\bin");
        let mut kits: Vec<_> = std::fs::read_dir(&bin)
            .ok()?
            .flatten()
            .map(|e| e.path())
            .collect();
        kits.sort();
        kits.into_iter()
            .rev()
            .map(|k| k.join("x64").join("makeappx.exe"))
            .find(|p| p.is_file())
    });
    let makeappx =
        makeappx.expect("makeappx.exe not found: install the Windows SDK or set MAKEAPPX");

    // resources.pri lets Windows pick the scale-/targetsize- images.
    let makepri = makeappx.with_file_name("makepri.exe");
    let config = out.join("priconfig.xml");
    let run = |cmd: &mut std::process::Command, what: &str| {
        let r = cmd.output().unwrap_or_else(|e| panic!("run {what}: {e}"));
        if !r.status.success() {
            eprintln!("{}", String::from_utf8_lossy(&r.stdout));
            panic!("{what} failed");
        }
    };
    run(
        std::process::Command::new(&makepri)
            .args(["createconfig", "/o", "/pv", "10.0.0", "/dq", "en-US", "/cf"])
            .arg(&config),
        "makepri createconfig",
    );
    // One package, so one resources.pri: drop the split into resource packs.
    let xml = std::fs::read_to_string(&config).expect("read priconfig.xml");
    if let (Some(a), Some(b)) = (xml.find("<packaging>"), xml.find("</packaging>")) {
        let xml = format!("{}{}", &xml[..a], &xml[b + "</packaging>".len()..]);
        std::fs::write(&config, xml).expect("write priconfig.xml");
    }
    run(
        std::process::Command::new(&makepri)
            .args(["new", "/o", "/pr"])
            .arg(&layout)
            .arg("/cf")
            .arg(&config)
            .arg("/mn")
            .arg(layout.join("AppxManifest.xml"))
            .arg("/of")
            .arg(layout.join("resources.pri")),
        "makepri new",
    );
    let package = out.join("PhitPup.msix");
    let status = std::process::Command::new(makeappx)
        .args(["pack", "/o", "/d"])
        .arg(&layout)
        .arg("/p")
        .arg(&package)
        .status()
        .expect("run makeappx");
    assert!(status.success(), "makeappx failed");
    println!("{} (version {version})", package.display());
}

/// The installer's images at 100–200% scaling, as 24-bit BMPs: the side
/// panel (164 × 314 at 100%) and the corner of the inner pages (55 × 55).
fn installer_art() {
    use phitpup_core::icon::{Tone, render};
    let dir = root().join("target").join("installer-art");
    std::fs::create_dir_all(&dir).expect("create target/installer-art/");
    const PAPER: [f32; 3] = [251.0, 247.0, 240.0];
    const RULE: [f32; 3] = [230.0, 222.0, 210.0];
    const MARGIN: [f32; 3] = [237.0, 198.0, 184.0];
    const PEN: [f32; 3] = [217.0, 58.0, 18.0];
    const WHITE: [f32; 3] = [255.0, 255.0, 255.0];

    struct Canvas {
        w: usize,
        h: usize,
        px: Vec<[f32; 3]>,
    }
    impl Canvas {
        fn new(w: usize, h: usize, bg: [f32; 3]) -> Self {
            Self {
                w,
                h,
                px: vec![bg; w * h],
            }
        }
        fn blend(&mut self, x: usize, y: usize, c: [f32; 3], a: f32) {
            if x < self.w && y < self.h && a > 0.0 {
                let p = &mut self.px[y * self.w + x];
                for i in 0..3 {
                    p[i] += (c[i] - p[i]) * a.min(1.0);
                }
            }
        }
        /// An anti-aliased stroke along the points (a pen line).
        fn stroke(&mut self, pts: &[(f32, f32)], width: f32, c: [f32; 3]) {
            let r = width / 2.0;
            for y in 0..self.h {
                for x in 0..self.w {
                    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                    let d = pts
                        .windows(2)
                        .map(|s| segment_distance((px, py), s[0], s[1]))
                        .fold(f32::MAX, f32::min);
                    self.blend(x, y, c, (r + 0.5 - d).clamp(0.0, 1.0));
                }
            }
        }
        /// The icon's straight-alpha RGBA pixels with their top left at (x, y).
        fn image(&mut self, rgba: &[u8], size: usize, x0: usize, y0: usize) {
            for y in 0..size {
                for x in 0..size {
                    let p = &rgba[(y * size + x) * 4..][..4];
                    let c = [f32::from(p[0]), f32::from(p[1]), f32::from(p[2])];
                    self.blend(x0 + x, y0 + y, c, f32::from(p[3]) / 255.0);
                }
            }
        }
        fn bmp(&self) -> Vec<u8> {
            let row = (self.w * 3).div_ceil(4) * 4;
            let size = 54 + row * self.h;
            let mut out = Vec::with_capacity(size);
            out.extend(b"BM");
            out.extend((size as u32).to_le_bytes());
            out.extend([0u8; 4]);
            out.extend(54u32.to_le_bytes());
            out.extend(40u32.to_le_bytes());
            out.extend((self.w as i32).to_le_bytes());
            out.extend((self.h as i32).to_le_bytes());
            out.extend(1u16.to_le_bytes());
            out.extend(24u16.to_le_bytes());
            out.extend([0u8; 24]);
            for y in (0..self.h).rev() {
                let start = out.len();
                for p in &self.px[y * self.w..][..self.w] {
                    out.extend([p[2], p[1], p[0]].map(|v| v.round() as u8));
                }
                out.resize(start + row, 0);
            }
            out
        }
    }
    fn segment_distance(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len = dx * dx + dy * dy;
        let t = if len == 0.0 {
            0.0
        } else {
            (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len).clamp(0.0, 1.0)
        };
        ((p.0 - a.0 - t * dx).powi(2) + (p.1 - a.1 - t * dy).powi(2)).sqrt()
    }

    for (pct, s) in [
        (100, 1.0f32),
        (125, 1.25),
        (150, 1.5),
        (175, 1.75),
        (200, 2.0),
    ] {
        let (w, h) = ((164.0 * s).round() as usize, (314.0 * s).round() as usize);
        let mut c = Canvas::new(w, h, PAPER);
        // Ruled lines and the red margin line of a school notebook.
        let mut y = 30.0 * s;
        while y < h as f32 {
            c.stroke(&[(0.0, y), (w as f32, y)], 1.0 * s, RULE);
            y += 22.0 * s;
        }
        c.stroke(&[(26.0 * s, 0.0), (26.0 * s, h as f32)], 1.2 * s, MARGIN);
        // The logo, circled in pen as the app circles the current page.
        let size = (72.0 * s).round() as usize;
        let (cx, cy) = (88.0 * s, 118.0 * s);
        let x0 = (cx - size as f32 / 2.0).round() as usize;
        let y0 = (cy - size as f32 / 2.0).round() as usize;
        c.image(&render(size as u32, Tone::Active), size, x0, y0);
        let lp: Vec<(f32, f32)> = (0..=100)
            .map(|i| {
                // A little more than one turn, slightly tilted, like a quick loop.
                let t = i as f32 / 100.0 * std::f32::consts::TAU * 1.08 - 0.9;
                let (rx, ry) = (58.0 * s * (1.0 + 0.03 * t.sin()), 52.0 * s);
                let (x, y) = (rx * t.cos(), ry * t.sin());
                let tilt: f32 = -0.12;
                (
                    cx + x * tilt.cos() - y * tilt.sin(),
                    cy + x * tilt.sin() + y * tilt.cos(),
                )
            })
            .collect();
        c.stroke(&lp, 2.4 * s, PEN);
        // A tick, as on the app's switches.
        c.stroke(
            &[
                (56.0 * s, 228.0 * s),
                (72.0 * s, 244.0 * s),
                (112.0 * s, 200.0 * s),
            ],
            4.0 * s,
            PEN,
        );
        let name = format!("wizard-{pct}.bmp");
        std::fs::write(dir.join(&name), c.bmp()).expect("write installer art");
        println!("target/installer-art/{name}");

        let n = (55.0 * s).round() as usize;
        let mut c = Canvas::new(n, n, WHITE);
        let size = (44.0 * s).round() as usize;
        let at = (n - size) / 2;
        c.image(&render(size as u32, Tone::Active), size, at, at);
        let name = format!("small-{pct}.bmp");
        std::fs::write(dir.join(&name), c.bmp()).expect("write installer art");
        println!("target/installer-art/{name}");
    }
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

/// Cargo's output folder: CARGO_TARGET_DIR, or `.cargo/config.toml`'s.
fn target_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("CARGO_TARGET_DIR") {
        return root().join(dir);
    }
    let config =
        std::fs::read_to_string(root().join(".cargo").join("config.toml")).unwrap_or_default();
    let dir = config
        .lines()
        .find_map(|l| l.trim().strip_prefix("target-dir"))
        .and_then(|v| v.split('"').nth(1))
        .unwrap_or("target");
    root().join(dir)
}

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
