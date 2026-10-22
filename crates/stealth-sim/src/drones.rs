//! Attack drones (the remake's Ukraine theater): they fly straight at their target with a
//! gentle weave, never fight, and dive into it. The drone defense objective counts the kills
//! and the drones that got through.

use serde::{Deserialize, Serialize};

use crate::Sim;
use crate::math::{approx, bearing, cdiv, clamp_q, s16};
use crate::types::*;

/// Map units from the target where a drone starts its terminal dive.
const DIVE_RANGE: i32 = 0x60;
/// Map units from the target that count as a hit.
const IMPACT_RANGE: i32 = 0x18;

/// The drones of a mission and how they fared.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Drones {
    /// Drone defense (the primary objective); otherwise drones crossing a strike mission.
    pub defend: bool,
    /// Object they fly at.
    pub target: i32,
    pub nuclear: bool,
    /// Their flight units.
    pub first: usize,
    pub count: usize,
    /// Drones that may get through before the defense fails.
    pub allowance: i32,
    /// Shot down by the player.
    pub killed: i32,
    /// Reached the target.
    pub leaked: i32,
    /// A drone hit the nuclear plant.
    pub nuclear_hit: bool,
    /// Every drone is down or through (the objective was decided).
    pub resolved: bool,
}

impl Drones {
    pub fn contains(&self, unit: usize) -> bool {
        (self.first..self.first + self.count).contains(&unit)
    }
}

impl Sim {
    /// The mission's drones (None without any).
    pub fn drones(&self) -> Option<&Drones> {
        self.drones.as_ref()
    }

    pub(crate) fn fly_drone(&mut self, i: usize) {
        let fs = self.fs;
        let mut u = self.flights[i];
        let t = self.obj(u.wp);
        let (dx, dy) = (t.x - u.x, t.y - u.y);
        let d = approx(dx, dy);
        let brg = bearing(dx, s16(-dy));
        // A slow weave of up to ~10 degrees either side, out of step between drones.
        let weave = if d > 4 * DIVE_RANGE {
            self.trig.sin_mul(s16(self.frame * 24 + i as i32 * 0x2345), 0x700)
        } else {
            0
        };
        let turn = clamp_q(s16(brg + weave - u.heading), -0x2000, 0x2000);
        u.bank = clamp_q(turn * 2, -0x1800, 0x1800);
        u.heading = s16(u.heading + clamp_q(cdiv(turn, 2 * fs), -0x100, 0x100));
        // Level cruise, then the terminal dive.
        u.pitch =
            if d < DIVE_RANGE { clamp_q(-bearing(u.alt >> 6, d.max(1)), -0x2800, 0) } else { 0 };
        let mv = cdiv(((0x8000 - u.pitch) & 0xFFFF) * u.speed >> 15, fs);
        let h = self.trig.cos_mul(u.pitch, mv);
        u.wx = u.wx.wrapping_add(self.trig.sin_mul(u.heading, h));
        u.wy = u.wy.wrapping_sub(self.trig.cos_mul(u.heading, h));
        u.alt += self.trig.sin_mul(u.pitch, mv);
        u.x = (u.wx >> 5) & 0xFFFF;
        u.y = (u.wy >> 5) & 0xFFFF;
        self.flights[i] = u;
        if d < IMPACT_RANGE || u.alt <= 0 {
            self.drone_impact(i, d < IMPACT_RANGE);
        }
    }

    /// A drone reached its target (`on_target`), or the ground short of it.
    fn drone_impact(&mut self, i: usize, on_target: bool) {
        let u = self.flights[i];
        self.flights[i].flags = 0;
        // A campaign drone destroys its target and is gone (nobody's kill).
        if let Some(Some(f)) = self.campaign.as_mut().and_then(|c| c.flights.get_mut(i))
            && f.kamikaze
        {
            let target = f.strike.take().filter(|_| on_target);
            f.killed_by_ai = true;
            self.flights[i].flags = fflags::EXISTS | fflags::DESTROYED;
            if let Some(t) = target {
                self.remove_object(t as usize);
            }
        }
        if i as i32 == self.pl.air_lock {
            self.pl.air_lock = -1;
        }
        let t = self.obj(u.wp);
        self.flash = Flash { x: t.x, y: t.y, alt: 0, timer: -6 };
        self.emit(SimEvent::Explosion { x: u.x, y: u.y, alt: 0, kind: -6 });
        self.emit(SimEvent::DroneImpact { unit: i, x: u.x, y: u.y });
        let Some(mut w) = self.drones.take() else { return };
        if w.contains(i) {
            w.leaked += 1;
            let place = self.place_name(u.wp.max(0) as usize);
            if w.nuclear {
                w.nuclear_hit = true;
                self.msg(format!("Drone hit {place} NPP"));
            } else {
                self.msg(format!("Drone hit {place}"));
            }
            self.log_event(0x0D, u.spec);
        }
        self.drones = Some(w);
        self.resolve_drones();
    }

    /// A drone was shot down.
    pub(crate) fn drone_killed(&mut self, unit: usize) {
        let Some(w) = self.drones.as_mut() else { return };
        if w.contains(unit) {
            w.killed += 1;
            self.resolve_drones();
        }
    }

    /// Decides the drone defense once every drone is down or through.
    fn resolve_drones(&mut self) {
        let Some(w) = self.drones.as_mut() else { return };
        if w.resolved || (w.killed + w.leaked) < w.count as i32 {
            return;
        }
        w.resolved = true;
        let (defend, ok, leaked) = (w.defend, w.leaked <= w.allowance && !w.nuclear_hit, w.leaked);
        if !defend {
            return;
        }
        if ok {
            self.mark_target_reached(0);
        } else {
            self.msg(format!("Defense failed - {leaked} drones through"));
        }
    }
}
