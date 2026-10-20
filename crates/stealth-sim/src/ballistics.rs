//! Bomb ballistics (not the original's: its free-fall bombs flew on at half the aircraft's
//! speed, turning down at a fixed rate to a floor set by the release altitude, and its
//! retarded bombs fell straight down). A bomb leaves with the aircraft's velocity and falls
//! under gravity against a linear drag. The path has a closed form, and the bombsights (CCIP,
//! CCRP) predict the impact with the same functions the bombs fly by.
//!
//! Units are the flight models': world units horizontally and feet vertically, both taken as
//! feet (an aircraft flies velocity / 16 of them per second).

use std::f64::consts::TAU;

/// Gravity (ft/s²).
pub const GRAVITY: f64 = 32.174;
/// Linear drag (1/s): a slick bomb keeps most of its speed; a retarded one (fins or a chute)
/// sheds it within seconds and lands well behind the aircraft on a short, steep arc.
pub const SLICK_DRAG: f64 = 0.03;
pub const RETARDED_DRAG: f64 = 0.4;
/// Bombs leave the bay this far below the aircraft (ft).
pub const RELEASE_DROP: f64 = 20.0;

/// Drag of a projectile type that falls ballistically: free-fall (30) and retarded (29)
/// bombs, and the supply pod (38, under a chute) when the original's bugs are fixed.
pub fn drag(spec: i32, fix_original_bugs: bool) -> Option<f64> {
    match spec {
        30 => Some(SLICK_DRAG),
        29 => Some(RETARDED_DRAG),
        38 if fix_original_bugs => Some(RETARDED_DRAG),
        _ => None,
    }
}

/// The aircraft's velocity (ft/s: east, south, up) from its heading (BAM), velocity
/// (knots * 27) and climb rate (ft/s).
pub fn release_velocity(heading: i32, velocity: i32, climb: i32) -> [f64; 3] {
    let v = velocity as f64 / 16.0;
    let vz = (climb as f64).clamp(-v, v);
    let vh = (v * v - vz * vz).max(0.0).sqrt();
    let h = (heading as u16) as f64 * TAU / 65536.0;
    [vh * h.sin(), -vh * h.cos(), vz]
}

/// The integral of exp(-k s) over 0..t.
fn decay(k: f64, t: f64) -> f64 {
    if k * t < 1e-6 { t * (1.0 - k * t / 2.0) } else { (1.0 - (-k * t).exp()) / k }
}

/// Offset from the release point after `t` seconds, for release velocity `v`.
pub fn offset(v: [f64; 3], k: f64, t: f64) -> [f64; 3] {
    let e = decay(k, t);
    // z'' = -g - k z': z = vz E - g (t - E) / k, with (t - E) / k -> t²/2 as k -> 0.
    let sag = if k * t < 1e-4 { t * t / 2.0 * (1.0 - k * t / 3.0) } else { (t - e) / k };
    [v[0] * e, v[1] * e, v[2] * e - GRAVITY * sag]
}

/// Velocity after `t` seconds.
pub fn velocity(v: [f64; 3], k: f64, t: f64) -> [f64; 3] {
    let d = (-k * t).exp();
    [v[0] * d, v[1] * d, v[2] * d - GRAVITY * decay(k, t)]
}

/// Seconds to fall `h` feet (None if not above the ground).
pub fn fall_time(vz: f64, k: f64, h: f64) -> Option<f64> {
    if h <= 0.0 {
        return None;
    }
    let z = |t: f64| offset([0.0, 0.0, vz], k, t)[2] + h;
    let mut hi = (vz.max(0.0) + (vz * vz + 2.0 * GRAVITY * h).sqrt()) / GRAVITY;
    while z(hi) > 0.0 {
        hi *= 2.0;
    }
    let mut lo = 0.0;
    // The path is concave (z'' = -g - k z' < 0 above terminal speed): one root.
    for _ in 0..60 {
        let mid = 0.5 * (lo + hi);
        if z(mid) > 0.0 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Some(hi)
}

/// Where a bomb released `h` feet above the ground with velocity `v` lands: its offset from
/// the release point (ft: east, south, down to the ground) and the time of fall.
pub fn impact(v: [f64; 3], k: f64, h: f64) -> Option<([f64; 3], f64)> {
    let t = fall_time(v[2], k, h)?;
    let d = offset(v, k, t);
    Some(([d[0], d[1], -h], t))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_drag_is_a_parabola() {
        let v = [400.0, 0.0, 30.0];
        let t = 3.0;
        let d = offset(v, 0.0, t);
        assert!((d[0] - 1200.0).abs() < 1e-6);
        assert!((d[2] - (90.0 - 0.5 * GRAVITY * 9.0)).abs() < 1e-6);
        let ft = fall_time(0.0, 0.0, 500.0).unwrap();
        assert!((ft - (1000.0 / GRAVITY).sqrt()).abs() < 1e-6, "{ft}");
    }

    #[test]
    fn stepping_matches_the_closed_form() {
        let (v0, k) = ([500.0, -200.0, 50.0], RETARDED_DRAG);
        let (mut p, mut v) = ([0.0f64; 3], v0);
        for _ in 0..45 {
            let d = offset(v, k, 1.0 / 15.0);
            v = velocity(v, k, 1.0 / 15.0);
            p = [p[0] + d[0], p[1] + d[1], p[2] + d[2]];
        }
        let want = offset(v0, k, 3.0);
        for a in 0..3 {
            assert!((p[a] - want[a]).abs() < 1e-6, "{p:?} {want:?}");
        }
    }

    #[test]
    fn retarded_bombs_land_short_and_late() {
        let v = release_velocity(0x4000, 450 * 27, 0);
        assert!((v[0] - 450.0 * 27.0 / 16.0).abs() < 1e-6 && v[1].abs() < 1e-6);
        let (slick, ts) = impact(v, SLICK_DRAG, 500.0).unwrap();
        let (retarded, tr) = impact(v, RETARDED_DRAG, 500.0).unwrap();
        assert!(retarded[0] > 0.0 && retarded[0] < slick[0] * 0.75, "{retarded:?} {slick:?}");
        assert!(tr > ts);
        // Thrown up first: falls longer than from level flight.
        let up = release_velocity(0, 400 * 27, 200);
        assert!(fall_time(up[2], SLICK_DRAG, 500.0).unwrap() > fall_time(0.0, SLICK_DRAG, 500.0).unwrap() + 5.0);
    }
}
