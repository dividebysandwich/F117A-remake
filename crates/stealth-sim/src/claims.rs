//! Coop gun rounds flown on the client: what a player sees hit is a hit.
//!
//! A client's mirror flies the player's own rounds from his trigger ([`Sim::fly_own_rounds`])
//! against the world as it shows it (the host's frame it is at) and reports the hits
//! ([`GunHit`]). The host checks each report ([`Sim::claim_hit`], applied in the next step):
//! the pilot fired lately, the unit was where the report says at that frame (the host keeps
//! the flight units' positions of the last second: lag compensation), and within the
//! cannon's reach of him; then it is his kill as if the host's rounds had hit. Another
//! pilot's aircraft hit is checked the same way (the host keeps the pilots' positions too).

use serde::{Deserialize, Serialize};

use crate::Sim;
use crate::types::*;

/// Steps of flight unit positions a host keeps (a second and a half at 15 fps).
const HISTORY: usize = 24;
/// A report counts if the pilot fired within this many seconds before it.
const FIRED_WITHIN: f64 = 2.5;
/// Reports a pilot may make in one step.
const CLAIMS_PER_STEP: u8 = 4;
/// The cannon's reach (metres): the rounds' life and some way for the target.
const REACH: f64 = 4000.0;
/// A reported round may be this many hit radii from where the unit was (it hit within one).
const SLACK: f64 = 1.5;
/// A unit is reported hit once in this many steps.
const CLAIM_AGAIN: u32 = 15;

/// The pilots' positions at a step (id, world units like the rounds').
pub(crate) type PilotsAt = Vec<(u8, (i32, i32, i32))>;

/// A client's report of one of his rounds hitting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GunHit {
    /// Flight unit `unit` at the host's frame `step` (the one shown), the round at `at`
    /// (world units like the units' wx, wy and alt).
    Aircraft { unit: u16, step: u32, at: (i32, i32, i32) },
    /// Pilot `id`'s aircraft, like [`GunHit::Aircraft`].
    Pilot { id: u8, step: u32, at: (i32, i32, i32) },
    /// The round hit the ground at (x, y) (fine units, like [`SimEvent::RoundImpact`]).
    Ground { x: i32, y: i32 },
}

impl Sim {
    /// The host's frame a mirror shows (the frame it last applied).
    pub fn set_host_step(&mut self, step: u32) {
        self.steps = step;
    }

    /// Steps run (a host's), the host's frame shown (a mirror's).
    pub fn step_count(&self) -> u32 {
        self.steps
    }

    /// Pilot `id` flies his gun rounds on his client, which reports their hits.
    pub fn set_client_rounds(&mut self, id: u8, on: bool) {
        let k = id as usize;
        if k == self.pl.id as usize {
            self.pl.client_rounds = on;
        } else if let Some(p) = self.parked.get_mut(k).filter(|p| p.id == id) {
            p.client_rounds = on;
        }
    }

    /// Pilot `id`'s report of a hit, for the next step.
    pub fn claim_hit(&mut self, id: u8, hit: GunHit) {
        if self.pending_claims.len() < 4 * crate::MAX_PILOTS {
            self.pending_claims.push((id, hit));
        }
    }

    /// The flight units' and pilots' positions of this step, while some pilot reports his own
    /// hits.
    pub(crate) fn record_flights(&mut self) {
        if !self.present().iter().any(|&k| self.pilot_ref(k).client_rounds) {
            self.flight_history.clear();
            self.pilot_history.clear();
            return;
        }
        let now = self.flights.iter().map(|u| (u.wx, u.wy, u.alt)).collect();
        self.flight_history.push_back((self.steps, now));
        while self.flight_history.len() > HISTORY {
            self.flight_history.pop_front();
        }
        let pilots = self
            .present()
            .into_iter()
            .map(|k| (k as u8, Self::round_point(&self.pilot_ref(k).p)))
            .collect();
        self.pilot_history.push_back((self.steps, pilots));
        while self.pilot_history.len() > HISTORY {
            self.pilot_history.pop_front();
        }
    }

    /// Where pilot `id` was at step `step`, like [`Self::flight_at`].
    fn pilot_at(&self, id: u8, step: u32) -> Option<(i32, i32, i32)> {
        let newest = self.pilot_history.back().map_or(self.steps, |&(s, _)| s);
        if step.wrapping_sub(newest) as i32 > 0 {
            return Some(Self::round_point(&self.pilot_ref(id as usize).p));
        }
        let (_, at) = self.pilot_history.iter().find(|(s, _)| *s == step)?;
        at.iter().find(|(k, _)| *k == id).map(|&(_, p)| p)
    }

    /// Where flight unit `j` was at step `step`; None when that is too long ago. A step not
    /// recorded yet (the report is ahead) is now.
    fn flight_at(&self, j: usize, step: u32) -> Option<(i32, i32, i32)> {
        let newest = self.flight_history.back().map_or(self.steps, |&(s, _)| s);
        if step.wrapping_sub(newest) as i32 > 0 {
            return self.flights.get(j).map(|u| (u.wx, u.wy, u.alt));
        }
        let (_, at) = self.flight_history.iter().find(|(s, _)| *s == step)?;
        at.get(j).copied()
    }

