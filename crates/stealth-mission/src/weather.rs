//! Weather: rolled from its own RNG stream after the generator has run, so the classic
//! generation (and its RNG state) is untouched.
//!
//! The sky is a mix of up to three cloud layers, each with its own coverage:
//! - cumulus: puffs on a [`CELL`] grid, each square holding a cloud with the layer's
//!   coverage as probability;
//! - stratus (stratocumulus, altostratus): a sheet between two altitudes, present where a
//!   smooth noise field exceeds a threshold chosen so that the layer's coverage share of the
//!   map is covered (100 % = overcast, 50 % = big gaps of clear sky);
//! - cumulonimbus: storm towers on a coarser [`STORM_CELL`] grid.
//!
//! The simulation (line of sight) and the renderer (palette.wgsl `stratus_field`) evaluate
//! the same fields, so what is drawn is what blocks.

use serde::{Deserialize, Serialize};

use crate::MscRand;

/// Cumulus grid square in map units (64 per km).
pub const CELL: i32 = 192;
/// Cumulonimbus grid square: storm towers are far wider than cumulus.
pub const STORM_CELL: i32 = 4 * CELL;
/// Stratus noise octaves: lattice size in map units and weight.
pub const STRATUS_OCTAVES: [(f32, f32); 3] = [(512.0, 0.6), (160.0, 0.3), (48.0, 0.1)];

/// Weather preset: picks a realistic mix of cloud layers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Sky {
    #[default]
    Clear,
    Scattered,
    Broken,
    Overcast,
    Storm,
}

impl Sky {
    pub const ALL: [Sky; 5] =
        [Self::Clear, Self::Scattered, Self::Broken, Self::Overcast, Self::Storm];

    pub fn name(self) -> &'static str {
        match self {
            Self::Clear => "Clear",
            Self::Scattered => "Scattered",
            Self::Broken => "Broken",
            Self::Overcast => "Overcast",
            Self::Storm => "Storm",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.name().eq_ignore_ascii_case(s))
    }
}

/// Odds of [`Sky::ALL`] per theater ([`crate::THEATER_NAMES`] order).
const ODDS: [[u8; 5]; 10] = [
    [60, 25, 10, 4, 1],   // Libya
    [65, 20, 10, 3, 2],   // Persian Gulf
    [15, 20, 25, 30, 10], // North Cape
    [15, 25, 25, 25, 10], // Central Europe
    [15, 20, 20, 15, 30], // Vietnam
    [55, 25, 12, 6, 2],   // Middle East
    [50, 25, 15, 8, 2],   // Desert Storm
    [30, 30, 20, 5, 15],  // Cuba
    [25, 25, 25, 20, 5],  // Korea
    [20, 25, 25, 22, 8],  // Ukraine
];

/// Odds (percent) of [`Sky::ALL`] in a theater.
pub fn odds(theater: usize) -> [u8; 5] {
    ODDS[theater.min(ODDS.len() - 1)]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CloudKind {
    Cumulus,
    Stratus,
    Cumulonimbus,
}

/// One cloud layer; coverage 0 = absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Layer {
    /// Share of the sky covered, percent.
    pub cover: u8,
    pub base_ft: i32,
    pub top_ft: i32,
    /// Seed of the layer's clouds; 0: the weather's seed.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub seed: u32,
}

fn is_zero(v: &u32) -> bool {
    *v == 0
}

impl Layer {
    pub fn new(cover: i32, base_ft: i32, top_ft: i32) -> Self {
        Self { cover: cover.clamp(0, 100) as u8, base_ft, top_ft, seed: 0 }
    }

