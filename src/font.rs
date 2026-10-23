//! The flight font (3x5 pixels, advance 4, drawn below as ASCII art) and text drawing into
//! palette canvases.
//!
//! Strings are byte strings: bytes >= 0x80 change the foreground colour to `b & 0x7F`
//! (use [`ink`]), 0 ends the string. Lower case is drawn as upper case.

use bevy::prelude::*;
use stealth_formats::font::{Font, Glyph};

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
        s.iter()
            .take_while(|&&b| b != 0)
            .filter(|&&b| b < 0x80)
            .filter_map(|&b| f.glyph(b))
            .map(|g| g.advance as i32)
            .sum()
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
            let mut rows: Vec<u16> = art
                .iter()
                .map(|r| {
                    r.bytes()
                        .enumerate()
                        .filter(|&(_, b)| b == b'#')
                        .fold(0, |m, (x, _)| m | 0x8000 >> x)
                })
                .collect();
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

pub fn draw_text(
    c: &mut Canvas,
    f: &Font,
    x: i32,
    y: i32,
    s: &[u8],
    fg: u8,
    bg: Option<u8>,
) -> i32 {
    draw_text_scaled(c, f, x, y, s, fg, bg, 1)
}

#[allow(clippy::too_many_arguments)]
pub fn draw_text_scaled(
    c: &mut Canvas,
    f: &Font,
    mut x: i32,
    y: i32,
    s: &[u8],
    mut fg: u8,
    bg: Option<u8>,
    scale: i32,
) -> i32 {
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

/// Vector stroke font of the high-resolution HUD: each glyph is polylines on a 4 x 6 grid
/// (y down), separated by `|`.
const STROKES: &[(char, &str)] = &[
    ('0', "0,0 4,0 4,6 0,6 0,0"),
    ('1', "1,1 2,0 2,6|1,6 3,6"),
    ('2', "0,0 4,0 4,3 0,3 0,6 4,6"),
    ('3', "0,0 4,0 4,6 0,6|1,3 4,3"),
    ('4', "0,0 0,3 4,3|4,0 4,6"),
    ('5', "4,0 0,0 0,3 4,3 4,6 0,6"),
    ('6', "4,0 0,0 0,6 4,6 4,3 0,3"),
    ('7', "0,0 4,0 4,1 2,6"),
    ('8', "0,0 4,0 4,6 0,6 0,0|0,3 4,3"),
    ('9', "4,3 0,3 0,0 4,0 4,6 0,6"),
    ('A', "0,6 0,2 2,0 4,2 4,6|0,3.5 4,3.5"),
    ('B', "0,0 3,0 4,1 4,2 3,3 0,3|3,3 4,4 4,5 3,6 0,6 0,0"),
    ('C', "4,0 0,0 0,6 4,6"),
    ('D', "0,0 3,0 4,1 4,5 3,6 0,6 0,0"),
    ('E', "4,0 0,0 0,6 4,6|0,3 3,3"),
    ('F', "4,0 0,0 0,6|0,3 3,3"),
    ('G', "4,0 0,0 0,6 4,6 4,3 2,3"),
    ('H', "0,0 0,6|4,0 4,6|0,3 4,3"),
    ('I', "1,0 3,0|2,0 2,6|1,6 3,6"),
    ('J', "4,0 4,6 0,6 0,4"),
    ('K', "0,0 0,6|4,0 0,3 4,6"),
    ('L', "0,0 0,6 4,6"),
    ('M', "0,6 0,0 2,3 4,0 4,6"),
    ('N', "0,6 0,0 4,6 4,0"),
    ('O', "0,0 4,0 4,6 0,6 0,0"),
    ('P', "0,6 0,0 4,0 4,3 0,3"),
    ('Q', "0,0 4,0 4,6 0,6 0,0|2.5,4.5 4,6"),
    ('R', "0,6 0,0 4,0 4,3 0,3 4,6"),
    ('S', "4,0 0,0 0,3 4,3 4,6 0,6"),
    ('T', "0,0 4,0|2,0 2,6"),
    ('U', "0,0 0,6 4,6 4,0"),
    ('V', "0,0 2,6 4,0"),
    ('W', "0,0 1,6 2,3 3,6 4,0"),
    ('X', "0,0 4,6|4,0 0,6"),
    ('Y', "0,0 2,3 4,0|2,3 2,6"),
    ('Z', "0,0 4,0 0,6 4,6"),
    ('-', "1,3 3,3"),
    ('+', "1,3 3,3|2,2 2,4"),
    ('.', "2,5.7 2,6"),
    (',', "2,5 1.5,7"),
    (':', "2,1.7 2,2|2,4.3 2,4.6"),
    ('/', "0,6 4,0"),
    ('>', "0,0 4,3 0,6"),
    ('<', "4,0 0,3 4,6"),
    ('(', "3,0 1,2 1,4 3,6"),
    (')', "1,0 3,2 3,4 1,6"),
    ('=', "1,2 3,2|1,4 3,4"),
    ('\'', "2,0 2,1.5"),
    ('!', "2,0 2,4|2,5.7 2,6"),
    ('?', "0,1 1,0 3,0 4,1 4,2 2,3 2,4|2,5.7 2,6"),
    ('%', "0,6 4,0|0,0 1,0 1,1 0,1 0,0|3,5 4,5 4,6 3,6 3,5"),
    ('#', "1,0 1,6|3,0 3,6|0,2 4,2|0,4 4,4"),
    ('*', "2,1 2,5|0,2 4,4|0,4 4,2"),
    ('[', "3,0 1,0 1,6 3,6"),
    (']', "1,0 3,0 3,6 1,6"),
    (';', "2,1.7 2,2|2,5 1.5,7"),
    ('\\', "0,0 4,6"),
    ('`', "1.5,0 2.5,1.5"),
    ('_', "0,6 4,6"),
    ('"', "1,0 1,1.5|3,0 3,1.5"),
    ('^', "1,1.5 2,0 3,1.5"),
    ('&', "4,6 1,2.5 1,0.5 1.5,0 2.5,0 3,0.5 3,1.5 0,4 0,6 2,6 4,3.5"),
    ('$', "4,1 0,1 0,3 4,3 4,5 0,5|2,0 2,6"),
    ('§', "3,0 1,0 1,2 3,3 3,4|1,2 1,3 3,4 3,6 1,6"),
    ('²', "1,0 3,0 3,1.5 1,1.5 1,3 3,3"),
    ('µ', "0,2 0,7|0,6 4,6 4,2"),
    ('£', "4,1 3,0 2,0 1,1 1,5 0,6 4,6|0,3 3,3"),
    ('¤', "1,2 3,2 3,4 1,4 1,2|0,1 1,2|4,1 3,2|0,5 1,4|4,5 3,4"),
    ('¨', "1,0.5 1,0.8|3,0.5 3,0.8"),
    ('°', "1.5,0 2.5,0 2.5,1 1.5,1 1.5,0"),
    ('´', "1.5,1.5 2.5,0"),
    ('~', "0.5,3.5 1.5,2.5 2.5,3.5 3.5,2.5"),
    ('|', "2,-0.5 2,6.5"),
    ('½', "0,1 1,0 1,3|0,6 4,0|2.5,3.5 4,3.5 4,4.8 2.5,4.8 2.5,6 4,6"),
    ('º', "1,0 3,0 3,2 1,2 1,0|1,3 3,3"),
    ('¡', "2,0 2,0.3|2,2 2,6"),
    ('«', "2,1.5 0,3 2,4.5|4,1.5 2,3 4,4.5"),
    ('»', "0,1.5 2,3 0,4.5|2,1.5 4,3 2,4.5"),
    ('ß', "0,6 0,1 1,0 3,0 4,1 4,2 2,3 4,4 4,5 3,6 1,6"),
    ('Æ', "0,6 0,2 2,0 4,0|2,0 2,6 4,6|0,3.5 2,3.5|2,3 3.5,3"),
    ('Ø', "0,0 4,0 4,6 0,6 0,0|0,6.5 4,-0.5"),
];

/// Small letters (x-height 2 to 6, l and t rise to 0), the bases of the accented letters that
/// the French, Italian, Hungarian, Czech and Slovak keycaps print in lower case.
const SMALL: &[(char, &str)] = &[
    ('a', "0,2 4,2 4,6 0,6 0,4 4,4"),
    ('c', "4,2 0,2 0,6 4,6"),
    ('e', "4,6 0,6 0,2 4,2 4,4 0,4"),
    ('i', "2,2 2,6"),
    ('l', "2,0 2,6"),
    ('n', "0,6 0,2 4,2 4,6"),
    ('o', "0,2 4,2 4,6 0,6 0,2"),
    ('r', "0,6 0,2 4,2"),
    ('s', "4,2 0,2 0,4 4,4 4,6 0,6"),
    ('t', "2,0 2,6 4,6|0,2 4,2"),
    ('u', "0,2 0,6 4,6 4,2"),
    ('y', "0,2 0,4 4,4|4,2 4,7.5 1,7.5"),
    ('z', "0,2 4,2 0,6 4,6"),
];

/// The diacritics: strokes over the x-height (y 0 to 1.3) or under the baseline, whether they
/// are raised over a capital, and the base letters with their accented letters, char by char.
const MARKS: &[(&str, bool, &str, &str)] = &[
    ("1.5,1.3 2.5,0", true, "aeiouyAEIOUY", "áéíóúýÁÉÍÓÚÝ"),
    ("1.5,0 2.5,1.3", true, "aeiouAEIOU", "àèìòùÀÈÌÒÙ"),
    ("1,1.3 2,0 3,1.3", true, "aeiouAEIOU", "âêîôûÂÊÎÔÛ"),
    ("1,0.5 1,0.8|3,0.5 3,0.8", true, "aeiouyAEIOU", "äëïöüÿÄËÏÖÜ"),
    ("1,0 2,1.3 3,0", true, "censrzCENSRZLT", "čěňšřžČĚŇŠŘŽĽŤ"),
    ("1,1.3 1.8,0|2.5,1.3 3.3,0", true, "ouOU", "őűŐŰ"),
    ("1.5,0 2.5,0 2.5,1 1.5,1 1.5,0", true, "auAU", "åůÅŮ"),
    ("0.5,1 1.5,0.3 2.5,1 3.5,0.3", true, "noNO", "ñõÑÕ"),
    // The caron of ľ and ť is an apostrophe beside the stem.
    ("3.5,0 3,1.3", false, "lt", "ľť"),
    ("2,6 2.5,7 1.5,7.5", false, "cC", "çÇ"),
];

/// Raise of a diacritic over a capital (grid units).
const CAPITAL_MARK: f32 = -1.7;

/// The strokes of `ch` (ASCII upper-cased): its glyph, or an accented letter's base and its
/// diacritic with the diacritic's vertical offset.
fn glyph(ch: char) -> Option<(&'static str, Option<(&'static str, f32)>)> {
    let ch = ch.to_ascii_uppercase();
    if let Some((_, g)) = STROKES.iter().find(|(k, _)| *k == ch) {
        return Some((g, None));
    }
    MARKS.iter().find_map(|&(mark, raise, bases, accented)| {
        let (base, _) = bases.chars().zip(accented.chars()).find(|&(_, a)| a == ch)?;
        let table = if base.is_ascii_uppercase() { STROKES } else { SMALL };
        let (_, g) = table.iter().find(|(k, _)| *k == base)?;
        let dy = if raise && base.is_ascii_uppercase() { CAPITAL_MARK } else { 0.0 };
        Some((*g, Some((mark, dy))))
    })
}

