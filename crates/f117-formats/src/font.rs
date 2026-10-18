//! Bitmap fonts of the MCGA graphics driver (`MGRAPHIC.EXE`). Format: docs/re/cockpit.md
//! section 6 (reference implementation `tools/re/py/extract_font.py`).
//!
//! The driver's data segment starts at the EXE image start. Word `0xD0` = highest font index,
//! words `0xD2 + 2i` = pointer P to font i's bitmap. Header bytes before P:
//! `P-8` first char, `P-7` last char, `P-6` log2(bytes per glyph row)+1, `P-5` fixed width
//! (0 = proportional), `P-4` height, `P-3` spacing (added to the width), `P-2` extra rows painted
//! in opaque mode, `P-1` unused; the per-char width table (proportional fonts) ends at `P-9`.
//! Bitmap: row r of char c is the big-endian word at `P + r*stride + ((c-first) << shift)`,
//! `stride = (last-first+1) << shift`, MSB = leftmost pixel.
//!
//! In strings, bytes >= 0x80 set the foreground colour (`b & 0x7F`) and 0 ends the string.

use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};

/// One glyph: `rows[r]` holds the pixels of row r, bit 15 = leftmost.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Glyph {
    pub code: u8,
    pub width: u8,
    /// width + spacing
    pub advance: u8,
    /// `height + extra_height` rows.
    pub rows: Vec<u16>,
}

impl Glyph {
    #[inline]
    pub fn pixel(&self, x: usize, y: usize) -> bool {
        x < 16 && self.rows.get(y).is_some_and(|r| r & (0x8000 >> x) != 0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Font {
    pub index: u8,
    pub first: u8,
    pub last: u8,
    pub height: u8,
    /// Rows added below the glyphs in opaque mode.
    pub extra_height: u8,
    /// 0 = proportional.
    pub fixed_width: u8,
    pub spacing: u8,
    /// Glyphs for codes `first..=last`.
    pub glyphs: Vec<Glyph>,
}

impl Font {
    pub fn glyph(&self, code: u8) -> Option<&Glyph> {
        if code < self.first || code > self.last {
            return None;
        }
        self.glyphs.get((code - self.first) as usize)
    }
}

/// The load module (image after the header) of an MZ executable.
pub fn mz_image(exe: &[u8]) -> Result<&[u8]> {
    ensure!(exe.len() > 0x1C && &exe[..2] == b"MZ", "not an MZ executable");
    let header = u16::from_le_bytes([exe[8], exe[9]]) as usize * 16;
    ensure!(header <= exe.len(), "MZ header larger than the file");
    Ok(&exe[header..])
}

/// Parses all fonts from the MGRAPHIC.EXE load module.
pub fn parse_fonts(img: &[u8]) -> Result<Vec<Font>> {
    let word = |o: usize| -> Option<usize> { img.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize) };
    let Some(highest) = word(0xD0) else { bail!("font table missing") };
    let mut fonts = Vec::new();
    for i in 0..=highest {
        let Some(p) = word(0xD2 + 2 * i) else { continue };
        if p < 16 || p >= img.len() {
            continue;
        }
        let h = &img[p - 8..p];
        let (first, last, sh, fixw, height, spacing, xh) = (h[0], h[1], h[2], h[3], h[4], h[5], h[6]);
        // The table's last slot overlaps another font's header: reject implausible entries.
        if !(sh > 0 && sh <= 2 && height > 0 && height <= 16 && first <= last && fixw <= 16) {
            continue;
        }
        let n = (last - first) as usize + 1;
        let shift = (sh - 1) as usize;
        let stride = n << shift;
        if p + stride * (height + xh) as usize + 2 > img.len() {
            continue;
        }
        let widths: Vec<u8> =
            if fixw == 0 { img[p - 9 - (last - first) as usize..p - 8].to_vec() } else { vec![fixw; n] };
        let glyphs = (0..n)
            .map(|k| {
                let rows = (0..(height + xh) as usize)
                    .map(|r| {
                        let o = p + r * stride + (k << shift);
                        u16::from_be_bytes([img[o], img[o + 1]])
                    })
                    .collect();
                Glyph { code: first + k as u8, width: widths[k], advance: widths[k] + spacing, rows }
            })
            .collect();
        fonts.push(Font { index: i as u8, first, last, height, extra_height: xh, fixed_width: fixw, spacing, glyphs });
    }
    Ok(fonts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mgraphic_fonts() {
        let Some(g) = crate::GameDir::default_location() else { return };
        let exe = g.read("MGRAPHIC.EXE").unwrap();
        let fonts = parse_fonts(mz_image(&exe).unwrap()).unwrap();
        assert_eq!(fonts.len(), 7);
        let f0 = &fonts[0];
        assert_eq!((f0.first, f0.last, f0.height, f0.fixed_width, f0.spacing), (0, 127, 5, 3, 1));
        // '1' is a single column in the middle of the 3x5 cell.
        let one = f0.glyph(b'1').unwrap();
        assert!(one.pixel(1, 0) && one.pixel(1, 4));
    }
}
