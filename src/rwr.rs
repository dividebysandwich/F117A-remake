//! Radar warning receiver: the radars painting the player
//! (`Sim::emitters`), published every frame, and the circular threat display of the F-16's RWR.
//! The HARM display (modern avionics) shows the same picture, of the radars heard lately.
//!
//! Like a real RWR the scope does not show range: a symbol's distance from the centre gives its
//! priority (the most dangerous emitters innermost), its direction the relative bearing.

use std::collections::HashMap;

use bevy::prelude::*;
use stealth_sim::{Emitter, EmitterSource};

use crate::canvas::Canvas;
use crate::font::{Align, stroke_text_at};

/// A radar painting the player, for the displays.
#[derive(Debug, Clone, PartialEq)]
pub struct EmitterView {
    pub source: EmitterSource,
    /// World units (east, north).
    pub east: f64,
    pub north: f64,
    /// Bearing relative to the nose (radians, clockwise).
    pub rel_bearing: f32,
    pub range_km: f32,
    /// Its missile's lock range (km).
    pub lock_range_km: f32,
    /// Symbol text ("10", "HK", "29").
    pub label: String,
    /// Airborne radar (drawn with a hat).
    pub air: bool,
    pub tracking: bool,
    pub launch: bool,
    pub priority: i32,
    /// Its signal reached the player with some strength lately (shown on the HARM display).
    pub heard: bool,
}

/// The HARM display shows a radar whose signal at the player has been at least this strong
/// (the radar sweep's score) in the last `HEARD_SECONDS` of mission time.
pub const HEARD_SIGNAL: i32 = 5;
pub const HEARD_SECONDS: i32 = 60;

/// The emitters, most dangerous first.
#[derive(Resource, Default, Debug)]
pub struct Emitters {
    pub list: Vec<EmitterView>,
    /// Mission second each radar's signal was last heard (see [`HEARD_SIGNAL`]).
    pub heard: HashMap<EmitterSource, i32>,
}

/// RWR symbol of a radar type ("SA-10" -> "10", "SA-N-6" -> "N6") or an aircraft type
/// ("MIG-29" -> "29", "F-14" -> "F14").
pub fn rwr_label(name: &str, air: bool) -> String {
    let up = name.to_ascii_uppercase();
    let named = [
        ("HAWK", "HK"),
        ("RAPIER", "RA"),
        ("TIGER", "TG"),
        ("SEACAT", "SC"),
        ("OTH", "OT"),
        ("IL76", "AW"),
        ("IL-76", "AW"),
    ];
    if let Some((_, l)) = named.iter().find(|(n, _)| up == *n) {
        return l.to_string();
    }
    if let Some(rest) = up.strip_prefix("SA-") {
        return rest.replace('-', "");
    }
    let digits: String =
        up.rsplit('-').next().unwrap_or("").chars().filter(char::is_ascii_alphanumeric).collect();
    if air && up.starts_with("F-") {
        return format!("F{digits}");
    }
    if !digits.is_empty() && digits.len() <= 3 {
        return digits;
    }
    let short: String = up.chars().filter(char::is_ascii_alphanumeric).take(2).collect();
    if short.is_empty() { "U".to_string() } else { short }
}

/// The display view of a sim emitter: positions in world units, bearing relative to the
/// aircraft heading (BAM).
pub fn view(e: &Emitter, name: &str, heading_bam: f32) -> EmitterView {
    let (east, north) = crate::combat::map_wu(e.x, e.y);
    let rel =
        (e.bearing as f32 - heading_bam).rem_euclid(65536.0) / 65536.0 * std::f32::consts::TAU;
    let air = matches!(e.source, EmitterSource::Aircraft(_));
    EmitterView {
        source: e.source,
        east,
        north,
        rel_bearing: rel,
        range_km: km(e.range),
        lock_range_km: km(e.lock_range),
        label: rwr_label(name, air),
        air,
        tracking: e.tracking,
        launch: e.launch,
        priority: e.priority,
        heard: false,
    }
}

/// Simulation range units (map >> 6) -> km.
pub fn km(r: i32) -> f32 {
    r as f32 * 64.0 * 32.0 * crate::world::WU as f32 / 1000.0
}

/// Distance of a symbol from the centre (fraction of the scope radius): a launching and
/// tracking SAM sits near the centre, a radar only searching near the rim.
pub fn rwr_radius(priority: i32) -> f32 {
    0.9 - 0.72 * (priority as f32 / 520.0).clamp(0.0, 1.0)
}