/// Whether the stroke font draws `ch` (ASCII upper-cased; a space is blank).
pub fn stroke_has(ch: char) -> bool {
    ch == ' ' || glyph(ch).is_some()
}

/// Advance of the stroke font in grid units (4 wide + 2 apart).
const STROKE_ADVANCE: f32 = 6.0;

/// Width in pixels of `s` drawn with [`stroke_text`] at glyph height `h`.
pub fn stroke_width(s: &str, h: f32) -> f32 {
    let n = s.chars().count() as f32;
    if n == 0.0 { 0.0 } else { (n * STROKE_ADVANCE - 2.0) * h / 6.0 }
}

/// `ch` in capitals if the stroke font has its capital, else as it is ("ß", whose capital is
/// "SS", and the micro sign, whose capital is Greek, stay).
fn capital(ch: char) -> char {
    let mut upper = ch.to_uppercase();
    match (upper.next(), upper.next()) {
        (Some(u), None) if glyph(u).is_some() => u,
        _ => ch,
    }
}

/// Draws `s` (upper-cased) with the stroke font, top-left at (x, y), glyph height `h`, pen
/// width `w`. Returns the x after the text.
pub fn stroke_text(c: &mut Canvas, x: f32, y: f32, s: &str, h: f32, w: f32, col: u8) -> f32 {
    stroke_chars(c, x, y, s.chars().map(capital), h, w, col)
}

