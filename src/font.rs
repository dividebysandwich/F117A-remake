//! The flight font (3x5 pixels, advance 4, drawn below as ASCII art) and text drawing into
//! palette canvases.
//!
//! Strings are byte strings: bytes >= 0x80 change the foreground colour to `b & 0x7F`
//! (use [`ink`]), 0 ends the string. Lower case is drawn as upper case.

use bevy::prelude::*;
use f117_formats::font::{Font, Glyph};

use crate::canvas::Canvas;

/// Font used for all HUD, MFD and panel text in flight.
pub const HUD_FONT: usize = 0;

#[derive(Resource, Clone)]
pub struct Fonts(pub Vec<Font>);

impl Fonts {
    pub fn builtin() -> Self {
        Self(vec![flight_font()])
    }

    pub fn font(&self, i: usize) -> &Font {
        &self.0[i.min(self.0.len() - 1)]
    }

    /// Draws `s` with the flight font (transparent background). Returns the x after the text.
    pub fn text(&self, c: &mut Canvas, x: i32, y: i32, s: &[u8], fg: u8) -> i32 {
        draw_text(c, self.font(HUD_FONT), x, y, s, fg, None)
    }

    /// `text` with every font pixel drawn as a `scale` x `scale` block.
    pub fn text_scaled(&self, c: &mut Canvas, x: i32, y: i32, s: &[u8], fg: u8, scale: i32) -> i32 {
        draw_text_scaled(c, self.font(HUD_FONT), x, y, s, fg, None, scale)
    }

    /// Width of `s` in unscaled pixels (colour-change bytes take no room).
    pub fn width(&self, s: &[u8]) -> i32 {
        let f = self.font(HUD_FONT);
        s.iter().take_while(|&&b| b != 0).filter(|&&b| b < 0x80).filter_map(|&b| f.glyph(b)).map(|g| g.advance as i32).sum()
    }

    /// Glyph height of the flight font in unscaled pixels.
    pub fn height(&self) -> i32 {
        self.font(HUD_FONT).height as i32
    }

}

/// Glyphs of the flight font, `#` = set pixel.
const GLYPHS: &[(char, [&str; 5])] = &[
    (' ', ["...", "...", "...", "...", "..."]),
    ('!', [".#.", ".#.", ".#.", "...", ".#."]),
    ('"', ["#.#", "#.#", "...", "...", "..."]),
    ('#', ["#.#", "###", "#.#", "###", "#.#"]),
    ('$', [".##", "##.", ".#.", ".##", "##."]),
    ('%', ["#..", "..#", ".#.", "#..", "..#"]),
    ('&', [".#.", "#.#", ".#.", "#.#", ".##"]),
    ('\'', [".#.", ".#.", "...", "...", "..."]),
    ('(', ["..#", ".#.", ".#.", ".#.", "..#"]),
    (')', ["#..", ".#.", ".#.", ".#.", "#.."]),
    ('*', ["...", "#.#", ".#.", "#.#", "..."]),
    ('+', ["...", ".#.", "###", ".#.", "..."]),
    (',', ["...", "...", "...", ".#.", "#.."]),
    ('-', ["...", "...", "###", "...", "..."]),
    ('.', ["...", "...", "...", "...", ".#."]),
    ('/', ["..#", "..#", ".#.", "#..", "#.."]),
    ('0', ["###", "#.#", "#.#", "#.#", "###"]),
    ('1', [".#.", "##.", ".#.", ".#.", "###"]),
    ('2', ["###", "..#", "###", "#..", "###"]),
    ('3', ["###", "..#", ".##", "..#", "###"]),
    ('4', ["#.#", "#.#", "###", "..#", "..#"]),
    ('5', ["###", "#..", "###", "..#", "###"]),
    ('6', ["###", "#..", "###", "#.#", "###"]),
    ('7', ["###", "..#", "..#", ".#.", ".#."]),
    ('8', ["###", "#.#", "###", "#.#", "###"]),
    ('9', ["###", "#.#", "###", "..#", "###"]),
    (':', ["...", ".#.", "...", ".#.", "..."]),
    (';', ["...", ".#.", "...", ".#.", "#.."]),
    ('<', ["..#", ".#.", "#..", ".#.", "..#"]),
    ('=', ["...", "###", "...", "###", "..."]),
    ('>', ["#..", ".#.", "..#", ".#.", "#.."]),
    ('?', ["###", "..#", ".##", "...", ".#."]),
    ('@', ["###", "#.#", "###", "#..", "###"]),
    ('A', [".#.", "#.#", "###", "#.#", "#.#"]),
    ('B', ["##.", "#.#", "##.", "#.#", "##."]),
    ('C', [".##", "#..", "#..", "#..", ".##"]),
    ('D', ["##.", "#.#", "#.#", "#.#", "##."]),
    ('E', ["###", "#..", "##.", "#..", "###"]),
    ('F', ["###", "#..", "##.", "#..", "#.."]),
    ('G', [".##", "#..", "#.#", "#.#", ".##"]),
    ('H', ["#.#", "#.#", "###", "#.#", "#.#"]),
    ('I', ["###", ".#.", ".#.", ".#.", "###"]),
    ('J', ["..#", "..#", "..#", "#.#", ".#."]),
    ('K', ["#.#", "#.#", "##.", "#.#", "#.#"]),
    ('L', ["#..", "#..", "#..", "#..", "###"]),
    ('M', ["#.#", "###", "###", "#.#", "#.#"]),
    ('N', ["##.", "#.#", "#.#", "#.#", "#.#"]),
    ('O', [".#.", "#.#", "#.#", "#.#", ".#."]),
    ('P', ["##.", "#.#", "##.", "#..", "#.."]),
    ('Q', [".#.", "#.#", "#.#", "##.", ".##"]),
    ('R', ["##.", "#.#", "##.", "#.#", "#.#"]),
    ('S', [".##", "#..", ".#.", "..#", "##."]),
    ('T', ["###", ".#.", ".#.", ".#.", ".#."]),
    ('U', ["#.#", "#.#", "#.#", "#.#", "###"]),
    ('V', ["#.#", "#.#", "#.#", "#.#", ".#."]),
    ('W', ["#.#", "#.#", "###", "###", "#.#"]),
    ('X', ["#.#", "#.#", ".#.", "#.#", "#.#"]),
    ('Y', ["#.#", "#.#", ".#.", ".#.", ".#."]),
    ('Z', ["###", "..#", ".#.", "#..", "###"]),
    ('[', ["##.", "#..", "#..", "#..", "##."]),
    ('\\', ["#..", "#..", ".#.", "..#", "..#"]),
    (']', [".##", "..#", "..#", "..#", ".##"]),
    ('^', [".#.", "#.#", "...", "...", "..."]),
    ('_', ["...", "...", "...", "...", "###"]),
    ('`', ["#..", ".#.", "...", "...", "..."]),
    ('{', [".##", ".#.", "##.", ".#.", ".##"]),
    ('|', [".#.", ".#.", ".#.", ".#.", ".#."]),
    ('}', ["##.", ".#.", ".##", ".#.", "##."]),
    ('~', ["...", ".##", "##.", "...", "..."]),
];

