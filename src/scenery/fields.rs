//! Farmland (the remake's steppe theaters): the open land is cut into square sections, each
//! ploughed as one to three long fields, with tracks between them and shelterbelts (rows of
//! trees) along some section edges. Everything follows from the section's hash, so any block
//! can be built alone. Fields only go where the ground is open land, clear of water, woods,
//! concrete, raised tile geometry and towns.
//!
//! Autumn crops: stubble and late wheat, fresh ploughland, dark harrowed soil, green winter
//! wheat, dried sunflowers waiting for the harvest; some sections lie fallow (the ground
//! gradient shows).

use bevy::math::DVec2;

use super::network::{Rng, hash};

/// Section size (metres).
pub const SECTION: f64 = 1400.0;
/// Track between fields and margin along the section edges (metres).
const TRACK: f64 = 9.0;
/// Shelterbelt width (metres).
pub const BELT_WIDTH: f64 = 14.0;

pub struct Field {
    pub min: DVec2,
    pub max: DVec2,
    pub colour: u8,
}

/// Crop colours (model colour bytes) with their weights.
const CROPS: [(u8, f64); 9] = [
    (0xC3, 0.12), // stubble
    (0xC5, 0.07), // late wheat
    (0xCC, 0.12), // harrowed
    (0xCD, 0.15), // ploughed
    (0xCE, 0.08), // fresh ploughland, dark
    (0xA5, 0.12), // winter wheat
    (0xA7, 0.07), // winter rape
    (0xCF, 0.05), // dried sunflowers
    (0x00, 0.08), // fallow
];

/// The fields of section (sx, sy).
pub fn section(seed: u64, sx: i64, sy: i64) -> Vec<Field> {
    let mut rng = Rng::new(hash(seed, sx, sy, 0xF1E1D));
    let min = DVec2::new(sx as f64, sy as f64) * SECTION + TRACK;
    let max = min + SECTION - 2.0 * TRACK;
    let n = 1 + rng.weighted(&[0.3, 0.45, 0.25]);
    let along_x = rng.chance(0.5);
    // Cut positions across the section.
    let mut cuts = vec![0.0];
    for i in 1..n {
        cuts.push((i as f64 + rng.range(-0.25, 0.25)) / n as f64);
    }
    cuts.push(1.0);
    let weights: Vec<f64> = CROPS.iter().map(|c| c.1).collect();
    let mut out = Vec::new();
    for w in cuts.windows(2) {
        let colour = CROPS[rng.weighted(&weights)].0;
        if colour == 0 {
            continue;
        }
        let (a, b) = (w[0], w[1]);
        let span = max - min;
        let (lo, hi) = if along_x {
            (DVec2::new(min.x, min.y + span.y * a), DVec2::new(max.x, min.y + span.y * b))
        } else {
            (DVec2::new(min.x + span.x * a, min.y), DVec2::new(min.x + span.x * b, max.y))
        };
        let gap = if a > 0.0 { TRACK * 0.5 } else { 0.0 };
        let gap_hi = if b < 1.0 { TRACK * 0.5 } else { 0.0 };
        let (lo, hi) = if along_x {
            (lo + DVec2::new(0.0, gap), hi - DVec2::new(0.0, gap_hi))
        } else {
            (lo + DVec2::new(gap, 0.0), hi - DVec2::new(gap_hi, 0.0))
        };
        out.push(Field { min: lo, max: hi, colour });
    }
    out
}

/// Shelterbelts on the south and west edges of section (sx, sy) (each edge belongs to the
/// section north or east of it): line segments in metres.
pub fn belts(seed: u64, sx: i64, sy: i64) -> Vec<(DVec2, DVec2)> {
    let o = DVec2::new(sx as f64, sy as f64) * SECTION;
    let mut out = Vec::new();
    if Rng::new(hash(seed, sx, sy, 0xBE17)).chance(0.4) {
        out.push((o, o + DVec2::new(SECTION, 0.0)));
    }
    if Rng::new(hash(seed, sx, sy, 0xBE18)).chance(0.4) {
        out.push((o, o + DVec2::new(0.0, SECTION)));
    }
    out
}

/// Sections overlapping a rectangle.
pub fn sections(min: DVec2, max: DVec2) -> impl Iterator<Item = (i64, i64)> {
    let (a, b) = ((min / SECTION).floor(), (max / SECTION).floor());
    (a.y as i64..=b.y as i64).flat_map(move |sy| (a.x as i64..=b.x as i64).map(move |sx| (sx, sy)))
}
