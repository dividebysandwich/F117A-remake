//! Dynamic launch zone of the selected air-to-ground missile against the ground lock: the
//! ground ranges from which a missile fired now, with the aircraft's speed, altitude, heading
//! and pitch, reaches the target. Found by flying the missile out with the simulation's own
//! flight ([`Sim::classic_step`] or [`guidance::flight_step`]) at targets along the line to the
//! locked one, and capped by the range the seeker locks on from at launch (the LOCK oval).

use crate::Sim;
use crate::guidance::{self, Step, Target};
use crate::math::s16;
use crate::missiles::seeker_geometry;
use crate::types::*;

/// Ranges sampled across the lock range, before the zone's edges are bisected.
const SAMPLES: i32 = 20;
/// Bisection steps per edge.
const REFINE: u32 = 7;
/// Angle off the missile's heading (BAM) within which its seeker sees the target.
const SEEKER_CONE: i32 = 0x1000;

/// Launch zone against the ground lock (ground ranges in map units).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaunchZone {
    /// Range to the target (straight, not the octagonal approximation of the HUD's range).
    pub range: i32,
    /// The longest range the seeker locks on from (the scale's end).
    pub lock_max: i32,
    /// Shortest and longest range a missile fired now reaches the target from; None when it
    /// reaches it from none (too low, too slow, pointing away).
    pub zone: Option<(i32, i32)>,
}

impl LaunchZone {
    /// A missile fired now reaches the target.
    pub fn in_zone(&self) -> bool {
        self.zone.is_some_and(|(lo, hi)| (lo..=hi).contains(&self.range))
    }

    /// The target is inside the shortest range.
    pub fn too_close(&self) -> bool {
        self.zone.is_some_and(|(lo, _)| self.range < lo)
    }
}

impl Sim {
    /// The launch zone of the selected air-to-ground missile (HARM, Penguin, Harpoon,
    /// Maverick) against the ground lock in GND mode; None without one, or when the missile
    /// can do nothing to the target.
    pub fn launch_zone(&self) -> Option<LaunchZone> {
        if self.pl.hud_mode != HudMode::Gnd || self.pl.gun_selected {
            return None;
        }
        let (w, count) = self.selected_weapon();
        let spec = self.selected_spec();
        let mt = self.missile(spec);
        let cls = mt.class as i32;
        if count <= 0 || !matches!(cls, 4..=6) {
            return None;
        }
        let g = usize::try_from(self.pl.gnd_lock).ok().filter(|&g| g < self.objects.len())?;
        let o = self.objects[g];
        if o.flags & oflags::DESTROYED != 0
            || self.compat(w, g as i32) == 0
            || (cls == 4 && o.unit_type == 0)
            || (cls == 5 && !self.is_ship(g as i32))
        {
            return None;
        }
        // The oval locks on within 56 map units per lock range unit ahead (lock_oval).
        let slant = 56.0 * mt.lock_range as f64;
        let height = (self.pl.p.view_z >> 5) as f64;
        let lock_max = (slant * slant - height * height).max(0.0).sqrt() as i32;
        let (dx, dy) = (s16(o.x - self.pl.p.mx) as f64, s16(o.y - self.pl.p.my) as f64);
        let d = dx.hypot(dy);
        let range = d.round() as i32;
        let zone = if lock_max < 1 || d < 1.0 {
            None
        } else {
            let at = |r: i32| {
                let k = r as f64 / d;
                let tx = (self.pl.p.mx + (dx * k).round() as i32) & 0xFFFF;
                let ty = (self.pl.p.my + (dy * k).round() as i32) & 0xFFFF;
                self.reaches(w, spec, tx, ty)
            };
            find_zone(lock_max, at)
        };
        Some(LaunchZone { range, lock_max, zone })
    }

    /// A missile `spec` (store `w`) fired now at a target on the ground at map point
    /// (tx, ty) reaches it.
    fn reaches(&self, w: i32, spec: i32, tx: i32, ty: i32) -> bool {
        let mut m = self.launch_projectile(w, spec);
        let fs = self.fs;
        let guided = self.guided(spec);
        if guided {
            let (pos, vel) = self.launch_motion();
            guidance::start_flight(&mut m, pos, vel, fs);
        }
        let vmax = self.guided_vmax(spec);
        let target =
            Target { p: [tx as f64 * 32.0 + 16.0, ty as f64 * 32.0 + 16.0, 0.0], v: [0.0; 3] };
        let top = (self.missile(spec).max_speed as i32) >> 6;
        // Fired in the next simulation frame, which moves it with this frame's number (the
        // number advances at the end of a frame).
        let mut frame = self.frame - 1;
        while m.ttl > 0 {
            frame += 1;
            if !guided && m.speed < top && frame & 1 != 0 {
                m.speed += 1;
            }
            let (r, aim, off) = seeker_geometry(&m, tx, ty);
            // Out of the seeker's view the missile flies straight on and never sees it again.
            if off > SEEKER_CONE {
                return false;
            }
            if guided {
                let guide = (aim != 0).then_some(target);
                match guidance::flight_step(
                    &mut m,
                    vmax,
                    guidance::FUSE_GROUND,
                    fs,
                    guide,
                    Some(target),
                ) {
                    Step::Fused => return true,
                    Step::Flying => {}
                    Step::Ground | Step::Spent => return false,
                }
            } else {
                if self.classic_step(&mut m, false, true, aim, r, 0) {
                    return true;
                }
                if m.alt < 0 {
                    return false;
                }
            }
        }
        false
    }
}

/// The ranges in 1..=`max` for which `reaches` holds, as (shortest, longest): sampled, then
/// the edges bisected. The samples are closer together at short range, where the shortest
/// range lies. Only the run of ranges up to the longest counts: a missile that misses from
/// farther in may still hit from close by by luck (diving down steeply onto the target).
fn find_zone(max: i32, reaches: impl Fn(i32) -> bool) -> Option<(i32, i32)> {
    let sample = |k: i32| {
        ((max as f64 * (k as f64 / SAMPLES as f64).powf(1.5)).round() as i32).clamp(1, max)
    };
    let hits: Vec<bool> = (1..=SAMPLES).map(|k| reaches(sample(k))).collect();
    let last = hits.iter().rposition(|&h| h)? as i32 + 1;
    let first = (1..=last).rev().take_while(|&k| hits[k as usize - 1]).last()?;
    // Between a miss and a hit.
    let edge = |mut miss: i32, mut hit: i32| {
        for _ in 0..REFINE {
            if (miss - hit).abs() <= 1 {
                break;
            }
            let mid = (miss + hit) / 2;
            if reaches(mid) {
                hit = mid;
            } else {
                miss = mid;
            }
        }
        hit
    };
    let lo = edge(if first == 1 { 0 } else { sample(first - 1) }, sample(first));
    let hi = if last == SAMPLES { max } else { edge(sample(last + 1), sample(last)) };
    Some((lo, hi))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_zone_edges() {
        let z = find_zone(2000, |r| (300..=1400).contains(&r)).unwrap();
        assert!((z.0 - 300).abs() <= 20 && (z.1 - 1400).abs() <= 20, "{z:?}");
        let z = find_zone(2000, |r| r >= 100).unwrap();
        assert!((z.0 - 100).abs() <= 5 && z.1 == 2000, "{z:?}");
        // A lucky hit from close in is no part of the zone.
        let z = find_zone(2000, |r| (40..60).contains(&r) || (300..=1400).contains(&r)).unwrap();
        assert!((z.0 - 300).abs() <= 20, "{z:?}");
        assert_eq!(find_zone(2000, |_| false), None);
    }
}