/// Builds the flight font: printable ASCII, lower case sharing the upper-case glyphs, and a
/// blank row below each glyph for opaque backgrounds.
fn flight_font() -> Font {
    let (first, last) = (b' ', b'~');
    let glyphs = (first..=last)
        .map(|code| {
            let ch = code.to_ascii_uppercase() as char;
            let art = GLYPHS.iter().find(|(c, _)| *c == ch).map_or(["..."; 5], |(_, a)| *a);
            let mut rows: Vec<u16> =
                art.iter().map(|r| r.bytes().enumerate().filter(|&(_, b)| b == b'#').fold(0, |m, (x, _)| m | 0x8000 >> x)).collect();
            rows.push(0);
            Glyph { code, width: 3, advance: 4, rows }
        })
        .collect();
    Font { index: 0, first, last, height: 5, extra_height: 1, fixed_width: 3, spacing: 1, glyphs }
}

/// Colour-change byte for strings.
pub const fn ink(colour: u8) -> u8 {
    0x80 | (colour & 0x7F)
}

/// Upper-cases a string before drawing (flight text is upper case).
pub fn upper(s: &str) -> Vec<u8> {
    s.bytes().map(|b| b.to_ascii_uppercase()).collect()
}

pub fn draw_text(c: &mut Canvas, f: &Font, x: i32, y: i32, s: &[u8], fg: u8, bg: Option<u8>) -> i32 {
    draw_text_scaled(c, f, x, y, s, fg, bg, 1)
}

#[allow(clippy::too_many_arguments)]
pub fn draw_text_scaled(c: &mut Canvas, f: &Font, mut x: i32, y: i32, s: &[u8], mut fg: u8, bg: Option<u8>, scale: i32) -> i32 {
    let rows = if bg.is_some() { (f.height + f.extra_height) as i32 } else { f.height as i32 };
    for &b in s {
        if b == 0 {
            break;
        }
        if b >= 0x80 {
            fg = b & 0x7F;
            continue;
        }
        let Some(g) = f.glyph(b) else { continue };
        if let Some(bg) = bg {
            c.fill_rect(x, y, g.advance as i32 * scale, rows * scale, bg);
        }
        for r in 0..f.height as usize {
            for col in 0..g.width as usize {
                if g.pixel(col, r) {
                    if scale == 1 {
                        c.set(x + col as i32, y + r as i32, fg);
                    } else {
                        c.fill_rect(x + col as i32 * scale, y + r as i32 * scale, scale, scale, fg);
                    }
                }
            }
        }
        x += g.advance as i32 * scale;
    }
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flight_font_covers_ascii() {
        let fonts = Fonts::builtin();
        let f = fonts.font(HUD_FONT);
        for code in b' '..=b'~' {
            let g = f.glyph(code).unwrap_or_else(|| panic!("no glyph for {:?}", code as char));
            assert_eq!((g.width, g.advance, g.rows.len()), (3, 4, 6));
            assert!(code == b' ' || g.rows.iter().any(|&r| r != 0), "{:?} is blank", code as char);
        }
        assert_ne!(f.glyph(b'+').unwrap().rows, f.glyph(b'-').unwrap().rows);
        assert_eq!(f.glyph(b'a').unwrap().rows, f.glyph(b'A').unwrap().rows);
        assert_eq!(fonts.width(b"ZM+"), 12);
    }
}