    pub fn present(&self) -> bool {
        self.cover > 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Weather {
    /// The preset the layers were rolled from.
    pub sky: Sky,
    pub cumulus: Layer,
    pub stratus: Layer,
    pub cumulonimbus: Layer,
    /// Visibility under the clouds.
    pub vis_km: i32,
    /// Seed of the cloud fields.
    pub seed: u32,
    /// Stratus noise threshold (0..65535 for 0..1): covered where the field reaches it.
    pub stratus_threshold: u16,
}

/// One cloud of the cumulus or cumulonimbus field (map units, feet).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cloud {
    pub kind: CloudKind,
    /// Grid square (of the kind's grid).
    pub gx: i32,
    pub gy: i32,
    pub x: i32,
    pub y: i32,
    pub r: i32,
    pub base_ft: i32,
    pub top_ft: i32,
    /// The cloud exists while the layer's coverage exceeds this (0..99).
    pub rank: u8,
}

fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Lattice value 0..1 of the stratus noise (same integer hash as palette.wgsl `value_hash`).
fn value_hash(ix: i32, iy: i32, seed: u32) -> f32 {
    let mut h = (ix as u32).wrapping_mul(0x8DA6_B343)
        ^ (iy as u32).wrapping_mul(0xD816_3841)
        ^ seed.wrapping_mul(0xCB1A_B31F);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7FEB_352D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846C_A68B);
    h ^= h >> 16;
    (h >> 8) as f32 / 16_777_216.0
}

/// Smooth value noise 0..1.
fn value_noise(x: f32, y: f32, seed: u32) -> f32 {
    let (fx, fy) = (x.floor(), y.floor());
    let (ix, iy) = (fx as i32, fy as i32);
    let (tx, ty) = (x - fx, y - fy);
    let (ux, uy) = (tx * tx * (3.0 - 2.0 * tx), ty * ty * (3.0 - 2.0 * ty));
    let a = value_hash(ix, iy, seed);
    let b = value_hash(ix + 1, iy, seed);
    let c = value_hash(ix, iy + 1, seed);
    let d = value_hash(ix + 1, iy + 1, seed);
    let top = a + (b - a) * ux;
    let bottom = c + (d - c) * ux;
    top + (bottom - top) * uy
}

impl Weather {
    /// Rolls the weather for a mission from the generator's entry `holdrand` (own stream;
    /// the generator RNG is not touched). `pick` forces the preset, `cover` the coverage of
    /// its main layer (stratus for overcast and storm, cumulus otherwise).
    pub fn roll(holdrand: u32, theater: usize, pick: Option<Sky>, cover: Option<u8>) -> Self {
        let mut r = MscRand::new(holdrand ^ 0x5745_4154);
        let odds = odds(theater);
        let mut n = r.r(100);
        let rolled = Sky::ALL
            .into_iter()
            .zip(odds)
            .find(|&(_, p)| {
                n -= p as i32;
                n < 0
            })
            .map_or(Sky::Clear, |(s, _)| s);
        let sky = pick.unwrap_or(rolled);
        let seed = (r.rand() as u32) << 15 | r.rand() as u32;
        // Always the same draws, whatever the preset.
        let v: [i32; 8] = std::array::from_fn(|_| r.r(1000));
        let mut w = Self::from_sky(sky, seed, v);
        if let Some(c) = cover {
            let main = if matches!(sky, Sky::Overcast | Sky::Storm) {
                &mut w.stratus
            } else {
                &mut w.cumulus
            };
            if main.base_ft > 0 {
                main.cover = c.min(100);
            }
        }
        w.stratus_threshold = w.threshold_for(w.stratus.cover);
        w
    }

