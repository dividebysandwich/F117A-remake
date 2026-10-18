//! VGA palettes: 256 entries x 3 bytes of 6-bit RGB (0..=63).
//!
//! Sources:
//! - stand-alone `.PAL` files (768 bytes; `PALETTES.PAL` holds 9 of them, used by START);
//! - the flight base palette, stored in VGAME.EXE's DGROUP (see [`FLIGHT_PALETTE_OFFSET`]).
//!   VGAME derives the live palette from it at run time (sky/ground ramps scaled for time of day).

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

/// Byte offset of the base flight palette inside the unpacked VGAME.EXE load module
/// (DGROUP segment 0x0E23 relative, DS:1C4A).
pub const FLIGHT_PALETTE_OFFSET: usize = (0x383B - 0x1A18) * 16 + 0x1C4A;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Palette {
    /// 6-bit VGA DAC values.
    pub colors: Vec<[u8; 3]>,
}

impl Palette {
    pub fn from_vga(data: &[u8]) -> Result<Self> {
        ensure!(data.len() >= 768, "palette needs 768 bytes, got {}", data.len());
        Ok(Self { colors: data[..768].chunks(3).map(|c| [c[0] & 63, c[1] & 63, c[2] & 63]).collect() })
    }

    /// Split a multi-palette file (e.g. PALETTES.PAL) into palettes.
    pub fn many_from_vga(data: &[u8]) -> Result<Vec<Self>> {
        data.chunks_exact(768).map(Self::from_vga).collect()
    }

    /// 8-bit sRGB, scaled the way VGA hardware maps 6-bit DAC values (x*255/63).
    pub fn rgb8(&self, i: u8) -> [u8; 3] {
        let c = self.colors[i as usize];
        c.map(|v| ((v as u32 * 255 + 31) / 63) as u8)
    }
}

/// Model colour remap tables in VGAME.EXE's DGROUP (256 words each). A model colour byte c maps
/// to `table[c]`; the low byte is the VGA palette index, the high byte is 0xFF for normal colours
/// and 0xFB..0xFE for special entries (lights; meaning not yet traced).
/// Selected by detail level (DS:98DE): DS:8360 below detail 3, DS:7F60 at detail 3, DS:8160 for
/// the FLIR camera, DS:7D60 for the 16-colour map display (docs/re/visuals.md 1.6).
pub const REMAP_TABLES: [(&str, usize); 4] =
    [("detail_low", 0x8360), ("detail_high", 0x7F60), ("flir", 0x8160), ("map16", 0x7D60)];

/// Time-of-day flight palettes in DGROUP (visuals.md 1.1): day, dusk, night.
pub const DAY_PALETTE: usize = 0x164A;
pub const DUSK_PALETTE: usize = 0x194A;
pub const NIGHT_PALETTE: usize = 0x1C4A;
/// Theater ground class table (DS:00EE); class != 2 = desert: ground ramps patched.
const GROUND_CLASS: usize = 0x00EE;
/// Desert patches (VGAME 2A19:0876): (palette, byte offset in palette, source in DS), 0x30 bytes each.
const DESERT_PATCH: [(usize, usize, usize); 6] = [
    (DAY_PALETTE, 0x150, 0x1F4A),
    (DAY_PALETTE, 0x1E0, 0x1FDA),
    (DUSK_PALETTE, 0x150, 0x1F7A),
    (DUSK_PALETTE, 0x1E0, 0x200A),
    (NIGHT_PALETTE, 0x150, 0x1FAA),
    (NIGHT_PALETTE, 0x1E0, 0x203A),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TheaterPalettes {
    pub day: Palette,
    pub dusk: Palette,
    pub night: Palette,
}

/// Day/dusk/night palettes for a theater index (0..9 in `THEATERS` order).
pub fn theater_palettes(vgame_image: &[u8], theater: usize) -> Result<TheaterPalettes> {
    let ds = (0x383B - 0x1A18) * 16;
    let raw = |off: usize| vgame_image[ds + off..ds + off + 768].to_vec();
    let (mut day, mut dusk, mut night) = (raw(DAY_PALETTE), raw(DUSK_PALETTE), raw(NIGHT_PALETTE));
    if vgame_image[ds + GROUND_CLASS + theater] != 2 {
        for (pal, at, src) in DESERT_PATCH {
            let dst = match pal {
                DAY_PALETTE => &mut day,
                DUSK_PALETTE => &mut dusk,
                _ => &mut night,
            };
            dst[at..at + 0x30].copy_from_slice(&vgame_image[ds + src..ds + src + 0x30]);
        }
    }
    Ok(TheaterPalettes { day: Palette::from_vga(&day)?, dusk: Palette::from_vga(&dusk)?, night: Palette::from_vga(&night)? })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemapTable {
    pub name: String,
    pub entries: Vec<u16>,
}

/// Reads the colour remap tables from the unpacked VGAME.EXE image.
pub fn remap_tables(vgame_image: &[u8]) -> Vec<RemapTable> {
    let ds = (0x383B - 0x1A18) * 16;
    REMAP_TABLES
        .iter()
        .map(|&(name, off)| RemapTable {
            name: name.to_string(),
            entries: vgame_image[ds + off..ds + off + 512].chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect(),
        })
        .collect()
}
