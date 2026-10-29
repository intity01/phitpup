//! The PhitPup icon, drawn from geometry so every size is sharp.
//!
//! A big white "p" on a coral squircle; upside down the same shape reads as
//! Thai "ป", which is what typing in the wrong layout feels like. The dark
//! bar is the text cursor. Small sizes (tray) use bolder strokes.
//!
//! Coordinates are in a 120 × 120 design grid. Besides drawing, this module
//! writes PNG, ICO and Windows `.res` files with no dependencies, so the
//! build can embed the icon in the exe and `cargo xtask icons` can export it.

/// Which colours to draw in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Active,
    Paused,
}

/// Sizes in the exe's icon, as Windows asks for them at 100–300% scaling.
pub const ICO_SIZES: [u32; 9] = [16, 20, 24, 32, 40, 48, 64, 128, 256];

type Rgb = [f32; 3];

const fn hex(v: u32) -> Rgb {
    [
        ((v >> 16) & 0xFF) as f32 / 255.0,
        ((v >> 8) & 0xFF) as f32 / 255.0,
        (v & 0xFF) as f32 / 255.0,
    ]
}

struct Colors {
    top: Rgb,
    bottom: Rgb,
    mark: Rgb,
    cursor: Rgb,
}

const ACTIVE: Colors = Colors {
    top: hex(0xFF8F5E),
    bottom: hex(0xFF3B1F),
    mark: hex(0xFFFFFF),
    cursor: hex(0x2A0A05),
};

const PAUSED: Colors = Colors {
    top: hex(0xC3C6CC),
    bottom: hex(0x7D828B),
    mark: hex(0xFFFFFF),
    cursor: hex(0x2B2D31),
};

/// Stroke weights; tray sizes need heavier lines to stay readable.
struct Weights {
    stroke: f32,
    ring_r: f32,
    ring_w: f32,
    /// The cursor bar: left, top, right, bottom.
    cursor: [f32; 4],
}

const REGULAR: Weights = Weights {
    stroke: 15.0,
    ring_r: 9.5,
    ring_w: 11.0,
    cursor: [92.0, 84.0, 101.0, 106.0],
};

const BOLD: Weights = Weights {
    stroke: 19.0,
    ring_r: 9.0,
    ring_w: 14.0,
    cursor: [95.0, 86.0, 107.0, 110.0],
};

fn length(x: f32, y: f32) -> f32 {
    (x * x + y * y).sqrt()
}

/// Distance from p to the segment a–b.
fn segment(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    length(p.0 - a.0 - t * dx, p.1 - a.1 - t * dy)
}

/// Distance from p to a quarter circle around c, in the quadrant given by
/// the signs (sx, sy) (screen coordinates, y down).
fn quarter(p: (f32, f32), c: (f32, f32), r: f32, sx: f32, sy: f32) -> f32 {
    let (dx, dy) = (p.0 - c.0, p.1 - c.1);
    if dx * sx >= 0.0 && dy * sy >= 0.0 {
        (length(dx, dy) - r).abs()
    } else {
        let a = length(dx - sx * r, dy);
        let b = length(dx, dy - sy * r);
        a.min(b)
    }
}

/// Signed distance to a rounded rectangle (negative inside).
fn rounded(p: (f32, f32), [l, t, r, b]: [f32; 4], rad: f32) -> f32 {
    let (cx, cy) = ((l + r) / 2.0, (t + b) / 2.0);
    let qx = (p.0 - cx).abs() - ((r - l) / 2.0 - rad);
    let qy = (p.1 - cy).abs() - ((b - t) / 2.0 - rad);
    length(qx.max(0.0), qy.max(0.0)) + qx.max(qy).min(0.0) - rad
}

/// Signed distance to the squircle filling the grid (approximate near the
/// edge, which is all anti-aliasing needs).
fn squircle(p: (f32, f32)) -> f32 {
    let (x, y) = ((p.0 - 60.0) / 60.0, (p.1 - 60.0) / 60.0);
    (x.abs().powi(5) + y.abs().powi(5)).powf(0.2) * 60.0 - 60.0
}

