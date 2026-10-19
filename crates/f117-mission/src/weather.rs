//! Weather: not part of the original game. Rolled from its own RNG stream after the
//! generator has run, so the original generation (and its RNG state) is untouched.
//!
//! Clouds are a procedural field shared by the simulation (line of sight) and the renderer:
//! the map is cut into [`CELL`]-sized squares, each holding at most one cloud whose position,
//! size and height follow from a hash of the weather seed and the square. Overcast and storm
//! add a deck covering the whole map.

use serde::{Deserialize, Serialize};

use crate::MscRand;

/// Cloud grid square in map units (64 per km).
pub const CELL: i32 = 192;

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
    pub const ALL: [Sky; 5] = [Self::Clear, Self::Scattered, Self::Broken, Self::Overcast, Self::Storm];

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

    /// A deck covers the whole map.
    pub fn has_deck(self) -> bool {
        matches!(self, Self::Overcast | Self::Storm)
    }

    /// Share of grid squares holding a cloud.
    fn cell_cover(self) -> u64 {
        match self {
            Self::Clear | Self::Overcast => 0,
            Self::Scattered => 25,
            Self::Broken => 55,
            Self::Storm => 12,
        }
    }
}

/// Odds of [`Sky::ALL`] per theater ([`crate::THEATER_NAMES`] order).
const ODDS: [[u8; 5]; 9] = [
    [60, 25, 10, 4, 1],   // Libya
    [65, 20, 10, 3, 2],   // Persian Gulf
    [15, 20, 25, 30, 10], // North Cape
    [15, 25, 25, 25, 10], // Central Europe
    [15, 20, 20, 15, 30], // Vietnam
    [55, 25, 12, 6, 2],   // Middle East
    [50, 25, 15, 8, 2],   // Desert Storm
    [30, 30, 20, 5, 15],  // Cuba
    [25, 25, 25, 20, 5],  // Korea
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Weather {
    pub sky: Sky,
    /// Cloud base (lowest cloud bottom) in feet.
    pub base_ft: i32,
    /// Deck top (overcast / storm), or the highest cumulus top, in feet.
    pub top_ft: i32,
    /// Visibility under the clouds.
    pub vis_km: i32,
    /// Seed of the cloud field.
    pub seed: u32,
}

/// One cloud of the field (map units, feet).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cloud {
    /// Grid square, for stable per-cloud detail (shape) in the renderer.
    pub gx: i32,
    pub gy: i32,
    pub x: i32,
    pub y: i32,
    pub r: i32,
    pub base_ft: i32,
    pub top_ft: i32,
}

fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

impl Weather {
    /// Rolls the weather for a mission from the generator's entry `holdrand` (own stream;
    /// the generator RNG is not touched). `pick` forces the sky.
    pub fn roll(holdrand: u32, theater: usize, pick: Option<Sky>) -> Self {
        let mut r = MscRand::new(holdrand ^ 0x5745_4154);
        let odds = ODDS[theater.min(ODDS.len() - 1)];
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
        let (base_ft, top_ft, vis_km) = match sky {
            Sky::Clear => (0, 0, 40),
            Sky::Scattered => {
                let b = 2500 + r.r(36) * 100;
                (b, b + 5300, 30)
            }
            Sky::Broken => {
                let b = 2000 + r.r(31) * 100;
                (b, b + 5300, 25)
            }
            Sky::Overcast => {
                let b = 1500 + r.r(36) * 100;
                (b, b + 3000 + r.r(51) * 100, 20)
            }
            Sky::Storm => {
                let b = 1000 + r.r(21) * 100;
                (b, b + 4000 + r.r(61) * 100, 8)
            }
        };
        Self { sky, base_ft, top_ft, vis_km, seed }
    }

