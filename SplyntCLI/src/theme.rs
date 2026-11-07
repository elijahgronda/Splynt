//! Colors and the placeholder art palette, the same ones the iPhone app uses.

use ratatui::style::{Color, Modifier, Style};

pub const BG: Color = Color::Rgb(18, 18, 18);
pub const SIDEBAR: Color = Color::Rgb(0, 0, 0);
pub const BAR: Color = Color::Rgb(13, 13, 13);
pub const CARD: Color = Color::Rgb(24, 24, 24);
pub const SEL: Color = Color::Rgb(42, 42, 42);
pub const CHIP: Color = Color::Rgb(42, 42, 42);
pub const INK: Color = Color::Rgb(255, 255, 255);
pub const MUTED: Color = Color::Rgb(179, 179, 179);
pub const DIM: Color = Color::Rgb(115, 115, 115);
pub const FAINT: Color = Color::Rgb(70, 70, 70);
static ACCENT_RGB: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x2BE06B);

pub fn set_accent(rgb: (u8, u8, u8)) {
    let v = ((rgb.0 as u32) << 16) | ((rgb.1 as u32) << 8) | rgb.2 as u32;
    ACCENT_RGB.store(v, std::sync::atomic::Ordering::Relaxed);
}

/// The accent color the listener picked under Appearance.
#[allow(non_snake_case)]
pub fn ACCENT() -> Color {
    let v = ACCENT_RGB.load(std::sync::atomic::Ordering::Relaxed);
    Color::Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}
pub const ACCENT_INK: Color = Color::Rgb(0, 0, 0);
pub const ERROR: Color = Color::Rgb(241, 94, 108);

const PALETTE: [u32; 16] = [
    0xE0552B, 0x2B6FE0, 0x9B2BE0, 0xE02B7A, 0x2BAEE0, 0xE0B02B, 0x4AE02B, 0xE02B2B, 0x6B4AE0,
    0x2BE0A0, 0xE07A2B, 0x2B4AE0, 0xC02BE0, 0xE0D02B, 0x2BE05F, 0xE02B4A,
];

/// FNV-1a, the same stable hash as `Palette.stableHash`.
pub fn stable_hash(seed: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in seed.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

pub fn seed_rgb(seed: &str) -> (u8, u8, u8) {
    let c = PALETTE[(stable_hash(seed) % PALETTE.len() as u64) as usize];
    ((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

pub fn mix(a: (u8, u8, u8), b: (u8, u8, u8), t: f32) -> Color {
    let l = |x: u8, y: u8| (x as f32 * (1.0 - t) + y as f32 * t) as u8;
    Color::Rgb(l(a.0, b.0), l(a.1, b.1), l(a.2, b.2))
}

pub fn glyph(seed: &str) -> char {
    (b'A' + (stable_hash(seed) % 26) as u8) as char
}

pub fn readable_ink(rgb: (u8, u8, u8)) -> Color {
    let lum = 0.299 * rgb.0 as f32 + 0.587 * rgb.1 as f32 + 0.114 * rgb.2 as f32;
    if lum > 153.0 { Color::Rgb(20, 20, 20) } else { Color::Rgb(240, 240, 240) }
}

pub fn ink() -> Style {
    Style::default().fg(INK)
}
pub fn muted() -> Style {
    Style::default().fg(MUTED)
}
pub fn dim() -> Style {
    Style::default().fg(DIM)
}
pub fn accent() -> Style {
    Style::default().fg(ACCENT())
}
pub fn bold() -> Style {
    Style::default().fg(INK).add_modifier(Modifier::BOLD)
}

/// Whether the terminal takes 24-bit color. Terminal.app doesn't, and it
/// reads `38;2;42;42;42` as four separate codes, where 42 means a green
/// background. Terminals that do support it set `COLORTERM`.
pub fn truecolor() -> bool {
    // Every Windows console since Windows 10 does 24-bit color, and Windows
    // Terminal doesn't set COLORTERM.
    if cfg!(windows) {
        return true;
    }
    let ct = std::env::var("COLORTERM").unwrap_or_default();
    let term = std::env::var("TERM").unwrap_or_default();
    ct.contains("truecolor") || ct.contains("24bit") || term.contains("direct")
}

/// Swaps every RGB color in a drawn frame for the nearest of the 256
/// standard colors.
pub fn downgrade(buf: &mut ratatui::buffer::Buffer) {
    for cell in buf.content.iter_mut() {
        cell.fg = to_256(cell.fg);
        cell.bg = to_256(cell.bg);
    }
}

fn to_256(c: Color) -> Color {
    let Color::Rgb(r, g, b) = c else { return c };
    // 6x6x6 cube, indexes 16..=231.
    const LEVELS: [i32; 6] = [0, 95, 135, 175, 215, 255];
    let near = |v: u8| {
        let v = v as i32;
        (0..6).min_by_key(|&i| (LEVELS[i] - v).abs()).unwrap()
    };
    let (ri, gi, bi) = (near(r), near(g), near(b));
    let cube = (LEVELS[ri], LEVELS[gi], LEVELS[bi]);
    // Gray ramp, indexes 232..=255, levels 8 + 10n.
    let avg = (r as i32 + g as i32 + b as i32) / 3;
    let gn = ((avg - 8 + 5) / 10).clamp(0, 23);
    let gray = 8 + 10 * gn;
    let dist = |(x, y, z): (i32, i32, i32)| {
        (x - r as i32).pow(2) + (y - g as i32).pow(2) + (z - b as i32).pow(2)
    };
    if dist((gray, gray, gray)) < dist(cube) {
        Color::Indexed(232 + gn as u8)
    } else {
        Color::Indexed(16 + 36 * ri as u8 + 6 * gi as u8 + bi as u8)
    }
}