/// Screen position of an emitter on a scope of radius `r` centred at `c` (nose up).
pub fn scope_point(e: &EmitterView, c: Vec2, r: f32) -> Vec2 {
    let d = rwr_radius(e.priority) * r;
    c + Vec2::new(e.rel_bearing.sin(), -e.rel_bearing.cos()) * d
}

/// Colours (stable palette indices): dark green, green, white.
const DIM: u8 = 4;
const GREEN: u8 = 10;
const WHITE: u8 = 15;

/// Draws the emitter symbols on a scope of radius `r` centred at `c`: the label (with a hat for
/// airborne radars), a diamond around the most dangerous one, a circle around a launching one
/// (flashing with `blink`). `locked`: a box around that emitter.
pub fn draw_symbols(
    c: &mut Canvas,
    emitters: &[EmitterView],
    centre: Vec2,
    r: f32,
    blink: bool,
    locked: Option<EmitterSource>,
    filter: impl Fn(&EmitterView) -> bool,
) {
    let h = (r * 0.13).clamp(7.0, 22.0);
    let w = (h / 7.0).max(1.0);
    for (rank, e) in emitters.iter().enumerate().filter(|(_, e)| filter(e)).rev() {
        let p = scope_point(e, centre, r);
        let col = if e.tracking || e.launch { GREEN } else { DIM };
        stroke_text_at(c, p.x, p.y, &e.label, h, w, col, Align::Centre);
        if e.air {
            let half = h * 0.6;
            c.polyline(
                &[
                    p + Vec2::new(-half, -h * 0.75),
                    p + Vec2::new(0.0, -h * 1.05),
                    p + Vec2::new(half, -h * 0.75),
                ],
                w,
                col,
            );
        }
        let s = h * 1.15;
        if rank == 0 {
            c.polyline(
                &[
                    p + Vec2::new(0.0, -s),
                    p + Vec2::new(s, 0.0),
                    p + Vec2::new(0.0, s),
                    p + Vec2::new(-s, 0.0),
                    p + Vec2::new(0.0, -s),
                ],
                w,
                GREEN,
            );
        }
        if e.launch && blink {
            c.ring(p, s * 1.2, w, WHITE);
        }
        if locked == Some(e.source) {
            c.frame(p - Vec2::splat(s * 1.3), p + Vec2::splat(s * 1.3), w, WHITE);
        }
    }
}

/// The F-16's RWR scope filling a square canvas: rings, the centre cross and the emitters.
pub fn draw_rwr(c: &mut Canvas, emitters: &[EmitterView], blink: bool) {
    c.px.fill(0);
    let n = c.w.min(c.h) as f32;
    let centre = Vec2::new(c.w as f32 / 2.0, c.h as f32 / 2.0);
    let r = n / 2.0 - 1.0;
    // Black face inside the bezel ring.
    for y in 0..c.h {
        for x in 0..c.w {
            if (Vec2::new(x as f32 + 0.5, y as f32 + 0.5) - centre).length() <= r {
                c.set(x as i32, y as i32, 5);
            }
        }
    }
    let w = (n / 160.0).max(1.0);
    for f in [0.33, 0.66] {
        c.ring(centre, r * f, w, DIM);
    }
    c.ring(centre, r - w, w, DIM);
    let t = r * 0.06;
    c.stroke(centre - Vec2::X * t, centre + Vec2::X * t, w, DIM);
    c.stroke(centre - Vec2::Y * t, centre + Vec2::Y * t, w, DIM);
    // Ticks every 30 degrees on the rim.
    for k in 0..12 {
        let a = k as f32 * std::f32::consts::TAU / 12.0;
        let d = Vec2::new(a.sin(), -a.cos());
        c.stroke(centre + d * (r - w * 2.0), centre + d * (r * 0.9), w, DIM);
    }
    draw_symbols(c, emitters, centre, r, blink, None, |_| true);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels() {
        assert_eq!(rwr_label("SA-10", false), "10");
        assert_eq!(rwr_label("SA-N-6", false), "N6");
        assert_eq!(rwr_label("Hawk", false), "HK");
        assert_eq!(rwr_label("IL76", true), "AW");
        assert_eq!(rwr_label("MIG-29", true), "29");
        assert_eq!(rwr_label("F-14", true), "F14");
        assert_eq!(rwr_label("Su-27", true), "27");
        assert_eq!(rwr_label("", false), "U");
    }

    #[test]
    fn priority_towards_the_centre() {
        assert!(rwr_radius(600) < rwr_radius(200));
        assert!(rwr_radius(200) < rwr_radius(20));
        assert!(rwr_radius(10_000) > 0.1 && rwr_radius(-5) < 0.95);
    }
}
