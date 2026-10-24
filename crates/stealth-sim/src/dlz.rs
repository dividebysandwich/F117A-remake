//! Dynamic launch zones of the selected missile: the ranges from which a missile fired now,
//! with the aircraft's speed, altitude, heading and pitch, reaches the locked target. Found by
//! flying the missile out with the simulation's own flight ([`Sim::classic_step`] or
//! [`guidance::flight_step`]) at targets along the line to the locked one.
//!
//! - Air-to-ground missiles against the ground lock (GND): capped by the range the seeker
//!   locks on from at launch (the LOCK oval); fired without that lock the missile flies blind.
//!   The Hellfire against the targeting pod's laser spot, which its seeker sees at any range.
//!   Mavericks and Hellfires against the pod's ground point (the SPI) as it is slewed, the
//!   ground lock while the pod rests on it or without the pod.
//! - Air-to-air missiles against the air lock (AIR): the target flies on as it does now, and
//!   for the no-escape range turns tail at the launch.

use crate::Sim;
use crate::guidance::{self, Step, Target};
use crate::math::{approx, s16};
use crate::missiles::seeker_geometry;
use crate::types::*;

/// Ranges sampled across the searched ranges, before the zone's edges are bisected.
const SAMPLES: i32 = 20;
/// Bisection steps per edge.
const REFINE: u32 = 7;
/// Angle off the missile's heading (BAM) within which its seeker sees the target.
const SEEKER_CONE: i32 = 0x1000;
/// Hellfire: farthest range searched (map units, some 8 km).
const HELLFIRE_SEARCH: i32 = 0x200;
/// Map units ahead per unit of lock range within which the LOCK oval locks on: ground and air
/// (lock_oval).
const GROUND_LOCK: f64 = 56.0;
const AIR_LOCK: i32 = 64;
/// Air-to-air: farthest range searched, the farthest the radar locks on from (select_air; a
/// missile fired head-on at a fast target reaches well beyond the oval's lock range).
const AIR_SEARCH: i32 = 0x12C0;
/// Map units an aircraft target draws away from a missile's closest approach before the
/// missile counts as beaten (more than the rounding of the positions).
const DRAWING_AWAY: i32 = 4;
/// Air-to-air: ranges around a miss checked for hits, a share of the range (1/n) and the
/// least (map units).
const NEAR_MISS_SPREAD: i32 = 50;
const NEAR_MISS_MIN: i32 = 8;

/// What a predicted missile flies at: its position at the launch (wu: east, south, up), its
/// velocity (wu/s) and its altitude as the classic flight compares it (viewZ for aircraft).
#[derive(Debug, Clone, Copy)]
struct Track {
    p: [f64; 3],
    v: [f64; 3],
    alt: i32,
}

