//! Helicopters and combat search and rescue (not in the original game).
//!
//! Helicopters lift off vertically, fly low, slow down to a hover at their goal and turn on
//! the spot. After a good ejection over enemy territory the mission goes on: a UH-60 comes
//! from the nearest friendly airfield for the pilot while Hinds search the area. The pilot's
//! signal flares guide the rescue in (and show the Hinds where to look). Rescued: the mission
//! ends like a parachute landing (landing type 4); found by a Hind, or too long on the ground:
//! captured (outcome 8).

use serde::{Deserialize, Serialize};

use crate::Sim;
use crate::math::{approx, bearing, cdiv, clamp_q, s16};
use crate::types::*;

/// Aircraft types (sim.json `aircraft`).
pub const HIND: i32 = 16;
pub const BLACKHAWK: i32 = 23;
/// Cruise height of helicopters (world units, about 150 m).
const CRUISE_ALT: i32 = 300;
/// Hover height over the pilot.
const HOVER_ALT: i32 = 30;
/// Within this many map units the rescue needs a signal flare to find the pilot (3 km).
const SEARCH_RANGE: i32 = 0xC0;
/// A Hind this close to the pilot (and low) captures him.
const CAPTURE_RANGE: i32 = 0x20;
/// A Hind passing this close may spot the pilot (1 km).
const SPOT_RANGE: i32 = 0x40;
/// A signal flare shows the pilot to Hinds this close (8 km).
const FLARE_SEEN_RANGE: i32 = 0x200;
/// Seconds the rescue hovers over the pilot for the pickup.
const PICKUP_SECONDS: i32 = 10;
/// Signal flares the pilot carries.
const SIGNAL_FLARES: i32 = 3;
/// The rescue helicopter is on alert this far from the pilot at most (map units, 40 km): CSAR
/// helicopters wait close to the front, not at the main bases.
const RESCUE_RANGE: i32 = 0xA00;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum CsarState {
    /// The rescue is on its way.
    #[default]
    Inbound,
    /// Hovering over the pilot (frames so far).
    Pickup(i32),
    /// Rescued.
    Rescued,
    Captured,
}

/// The rescue of a downed pilot.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Csar {
    /// Pilot on the ground (map units).
    pub pilot: (i32, i32),
    /// Flight unit of the UH-60, and of the Hinds searching for the pilot.
    pub rescue: usize,
    pub hunters: Vec<usize>,
    pub state: CsarState,
    /// Frames on the ground.
    pub frames: i32,
    /// Captured when the pilot has been on the ground this many frames.
    pub capture_frames: i32,
    pub flares: i32,
    /// Frames since the last signal flare (None: none fired yet).
    pub since_flare: Option<i32>,
    /// The rescue crew saw a flare close by and knows where the pilot is.
    pub located: bool,
    /// Where each Hind searches (map units), changed now and then.
    pub search: Vec<(i32, i32)>,
    /// The Hinds that have spotted the pilot (they go down for him).
    #[serde(default)]
    pub spotted: Vec<bool>,
}

impl Sim {
    /// The rescue after an ejection over enemy territory (None otherwise).
    pub fn csar(&self) -> Option<&Csar> {
        self.csar.as_ref()
    }

    /// Spawns a helicopter on the ground at base object `b` (appended after the other flights).
    fn spawn_helicopter(&mut self, spec: i32, b: usize) -> usize {
        let o = self.objects[b];
        let i = self.flights.len();
        let mut u = FlightUnit {
            spec,
            wp: b as i32,
            x: o.x + 6,
            y: o.y,
            alt: 0,
            speed: 1,
            timer: 0x7FFF,
            ..Default::default()
        };
        u.wx = u.x << 5;
        u.wy = u.y << 5;
        u.flags = fflags::EXISTS | fflags::AIRBORNE | fflags::TAKEOFF;
        self.flights.push(u);
        self.emit(SimEvent::AircraftTakeoff { unit: i, base: b });
        i
    }