    /// The reports of this step, checked, as the pilots' hits.
    pub(crate) fn apply_claims(&mut self, present: &[usize]) {
        let claims = std::mem::take(&mut self.pending_claims);
        let fs = self.fs.max(1) as f64;
        for (id, hit) in claims {
            let k = id as usize;
            if !present.contains(&k) {
                continue;
            }
            self.activate(k);
            let pl = &self.pl;
            let fired = self.steps.wrapping_sub(pl.fired_step) as f64 <= FIRED_WITHIN * fs;
            if !pl.client_rounds || pl.lockheed || pl.fired_step == 0 || !fired {
                continue;
            }
            let (s, n) = pl.claims;
            let n = if s == self.steps { n } else { 0 };
            if n >= CLAIMS_PER_STEP {
                continue;
            }
            self.pl.claims = (self.steps, n + 1);
            // The pilot as the rounds' positions count (fine units over ROUND_FINE).
            let me = (self.pl.p.wx, 0x100000 - self.pl.p.wy, self.pl.p.view_z);
            let reach = REACH / WU_METRES;
            let far = |(x, y, z): (i32, i32, i32)| {
                let d = [(x - me.0) as f64, (y - me.1) as f64, (z - me.2) as f64];
                d.iter().map(|v| v * v).sum::<f64>() > reach * reach
            };
            self.credit = Some(id);
            match hit {
                GunHit::Aircraft { unit, step, at } => {
                    let j = unit as usize;
                    let Some(u) = self.flights.get(j).copied() else { continue };
                    if u.flags & fflags::AIRBORNE == 0 || u.flags & fflags::DESTROYED != 0 {
                        continue;
                    }
                    let Some(was) = self.flight_at(j, step) else { continue };
                    let r = self.gun_hit_radius() * SLACK;
                    let d = [(at.0 - was.0) as f64, (at.1 - was.1) as f64, (at.2 - was.2) as f64];
                    if d.iter().map(|v| v * v).sum::<f64>() > r * r || far(was) {
                        continue;
                    }
                    self.gun_kill(j);
                }
                GunHit::Pilot { id: victim, step, at } => {
                    if !self.pilot_airborne(victim) {
                        continue;
                    }
                    let Some(was) = self.pilot_at(victim, step) else { continue };
                    let r = self.gun_hit_radius() * SLACK;
                    let d = [(at.0 - was.0) as f64, (at.1 - was.1) as f64, (at.2 - was.2) as f64];
                    if d.iter().map(|v| v * v).sum::<f64>() > r * r || far(was) {
                        continue;
                    }
                    self.gun_hit_pilot(victim, was);
                }
                GunHit::Ground { x, y } => {
                    let f = ROUND_FINE;
                    if far((x / f, y / f, 0)) {
                        continue;
                    }
                    self.round_impact(x, y, (x / (32 * f), y / (32 * f)));
                }
            }
            self.credit = None;
        }
        self.credit = None;
    }

    /// A mirror's: the player's own gun rounds for one of his flight model's frames, from his
    /// trigger (`ps`), against the world as shown; the hits to report.
    pub fn fly_own_rounds(&mut self, ps: &PlayerState) -> Vec<GunHit> {
        let fs = self.fs.max(1);
        let nb = self.n_bullets.min(self.bullets.len());
        if nb == 0 {
            return Vec::new();
        }
        let trigger = ps.cmd.gun || (self.pl.gun_selected && ps.cmd.trigger);
        let slot = self.local_round % nb;
        self.local_round = (self.local_round + 1) % nb;
        let fire = self.has_gun() && trigger && self.pl.gun_ammo > 0 && self.pl.eject == 0;
        self.bullets[slot] = if fire {
            Self::gun_round(&self.pl.p, self.turret_aim(ps), fs)
        } else {
            Bullet::default()
        };
        let g = Self::gun_round_gravity(fs);
        for b in self.bullets[..nb].iter_mut().filter(|b| b.x != 0) {
            b.x += b.vx;
            b.y += b.vy;
            b.alt += b.vz;
            b.vz -= g;
        }
        let step = self.steps;
        self.claimed.retain(|&(_, s)| step.wrapping_sub(s) < CLAIM_AGAIN);
        self.claimed_pilots.retain(|&(_, s)| step.wrapping_sub(s) < CLAIM_AGAIN);
        let mut out = Vec::new();
        let f = ROUND_FINE;
        for i in 0..nb {
            let b = self.bullets[i];
            if b.x == 0 {
                continue;
            }
            for j in self.round_hits(&b) {
                self.bullets[i].x = 0;
                if self.claimed.iter().any(|&(u, _)| u as usize == j) {
                    continue;
                }
                self.claimed.push((j as u16, step));
                out.push(GunHit::Aircraft {
                    unit: j as u16,
                    step,
                    at: (b.x / f, b.y / f, b.alt / f),
                });
            }
            if self.bullets[i].x != 0
                && let Some(k) = self.round_hits_pilots(&b).first().copied()
            {
                self.bullets[i].x = 0;
                if !self.claimed_pilots.iter().any(|&(p, _)| p == k) {
                    self.claimed_pilots.push((k, step));
                    out.push(GunHit::Pilot { id: k, step, at: (b.x / f, b.y / f, b.alt / f) });
                }
            }
            if let Some((x, y)) = self.bullets[i].ground_crossing() {
                self.bullets[i].x = 0;
                out.push(GunHit::Ground { x, y });
            }
        }
        out
    }
}