/// Launch zone against the locked target (ground ranges in map units).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaunchZone {
    /// Range to the target (straight, not the octagonal approximation of the HUD's range).
    pub range: i32,
    /// The longest range the LOCK oval locks on from.
    pub lock_max: i32,
    /// Shortest and longest range a missile fired now reaches the target from; None when it
    /// reaches it from none (too low, too slow, pointing away). Air-to-air: with the target
    /// flying on as it does now, which may take the longest range past `lock_max`.
    pub zone: Option<(i32, i32)>,
    /// Air-to-air: the longest range from which the missile still reaches a target that turns
    /// tail at the launch (no escape inside it); None for ground targets or when there is none.
    pub no_escape: Option<i32>,
    /// The zone is against the lock (air, or the ground lock), not a pod point away from it.
    pub at_lock: bool,
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
    /// The launch zone of the selected missile: an air-to-ground one (HARM, Penguin, Harpoon,
    /// Maverick) against the ground lock in GND mode (the Hellfire against the pod's spot), an
    /// air-to-air one against the air lock in AIR mode; None without one, or when the missile
    /// can do nothing to the target.
    pub fn launch_zone(&self) -> Option<LaunchZone> {
        if self.pl.gun_selected {
            return None;
        }
        match self.pl.hud_mode {
            HudMode::Gnd => self.ground_zone(),
            HudMode::Air => self.air_zone(),
            HudMode::Nav => None,
        }
    }

    fn ground_zone(&self) -> Option<LaunchZone> {
        let (w, count) = self.selected_weapon();
        let spec = self.selected_spec();
        let mt = self.missile(spec);
        let cls = mt.class as i32;
        if count <= 0 || !matches!(cls, 4..=6) {
            return None;
        }
        let lock = usize::try_from(self.pl.gnd_lock).ok().filter(|&g| g < self.objects.len());
        // The pod's ground point, unless it rests on the lock.
        let spi = self.pl.last_input.pod.and_then(|p| p.ground).filter(|&(x, y)| {
            cls == 6
                && lock.is_none_or(|g| {
                    let o = self.objects[g];
                    approx(o.x - x, o.y - y) > DESIGNATE_RADIUS
                })
        });
        let slant = GROUND_LOCK * mt.lock_range as f64;
        let height = (self.pl.p.view_z >> 5) as f64;
        let lock_max = (slant * slant - height * height).max(0.0).sqrt() as i32;
        let (tx, ty, lock_max, at_lock) = if spec == HELLFIRE_SPEC {
            let (x, y) = self.pl.last_input.pod.and_then(|p| p.ground)?;
            (x, y, HELLFIRE_SEARCH, spi.is_none())
        } else if let Some((x, y)) = spi {
            (x, y, lock_max, false)
        } else {
            let g = lock?;
            let o = self.objects[g];
            if o.flags & oflags::DESTROYED != 0
                || self.compat(w, g as i32) == 0
                || (cls == 4 && o.unit_type == 0)
                || (cls == 5 && !self.is_ship(g as i32))
            {
                return None;
            }
            (o.x, o.y, lock_max, true)
        };
        let (dx, dy) = (s16(tx - self.pl.p.mx) as f64, s16(ty - self.pl.p.my) as f64);
        let d = dx.hypot(dy);
        let range = d.round() as i32;
        let zone = if lock_max < 1 || d < 1.0 {
            None
        } else {
            let at = |r: i32| {
                let k = r as f64 / d;
                let tx = self.pl.p.mx + (dx * k).round() as i32;
                let ty = self.pl.p.my + (dy * k).round() as i32;
                let p = [tx as f64 * 32.0 + 16.0, ty as f64 * 32.0 + 16.0, 0.0];
                self.reaches(w, spec, Track { p, v: [0.0; 3], alt: 0 }, false)
            };
            find_zone(lock_max, 0, at)
        };
        Some(LaunchZone { range, lock_max, zone, no_escape: None, at_lock })
    }

    fn air_zone(&self) -> Option<LaunchZone> {
        let (w, count) = self.selected_weapon();
        let spec = self.selected_spec();
        let mt = self.missile(spec);
        if count <= 0 || mt.class != 7 {
            return None;
        }
        let lock_max = AIR_LOCK * mt.lock_range as i32;
        let lock = usize::try_from(self.pl.air_lock).ok().filter(|&i| {
            self.flights.get(i).is_some_and(|u| {
                u.flags & fflags::AIRBORNE != 0 && u.flags & fflags::DESTROYED == 0 && u.speed != 0
            })
        });
        let (tg, alt) = if let Some(i) = lock {
            (self.unit_target(i), self.flights[i].alt)
        } else {
            let k = self.pl.pilot_lock.filter(|&k| self.pilot_airborne(k))?;
            (self.pilot_target(k), self.pilot_ref(k as usize).p.view_z)
        };
        let me = self.player_target().p;
        let (dx, dy) = (tg.p[0] - me[0], tg.p[1] - me[1]);
        let d = dx.hypot(dy) / 32.0;
        let range = d.round() as i32;
        if d < 1.0 || lock_max < 1 {
            return Some(LaunchZone {
                range,
                lock_max,
                zone: None,
                no_escape: None,
                at_lock: true,
            });
        }
        let (ux, uy) = (dx / 32.0 / d, dy / 32.0 / d);
        let hits = |r: i32, v: [f64; 3]| {
            let p = [me[0] + ux * r as f64 * 32.0, me[1] + uy * r as f64 * 32.0, tg.p[2]];
            self.reaches(w, spec, Track { p, v, alt }, true)
        };
        // A missile passing just outside its fuse radius misses from a range here and there
        // all through the zone, a different one with every change of the geometry: a miss
        // between hits a little closer and a little farther counts as a hit, or the zone
        // would jump about.
        let at = |r: i32, v: [f64; 3]| {
            let d = (r / NEAR_MISS_SPREAD).max(NEAR_MISS_MIN);
            hits(r, v) || (hits(r - d, v) && hits(r + d, v))
        };
        let zone = find_zone(AIR_SEARCH, 1, |r| at(r, tg.v));
        // Turning tail: away from the launch at its speed.
        let vh = tg.v[0].hypot(tg.v[1]);
        let away = [ux * vh, uy * vh, tg.v[2]];
        let no_escape = zone.and_then(|(lo, hi)| far_edge(lo, hi, |r| at(r, away)));
        Some(LaunchZone { range, lock_max, zone, no_escape, at_lock: true })
    }

    /// A missile `spec` (store `w`) fired now reaches the target flying `track` (`air`: an
    /// aircraft, else on the ground).
    fn reaches(&self, w: i32, spec: i32, track: Track, air: bool) -> bool {
        let mut m = self.launch_projectile(w, spec);
        let fs = self.fs;
        let guided = self.guided(spec);
        let hellfire = spec == HELLFIRE_SPEC;
        let (pos, mut vel) = self.launch_motion();
        if hellfire {
            // It pitches onto the spot as it leaves the rail (`fire_missile`).
            (m.heading, m.pitch) = guidance::heading_pitch([
                track.p[0] - pos[0],
                track.p[1] - pos[1],
                track.p[2] - pos[2],
            ]);
            let speed = vel.iter().map(|v| v * v).sum::<f64>().sqrt();
            vel = guidance::direction(m.heading, m.pitch).map(|d| d * speed);
        }
        if guided {
            guidance::start_flight(&mut m, pos, vel, fs);
        }
        let cone = if hellfire { HELLFIRE_SEEKER } else { SEEKER_CONE };
        let vmax = self.guided_vmax(spec);
        let radius = if air { guidance::FUSE_AAM } else { guidance::FUSE_GROUND };
        let top = (self.missile(spec).max_speed as i32) >> 6;
        let dt = 1.0 / fs.max(1) as f64;
        // Fired in the next simulation frame, which moves it with this frame's number (the
        // number advances at the end of a frame).
        let mut frame = self.frame - 1;
        let mut t = 0.0;
        let mut closest = i32::MAX;
        let burn = guidance::profile(spec).map_or(0.0, |p| p.burn);
        while m.ttl > 0 {
            frame += 1;
            if !guided && m.speed < top && frame & 1 != 0 {
                m.speed += 1;
            }
            let p: [f64; 3] = std::array::from_fn(|k| track.p[k] + track.v[k] * t);
            let talt = track.alt + (track.v[2] * t) as i32;
            t += dt;
            let (tx, ty) =
                (((p[0] / 32.0).floor() as i32) & 0xFFFF, ((p[1] / 32.0).floor() as i32) & 0xFFFF);
            let (r, aim, off) = seeker_geometry(&m, tx, ty);
            let seen = off <= cone;
            // Out of the seeker's view the missile flies straight on: it never sees a target
            // on the ground again, nor one flying on straight once that draws away. Nor does
            // a missile at its full speed or burnt out catch an aircraft drawing away.
            // Straight, not the seeker's octagonal approximation: that grows as the bearing
            // swings with the missile turning onto the target.
            let dist = (s16(tx - m.x) as f64).hypot(s16(ty - m.y) as f64) as i32;
            let away = dist > closest.saturating_add(DRAWING_AWAY);
            let spent = match m.flight {
                Some(f) => f.t as f64 * dt > burn,
                None => m.speed >= top,
            };
            if !seen && (!air || away) || (air && away && spent) {
                return false;
            }
            closest = closest.min(dist);
            if guided {
                let target = seen.then_some(Target { p, v: track.v });
                let guide = target.filter(|_| aim != 0);
                match guidance::flight_step(&mut m, vmax, radius, fs, guide, target) {
                    Step::Fused => return true,
                    Step::Flying => {}
                    Step::Ground => return false,
                }
            } else {
                if self.classic_step(&mut m, false, seen, aim, r, talt) {
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
/// range lies. Only the run of ranges up to the longest counts, through no more than `gap`
/// missing samples in a row: a missile that misses from farther in may still hit from close
/// by by luck (diving down steeply onto the target).
fn find_zone(max: i32, gap: usize, reaches: impl Fn(i32) -> bool) -> Option<(i32, i32)> {
    let sample = |k: i32| {
        ((max as f64 * (k as f64 / SAMPLES as f64).powf(1.5)).round() as i32).clamp(1, max)
    };
    let hits: Vec<bool> = (1..=SAMPLES).map(|k| reaches(sample(k))).collect();
    let last = hits.iter().rposition(|&h| h)? as i32 + 1;
    let (mut first, mut misses) = (last, 0);
    for k in (1..last).rev() {
        if hits[k as usize - 1] {
            first = k;
            misses = 0;
        } else {
            misses += 1;
            if misses > gap {
                break;
            }
        }
    }
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

/// The longest range in `lo`..=`hi` for which `reaches` holds, `reaches` holding up to it
/// from `lo`: bisected from a hit found coming down from `hi`.
fn far_edge(lo: i32, hi: i32, reaches: impl Fn(i32) -> bool) -> Option<i32> {
    if reaches(hi) {
        return Some(hi);
    }
    let mut miss = hi;
    let mut hit = (1..=4).rev().map(|k| lo + (hi - lo) * k / 5).chain([lo]).find(|&r| {
        let h = reaches(r);
        if !h {
            miss = r;
        }
        h
    })?;
    for _ in 0..REFINE {
        if miss - hit <= 1 {
            break;
        }
        let mid = (miss + hit) / 2;
        if reaches(mid) {
            hit = mid;
        } else {
            miss = mid;
        }
    }
    Some(hit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_zone_edges() {
        let z = find_zone(2000, 0, |r| (300..=1400).contains(&r)).unwrap();
        assert!((z.0 - 300).abs() <= 20 && (z.1 - 1400).abs() <= 20, "{z:?}");
        let z = find_zone(2000, 0, |r| r >= 100).unwrap();
        assert!((z.0 - 100).abs() <= 5 && z.1 == 2000, "{z:?}");
        // A lucky hit from close in is no part of the zone.
        let z = find_zone(2000, 0, |r| (40..60).contains(&r) || (300..=1400).contains(&r)).unwrap();
        assert!((z.0 - 300).abs() <= 20, "{z:?}");
        assert_eq!(find_zone(2000, 0, |_| false), None);
        // A single missing sample within the zone, bridged.
        let z = find_zone(2000, 1, |r| (300..=1400).contains(&r) && !(1000..1010).contains(&r));
        assert!(z.is_some_and(|z| (z.0 - 300).abs() <= 20), "{z:?}");
    }

    #[test]
    fn finds_the_far_edge() {
        assert_eq!(far_edge(100, 2000, |r| r <= 2000), Some(2000));
        let e = far_edge(100, 2000, |r| r <= 700).unwrap();
        assert!((e - 700).abs() <= 15, "{e}");
        let e = far_edge(100, 2000, |r| r <= 150).unwrap();
        assert!((e - 150).abs() <= 3, "{e}");
        assert_eq!(far_edge(100, 2000, |_| false), None);
    }
}