    /// Nearest airfield (not disabled) to a point, friendly or enemy.
    fn nearest_airfield(&self, x: i32, y: i32, friendly: bool) -> Option<usize> {
        self.objects
            .iter()
            .enumerate()
            .skip(3)
            .filter(|(_, o)| {
                o.flags & oflags::AIRFIELD != 0
                    && o.flags & (oflags::DISABLED | oflags::NEUTRAL | oflags::CARRIER) == 0
                    && (o.flags & oflags::FRIENDLY != 0) == friendly
            })
            .min_by_key(|(_, o)| approx(o.x - x, o.y - y))
            .map(|(i, _)| i)
    }

    /// The chute is down: over enemy territory the rescue starts (true), else the mission ends
    /// as before.
    pub(crate) fn start_csar(&mut self) -> bool {
        let (x, y, _) = self.chute;
        let cell = ((y >> 11).clamp(0, 15) * 16 + (x >> 11).clamp(0, 15)) as usize;
        let enemy = self.grid.get(cell).is_some_and(|g| g & 3 == 0);
        let Some(base) = self.nearest_airfield(x, y, true) else { return false };
        if !enemy {
            return false;
        }
        let rescue = self.spawn_helicopter(BLACKHAWK, base);
        // Already airborne, at most RESCUE_RANGE out on the way from its base.
        let b = self.objects[base];
        let d = approx(b.x - x, b.y - y).max(1);
        if d > RESCUE_RANGE {
            let u = &mut self.flights[rescue];
            u.x = x + (b.x - x) * RESCUE_RANGE / d;
            u.y = y + (b.y - y) * RESCUE_RANGE / d;
            u.wx = u.x << 5;
            u.wy = u.y << 5;
            u.alt = CRUISE_ALT;
            u.flags &= !fflags::TAKEOFF;
        }
        let mut hunters = Vec::new();
        let n = 1 + self.diff / 2 + (self.roe == 2) as i32;
        if let Some(b) = self.nearest_airfield(x, y, false) {
            for _ in 0..n {
                hunters.push(self.spawn_helicopter(HIND, b));
            }
        }
        // 15 minutes against green opponents, 9 against elite ones.
        let capture_frames = (15 - 2 * self.diff) * 60 * self.fs;
        let search = hunters.iter().map(|_| (x, y)).collect();
        let spotted = vec![false; hunters.len()];
        self.csar = Some(Csar {
            pilot: (x, y),
            rescue,
            hunters,
            capture_frames,
            flares: SIGNAL_FLARES,
            search,
            spotted,
            ..Default::default()
        });
        let base_name = self.place_name(base);
        self.msg(format!("Down in enemy territory - CSAR from {base_name}"));
        self.emit(SimEvent::CancelTimeAccel);
        true
    }

