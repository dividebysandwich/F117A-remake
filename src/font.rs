//! The original bitmap fonts (MGRAPHIC.EXE, exported to `assets/fonts/fonts.ron` by
//! f117-convert) and text drawing into palette canvases (docs/re/cockpit.md section 6).
//!
//! Strings are byte strings: bytes >= 0x80 change the foreground colour to `b & 0x7F`
//! (use [`ink`]), 0 ends the string. Flight text is font 0 (3x5, advance 4), upper-cased.

use bevy::prelude::*;
use f117_formats::font::Font;

use crate::canvas::Canvas;

/// Font used for all HUD, MFD and panel text in flight.
pub const HUD_FONT: usize = 0;

#[derive(Resource, Clone)]
pub struct Fonts(pub Vec<Font>);

impl Fonts {
    pub fn load() -> Self {
        let path = crate::asset_path::asset("fonts/fonts.ron");
        let p = path.display();
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{p}: {e} (run tools/build-assets.sh)"));
        Self(ron::from_str(&text).unwrap_or_else(|e| panic!("{p}: {e}")))
    }

    pub fn font(&self, i: usize) -> &Font {
        &self.0[i.min(self.0.len() - 1)]
    }

    /// Draws `s` with the flight font (transparent background). Returns the x after the text.
    pub fn text(&self, c: &mut Canvas, x: i32, y: i32, s: &[u8], fg: u8) -> i32 {
        draw_text(c, self.font(HUD_FONT), x, y, s, fg, None)
    }

    /// Opaque variant: every character cell (advance x height+extra) is painted `bg` first.
    pub fn text_opaque(&self, c: &mut Canvas, x: i32, y: i32, s: &[u8], fg: u8, bg: u8) -> i32 {
        draw_text(c, self.font(HUD_FONT), x, y, s, fg, Some(bg))
    }
}

/// Colour-change byte for strings.
pub const fn ink(colour: u8) -> u8 {
    0x80 | (colour & 0x7F)
}

/// Upper-cases a string as VGAME does before drawing (`1A18:F2B8`).
pub fn upper(s: &str) -> Vec<u8> {
    s.bytes().map(|b| b.to_ascii_uppercase()).collect()
}

pub fn draw_text(c: &mut Canvas, f: &Font, mut x: i32, y: i32, s: &[u8], mut fg: u8, bg: Option<u8>) -> i32 {
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
            c.fill_rect(x, y, g.advance as i32, rows, bg);
        }
        for r in 0..f.height as usize {
            for col in 0..g.width as usize {
                if g.pixel(col, r) {
                    c.set(x + col as i32, y + r as i32, fg);
                }
            }
        }
        x += g.advance as i32;
    }
    x
}