    /// The layers of a preset from eight draws 0..999 (the stratus threshold is left unset).
    pub fn from_sky(sky: Sky, seed: u32, v: [i32; 8]) -> Self {
        let mut w = Self { sky, seed, vis_km: 40, ..Default::default() };
        match sky {
            Sky::Clear => {}
            Sky::Scattered => {
                let b = 2500 + v[1] % 36 * 100;
                w.cumulus = Layer::new(20 + v[0] % 26, b, b + 3000 + v[2] % 31 * 100);
                // Sometimes a thin altostratus sheet high above.
                if v[3] % 10 < 3 {
                    let s = 12000 + v[5] % 61 * 100;
                    w.stratus = Layer::new(10 + v[4] % 31, s, s + 1000 + v[6] % 11 * 100);
                }
                w.vis_km = 30;
            }
            Sky::Broken => {
                let b = 2000 + v[1] % 31 * 100;
                w.cumulus = Layer::new(45 + v[0] % 26, b, b + 4000 + v[2] % 31 * 100);
                // Often a stratocumulus layer above the cumulus.
                if v[3] % 2 == 0 {
                    let s = w.cumulus.top_ft + 1000 + v[5] % 31 * 100;
                    w.stratus = Layer::new(30 + v[4] % 31, s, s + 1500 + v[6] % 16 * 100);
                }
                w.vis_km = 25;
            }
            Sky::Overcast => {
                let b = 1500 + v[1] % 36 * 100;
                w.stratus = Layer::new(80 + v[0] % 21, b, b + 2000 + v[2] % 31 * 100);
                // A few cumulus fragments below it.
                if v[3] % 10 < 4 && b - 1500 >= 800 {
                    let cb = 800 + v[5] % ((b - 1500 - 800) / 100 + 1) * 100;
                    w.cumulus = Layer::new(10 + v[4] % 11, cb, b - 200);
                }
                w.vis_km = 20;
            }
            Sky::Storm => {
                let b = 1000 + v[1] % 21 * 100;
                w.cumulonimbus = Layer::new(12 + v[0] % 19, b, 30000 + v[2] % 11 * 1000);
                w.stratus = Layer::new(40 + v[3] % 51, b + 500, b + 3500 + v[4] % 41 * 100);
                w.cumulus = Layer::new(20 + v[5] % 21, b, b + 2500 + v[6] % 26 * 100);
                w.vis_km = 8;
            }
        }
        w
    }

    /// Stratus field threshold covering `cover` percent of the map.
    fn threshold_for(&self, cover: u8) -> u16 {
        self.stratus_thresholds()[cover.min(100) as usize]
    }

    /// Stratus field thresholds covering 0..=100 percent of the map (from a sample grid).
    pub fn stratus_thresholds(&self) -> [u16; 101] {
        let mut v: Vec<f32> = (0..128)
            .flat_map(|j| (0..128).map(move |i| (i * 512 + 97, j * 512 + 211)))
            .map(|(x, y)| self.stratus_field(x as f32, y as f32))
            .collect();
        v.sort_by(f32::total_cmp);
        std::array::from_fn(|cover| match cover {
            // Just above the field's highest sample (nothing covered).
            0 => ((v[v.len() - 1] * 65535.0) as u16).saturating_add(64),
            100 => 0,
            _ => (v[((100 - cover) * v.len() / 100).min(v.len() - 1)] * 65535.0) as u16,
        })
    }

    /// Seed of a layer's clouds.
    pub fn layer_seed(&self, kind: CloudKind) -> u32 {
        let l = match kind {
            CloudKind::Cumulus => self.cumulus,
            CloudKind::Stratus => self.stratus,
            CloudKind::Cumulonimbus => self.cumulonimbus,
        };
        if l.seed != 0 { l.seed } else { self.seed }
    }

    /// Stratus noise field 0..1 at a map position (palette.wgsl `stratus_field`).
    pub fn stratus_field(&self, x: f32, y: f32) -> f32 {
        let s = self.layer_seed(CloudKind::Stratus) & 0xFFFF;
        STRATUS_OCTAVES
            .iter()
            .enumerate()
            .map(|(k, &(cell, weight))| weight * value_noise(x / cell, y / cell, s + k as u32))
            .sum()
    }

    /// Stratus covers this map position (whatever the altitude).
    pub fn stratus_at(&self, x: f32, y: f32) -> bool {
        self.stratus.present()
            && self.stratus_field(x, y) * 65535.0 >= self.stratus_threshold as f32
    }

    /// Any clouds at all (a clear sky is classic).
    pub fn any(&self) -> bool {
        self.cumulus.present() || self.stratus.present() || self.cumulonimbus.present()
    }

    pub fn has_storms(&self) -> bool {
        self.cumulonimbus.present()
    }