    /// One frame of the rescue: signal flares, the pickup, capture.
    pub(crate) fn update_csar(&mut self, ps: &PlayerState) {
        let fs = self.fs;
        let Some(mut c) = self.csar.take() else { return };
        if matches!(c.state, CsarState::Rescued | CsarState::Captured) {
            self.csar = Some(c);
            return;
        }
        c.frames += 1;
        c.since_flare = c.since_flare.map(|f| f + 1);
        if ps.cmd.flare {
            if c.flares > 0 {
                c.flares -= 1;
                c.since_flare = Some(0);
                self.msg(format!("Signal flare - {} left", c.flares));
                self.sound(0x1C, 1);
                self.emit(SimEvent::SignalFlare { x: c.pilot.0, y: c.pilot.1 });
            } else {
                self.msg("No signal flares left");
            }
        }
        // Hinds search around the pilot, closer as time goes on; a flare shows them the spot.
        for (k, &h) in c.hunters.iter().enumerate() {
            let u = self.flights[h];
            if u.flags & fflags::DESTROYED != 0 {
                continue;
            }
            let near = approx(u.x - c.pilot.0, u.y - c.pilot.1);
            // Spotted: from a recent flare, or by chance when passing close (better crews
            // spot more; at night half as often).
            if !c.spotted[k] {
                let flare = c.since_flare.is_some_and(|f| f < 30 * fs) && near < FLARE_SEEN_RANGE;
                let chance = near < SPOT_RANGE
                    && (self.frame + k as i32) % (2 * fs) == 0
                    && self.rng.random(20 << self.night as i32) <= self.diff;
                if flare || chance {
                    c.spotted[k] = true;
                    self.msg("A Hind has spotted you");
                }
            }
            if c.spotted[k] {
                c.search[k] = c.pilot;
            } else if (self.frame + k as i32 * 97) % (20 * fs) == 0 {
                let spread = (0x180 - c.frames / (4 * fs)).max(0x30);
                c.search[k] = (
                    c.pilot.0 + self.rng.random(2 * spread) - spread,
                    c.pilot.1 + self.rng.random(2 * spread) - spread,
                );
            }
            if c.spotted[k] && near < CAPTURE_RANGE && u.alt < 100 {
                c.state = CsarState::Captured;
            }
        }
        if c.frames > c.capture_frames {
            c.state = CsarState::Captured;
        }
        // The pickup.
        let r = self.flights[c.rescue];
        let d = approx(r.x - c.pilot.0, r.y - c.pilot.1);
        if d < 2 * SEARCH_RANGE && c.since_flare.is_some_and(|f| f < 60 * fs) {
            c.located = true;
        }
        if r.flags & fflags::DESTROYED != 0 && c.state != CsarState::Captured {
            self.msg("Rescue helicopter lost");
            c.state = CsarState::Captured;
        }
        match c.state {
            CsarState::Inbound if d < 0x10 && r.alt <= HOVER_ALT + 10 => {
                c.state = CsarState::Pickup(0)
            }
            CsarState::Pickup(n) if n >= PICKUP_SECONDS * fs => c.state = CsarState::Rescued,
            CsarState::Pickup(n) => c.state = CsarState::Pickup(n + 1),
            _ => {}
        }
        let state = c.state;
        self.csar = Some(c);
        match state {
            CsarState::Rescued => {
                self.msg("Rescued!");
                self.landing_type = 4;
                self.finalize(0);
            }
            CsarState::Captured => {
                self.msg("Captured");
                self.finalize(8);
            }
            _ => {}
        }
    }

    /// Where helicopter `i` is going: (map x, map y, height, hover there).
    fn helo_goal(&mut self, i: usize, u: &FlightUnit) -> (i32, i32, i32, bool) {
        if let Some(c) = &self.csar {
            if i == c.rescue {
                let d = approx(u.x - c.pilot.0, u.y - c.pilot.1);
                // Close in, it needs a flare to find the pilot; until then it circles (and comes in
                // anyway in the last minute).
                if d < SEARCH_RANGE && !c.located && c.frames < c.capture_frames - 60 * self.fs {
                    let a = s16(self.frame * 40 + 0x4000);
                    let (ox, oy) = (
                        self.trig.sin_mul(a, SEARCH_RANGE / 2),
                        self.trig.cos_mul(a, SEARCH_RANGE / 2),
                    );
                    return (c.pilot.0 + ox, c.pilot.1 + oy, CRUISE_ALT / 2, false);
                }
                return (
                    c.pilot.0,
                    c.pilot.1,
                    if d < 0x20 { HOVER_ALT } else { CRUISE_ALT / 2 },
                    true,
                );
            }
            if let Some(k) = c.hunters.iter().position(|&h| h == i) {
                let (x, y) = c.search[k];
                return if c.spotted[k] {
                    (x, y, HOVER_ALT, true)
                } else {
                    (x, y, CRUISE_ALT, false)
                };
            }
        }
        // Other helicopters (Hinds of the mission): patrol low between objects, or hunt the
        // player while the alarm is up and he is low.
        if u.flags & fflags::RTB != 0 {
            let b = self.obj(u.wp);
            return (b.x, b.y, CRUISE_ALT, true);
        }
        if self.alarm_timer != 0 && self.p.view_z < 3000 {
            return (self.ref_x, self.ref_y, CRUISE_ALT, false);
        }
        let o = self.obj(u.wp);
        if approx(o.x - u.x, o.y - u.y) < 0x40 && self.woc > 3 {
            let w = 3 + self.rng.random(self.woc as i32 - 3);
            if self.obj(w).flags & oflags::FRIENDLY == 0 {
                self.flights[i].wp = w;
            }
        }
        (o.x, o.y, CRUISE_ALT, false)
    }

