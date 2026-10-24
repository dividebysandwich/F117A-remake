//! Low-level air defences (`SimOptions::air_defence`): radar-laid
//! ZSU-23-4 Shilkas and optically laid S-60 batteries around the hostile airfields and the
//! targets, and MANPADS teams (SA-14) in the towns. They make flying low over defended places
//! dangerous: the Shilka's Gun Dish radar sees the player like any other radar (EMV), the S-60
//! and the MANPADS need to see him (day or moonlight, clouds).
//!
//! The units live outside the world object list and draw from their own random stream, so the
//! classic behaviour is untouched while the option is off. Shells fly in metres (x east,
//! y north, z up from the ground) with gravity and no drag.

use serde::{Deserialize, Serialize};
use stealth_mission::Mission;

use crate::Sim;
use crate::math::{approx, bearing, s16};
use crate::radar::DetectCtx;
use crate::radar::detect;
use crate::rng::Rng;
use crate::types::*;

/// Metres per map unit (32 world units of 0.48828125 m).
pub const M_PER_MAP: f64 = 15.625;
const M_PER_WU: f64 = M_PER_MAP / 32.0;
const FT: f64 = 0.3048;
const G: f64 = 9.81;

/// The Gun Dish radar for [`detect`]: lethality (km), threat tier, Doppler.
const GUN_DISH: (i32, i32, i32) = (12, 1, 1);
/// SA-14 (missile table index).
pub const SA14: i32 = 0x21;
/// The friendly side's missiles: Stinger (MANPADS) and Patriot (MIM-104).
pub const STINGER: i32 = 46;
pub const PATRIOT: i32 = 45;
/// Launcher of a MANPADS missile (fire and forget: no launcher radar to keep its lock).
pub const MANPADS_LAUNCHER: i32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum AaaKind {
    /// ZSU-23-4 Shilka: four 23 mm guns, Gun Dish radar.
    #[default]
    Zsu,
    /// S-60: 57 mm gun with timed shells, optically laid.
    S60,
    /// MANPADS team with SA-14s (Stingers on the friendly side).
    Manpads,
    /// Patriot M901 launcher: four missiles, fires on its battery radar's track.
    PatriotLauncher,
    /// Patriot AN/MPQ-53 radar: tracks for the launchers around it.
    PatriotRadar,
    /// Gepard: two 35 mm guns, search and tracking radar.
    Gepard,
}

