//! Bomb ballistics (classic free-fall bombs flew on at half the aircraft's speed, turning
//! down at a fixed rate to a floor set by the release altitude, and classic retarded bombs
//! fell straight down). A bomb leaves with the aircraft's velocity and falls
//! under gravity against air drag (proportional to the speed squared), integrated in steps of
//! one simulation frame. The bombsights (CCIP, CCRP) predict the impact by flying the same
//! steps, so a bomb lands where they show.
//!
//! Units are the flight models': world units horizontally and feet vertically, both taken as
//! feet (an aircraft flies velocity / 16 of them per second).

use std::f64::consts::TAU;

/// Gravity (ft/s²).
pub const GRAVITY: f64 = 32.174;
/// Terminal velocities (ft/s): a slick bomb keeps most of its speed; a retarded one (fins or
/// a chute) sheds it within seconds and lands well behind the aircraft on a short, steep arc.
pub const SLICK_TERMINAL: f64 = 1100.0;
pub const RETARDED_TERMINAL: f64 = 250.0;
/// Bombs leave the bay this far below the aircraft (ft).
pub const RELEASE_DROP: f64 = 20.0;
/// Longest fall predicted (s).
const MAX_FALL: f64 = 300.0;

/// Drag constant (1/ft: deceleration = drag * speed²) of a projectile type that falls
/// ballistically: free-fall (30) and retarded (29) bombs, and the supply pod (38, under a
/// chute) when classic bugs are fixed.
pub fn drag(spec: i32, fix_classic_bugs: bool) -> Option<f64> {
    let terminal = match spec {
        30 => SLICK_TERMINAL,
        29 => RETARDED_TERMINAL,
        38 if fix_classic_bugs => RETARDED_TERMINAL,
        _ => return None,
    };
    Some(GRAVITY / (terminal * terminal))
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

/// One step of `dt` seconds: position `p` and velocity `v` (east, south, up).
pub fn step(p: &mut [f64; 3], v: &mut [f64; 3], drag: f64, dt: f64) {
    // Semi-implicit: the drag of the new speed, so it never reverses the motion.
    let speed = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let k = 1.0 / (1.0 + drag * speed * dt);
    v[2] -= GRAVITY * dt;
    for a in 0..3 {
        v[a] *= k;
        p[a] += v[a] * dt;
    }
}

/// Where a bomb released `h` feet above the ground with velocity `v`, flown in steps of `dt`,
/// lands: its offset from the release point (ft: east, south, down to the ground) and the
/// time of fall.
pub fn impact(v: [f64; 3], drag: f64, h: f64, dt: f64) -> Option<([f64; 3], f64)> {
    if h <= 0.0 || dt <= 0.0 {
        return None;
    }
    let (mut p, mut v) = ([0.0, 0.0, h], v);
    let mut t = 0.0;
    while t < MAX_FALL {
        let before = p;
        step(&mut p, &mut v, drag, dt);
        if p[2] <= 0.0 {
            let k = before[2] / (before[2] - p[2]);
            let at = |a: usize| before[a] + (p[a] - before[a]) * k;
            return Some(([at(0), at(1), -h], t + dt * k));
        }
        t += dt;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f64 = 1.0 / 15.0;

    #[test]
    fn falls_like_a_bomb() {
        // Nearly no drag: half g t².
        let (d, t) = impact([0.0, 0.0, 0.0], 1e-9, 500.0, 0.001).unwrap();
        assert!((t - (1000.0 / GRAVITY).sqrt()).abs() < 0.01, "{t}");
        assert!(d[0].abs() < 1e-9);
        // Dropped from high up it reaches its terminal velocity.
        let k = drag(29, false).unwrap();
        let (mut p, mut v) = ([0.0; 3], [0.0; 3]);
        for _ in 0..15 * 60 {
            step(&mut p, &mut v, k, DT);
        }
        assert!((v[2] + RETARDED_TERMINAL).abs() < 1.0, "{v:?}");
    }

    #[test]
    fn retarded_bombs_land_short_and_late() {
        let v = release_velocity(0x4000, 450 * 27, 0);
        assert!((v[0] - 450.0 * 27.0 / 16.0).abs() < 1e-6 && v[1].abs() < 1e-6);
        let slick = drag(30, false).unwrap();
        let retarded = drag(29, false).unwrap();
        let (ds, ts) = impact(v, slick, 500.0, DT).unwrap();
        let (dr, tr) = impact(v, retarded, 500.0, DT).unwrap();
        assert!(dr[0] > 0.0 && dr[0] < ds[0] * 0.75, "{dr:?} {ds:?}");
        assert!(tr > ts);
        // From 3,000 ft a retarded bomb still lands within about 20 s.
        let (_, t) = impact(v, retarded, 3000.0, DT).unwrap();
        assert!(t < 20.0, "{t}");
        // Thrown up first: falls longer than from level flight.
        let up = release_velocity(0, 400 * 27, 200);
        assert!(impact(up, slick, 500.0, DT).unwrap().1 > ts + 5.0);
    }
}