    pub(crate) fn fly_helo(&mut self, i: usize) {
        let fs = self.fs;
        let t = self.ac_type(self.flights[i].spec).clone();
        let max = t.max_speed as i32;
        let u0 = self.flights[i];
        let (gx, gy, galt, hover) = self.helo_goal(i, &u0);
        let mut u = self.flights[i];
        let (dx, dy) = (gx - u.x, gy - u.y);
        let d = approx(dx, dy);
        // Lift off straight up, then fly.
        if u.flags & fflags::TAKEOFF != 0 {
            u.alt += cdiv(40, fs).max(1);
            if u.alt >= 120 {
                u.flags &= !fflags::TAKEOFF;
            }
        } else {
            let want = if !hover {
                max
            } else if d < 4 {
                0
            } else {
                (max * d / 0x80).clamp(20, max)
            };
            let acc = cdiv(max, 6 * fs).max(1);
            // Never 0 in the air: a stopped aircraft counts as parked.
            u.speed = (u.speed + clamp_q(want - u.speed, -acc, acc)).max(1);
            let brg = bearing(dx, s16(-dy));
            let turn = clamp_q(s16(brg - u.heading), -cdiv(0x2000, fs), cdiv(0x2000, fs));
            if d > 4 {
                u.heading = s16(u.heading + turn);
            }
            let climb = cdiv(30, fs).max(1);
            u.alt += clamp_q(galt - u.alt, -climb, climb);
        }
        // Nose down with speed, banked into turns (attitude only).
        u.pitch = -(u.speed * 0x600 / max.max(1));
        u.bank = clamp_q(s16(bearing(dx, s16(-dy)) - u.heading) * 2, -0x1800, 0x1800)
            * (u.speed > 20) as i32;
        let mv = cdiv(u.speed, fs);
        u.wx = u.wx.wrapping_add(self.trig.sin_mul(u.heading, mv));
        u.wy = u.wy.wrapping_sub(self.trig.cos_mul(u.heading, mv));
        u.x = (u.wx >> 5) & 0xFFFF;
        u.y = (u.wy >> 5) & 0xFFFF;
        // Landed back at base.
        if hover && u.flags & fflags::RTB != 0 && d < 0x10 {
            u.alt = (u.alt - cdiv(30, fs).max(1)).max(0);
            if u.alt == 0 {
                u.flags &= 0x1C1;
                u.speed = 0;
            }
        }
        // Hinds use their gun on a low, close player.
        let (px, py, pz) = (self.p.mx, self.p.my, self.p.view_z);
        let prange = approx(px - u.x, py - u.y);
        if u.spec == HIND
            && prange < 0x40
            && (pz - u.alt).abs() < 600
            && self.eject == 0
            && self.frame & 3 == 0
        {
            let to_player = bearing(px - u.x, s16(u.y - py));
            if s16(to_player - u.heading).abs() < 0x1000 {
                self.fire_gun(&u);
            }
        }
        u.timer = s16(u.timer - 1);
        if u.timer == 0
            && self.csar.as_ref().is_none_or(|c| c.rescue != i && !c.hunters.contains(&i))
        {
            u.flags |= fflags::RTB;
        }
        self.flights[i] = u;
        let per = (fs << 4).max(1);
        if t.radar > 0
            && self.frame % per == (i as i32 & 7) * 2 * fs
            && u.flags & fflags::DESTROYED == 0
        {
            self.radar_sweep(i);
        }
    }
}