impl AaaKind {
    pub fn name(self) -> &'static str {
        match self {
            AaaKind::Zsu => "ZSU-23-4",
            AaaKind::S60 => "S-60",
            AaaKind::Manpads => "SA-14",
            AaaKind::PatriotLauncher => "Patriot",
            AaaKind::PatriotRadar => "Patriot radar",
            AaaKind::Gepard => "Gepard",
        }
    }

    /// Name on a side: the friendly MANPADS teams carry Stingers.
    pub fn name_on(self, blue: bool) -> &'static str {
        match self {
            AaaKind::Manpads if blue => "Stinger",
            _ => self.name(),
        }
    }

    /// Patriot launcher or radar (no gun, no MANPADS).
    pub fn patriot(self) -> bool {
        matches!(self, AaaKind::PatriotLauncher | AaaKind::PatriotRadar)
    }

    /// Muzzle velocity (m/s), slant range (m) and highest target (m) it engages.
    fn gun(self) -> (f64, f64, f64) {
        match self {
            AaaKind::Zsu => (970.0, 2500.0, 2000.0),
            AaaKind::S60 => (1000.0, 6000.0, 4500.0),
            AaaKind::Manpads => (0.0, 5000.0, 3500.0),
            AaaKind::Gepard => (1175.0, 3500.0, 3000.0),
            AaaKind::PatriotLauncher | AaaKind::PatriotRadar => (0.0, 0.0, 0.0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct AaaUnit {
    pub kind: AaaKind,
    /// Map units (x east, y south).
    pub x: i32,
    pub y: i32,
    pub destroyed: bool,
    /// The Gun Dish holds the player (Shilka), or the crew sees him.
    pub tracking: bool,
    /// Where the guns or the launcher point: azimuth (radians clockwise from north) and
    /// elevation.
    pub aim_az: f32,
    pub aim_el: f32,
    /// Frames left of the burst being fired, and until the next one may start.
    pub burst: i32,
    pub cooldown: i32,
    /// Patriot launcher: missiles ready.
    #[serde(default)]
    pub rounds: i32,
}

impl AaaUnit {
    /// Position in metres (east, north, up).
    pub fn metres(&self) -> [f64; 3] {
        [self.x as f64 * M_PER_MAP, (0x8000 - self.y) as f64 * M_PER_MAP, 2.0]
    }
}

/// A shell in flight (metres, x east, y north, z up; m/s).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Shell {
    pub pos: [f64; 3],
    pub vel: [f64; 3],
    /// Frames left in flight; an S-60 shell bursts when they run out.
    pub ttl: i32,
    pub flak: bool,
    /// The pilot it was fired at.
    #[serde(default)]
    pub pilot: u8,
}

/// A pilot as the air defences see him this frame.
struct Track {
    k: usize,
    me: [f64; 3],
    vel: [f64; 3],
    p: crate::PlayerView,
    emv: i32,
    on_ground: bool,
    lockheed: bool,
}

/// What the AAA does to the player in one frame.
const ZSU_HIT_RADIUS: f64 = 9.0;
const ZSU_HIT_CHANCE: i32 = 450; // per mille, per tracer passing within the radius
const FLAK_LETHAL: f64 = 14.0;
const FLAK_REACH: f64 = 40.0;
/// Units and shells at most.
const MAX_UNITS: usize = 72;
const MAX_SHELLS: usize = 160;

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn mul(a: [f64; 3], k: f64) -> [f64; 3] {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn len(a: [f64; 3]) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Smallest distance from the origin to the segment a .. a + d.
pub(crate) fn closest(a: [f64; 3], d: [f64; 3]) -> f64 {
    let k = (-dot(a, d) / dot(d, d).max(1e-9)).clamp(0.0, 1.0);
    len(add(a, mul(d, k)))
}

/// Unit vector from azimuth (clockwise from north) and elevation.
fn direction(az: f64, el: f64) -> [f64; 3] {
    [az.sin() * el.cos(), az.cos() * el.cos(), el.sin()]
}

/// Where to point a gun at `from` with shells of speed `v` to meet a target at `to` moving at
/// `vel`, gravity included: (azimuth, elevation, time of flight).
pub fn lead(from: [f64; 3], to: [f64; 3], vel: [f64; 3], v: f64) -> (f64, f64, f64) {
    let mut t = len(sub(to, from)) / v;
    let mut aim = to;
    for _ in 0..4 {
        let at = add(to, mul(vel, t));
        aim = add(at, [0.0, 0.0, 0.5 * G * t * t]);
        t = len(sub(at, from)) / v;
    }
    let d = sub(aim, from);
    (d[0].atan2(d[1]), d[2].atan2(d[0].hypot(d[1])), t)
}

impl Sim {
    pub fn aaa_units(&self) -> &[AaaUnit] {
        &self.aaa
    }

    /// The units (scenario tests place their own).
    pub fn aaa_units_mut(&mut self) -> &mut Vec<AaaUnit> {
        &mut self.aaa
    }

    pub fn aaa_shells(&self) -> &[Shell] {
        &self.aaa_shells
    }

    /// The player in metres (east, north, up) and his velocity (m/s), as the gunners see it:
    /// from how he moved over the last frames (the flight model covers more ground than its
    /// airspeed says, and the flight path is not the nose's direction).
    fn player_metres(&mut self) -> ([f64; 3], [f64; 3]) {
        let p = &self.pl.p;
        let pos = [p.wx as f64 * M_PER_WU, p.wy as f64 * M_PER_WU, p.alt as f64 * FT];
        let fs = self.fs.max(1) as f64;
        let vel = match self.pl.aaa_track {
            // A jump (a new start position) is not a velocity.
            Some((last, v)) if len(sub(pos, last)) * fs < 1500.0 => {
                add(mul(v, 0.7), mul(sub(pos, last), fs * 0.3))
            }
            _ => [0.0; 3],
        };
        self.pl.aaa_track = Some((pos, vel));
        (pos, vel)
    }

    /// A random number from about -1 to 1, roughly normal.
    fn aaa_gauss(rng: &mut Rng) -> f64 {
        (0..3).map(|_| rng.random(1000) as f64 / 1000.0).sum::<f64>() * 1.15 - 1.73
    }

    /// Places the units (`Sim::with_options` with the option on).
    pub(crate) fn place_aaa(&mut self, m: &Mission) {
        let mut rng = Rng::new(m.rng.holdrand_exit ^ 0x5EAD_AAA0);
        let war = self.roe;
        let mut units = Vec::new();
        let around =
            |rng: &mut Rng, units: &mut Vec<AaaUnit>, x: i32, y: i32, kind, lo: i32, hi: i32| {
                let a = rng.random(0x4000) * 4;
                let r = (lo + rng.random(hi - lo)) as f64;
                let t = a as f64 / 65536.0 * std::f64::consts::TAU;
                units.push(AaaUnit {
                    kind,
                    x: (x + (t.sin() * r) as i32).clamp(0x40, 0x7FC0),
                    y: (y - (t.cos() * r) as i32).clamp(0x40, 0x7FC0),
                    destroyed: false,
                    tracking: false,
                    aim_az: t as f32,
                    aim_el: 0.3,
                    burst: 0,
                    cooldown: 0,
                    rounds: 0,
                });
            };
        let hostile = |o: &GroundObject| o.flags & (oflags::FRIENDLY | oflags::NEUTRAL) == 0;
        // Airfields: Shilkas, and in a war an S-60 battery (2 to 4 guns around one spot).
        for i in self.ground_units..self.objects.len() {
            let o = self.objects[i];
            if o.flags & oflags::AIRFIELD == 0 || !hostile(&o) || (o.x, o.y) == (0, 0) {
                continue;
            }
            for _ in 0..1 + rng.random(war + 1) {
                around(&mut rng, &mut units, o.x, o.y, AaaKind::Zsu, 26, 90);
            }
            if war > 0 {
                let (bx, by) = (o.x + rng.random(120) - 60, o.y + rng.random(120) - 60);
                for _ in 0..2 + rng.random(3) {
                    around(&mut rng, &mut units, bx, by, AaaKind::S60, 3, 8);
                }
            }
        }
        // The targets.
        for t in self.pl.targets {
            let Some(o) = usize::try_from(t.object).ok().and_then(|i| self.objects.get(i)) else {
                continue;
            };
            if (o.x, o.y) == (0, 0) || !hostile(o) {
                continue;
            }
            let (x, y) = (o.x, o.y);
            for _ in 0..1 + rng.random(war + 1) {
                around(&mut rng, &mut units, x, y, AaaKind::Zsu, 20, 70);
            }
            if rng.random(3) < war {
                let (bx, by) = (x + rng.random(100) - 50, y + rng.random(100) - 50);
                for _ in 0..2 + rng.random(2) {
                    around(&mut rng, &mut units, bx, by, AaaKind::S60, 3, 8);
                }
            }
        }
        // MANPADS in and around the towns (the other places of the world).
        for i in self.ground_units..self.objects.len() {
            let o = self.objects[i];
            if o.flags & (oflags::AIRFIELD | oflags::SHIP) != 0
                || !hostile(&o)
                || (o.x, o.y) == (0, 0)
            {
                continue;
            }
            for _ in 0..rng.random(war + 2) {
                around(&mut rng, &mut units, o.x, o.y, AaaKind::Manpads, 15, 80);
            }
        }
        units.truncate(MAX_UNITS);
        self.aaa = units;
        self.aaa_rng = Some(rng);
    }

    /// One frame of the air defences: aiming, firing, the shells and the MANPADS launches.
    /// Each unit goes for the nearest pilot in the world.
    pub(crate) fn update_aaa(&mut self, present: &[usize]) {
        let Some(mut rng) = self.aaa_rng else { return };
        let fs = self.fs.max(1);
        let dt = 1.0 / fs as f64;
        let tracks: Vec<Track> = present
            .iter()
            .map(|&k| {
                self.activate(k);
                let (me, vel) = self.player_metres();
                Track {
                    k,
                    me,
                    vel,
                    p: self.pl.p,
                    emv: self.pl.emv,
                    on_ground: self.pl.on_ground,
                    lockheed: self.pl.lockheed,
                }
            })
            .collect();
        if tracks.is_empty() {
            return;
        }
        let mut units = std::mem::take(&mut self.aaa);
        self.aaa_pilot.resize(units.len(), tracks[0].k as u8);
        let vis_km = self.visual_range();
        // Accuracy: one standard deviation (radians) with a radar track / laid by eye.
        let skill = 1.5 - 0.2 * self.diff as f64;
        let mut fired: Vec<Shell> = Vec::new();
        let mut manpads: Vec<usize> = Vec::new();
        let mut rwr: Vec<(usize, RwrTone)> = Vec::new();
        for (i, u) in units.iter_mut().enumerate() {
            // Only the classic guns fight the player (the friendly ones never do).
            if u.destroyed || self.aaa_blue(i) || u.kind.patriot() || u.kind == AaaKind::Gepard {
                continue;
            }
            u.cooldown = (u.cooldown - 1).max(0);
            let t = if tracks.len() == 1 {
                &tracks[0]
            } else {
                tracks
                    .iter()
                    .min_by_key(|t| approx(s16(t.p.mx - u.x), s16(t.p.my - u.y)))
                    .expect("a pilot")
            };
            self.aaa_pilot[i] = t.k as u8;
            let (me, vel, on_ground) = (t.me, t.vel, t.on_ground);
            let ctx = DetectCtx {
                px: t.p.mx,
                py: t.p.my,
                view_z: t.p.view_z,
                heading: t.p.heading,
                velocity: t.p.velocity,
                diff: self.diff,
                lockheed: t.lockheed,
                grid: &self.grid,
            };
            let from = u.metres();
            let d = sub(me, from);
            let dist = len(d);
            let (v0, reach, ceiling) = u.kind.gun();
            let range_km = approx(s16(t.p.mx - u.x), s16(t.p.my - u.y)) >> 6;
            let seen = range_km < vis_km
                && !on_ground
                && (!self.weather.any()
                    || self.weather.los_clear((u.x, u.y, 0), (t.p.mx, t.p.my, t.p.alt.max(0))));
            let radar = u.kind == AaaKind::Zsu && {
                let (score, _, _) = detect(u.x, u.y, 0, 1, Some(GUN_DISH), &ctx);
                score + t.emv > 100
            };
            if radar && !u.tracking && !rwr.iter().any(|r| r.0 == t.k) {
                rwr.push((t.k, RwrTone::Scan));
            }
            u.tracking = radar || seen;
            let engage =
                u.tracking && !on_ground && dist < reach && me[2] < ceiling && me[2] > 15.0;
            if !engage {
                u.burst = 0;
                continue;
            }
            let pilot = t.k as u8;
            let (az, el, t) = if u.kind == AaaKind::Manpads {
                (d[0].atan2(d[1]), d[2].atan2(d[0].hypot(d[1])), 0.0)
            } else {
                lead(from, me, vel, v0)
            };
            u.aim_az = az as f32;
            u.aim_el = el as f32;
            match u.kind {
                AaaKind::Manpads => {
                    if u.cooldown == 0 {
                        manpads.push(i);
                        u.cooldown = 60 * fs;
                    }
                }
                AaaKind::Zsu => {
                    if u.burst == 0 && u.cooldown == 0 {
                        u.burst = fs * 3 / 2 + rng.random(fs);
                    }
                    if u.burst > 0 {
                        u.burst -= 1;
                        if u.burst == 0 {
                            u.cooldown = fs * 2 + rng.random(fs * 2);
                        }
                        // One tracer (of the many rounds) every other frame.
                        if u.burst & 1 == 0 {
                            let sigma = if radar { 0.004 } else { 0.010 } * skill;
                            let a = az + Self::aaa_gauss(&mut rng) * sigma;
                            let e = el + Self::aaa_gauss(&mut rng) * sigma;
                            let muzzle = add(from, [0.0, 0.0, 1.5]);
                            fired.push(Shell {
                                pos: muzzle,
                                vel: mul(direction(a, e), v0),
                                ttl: (reach * 1.3 / v0 * fs as f64) as i32,
                                flak: false,
                                pilot,
                            });
                        }
                    }
                }
                AaaKind::S60 => {
                    if u.cooldown == 0 {
                        u.cooldown = fs + rng.random(fs);
                        let sigma = 0.008 * skill;
                        let a = az + Self::aaa_gauss(&mut rng) * sigma;
                        let e = el + Self::aaa_gauss(&mut rng) * sigma;
                        // The fuse is set for the predicted time, a little off.
                        let fuse = t * (1.0 + Self::aaa_gauss(&mut rng) * 0.03);
                        fired.push(Shell {
                            pos: add(from, [0.0, 0.0, 1.5]),
                            vel: mul(direction(a, e), v0),
                            ttl: ((fuse * fs as f64) as i32).max(1),
                            flak: true,
                            pilot,
                        });
                    }
                }
                AaaKind::PatriotLauncher | AaaKind::PatriotRadar | AaaKind::Gepard => {}
            }
        }
        self.aaa = units;
        for (k, tone) in rwr {
            self.activate(k);
            self.emit(SimEvent::Rwr(tone));
        }
        let room = MAX_SHELLS.saturating_sub(self.aaa_shells.len());
        fired.truncate(room);
        if self.record_shells {
            self.new_shells.extend_from_slice(&fired);
        }
        self.aaa_shells.extend(fired);
        for i in manpads {
            self.activate(self.aaa_pilot[i] as usize);
            self.launch_manpads(i);
        }

        // Shells: gravity, the pilot passing through a burst of tracers, timed bursts.
        let mut hits = vec![(0, 0); tracks.len()];
        let mut bursts = Vec::new();
        let mut shells = std::mem::take(&mut self.aaa_shells);
        shells.retain_mut(|s| {
            let n = tracks.iter().position(|t| t.k == s.pilot as usize);
            let (me, vel) = n.map_or(([f64::MAX; 3], [0.0; 3]), |n| (tracks[n].me, tracks[n].vel));
            let step = mul(s.vel, dt);
            let rel = sub(s.pos, me);
            let rel_step = sub(step, mul(vel, dt));
            s.pos = add(s.pos, step);
            s.vel[2] -= G * dt;
            s.ttl -= 1;
            if s.pos[2] < 0.0 {
                return false;
            }
            let near = if n.is_some() { closest(rel, rel_step) } else { f64::MAX };
            if !s.flak && near < ZSU_HIT_RADIUS && rng.random(1000) < ZSU_HIT_CHANCE {
                hits[n.unwrap_or(0)].0 += 1;
                return false;
            }
            if s.flak && (s.ttl <= 0 || near < FLAK_LETHAL * 0.5) {
                let d = len(sub(s.pos, me));
                let x = (s.pos[0] / M_PER_MAP) as i32;
                let y = 0x8000 - (s.pos[1] / M_PER_MAP) as i32;
                bursts.push((n.map_or(0, |n| tracks[n].k), x, y, (s.pos[2] / FT) as i32));
                if d < FLAK_LETHAL
                    || (d < FLAK_REACH
                        && rng.random(1000)
                            < ((FLAK_REACH - d) / (FLAK_REACH - FLAK_LETHAL) * 600.0) as i32)
                {
                    hits[n.unwrap_or(0)].1 += 1;
                }
                return false;
            }
            s.ttl > 0
        });
        self.aaa_shells = shells;
        self.aaa_rng = Some(rng);
        for (k, x, y, alt) in bursts {
            self.activate(k);
            self.emit(SimEvent::Flak { x, y, alt });
        }
        for (n, &(hits, flak_hits)) in hits.iter().enumerate() {
            self.activate(tracks[n].k);
            for (n, text) in [(hits, "Hit by AAA"), (flak_hits, "Hit by flak")] {
                if n > 0 && !self.training() {
                    self.msg(text);
                    self.player_hit(false);
                    self.emit(SimEvent::Explosion {
                        x: self.pl.p.mx,
                        y: self.pl.p.my,
                        alt: self.pl.p.view_z,
                        kind: -1,
                    });
                }
            }
        }
    }

    /// A MANPADS team fires an SA-14 if a missile slot is free (never one in flight).
    fn launch_manpads(&mut self, i: usize) {
        let u = self.aaa[i];
        if !self.may_launch(SA14, (u.x, u.y, 0)) {
            return;
        }
        let Some(k) = (0..8).find(|&k| self.proj[k].ttl == 0) else { return };
        let brg = bearing(s16(self.pl.p.mx - u.x), s16(u.y - self.pl.p.my));
        let lock = self.missile(SA14).lock_range as i32;
        self.proj[k] = Projectile {
            x: u.x,
            y: u.y,
            alt: 0,
            speed: 1,
            heading: s16(brg),
            pitch: 0x2000,
            ttl: s16(lock * self.fs * 16),
            spec: SA14,
            target: -1,
            launcher: MANPADS_LAUNCHER,
            ..Default::default()
        };
        if self.guided(SA14) {
            let tgt = self.player_target().p;
            self.launch_flight_at(k, (u.x, u.y), tgt);
        }
        self.msg("Missile launch");
        self.proj_pilot[k] = self.pl.id;
        self.pl.launches += 1;
        self.emit(SimEvent::MissileLaunched {
            slot: k,
            spec: SA14,
            launcher: Launcher::Manpads(i),
        });
    }

    /// Player bombs, missiles and gun rounds that went off on the ground this frame destroy the
    /// units nearby.
    pub(crate) fn aaa_damage(&mut self) {
        let fine = (32 * ROUND_FINE) as f64;
        let blasts: Vec<(f64, f64, f64, Option<u8>)> = self
            .events
            .iter()
            .zip(&self.event_credit)
            .filter_map(|(e, &by)| match *e {
                SimEvent::Explosion { x, y, alt, kind }
                    if alt < 60 && kind != -1 && kind != -16 =>
                {
                    Some((x as f64, y as f64, 4.0, by))
                }
                SimEvent::RoundImpact { x, y } => Some((x as f64 / fine, y as f64 / fine, 1.0, by)),
                _ => None,
            })
            .collect();
        if blasts.is_empty() {
            return;
        }
        let Some(mut rng) = self.aaa_rng else { return };
        for k in 0..self.aaa.len() {
            let u = self.aaa[k];
            if u.destroyed {
                continue;
            }
            for &(x, y, r, by) in &blasts {
                let d = (x - u.x as f64).hypot(y - u.y as f64);
                // A gun round kills only now and then.
                if d < r && (r > 1.0 || rng.random(6) == 0) {
                    self.aaa[k].destroyed = true;
                    self.aaa[k].burst = 0;
                    if let Some(p) = by {
                        self.activate(p as usize);
                    }
                    self.msg(format!("{} destroyed", u.kind.name()));
                    self.emit(SimEvent::Explosion { x: u.x, y: u.y, alt: 0, kind: 0x10 });
                    self.emit(SimEvent::AaaDestroyed { unit: k, by });
                    break;
                }
            }
        }
        self.aaa_rng = Some(rng);
    }

    /// ZSU-23-4 radars for the radar warning receiver (range km, bearing from the player).
    pub(crate) fn aaa_emitters(&self, out: &mut Vec<Emitter>) {
        let (px, py) = (self.pl.p.mx, self.pl.p.my);
        for (i, u) in self.aaa.iter().enumerate() {
            if u.kind != AaaKind::Zsu || u.destroyed || self.aaa_blue(i) {
                continue;
            }
            let range = approx(s16(px - u.x), s16(py - u.y)) >> 6;
            if range >= GUN_DISH.0 {
                continue;
            }
            let launch = u.burst > 0;
            out.push(Emitter {
                source: EmitterSource::Gun(i),
                x: u.x,
                y: u.y,
                alt: 0,
                radar_type: 0,
                range,
                bearing: bearing(s16(u.x - px), s16(py - u.y)),
                lock_range: 3,
                painted: u.tracking,
                tracking: u.tracking,
                launch,
                priority: 8
                    + if launch { 300 } else { 0 }
                    + if u.tracking { 120 } else { 0 }
                    + if range < 3 { 60 } else { 0 }
                    + (GUN_DISH.0 - range) / 4,
                signal: detect(u.x, u.y, 0, 1, Some(GUN_DISH), &self.detect_ctx()).0.max(0),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lead_meets_a_crossing_target() {
        // A target 2 km north at 300 m flying east at 200 m/s: the shell meets it.
        let from = [0.0, 0.0, 0.0];
        let to = [0.0, 2000.0, 300.0];
        let vel = [200.0, 0.0, 0.0];
        let (az, el, t) = lead(from, to, vel, 970.0);
        let mut p = from;
        let mut v = mul(direction(az, el), 970.0);
        let dt = 0.001;
        let mut best = f64::MAX;
        let mut tt = 0.0;
        while tt < t * 1.5 {
            p = add(p, mul(v, dt));
            v[2] -= G * dt;
            tt += dt;
            best = best.min(len(sub(p, add(to, mul(vel, tt)))));
        }
        assert!(best < 2.0, "missed by {best} m");
        assert!(az > 0.0 && t > 2.0 && t < 2.3);
    }

    #[test]
    fn closest_approach() {
        assert!((closest([-5.0, 3.0, 0.0], [10.0, 0.0, 0.0]) - 3.0).abs() < 1e-9);
        assert!((closest([5.0, 0.0, 0.0], [10.0, 0.0, 0.0]) - 5.0).abs() < 1e-9);
    }
}