/// [`stroke_text`] keeping the accented small letters small: the key labels of the layouts
/// that print them so ("é" on AZERTY), as the CONTROLS dialog shows them.
pub fn stroke_text_keep_case(
    c: &mut Canvas,
    x: f32,
    y: f32,
    s: &str,
    h: f32,
    w: f32,
    col: u8,
) -> f32 {
    stroke_chars(c, x, y, s.chars(), h, w, col)
}

fn stroke_chars(
    c: &mut Canvas,
    x: f32,
    y: f32,
    s: impl Iterator<Item = char>,
    h: f32,
    w: f32,
    col: u8,
) -> f32 {
    let u = h / 6.0;
    let mut x = x;
    for ch in s {
        if let Some((g, mark)) = glyph(ch) {
            stroke_polylines(c, Vec2::new(x, y), g, u, w, col);
            if let Some((m, dy)) = mark {
                stroke_polylines(c, Vec2::new(x, y + dy * u), m, u, w, col);
            }
        }
        x += STROKE_ADVANCE * u;
    }
    x
}

/// Draws a glyph's polylines (grid units of `u` pixels) from `origin`.
fn stroke_polylines(c: &mut Canvas, origin: Vec2, g: &str, u: f32, w: f32, col: u8) {
    for poly in g.split('|') {
        let pts: Vec<Vec2> = poly
            .split(' ')
            .filter_map(|p| p.split_once(','))
            .map(|(a, b)| {
                origin
                    + Vec2::new(a.parse::<f32>().unwrap_or(0.0), b.parse::<f32>().unwrap_or(0.0))
                        * u
            })
            .collect();
        if pts.len() == 1 {
            c.stroke(pts[0], pts[0], w, col);
        }
        c.polyline(&pts, w, col);
    }
}