    /// The briefing line, e.g. "Weather: broken clouds 3500 - 8500 ft, visibility 25 km."
    pub fn describe(&self) -> String {
        let (b, t, v) = (self.base_ft, self.top_ft, self.vis_km);
        match self.sky {
            Sky::Clear => format!("Weather: clear, visibility {v} km."),
            Sky::Scattered => format!("Weather: scattered clouds from {b} ft, visibility {v} km."),
            Sky::Broken => format!("Weather: broken clouds from {b} ft, visibility {v} km."),
            Sky::Overcast => format!("Weather: overcast {b} - {t} ft, visibility {v} km."),
            Sky::Storm => format!("Weather: thunderstorms, overcast {b} - {t} ft, visibility {v} km."),
        }
    }

    /// The cloud in grid square (gx, gy), if any.
    pub fn cloud(&self, gx: i32, gy: i32) -> Option<Cloud> {
        let cover = self.sky.cell_cover();
        if cover == 0 {
            return None;
        }
        let h = mix(((self.seed as u64) << 32) ^ ((gx as u16 as u64) << 16) ^ gy as u16 as u64);
        if h % 100 >= cover {
            return None;
        }
        let f = |shift: u32, n: u64| ((h >> shift) % n) as i32;
        let r = CELL / 4 + f(8, (CELL / 4) as u64);
        let (base_ft, top_ft) = if self.sky == Sky::Storm {
            // Towering cells through the deck.
            (self.base_ft, 25000 + f(40, 10) * 1000)
        } else {
            let b = self.base_ft + f(40, 8) * 100;
            (b, b + 1500 + f(48, 36) * 100)
        };
        Some(Cloud {
            gx,
            gy,
            x: gx * CELL + CELL / 2 + f(16, (CELL / 2) as u64) - CELL / 4,
            y: gy * CELL + CELL / 2 + f(24, (CELL / 2) as u64) - CELL / 4,
            r,
            base_ft,
            top_ft,
        })
    }