/// Signed distance to the white "p" (stem, bowl and the ป-style loop).
fn mark(p: (f32, f32), w: &Weights) -> f32 {
    let half = w.stroke / 2.0;
    let path = segment(p, (40.0, 140.0), (40.0, 54.0))
        .min(quarter(p, (62.0, 54.0), 22.0, -1.0, -1.0))
        .min(segment(p, (62.0, 32.0), (64.0, 32.0)))
        .min(quarter(p, (64.0, 54.0), 22.0, 1.0, -1.0))
        .min(segment(p, (86.0, 54.0), (86.0, 62.0)));
    let ring = (length(p.0 - 76.0, p.1 - 68.0) - w.ring_r).abs() - w.ring_w / 2.0;
    (path - half).min(ring)
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

/// The icon as `size` × `size` RGBA pixels (straight alpha, rows top down).
pub fn render(size: u32, tone: Tone) -> Vec<u8> {
    let colors = if tone == Tone::Active {
        &ACTIVE
    } else {
        &PAUSED
    };
    let weights = if size <= 24 { &BOLD } else { &REGULAR };
    // Larger icons get a little room around the tile, like other app icons.
    let inset = if size >= 32 { size as f32 / 32.0 } else { 0.0 };
    let unit = 120.0 / (size as f32 - 2.0 * inset);
    let cover = |d: f32| (0.5 - d / unit).clamp(0.0, 1.0);

    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let p = (
                (x as f32 + 0.5 - inset) * unit,
                (y as f32 + 0.5 - inset) * unit,
            );
            let tile = cover(squircle(p));
            let mut c = mix(colors.top, colors.bottom, (p.1 / 120.0).clamp(0.0, 1.0));
            c = mix(c, colors.mark, cover(mark(p, weights)));
            let bar = weights.cursor;
            let radius = (bar[2] - bar[0]) / 2.0;
            c = mix(c, colors.cursor, cover(rounded(p, bar, radius)));
            for v in c {
                out.push((v.clamp(0.0, 1.0) * 255.0).round() as u8);
            }
            out.push((tile * 255.0).round() as u8);
        }
    }
    out
}

// ---- PNG -------------------------------------------------------------------