    fn layer_name(&self, kind: CloudKind) -> &'static str {
        match kind {
            CloudKind::Cumulus => "cumulus",
            CloudKind::Cumulonimbus => "cumulonimbus",
            CloudKind::Stratus if self.stratus.base_ft >= 6500 => "altostratus",
            CloudKind::Stratus if self.stratus.cover >= 90 => "stratus",
            CloudKind::Stratus => "stratocumulus",
        }
    }

    /// The briefing line, e.g. "Weather: broken - cumulus 55% from 3500 ft, stratocumulus
    /// 40% 9000-11000 ft, visibility 25 km."
    pub fn describe(&self) -> String {
        if !self.any() {
            return format!("Weather: clear, visibility {} km.", self.vis_km);
        }
        let mut parts = Vec::new();
        for (kind, l) in [
            (CloudKind::Cumulonimbus, self.cumulonimbus),
            (CloudKind::Cumulus, self.cumulus),
            (CloudKind::Stratus, self.stratus),
        ] {
            if !l.present() {
                continue;
            }
            let name = self.layer_name(kind);
            parts.push(match kind {
                CloudKind::Stratus => format!("{name} {}% {}-{} ft", l.cover, l.base_ft, l.top_ft),
                _ => format!("{name} {}% from {} ft", l.cover, l.base_ft),
            });
        }
        let preset = if self.sky == Sky::Storm {
            "thunderstorms".to_string()
        } else {
            self.sky.name().to_ascii_lowercase()
        };
        format!("Weather: {preset} - {}, visibility {} km.", parts.join(", "), self.vis_km)
    }

    fn grid(kind: CloudKind) -> i32 {
        if kind == CloudKind::Cumulonimbus { STORM_CELL } else { CELL }
    }

    /// The cumulus or cumulonimbus cloud in grid square (gx, gy) of that kind's grid, if any.
    pub fn cloud(&self, kind: CloudKind, gx: i32, gy: i32) -> Option<Cloud> {
        let layer = match kind {
            CloudKind::Cumulus => self.cumulus,
            CloudKind::Cumulonimbus => self.cumulonimbus,
            CloudKind::Stratus => return None,
        };
        if !layer.present() {
            return None;
        }
        let salt = if kind == CloudKind::Cumulonimbus { 0x5354_4F52_4D00_0000 } else { 0 };
        let seed = self.layer_seed(kind) as u64;
        let h = mix((seed << 32) ^ ((gx as u16 as u64) << 16) ^ gy as u16 as u64 ^ salt);
        if h % 100 >= layer.cover as u64 {
            return None;
        }
        let cell = Self::grid(kind);
        let f = |shift: u32, n: u64| ((h >> shift) % n) as i32;
        let r = cell / 4 + f(8, (cell / 5) as u64);
        let (base_ft, top_ft) = if kind == CloudKind::Cumulonimbus {
            (layer.base_ft, layer.top_ft - f(40, 8) * 1000)
        } else {
            let b = layer.base_ft + f(40, 8) * 100;
            (b, b + (layer.top_ft - layer.base_ft) * (40 + f(48, 61)) / 100)
        };
        Some(Cloud {
            kind,
            gx,
            gy,
            x: gx * cell + cell / 2 + f(16, (cell / 2) as u64) - cell / 4,
            y: gy * cell + cell / 2 + f(24, (cell / 2) as u64) - cell / 4,
            r,
            base_ft,
            top_ft,
            rank: (h % 100) as u8,
        })
    }

    /// Cumulus and cumulonimbus whose centre is within `radius` map units of (x, y).
    pub fn clouds_near(&self, x: i32, y: i32, radius: i32) -> impl Iterator<Item = Cloud> + '_ {
        let r2 = radius as i64 * radius as i64;
        [CloudKind::Cumulus, CloudKind::Cumulonimbus]
            .into_iter()
            .flat_map(move |kind| {
                let cell = Self::grid(kind);
                let (gx0, gx1) = ((x - radius).div_euclid(cell), (x + radius).div_euclid(cell));
                let (gy0, gy1) = ((y - radius).div_euclid(cell), (y + radius).div_euclid(cell));
                (gy0..=gy1)
                    .flat_map(move |gy| (gx0..=gx1).filter_map(move |gx| self.cloud(kind, gx, gy)))
            })
            .filter(move |c| {
                let (dx, dy) = ((c.x - x) as i64, (c.y - y) as i64);
                dx * dx + dy * dy <= r2
            })
    }

    /// Whether (x, y, alt_ft) is inside a cloud.
    pub fn in_cloud(&self, x: i32, y: i32, alt_ft: i32) -> bool {
        if (self.stratus.base_ft..=self.stratus.top_ft).contains(&alt_ft)
            && self.stratus_at(x as f32, y as f32)
        {
            return true;
        }
        self.clouds_near(x, y, STORM_CELL).any(|c| {
            let (dx, dy) = ((c.x - x) as i64, (c.y - y) as i64);
            dx * dx + dy * dy <= (c.r as i64).pow(2) && (c.base_ft..=c.top_ft).contains(&alt_ft)
        })
    }

    /// Under the stratus layer where it is covered, or under a storm cell (shade, rain).
    pub fn under_cloud(&self, x: i32, y: i32, alt_ft: i32) -> bool {
        if self.stratus.present()
            && alt_ft < self.stratus.base_ft
            && self.stratus_at(x as f32, y as f32)
        {
            return true;
        }
        self.clouds_near(x, y, STORM_CELL).any(|c| {
            let (dx, dy) = ((c.x - x) as i64, (c.y - y) as i64);
            c.kind == CloudKind::Cumulonimbus
                && dx * dx + dy * dy <= (c.r as i64).pow(2)
                && alt_ft < c.base_ft
        })
    }

    /// Under a storm cell (rain, turbulence).
    pub fn under_storm(&self, x: i32, y: i32, alt_ft: i32) -> bool {
        self.clouds_near(x, y, STORM_CELL).any(|c| {
            let (dx, dy) = ((c.x - x) as i64, (c.y - y) as i64);
            c.kind == CloudKind::Cumulonimbus
                && dx * dx + dy * dy <= (c.r as i64).pow(2)
                && alt_ft < c.top_ft
        })
    }

    /// Whether the straight line between two points (map units, feet) misses every cloud.
    pub fn los_clear(&self, a: (i32, i32, i32), b: (i32, i32, i32)) -> bool {
        if !self.any() {
            return true;
        }
        !self.stratus_blocks(a, b)
            && !self.cells_block(CloudKind::Cumulus, a, b)
            && !self.cells_block(CloudKind::Cumulonimbus, a, b)
    }

    /// Samples the stratus field along the part of the line inside the layer (every km).
    fn stratus_blocks(&self, a: (i32, i32, i32), b: (i32, i32, i32)) -> bool {
        let l = self.stratus;
        if !l.present() {
            return false;
        }
        let (lo, hi) = (a.2.min(b.2), a.2.max(b.2));
        if hi < l.base_ft || lo > l.top_ft {
            return false;
        }
        let dz = (b.2 - a.2) as f32;
        let (t0, t1) = if dz.abs() < 1.0 {
            (0.0, 1.0)
        } else {
            let (ta, tb) = ((l.base_ft - a.2) as f32 / dz, (l.top_ft - a.2) as f32 / dz);
            (ta.min(tb).max(0.0), ta.max(tb).min(1.0))
        };
        let (dx, dy) = ((b.0 - a.0) as f32, (b.1 - a.1) as f32);
        let len = (dx * dx + dy * dy).sqrt() * (t1 - t0);
        let n = ((len / 64.0) as usize).clamp(1, 64);
        (0..=n).any(|i| {
            let t = t0 + (t1 - t0) * i as f32 / n as f32;
            self.stratus_at(a.0 as f32 + dx * t, a.1 as f32 + dy * t)
        })
    }

    fn cells_block(&self, kind: CloudKind, a: (i32, i32, i32), b: (i32, i32, i32)) -> bool {
        let layer = if kind == CloudKind::Cumulus { self.cumulus } else { self.cumulonimbus };
        if !layer.present() {
            return false;
        }
        let (lo, hi) = (a.2.min(b.2), a.2.max(b.2));
        let (ax, ay) = (a.0 as f64, a.1 as f64);
        let (dx, dy) = ((b.0 - a.0) as f64, (b.1 - a.1) as f64);
        let len2 = dx * dx + dy * dy;
        let cell = Self::grid(kind);
        let reach = cell;
        let (gx0, gx1) =
            ((a.0.min(b.0) - reach).div_euclid(cell), (a.0.max(b.0) + reach).div_euclid(cell));
        let (gy0, gy1) =
            ((a.1.min(b.1) - reach).div_euclid(cell), (a.1.max(b.1) + reach).div_euclid(cell));
        for gy in gy0..=gy1 {
            for gx in gx0..=gx1 {
                // Skip squares far from the line before hashing.
                let (cx, cy) = ((gx * cell + cell / 2) as f64, (gy * cell + cell / 2) as f64);
                let t = if len2 > 0.0 {
                    (((cx - ax) * dx + (cy - ay) * dy) / len2).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let (px, py) = (ax + dx * t - cx, ay + dy * t - cy);
                if px * px + py * py > (reach as f64).powi(2) {
                    continue;
                }
                let Some(c) = self.cloud(kind, gx, gy) else { continue };
                if hi < c.base_ft || lo > c.top_ft {
                    continue;
                }
                // Part of the segment inside the cloud's circle: |a + d t - c| < r.
                let (fx, fy) = (ax - c.x as f64, ay - c.y as f64);
                let rr = (c.r as f64).powi(2);
                let (t0, t1) = if len2 > 0.0 {
                    let bq = fx * dx + fy * dy;
                    let disc = bq * bq - len2 * (fx * fx + fy * fy - rr);
                    if disc < 0.0 {
                        continue;
                    }
                    let s = disc.sqrt();
                    (((-bq - s) / len2).max(0.0), ((-bq + s) / len2).min(1.0))
                } else if fx * fx + fy * fy <= rr {
                    (0.0, 1.0)
                } else {
                    continue;
                };
                if t0 > t1 {
                    continue;
                }
                let alt = |t: f64| a.2 as f64 + (b.2 - a.2) as f64 * t;
                let (z0, z1) = (alt(t0).min(alt(t1)), alt(t0).max(alt(t1)));
                if z0 <= c.top_ft as f64 && z1 >= c.base_ft as f64 {
                    return true;
                }
            }
        }
        false
    }

    /// A single stratus layer (tests and tools).
    pub fn stratus_layer(base_ft: i32, top_ft: i32, cover: u8, seed: u32) -> Self {
        let mut w = Self { sky: Sky::Overcast, vis_km: 20, seed, ..Default::default() };
        w.stratus = Layer::new(cover as i32, base_ft, top_ft);
        w.stratus_threshold = w.threshold_for(w.stratus.cover);
        w
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cumulus(cover: i32) -> Weather {
        Weather {
            sky: Sky::Broken,
            cumulus: Layer::new(cover, 3000, 8000),
            vis_km: 20,
            seed: 1234,
            ..Default::default()
        }
    }

    #[test]
    fn roll_is_deterministic_and_respects_pick() {
        for t in 0..9 {
            assert_eq!(Weather::roll(0xABCD, t, None, None), Weather::roll(0xABCD, t, None, None));
            for s in Sky::ALL {
                assert_eq!(Weather::roll(0xABCD, t, Some(s), None).sky, s);
            }
        }
        let skies: std::collections::HashSet<_> =
            (0..500).map(|h| Weather::roll(h * 7919, 4, None, None).sky).collect();
        assert_eq!(skies.len(), 5);
        // Picking the preset that would have been rolled gives the same weather.
        let w = Weather::roll(77, 3, None, None);
        assert_eq!(Weather::roll(77, 3, Some(w.sky), None), w);
    }

    #[test]
    fn presets_mix_layers() {
        let w = Weather::roll(5, 3, Some(Sky::Storm), None);
        assert!(w.cumulonimbus.present() && w.cumulus.present() && w.stratus.present());
        assert!(w.stratus.cover < 100);
        let o = Weather::roll(5, 3, Some(Sky::Overcast), Some(60));
        assert_eq!(o.stratus.cover, 60);
        assert!(!Weather::roll(5, 3, Some(Sky::Clear), Some(60)).any());
    }

    #[test]
    fn clear_never_blocks() {
        let w = Weather::default();
        assert!(w.los_clear((0, 0, 0), (5000, 5000, 20000)));
        assert!(!w.in_cloud(100, 100, 5000));
    }

    #[test]
    fn stratus_coverage_matches_percentage() {
        for cover in [20u8, 50, 80, 100] {
            let w = Weather::stratus_layer(3000, 5000, cover, 4242);
            let n = 200;
            let hits = (0..n * n)
                .filter(|i| w.stratus_at((i % n * 300 + 50) as f32, (i / n * 300 + 70) as f32))
                .count();
            let pct = hits * 100 / (n * n);
            assert!((pct as i32 - cover as i32).abs() <= 6, "cover {cover}: {pct}%");
        }
    }

    #[test]
    fn stratus_blocks_only_where_covered() {
        let w = Weather::stratus_layer(3000, 5000, 50, 4242);
        let points = || (0..5000).map(|i| (i * 61 % 60000, i * 97 % 60000));
        let (x, y) = points().find(|&(x, y)| w.stratus_at(x as f32, y as f32)).unwrap();
        assert!(!w.los_clear((x, y, 0), (x, y, 10000)));
        assert!(w.in_cloud(x, y, 4000));
        assert!(w.under_cloud(x, y, 1000));
        let (x, y) = points().find(|&(x, y)| !w.stratus_at(x as f32, y as f32)).expect("a gap");
        assert!(w.los_clear((x, y, 0), (x, y, 10000)));
        assert!(!w.under_cloud(x, y, 1000));
        // Below or above the layer never blocks.
        assert!(w.los_clear((0, 0, 0), (5000, 0, 2500)));
        assert!(w.los_clear((0, 0, 6000), (5000, 0, 20000)));
        // Full cover blocks every crossing.
        let o = Weather::stratus_layer(3000, 5000, 100, 1);
        assert!(!o.los_clear((123, 456, 0), (130, 460, 9000)));
    }

    #[test]
    fn cumulus_blocks_through_its_middle() {
        let s = cumulus(55);
        let c = (0..100).find_map(|g| s.cloud(CloudKind::Cumulus, g, g)).expect("a cloud");
        let mid = (c.base_ft + c.top_ft) / 2;
        assert!(s.in_cloud(c.x, c.y, mid));
        assert!(!s.los_clear((c.x - 2 * c.r, c.y, mid), (c.x + 2 * c.r, c.y, mid)));
        assert!(!s.in_cloud(c.x, c.y, c.top_ft + 100));
        assert!(!s.in_cloud(c.x, c.y, c.base_ft - 100));
    }

    #[test]
    fn storm_towers_are_wide() {
        let s = Weather::roll(99, 4, Some(Sky::Storm), None);
        let towers: Vec<Cloud> = (0..20)
            .flat_map(|y| (0..20).filter_map(move |x| s.cloud(CloudKind::Cumulonimbus, x, y)))
            .collect();
        assert!(!towers.is_empty());
        for c in towers {
            assert!(c.r >= 3 * 64);
            assert!(c.top_ft > 20000);
        }
    }

    #[test]
    fn cumulus_cover_matches_layer() {
        let count = |cover| {
            (0..100)
                .flat_map(|y| (0..100).map(move |x| (x, y)))
                .filter(|&(x, y)| cumulus(cover).cloud(CloudKind::Cumulus, x, y).is_some())
                .count()
        };
        let (s, b) = (count(25), count(55));
        assert!((2000..3000).contains(&s), "{s}");
        assert!((5000..6000).contains(&b), "{b}");
    }
}
