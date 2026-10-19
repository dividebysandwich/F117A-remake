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