    /// Clouds whose centre is within `radius` map units of (x, y).
    pub fn clouds_near(&self, x: i32, y: i32, radius: i32) -> impl Iterator<Item = Cloud> + '_ {
        let (gx0, gx1) = ((x - radius).div_euclid(CELL), (x + radius).div_euclid(CELL));
        let (gy0, gy1) = ((y - radius).div_euclid(CELL), (y + radius).div_euclid(CELL));
        let r2 = radius as i64 * radius as i64;
        (gy0..=gy1).flat_map(move |gy| (gx0..=gx1).filter_map(move |gx| self.cloud(gx, gy))).filter(move |c| {
            let (dx, dy) = ((c.x - x) as i64, (c.y - y) as i64);
            dx * dx + dy * dy <= r2
        })
    }

    /// Whether (x, y, alt_ft) is inside a cloud or the deck.
    pub fn in_cloud(&self, x: i32, y: i32, alt_ft: i32) -> bool {
        if self.sky.has_deck() && (self.base_ft..=self.top_ft).contains(&alt_ft) {
            return true;
        }
        self.clouds_near(x, y, CELL).any(|c| {
            let (dx, dy) = ((c.x - x) as i64, (c.y - y) as i64);
            dx * dx + dy * dy <= (c.r as i64).pow(2) && (c.base_ft..=c.top_ft).contains(&alt_ft)
        })
    }

    /// Whether the straight line between two points (map units, feet) misses every cloud.
    pub fn los_clear(&self, a: (i32, i32, i32), b: (i32, i32, i32)) -> bool {
        if self.sky == Sky::Clear {
            return true;
        }
        let (lo, hi) = (a.2.min(b.2), a.2.max(b.2));
        if self.sky.has_deck() && lo <= self.top_ft && hi >= self.base_ft {
            return false;
        }
        if self.sky.cell_cover() == 0 {
            return true;
        }
        let (ax, ay) = (a.0 as f64, a.1 as f64);
        let (dx, dy) = ((b.0 - a.0) as f64, (b.1 - a.1) as f64);
        let len2 = dx * dx + dy * dy;
        let reach = CELL;
        let (gx0, gx1) = ((a.0.min(b.0) - reach).div_euclid(CELL), (a.0.max(b.0) + reach).div_euclid(CELL));
        let (gy0, gy1) = ((a.1.min(b.1) - reach).div_euclid(CELL), (a.1.max(b.1) + reach).div_euclid(CELL));
        for gy in gy0..=gy1 {
            for gx in gx0..=gx1 {
                // Skip squares far from the line before hashing.
                let (cx, cy) = ((gx * CELL + CELL / 2) as f64, (gy * CELL + CELL / 2) as f64);
                let t = if len2 > 0.0 { (((cx - ax) * dx + (cy - ay) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
                let (px, py) = (ax + dx * t - cx, ay + dy * t - cy);
                if px * px + py * py > (reach as f64).powi(2) {
                    continue;
                }
                let Some(c) = self.cloud(gx, gy) else { continue };
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
                    return false;
                }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(sky: Sky) -> Weather {
        Weather { sky, base_ft: 3000, top_ft: 8000, vis_km: 20, seed: 1234 }
    }

    #[test]
    fn roll_is_deterministic_and_respects_pick() {
        for t in 0..9 {
            assert_eq!(Weather::roll(0xABCD, t, None), Weather::roll(0xABCD, t, None));
            for s in Sky::ALL {
                assert_eq!(Weather::roll(0xABCD, t, Some(s)).sky, s);
            }
        }
        let skies: std::collections::HashSet<_> = (0..500).map(|h| Weather::roll(h * 7919, 4, None).sky).collect();
        assert_eq!(skies.len(), 5);
    }

    #[test]
    fn clear_never_blocks() {
        assert!(w(Sky::Clear).los_clear((0, 0, 0), (5000, 5000, 20000)));
        assert!(!w(Sky::Clear).in_cloud(100, 100, 5000));
    }

    #[test]
    fn deck_blocks_crossing_lines_only() {
        let o = w(Sky::Overcast);
        assert!(!o.los_clear((0, 0, 0), (100, 0, 10000)));
        assert!(!o.los_clear((0, 0, 5000), (100, 0, 5000)));
        assert!(o.los_clear((0, 0, 0), (1000, 0, 2500)));
        assert!(o.los_clear((0, 0, 9000), (1000, 0, 20000)));
        assert!(o.in_cloud(0, 0, 4000));
    }

    #[test]
    fn cumulus_blocks_through_its_middle() {
        let s = w(Sky::Broken);
        let c = (0..100).find_map(|g| s.cloud(g, g)).expect("a cloud");
        let mid = (c.base_ft + c.top_ft) / 2;
        assert!(s.in_cloud(c.x, c.y, mid));
        assert!(!s.los_clear((c.x - 2 * c.r, c.y, mid), (c.x + 2 * c.r, c.y, mid)));
        // Above and below it.
        assert!(!s.in_cloud(c.x, c.y, c.top_ft + 100));
        assert!(!s.in_cloud(c.x, c.y, c.base_ft - 100));
        // A vertical line next to it.
        let far = (c.x + CELL * 3, c.y);
        if s.clouds_near(far.0, far.1, CELL).all(|o| (o.x - far.0).abs() + (o.y - far.1).abs() > 2 * CELL) {
            assert!(s.los_clear((far.0, far.1, 0), (far.0, far.1, 30000)));
        }
    }

    #[test]
    fn cover_matches_sky() {
        let count = |sky| (0..100).flat_map(|y| (0..100).map(move |x| (x, y))).filter(|&(x, y)| w(sky).cloud(x, y).is_some()).count();
        assert_eq!(count(Sky::Overcast), 0);
        let (s, b) = (count(Sky::Scattered), count(Sky::Broken));
        assert!((2000..3000).contains(&s), "{s}");
        assert!((5000..6000).contains(&b), "{b}");
    }
}