/// Horizontal anchor of [`stroke_text_at`].
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Centre,
    Right,
}

/// [`stroke_text`] anchored at (x, y): x by `align`, y at the text's vertical middle.
#[allow(clippy::too_many_arguments)]
pub fn stroke_text_at(
    c: &mut Canvas,
    x: f32,
    y: f32,
    s: &str,
    h: f32,
    w: f32,
    col: u8,
    align: Align,
) -> f32 {
    let width = stroke_width(s, h);
    let x0 = match align {
        Align::Left => x,
        Align::Centre => x - width / 2.0,
        Align::Right => x - width,
    };
    stroke_text(c, x0, y - h / 2.0, s, h, w, col)
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

    #[test]
    fn stroke_font_covers_hud_text() {
        for ch in "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ-+.,:/><()=".chars() {
            assert!(STROKES.iter().any(|(k, _)| *k == ch), "no stroke glyph for {ch:?}");
        }
    }

    /// The keycaps of the European layouts, as the CONTROLS dialog names them (the kneeboard's
    /// key guide falls back to the US names for the others).
    #[test]
    fn stroke_font_covers_european_keys() {
        for ch in crate::keyboard_layout::EUROPEAN_KEYCAPS.chars() {
            assert!(stroke_has(ch), "no stroke glyph for {ch:?}");
        }
        for (_, _, bases, accented) in MARKS {
            assert_eq!(bases.chars().count(), accented.chars().count(), "{accented}");
        }
        assert!(!stroke_has('ф'));
    }

    /// Text is drawn in capitals where the font has them.
    #[test]
    fn capitals() {
        assert_eq!(capital('a'), 'A');
        assert_eq!(capital('ç'), 'Ç');
        assert_eq!(capital('ő'), 'Ő');
        assert_eq!(capital('ß'), 'ß');
        assert_eq!(capital('µ'), 'µ');
        assert_eq!(capital('_'), '_');
    }
}