/// A PNG file of `size` × `size` RGBA pixels.
pub fn png(rgba: &[u8], size: u32) -> Vec<u8> {
    let row = size as usize * 4;
    // Filter "Up" for every row: flat areas and vertical gradients become
    // runs that compress well.
    let mut raw = Vec::with_capacity(rgba.len() + size as usize);
    for y in 0..size as usize {
        raw.push(2);
        for x in 0..row {
            let above = if y == 0 { 0 } else { rgba[(y - 1) * row + x] };
            raw.push(rgba[y * row + x].wrapping_sub(above));
        }
    }

    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend(size.to_be_bytes());
    ihdr.extend(size.to_be_bytes());
    ihdr.extend([8, 6, 0, 0, 0]); // 8-bit RGBA, no interlace
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &zlib(&raw));
    chunk(&mut out, b"IEND", &[]);
    out
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend((data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend(kind);
    out.extend(data);
    let crc = crc32(&out[start..]);
    out.extend(crc.to_be_bytes());
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    b << 16 | a
}

/// Writes bits least-significant first, as deflate wants.
struct Bits {
    out: Vec<u8>,
    acc: u32,
    n: u32,
}

impl Bits {
    fn put(&mut self, value: u32, count: u32) {
        self.acc |= value << self.n;
        self.n += count;
        while self.n >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
    }

    /// A Huffman code, which deflate stores most-significant bit first.
    fn code(&mut self, code: u32, len: u32) {
        self.put(code.reverse_bits() >> (32 - len), len);
    }

    fn literal(&mut self, sym: u32) {
        match sym {
            0..=143 => self.code(0x30 + sym, 8),
            144..=255 => self.code(0x190 + sym - 144, 9),
            256..=279 => self.code(sym - 256, 7),
            _ => self.code(0xC0 + sym - 280, 8),
        }
    }
}

const LENGTH_BASE: [u32; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u32; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u32; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u32; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

/// zlib stream: one fixed-Huffman deflate block with greedy LZ77 matching.
fn zlib(data: &[u8]) -> Vec<u8> {
    const WINDOW: usize = 32768;
    let mut bits = Bits {
        out: vec![0x78, 0x01],
        acc: 0,
        n: 0,
    };
    bits.put(1, 1); // last block
    bits.put(1, 2); // fixed Huffman codes

    let hash = |i: usize| {
        ((data[i] as usize) << 10 ^ (data[i + 1] as usize) << 5 ^ data[i + 2] as usize) & 0x7FFF
    };
    let mut head = vec![usize::MAX; 0x8000];
    let mut i = 0;
    while i < data.len() {
        let mut best = (0usize, 0usize);
        if i + 3 <= data.len() {
            let h = hash(i);
            let j = head[h];
            head[h] = i;
            if j != usize::MAX && i - j <= WINDOW {
                let max = (data.len() - i).min(258);
                let len = (0..max).take_while(|&k| data[j + k] == data[i + k]).count();
                if len >= 3 {
                    best = (len, i - j);
                }
            }
        }
        if best.0 == 0 {
            bits.literal(data[i] as u32);
            i += 1;
            continue;
        }
        let (len, dist) = (best.0 as u32, best.1 as u32);
        let li = LENGTH_BASE.iter().rposition(|&b| b <= len).unwrap();
        bits.literal(257 + li as u32);
        bits.put(len - LENGTH_BASE[li], LENGTH_EXTRA[li]);
        let di = DIST_BASE.iter().rposition(|&b| b <= dist).unwrap();
        bits.code(di as u32, 5);
        bits.put(dist - DIST_BASE[di], DIST_EXTRA[di]);
        // Index the skipped positions so later matches can find them.
        for k in i + 1..(i + best.0).min(data.len().saturating_sub(2)) {
            head[hash(k)] = k;
        }
        i += best.0;
    }
    bits.literal(256); // end of block
    bits.put(0, 7); // flush the last partial byte
    let mut out = bits.out;
    out.extend(adler32(data).to_be_bytes());
    out
}

// ---- ICO and RES ---------------------------------------------------------

/// PNG images of the icon at [`ICO_SIZES`].
pub fn images(tone: Tone) -> Vec<(u32, Vec<u8>)> {
    ICO_SIZES
        .iter()
        .map(|&s| (s, png(&render(s, tone), s)))
        .collect()
}

/// An `.ico` file holding PNG images (Windows Vista and later).
pub fn ico(images: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend([0u8, 0, 1, 0]);
    out.extend((images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len() as u32;
    for (size, data) in images {
        out.extend(entry_head(*size));
        out.extend((data.len() as u32).to_le_bytes());
        out.extend(offset.to_le_bytes());
        offset += data.len() as u32;
    }
    for (_, data) in images {
        out.extend(data);
    }
    out
}

/// Width, height, colour count, reserved, planes, bits per pixel.
fn entry_head(size: u32) -> [u8; 8] {
    let b = if size >= 256 { 0 } else { size as u8 };
    [b, b, 0, 0, 1, 0, 32, 0]
}

/// A compiled resource file (`.res`) with the images as icon group 1, the
/// one Explorer shows for the exe, and `version` (from [`version_info`]) if
/// given. MSVC's linker takes it as an input.
pub fn res(images: &[(u32, Vec<u8>)], version: Option<&[u8]>) -> Vec<u8> {
    const RT_ICON: u16 = 3;
    const RT_GROUP_ICON: u16 = 14;
    const RT_VERSION: u16 = 16;
    let mut out = Vec::new();
    resource(&mut out, 0, 0, &[]); // the empty entry every .res starts with
    let mut group = vec![0u8, 0, 1, 0];
    group.extend((images.len() as u16).to_le_bytes());
    for (n, (size, data)) in images.iter().enumerate() {
        let id = n as u16 + 1;
        resource(&mut out, RT_ICON, id, data);
        group.extend(entry_head(*size));
        group.extend((data.len() as u32).to_le_bytes());
        group.extend(id.to_le_bytes());
    }
    resource(&mut out, RT_GROUP_ICON, 1, &group);
    if let Some(v) = version {
        resource(&mut out, RT_VERSION, 1, v);
    }
    out
}

/// A `VS_VERSIONINFO` block: what Explorer shows under Properties > Details,
/// and what installers and antivirus read. `version` is "major.minor.patch";
/// `strings` are pairs like ("ProductName", "PhitPup"), in US English.
pub fn version_info(version: &str, strings: &[(&str, &str)]) -> Vec<u8> {
    let mut parts = version.split('.').map(|p| p.parse::<u16>().unwrap_or(0));
    let mut part = || parts.next().unwrap_or(0);
    let (major, minor, patch) = (part(), part(), part());
    let ms = (u32::from(major) << 16) | u32::from(minor);
    let ls = u32::from(patch) << 16;
    let mut fixed = Vec::new();
    for v in [
        0xFEEF_04BD, // signature
        0x0001_0000, // structure version
        ms,
        ls, // file version
        ms,
        ls,          // product version
        0x3F,        // flags mask
        0,           // flags
        0x0004_0004, // VOS_NT_WINDOWS32
        1,           // VFT_APP
        0,
        0,
        0, // subtype, date
    ] {
        fixed.extend(u32::to_le_bytes(v));
    }
    let table: Vec<u8> = strings
        .iter()
        .flat_map(|(k, v)| {
            let text = utf16(v);
            version_node(k, &text, (text.len() / 2) as u16, 1, &[])
        })
        .collect();
    let table = version_node("040904B0", &[], 0, 1, &table);
    let string_info = version_node("StringFileInfo", &[], 0, 1, &table);
    let translation = [0x09u8, 0x04, 0xB0, 0x04]; // US English, Unicode
    let var = version_node("Translation", &translation, 4, 0, &[]);
    let var_info = version_node("VarFileInfo", &[], 0, 1, &var);
    let children = [string_info, var_info].concat();
    version_node("VS_VERSION_INFO", &fixed, fixed.len() as u16, 0, &children)
}

/// One node of a version block: length, value length (in words for text,
/// bytes otherwise), type (1 = text), key, value, then the children, each
/// part starting on a 4-byte boundary.
fn version_node(key: &str, value: &[u8], value_len: u16, kind: u16, children: &[u8]) -> Vec<u8> {
    let mut out = vec![0, 0];
    out.extend(value_len.to_le_bytes());
    out.extend(kind.to_le_bytes());
    out.extend(utf16(key));
    let pad = |out: &mut Vec<u8>| {
        while !out.len().is_multiple_of(4) {
            out.push(0);
        }
    };
    pad(&mut out);
    out.extend(value);
    if !children.is_empty() {
        pad(&mut out);
        out.extend(children);
    }
    let len = out.len() as u16;
    out[..2].copy_from_slice(&len.to_le_bytes());
    // The next sibling starts aligned; the padding is not counted.
    pad(&mut out);
    out
}

/// UTF-16LE with the closing NUL.
fn utf16(s: &str) -> Vec<u8> {
    s.encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect()
}

fn resource(out: &mut Vec<u8>, kind: u16, id: u16, data: &[u8]) {
    let (flags, lang): (u16, u16) = if kind == 0 { (0, 0) } else { (0x1010, 0x0409) };
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(32u32.to_le_bytes()); // header size
    out.extend([0xFF, 0xFF]);
    out.extend(kind.to_le_bytes());
    out.extend([0xFF, 0xFF]);
    out.extend(id.to_le_bytes());
    out.extend(0u32.to_le_bytes()); // data version
    out.extend(flags.to_le_bytes());
    out.extend(lang.to_le_bytes());
    out.extend(0u32.to_le_bytes()); // version
    out.extend(0u32.to_le_bytes()); // characteristics
    out.extend(data);
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_are_clear_and_middle_is_solid() {
        for size in [16, 32, 256] {
            let px = render(size, Tone::Active);
            let alpha = |x: u32, y: u32| px[((y * size + x) * 4 + 3) as usize];
            assert_eq!(alpha(0, 0), 0, "{size}");
            assert_eq!(alpha(size / 2, size / 2), 255, "{size}");
        }
    }

    #[test]
    fn checksums_match_known_values() {
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }

    #[test]
    fn png_compresses_the_icon() {
        let px = render(256, Tone::Active);
        let file = png(&px, 256);
        assert!(file.starts_with(b"\x89PNG"));
        assert!(file.len() < px.len() / 4, "{} bytes", file.len());
    }

    #[test]
    fn res_is_dword_aligned_with_one_group() {
        let imgs: Vec<_> = [16u32, 32]
            .iter()
            .map(|&s| (s, png(&render(s, Tone::Active), s)))
            .collect();
        let r = res(&imgs, None);
        assert_eq!(r.len() % 4, 0);
        assert_eq!(&r[..8], &[0, 0, 0, 0, 32, 0, 0, 0]);
    }

    #[test]
    fn version_info_holds_numbers_and_strings() {
        let v = version_info("0.3.12", &[("ProductName", "PhitPup")]);
        assert_eq!(usize::from(u16::from_le_bytes([v[0], v[1]])), v.len());
        let at = v
            .windows(4)
            .position(|w| w == [0xBD, 0x04, 0xEF, 0xFE])
            .unwrap();
        // File version 0.3.12.0, after the signature and structure version.
        assert_eq!(&v[at + 8..at + 16], &[3, 0, 0, 0, 0, 0, 12, 0]);
        assert!(
            v.windows(utf16("PhitPup").len())
                .any(|w| w == utf16("PhitPup"))
        );
        let r = res(&[], Some(&v));
        assert_eq!(r.len() % 4, 0);
    }
}
