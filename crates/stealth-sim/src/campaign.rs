//! Realtime dynamic campaign (`SimOptions::campaign`, not in the original). One simulation
//! runs for the whole session:
//!
//! * The player comes and goes: between sorties he is absent (nothing sees him, nothing
//!   happens to him) and a sortie respawns him on a friendly runway. A sortie's end (crash,
//!   ejection, landing) is a [`SimEvent::SortieEnded`], never the end of the simulation.
//! * Places change hands ([`Sim::set_side`]); destroyed objects are repaired
//!   ([`Sim::restore_object`]).
//! * The campaign launches aircraft ([`Sim::spawn_insertion`], [`Sim::spawn_fighter`]),
//!   tagged with its own ids; their losses and landings come back as events.
//! * The AI fights the AI: enemy SAM sites, guns and fighters engage the friendly helicopters,
//!   friendly guns and launchers the enemy aircraft, with missiles and tracers of their own
//!   (the original's projectile slots stay the player's business).
//! * Recon intelligence ([`Sim::set_intel`]) shortens a site's detection range and lets the
//!   player spot the object from twice as far.
//!
//! Everything lives in [`CampaignSim`] and draws from its own random stream, so the original's
//! behaviour is untouched while the option is off.

use serde::{Deserialize, Serialize};

use crate::Sim;
use crate::aaa::{AaaKind, AaaUnit, M_PER_MAP, SA14, Shell, lead};
use crate::csar::BLACKHAWK;
use crate::math::{approx, s16};
use crate::rng::Rng;
use crate::types::*;

const M_PER_WU: f64 = M_PER_MAP / 32.0;
const G: f64 = 9.81;
/// Detection range factor of a site with recon intelligence against it (tenths).
pub const INTEL_DETECTION: i32 = 7;
/// Seconds between a site's launches at AI aircraft, and a fighter's.
const SITE_RELOAD: i32 = 25;
const FIGHTER_RELOAD: i32 = 12;
const FIGHTER_MISSILES: i32 = 4;
/// A fighter goes for enemy aircraft this close (80 km, its radar).
const FIGHTER_SEARCH: i32 = 0x1400;
/// A missile's fuse (m) and the chance (per mille) it kills what it reaches.
const FUSE: f64 = 25.0;
const MISSILE_PK: i32 = 700;
/// A tracer this close to an aircraft hits it, with this chance (per mille).
const TRACER_RADIUS: f64 = 10.0;
const TRACER_HIT: i32 = 300;
const MAX_SHELLS: usize = 200;
/// Low-level air defences in a campaign at most.
const MAX_UNITS: usize = 400;

/// What a campaign aircraft is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    /// Helicopter with troops, landing at its goal object.
    Insertion,
    /// Fighter: patrols around its goal, goes for the other side's aircraft.
    Fighter,
}

/// An aircraft the campaign launched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CampaignFlight {
    pub tag: u32,
    pub role: Role,
    pub blue: bool,
    /// Patrol point (map units).
    pub goal: (i32, i32),
    /// Flight unit it is going for.
    pub target: Option<usize>,
    pub cooldown: i32,
    pub missiles: i32,
    /// Shot down by the AI (else by the player), and reported.
    pub killed_by_ai: bool,
    pub reported: bool,
}

/// A missile of the AI against an AI aircraft (metres, x east, y north, z up).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AiMissile {
    pub pos: [f64; 3],
    pub vel: [f64; 3],
    pub spec: i32,
    /// Flight unit it chases.
    pub target: usize,
    /// Speed (m/s) and turn rate (rad/s).
    pub speed: f64,
    pub turn: f64,
    pub ttl: i32,
    pub blue: bool,
}

/// A tracer of a gun firing at an AI aircraft.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AiShell {
    pub shell: Shell,
    pub target: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CampaignSim {
    /// The player is not in the world (between sorties).
    pub absent: bool,
    pub(crate) rng: Rng,
    /// Each object's radar type and shape as built (for repairs).
    pub(crate) original: Vec<(i32, i32)>,
    pub(crate) intel: Vec<bool>,
    /// Per flight unit: the campaign's aircraft.
    pub(crate) flights: Vec<Option<CampaignFlight>>,
    /// Per object: frames until a site may launch at an AI aircraft again.
    pub(crate) site_cool: Vec<i32>,
    /// Per AAA unit: friendly, and the flight unit it engages.
    pub(crate) aaa_blue: Vec<bool>,
    pub(crate) aaa_target: Vec<Option<usize>>,
    pub(crate) missiles: Vec<AiMissile>,
    pub(crate) shells: Vec<AiShell>,
    /// Countermeasure and gun stores at the start (a respawn reloads them).
    pub(crate) stores_init: [i32; 4],
    pub(crate) gun_init: i32,
}

/// Metres (east, north, up) of a flight unit.
fn flight_metres(u: &FlightUnit) -> [f64; 3] {
    [u.wx as f64 * M_PER_WU, (0x10_0000 - u.wy) as f64 * M_PER_WU, u.alt.max(0) as f64 * M_PER_WU]
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn mul(a: [f64; 3], k: f64) -> [f64; 3] {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn len(a: [f64; 3]) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

/// Radar horizon (map units) of a ground radar against a target at `alt_m` metres.
fn horizon(alt_m: f64) -> i32 {
    (4120.0 * (alt_m.max(0.0).sqrt() + 3.2) / M_PER_MAP) as i32
}

impl Sim {
    pub(crate) fn init_campaign(&mut self, stores: [i32; 4], gun: i32) {
        let n = self.objects.len();
        // The mission's own aircraft are not used: the campaign launches its own.
        for (i, u) in self.flights.iter_mut().enumerate() {
            if i + 4 < self.mission_flights || i >= self.mission_flights {
                *u = FlightUnit::default();
            }
        }
        self.drones = None;
        self.deadline = 0;
        let base = self.targets[0].base;
        self.targets = [TargetSlot { mission_type: 0, object: -1, base, code: 0 }; 2];
        let seed = self.rng.hold ^ 0xCA4D_A1A1;
        self.campaign = Some(Box::new(CampaignSim {
            absent: true,
            rng: Rng::new(seed),
            original: self.objects.iter().map(|o| (o.unit_type, o.shape)).collect(),
            intel: vec![false; n],
            flights: vec![None; self.flights.len()],
            site_cool: vec![0; n],
            aaa_blue: Vec::new(),
            aaa_target: Vec::new(),
            missiles: Vec::new(),
            shells: Vec::new(),
            stores_init: stores,
            gun_init: gun,
        }));
        self.leave_world();
    }

    /// The player is not in the world (campaign, between sorties).
    pub fn player_absent(&self) -> bool {
        self.campaign.as_ref().is_some_and(|c| c.absent)
    }

    pub fn campaign(&self) -> Option<&CampaignSim> {
        self.campaign.as_deref()
    }

    /// The player leaves the world: what was after him is called off.
    fn leave_world(&mut self) {
        if let Some(c) = &mut self.campaign {
            c.absent = true;
        }
        for p in self.proj.iter_mut() {
            *p = Projectile::default();
        }
        for b in self.bullets.iter_mut() {
            *b = Bullet::default();
        }
        self.alarm_timer = 0;
        self.air_lock = -1;
        self.gnd_lock = -1;
        self.csar = None;
        self.eject = 0;
        self.threat_count = 0;
        self.tracking = 0;
        self.egr = None;
        self.ear = None;
        self.arc = None;
        self.aaa_shells.clear();
        self.aaa_track = None;
    }

    /// Campaign: ends the player's sortie from outside (abort, or the flight model's crash) with
    /// the original's outcome codes; nothing if he is not flying.
    pub fn end_sortie(&mut self, outcome: u8) {
        if self.campaign.is_none() || self.player_absent() {
            return;
        }
        let e = MissionEnd {
            outcome,
            landing_type: self.landing_type,
            map_x: self.p.mx,
            map_y: self.p.my,
            damage: self.damage,
            hits: self.hits,
        };
        self.emit(SimEvent::SortieEnded(e));
        self.leave_world();
    }

    /// Campaign `finalize`: the sortie ends, the simulation goes on.
    pub(crate) fn finalize_sortie(&mut self, e: MissionEnd) {
        self.log_event(8, 0);
        self.emit(SimEvent::SortieEnded(e));
        self.leave_world();
    }

    /// Campaign: a new sortie from base object `base` with the objective (mission type 1
    /// photo or 2 destroy, object) in the primary slot. Returns the start state for the flight
    /// model.
    pub fn respawn_player(&mut self, base: usize, objective: Option<(i32, usize)>) -> PlayerState {
        let Some(c) = &mut self.campaign else { return self.start_state() };
        c.absent = false;
        let (stores, gun) = (c.stores_init, c.gun_init);
        self.damage = 0;
        self.hits = 2;
        self.control_fault = ControlFault::default();
        self.fire_frames = 0;
        self.breakup_frames = 0;
        self.irj_heat = 0;
        self.stores = stores;
        self.gun_ammo = gun;
        self.gun_firing = false;
        self.pflags &= pflags::BAY_OPEN;
        self.eject = 0;
        self.chute = (0, 0, 0);
        self.landing_type = 1;
        self.armed_landing = 0;
        self.corridor = true;
        self.photo_timer = 0;
        self.photo_frames = 0;
        self.cue_timers = [0; 2];
        self.waypoint = 0;
        self.targeting = TargetingState::default();
        self.target_hold = false;
        self.wreck = Wreck::default();
        self.smoke_src = -1;
        self.cms = [Countermeasure::default(); 4];
        self.log.clear();
        self.targets = [
            TargetSlot {
                mission_type: objective.map_or(0, |o| o.0),
                object: objective.map_or(-1, |o| o.1 as i32),
                base: base as i32,
                code: 0,
            },
            TargetSlot { mission_type: 0, object: -1, base: base as i32, code: 0 },
        ];
        self.leave_world();
        if let Some(c) = &mut self.campaign {
            c.absent = false;
        }
        let st = self.start_state();
        self.on_ground = true;
        self.prev_on_ground = true;
        st
    }

    /// Campaign: rearm and refuel on a friendly base (the game refuels the flight model).
    pub fn rearm(&mut self, stations: &[(i32, i32)]) {
        if let Some(c) = &self.campaign {
            self.stores = c.stores_init;
            self.gun_ammo = c.gun_init;
        }
        self.damage = 0;
        self.hits = 2;
        self.control_fault = ControlFault::default();
        self.set_loadout(stations);
        self.armed_landing = 0;
        self.emit(SimEvent::Resupplied);
    }

    /// Campaign: who holds object `i` (cities, airfields).
    pub fn set_side(&mut self, i: usize, friendly: bool, neutral: bool) {
        let Some(o) = self.objects.get_mut(i) else { return };
        o.flags &= !(oflags::FRIENDLY | oflags::NEUTRAL);
        if friendly {
            o.flags |= oflags::FRIENDLY;
        } else if neutral {
            o.flags |= oflags::NEUTRAL;
        }
        if self.traffic_base == i as i32 {
            self.traffic_base = -1;
        }
    }

    /// Campaign: object `i` is intact again (repaired, or taken over), with its tile object.
    pub fn restore_object(&mut self, i: usize) {
        let Some(&(unit_type, shape)) = self.campaign.as_ref().and_then(|c| c.original.get(i))
        else {
            return;
        };
        let o = self.objects[i];
        if o.flags & oflags::DESTROYED == 0 {
            return;
        }
        self.objects[i].flags &= !(oflags::DESTROYED | oflags::PAINTED | oflags::TRACKING);
        self.objects[i].unit_type = unit_type;
        self.objects[i].shape = shape;
        self.display_shape[i] = shape;
        self.destroyed_at.retain(|&p| p != (o.x, o.y));
        if self.tiles.is_some()
            && let Some(t) = self.find_nearest_tile(o.x << 5, (0x8000 - o.y) << 5)
            && let Some(k) = self
                .tile_damage
                .iter()
                .position(|d| (d.level, d.index, d.col, d.row) == (t.level, t.index, t.col, t.row))
        {
            let d = self.tile_damage.remove(k);
            self.emit(SimEvent::TileRestored(d));
        }
    }

    /// Campaign: object `i` is gone for good (the defences of a place taken over).
    pub fn remove_object(&mut self, i: usize) {
        if i < 3 || i >= self.objects.len() || self.objects[i].flags & oflags::DESTROYED != 0 {
            return;
        }
        let o = self.objects[i];
        self.objects[i].flags |= oflags::DESTROYED;
        self.objects[i].unit_type = 0;
        let shape = self.wreck_shape(i as i32);
        self.display_shape[i] = shape;
        self.destroyed_at.push((o.x, o.y));
        self.emit(SimEvent::GroundDestroyed { object: i, shape });
        if self.tiles.is_some()
            && let Some(t) = self.find_nearest_tile(o.x << 5, (0x8000 - o.y) << 5)
        {
            self.set_tile_shape(t, shape);
        }
    }

    /// Campaign: recon intelligence on object `i`.
    pub fn set_intel(&mut self, i: usize, on: bool) {
        if let Some(v) = self.campaign.as_mut().and_then(|c| c.intel.get_mut(i)) {
            *v = on;
        }
    }

    pub fn has_intel(&self, i: usize) -> bool {
        self.campaign.as_ref().is_some_and(|c| c.intel.get(i).copied().unwrap_or(false))
    }

    /// Detection range factor of site `i` in tenths (10 = as built).
    pub(crate) fn detection_tenths(&self, i: usize) -> i32 {
        if self.has_intel(i) { INTEL_DETECTION } else { 10 }
    }

    /// Campaign: low-level air defences around a place. Enemy places get the guns and MANPADS
    /// of `SimOptions::air_defence`; friendly ones Shilkas and MANPADS teams only. Replaces the
    /// other side's guns there.
    pub fn place_defences(&mut self, x: i32, y: i32, airfield: bool, blue: bool) {
        let Some(mut c) = self.campaign.take() else { return };
        let mut rng = c.rng;
        c.aaa_blue.resize(self.aaa.len(), false);
        for (k, u) in self.aaa.iter_mut().enumerate() {
            if !u.destroyed && c.aaa_blue[k] != blue && approx(u.x - x, u.y - y) < 0x180 {
                u.destroyed = true;
                u.burst = 0;
            }
        }
        let war = self.roe;
        let mut add = |rng: &mut Rng, kind, x: i32, y: i32, lo: i32, hi: i32| {
            let t = rng.random(0x4000) as f64 * 4.0 / 65536.0 * std::f64::consts::TAU;
            let r = (lo + rng.random(hi - lo)) as f64;
            self.aaa.push(AaaUnit {
                kind,
                x: (x + (t.sin() * r) as i32).clamp(0x40, 0x7FC0),
                y: (y - (t.cos() * r) as i32).clamp(0x40, 0x7FC0),
                destroyed: false,
                tracking: false,
                aim_az: t as f32,
                aim_el: 0.3,
                burst: 0,
                cooldown: 0,
            });
            c.aaa_blue.push(blue);
        };
        match (blue, airfield) {
            (false, true) => {
                for _ in 0..1 + rng.random(war + 1) {
                    add(&mut rng, AaaKind::Zsu, x, y, 26, 90);
                }
                if war > 0 {
                    let (bx, by) = (x + rng.random(120) - 60, y + rng.random(120) - 60);
                    for _ in 0..2 + rng.random(3) {
                        add(&mut rng, AaaKind::S60, bx, by, 3, 8);
                    }
                }
                add(&mut rng, AaaKind::Manpads, x, y, 15, 80);
            }
            (false, false) => {
                for _ in 0..1 + rng.random(war + 1) {
                    add(&mut rng, AaaKind::Manpads, x, y, 15, 80);
                }
                if rng.random(3) < war {
                    add(&mut rng, AaaKind::Zsu, x, y, 20, 70);
                }
            }
            (true, true) => {
                for _ in 0..2 {
                    add(&mut rng, AaaKind::Zsu, x, y, 26, 90);
                    add(&mut rng, AaaKind::Manpads, x, y, 15, 80);
                }
            }
            (true, false) => add(&mut rng, AaaKind::Manpads, x, y, 15, 60),
        }
        if self.aaa.len() > MAX_UNITS {
            self.aaa.truncate(MAX_UNITS);
            c.aaa_blue.truncate(MAX_UNITS);
        }
        c.rng = rng;
        c.aaa_target.resize(self.aaa.len(), None);
        self.campaign = Some(c);
        if self.aaa_rng.is_none() {
            self.aaa_rng = Some(Rng::new(self.rng.hold ^ 0x5EAD_AAA0));
        }
    }

    /// Campaign: SAM site `i` fired at an aircraft lately (it gave itself away).
    pub fn site_engaging(&self, i: usize) -> bool {
        self.campaign.as_ref().is_some_and(|c| c.site_cool.get(i).is_some_and(|&t| t > 0))
    }

    /// Campaign: AAA unit `k` is engaging something (an AI aircraft or the player).
    pub fn aaa_engaging(&self, k: usize) -> bool {
        let busy = self.aaa.get(k).is_some_and(|u| !u.destroyed && (u.burst > 0 || u.tracking));
        busy || self
            .campaign
            .as_ref()
            .is_some_and(|c| c.aaa_target.get(k).is_some_and(Option::is_some))
    }

    /// Campaign: AAA unit `k` is friendly.
    pub fn aaa_blue(&self, k: usize) -> bool {
        self.campaign.as_ref().is_some_and(|c| c.aaa_blue.get(k).copied().unwrap_or(false))
    }

    /// A free flight unit for a campaign aircraft (reusing finished ones).
    fn campaign_slot(&mut self) -> usize {
        let c = self.campaign.as_ref().expect("campaign");
        let free = (self.mission_flights..self.flights.len()).find(|&i| {
            self.flights[i].flags == 0
                && c.flights.get(i).is_none_or(Option::is_none)
                && self.csar.as_ref().is_none_or(|s| s.rescue != i && !s.hunters.contains(&i))
        });
        let i = free.unwrap_or_else(|| {
            self.flights.push(FlightUnit::default());
            self.flights.len() - 1
        });
        let c = self.campaign.as_mut().expect("campaign");
        c.flights.resize(self.flights.len(), None);
        i
    }

    /// Campaign: a helicopter with troops lifts off from base `from` to land at object
    /// `target`. Returns its flight unit.
    pub fn spawn_insertion(&mut self, from: usize, target: usize, tag: u32, n: usize) -> usize {
        let i = self.campaign_slot();
        let o = self.objects[from];
        let t = self.objects[target];
        let mut u = FlightUnit {
            spec: BLACKHAWK,
            wp: target as i32,
            x: o.x + 6 + 4 * n as i32,
            y: o.y + 3 * n as i32,
            alt: 0,
            speed: 1,
            timer: 0x7FFF,
            ..Default::default()
        };
        u.wx = u.x << 5;
        u.wy = u.y << 5;
        u.flags = fflags::EXISTS | fflags::AIRBORNE | fflags::TAKEOFF | fflags::RTB;
        self.flights[i] = u;
        let c = self.campaign.as_mut().expect("campaign");
        c.flights[i] = Some(CampaignFlight {
            tag,
            role: Role::Insertion,
            blue: true,
            goal: (t.x, t.y),
            target: None,
            cooldown: 0,
            missiles: 0,
            killed_by_ai: false,
            reported: false,
        });
        self.emit(SimEvent::AircraftTakeoff { unit: i, base: from });
        i
    }

    /// Campaign: a fighter takes off from base `from` to patrol around (x, y).
    pub fn spawn_fighter(&mut self, from: usize, x: i32, y: i32, tag: u32, blue: bool) -> usize {
        let i = self.campaign_slot();
        let pick =
            |c: &mut CampaignSim, list: &[i32]| list[c.rng.random(list.len() as i32) as usize];
        let c = self.campaign.as_mut().expect("campaign");
        let spec = if blue {
            pick(c, &[21, 22])
        } else if self.aircraft_types.len() > 32 && self.theater >= 9 {
            pick(c, &[32, 31, 2])
        } else {
            pick(c, &[0, 2, 4, 25])
        };
        // Its missile: radar-guided air-to-air of its era.
        let missile = match spec {
            0 | 25 => 19,
            21 | 22 => 31,
            _ => 21,
        };
        self.flights[i] = FlightUnit { spec, missile, ..Default::default() };
        self.spawn(i, from);
        let c = self.campaign.as_mut().expect("campaign");
        c.flights[i] = Some(CampaignFlight {
            tag,
            role: Role::Fighter,
            blue,
            goal: (x, y),
            target: None,
            cooldown: 0,
            missiles: FIGHTER_MISSILES,
            killed_by_ai: false,
            reported: false,
        });
        i
    }

    /// The campaign's aircraft of flight unit `i`.
    pub fn campaign_flight(&self, i: usize) -> Option<CampaignFlight> {
        self.campaign.as_ref().and_then(|c| c.flights.get(i).copied().flatten())
    }

    /// Where a campaign fighter patrols (circling its goal), None for other aircraft.
    pub(crate) fn campaign_goal(&self, i: usize, u: &FlightUnit) -> Option<(i32, i32)> {
        let f = self.campaign_flight(i).filter(|f| f.role == Role::Fighter)?;
        let (gx, gy) = f.goal;
        if approx(u.x - gx, u.y - gy) > 0x200 {
            return Some((gx, gy));
        }
        // On station: a wide circle around the point.
        let a = s16(u.heading + 0x2000);
        Some((u.x + self.trig.sin_mul(a, 0x100), u.y - self.trig.cos_mul(a, 0x100)))
    }

    /// The aircraft a campaign fighter chases (map x, y and altitude), None for others.
    pub(crate) fn campaign_pursuit(&self, i: usize) -> Option<(i32, i32, i32)> {
        let t = self.campaign_flight(i)?.target?;
        let u = self.flights.get(t)?;
        (u.flags & fflags::DESTROYED == 0 && u.flags & fflags::AIRBORNE != 0)
            .then_some((u.x, u.y, u.alt))
    }

    /// Side of an airborne aircraft (None for wrecks, parked and traffic).
    fn ai_side(&self, i: usize) -> Option<bool> {
        let u = self.flights.get(i)?;
        if u.flags & (fflags::EXISTS | fflags::AIRBORNE) != fflags::EXISTS | fflags::AIRBORNE
            || u.flags & fflags::DESTROYED != 0
            || u.speed == 0
            || self.is_traffic(i)
        {
            return None;
        }
        Some(self.campaign_flight(i).map_or_else(|| self.friendly_aircraft(u.spec), |f| f.blue))
    }

    /// One frame of the campaign: its aircraft's fate, and the AI fighting the AI.
    pub(crate) fn update_campaign(&mut self) {
        let Some(mut c) = self.campaign.take() else { return };
        c.flights.resize(self.flights.len(), None);
        c.aaa_blue.resize(self.aaa.len(), false);
        c.aaa_target.resize(self.aaa.len(), None);
        c.site_cool.resize(self.objects.len(), 0);
        self.campaign = Some(c);
        self.campaign_bookkeeping();
        let fs = self.fs.max(1);
        if self.frame % fs == 0 {
            self.campaign_acquire();
        }
        self.campaign_guns();
        self.campaign_missiles();
        self.campaign_shells();
    }

    /// Campaign aircraft shot down or landed: report them and free their slot.
    fn campaign_bookkeeping(&mut self) {
        let mut ev = Vec::new();
        let c = self.campaign.as_mut().expect("campaign");
        for (i, slot) in c.flights.iter_mut().enumerate() {
            let Some(f) = slot.as_mut() else { continue };
            let u = &mut self.flights[i];
            if u.flags & fflags::DESTROYED != 0 && !f.reported {
                f.reported = true;
                ev.push(SimEvent::CampaignFlightDestroyed {
                    unit: i,
                    tag: f.tag,
                    by_player: !f.killed_by_ai,
                });
            }
            // Down, or stopped at the end of its landing roll.
            let stopped = u.flags & fflags::ROLLOUT != 0 && u.speed <= 0;
            if u.flags & fflags::EXISTS == 0 || u.flags & fflags::AIRBORNE == 0 || stopped {
                if !f.reported {
                    ev.push(SimEvent::CampaignFlightLanded { unit: i, tag: f.tag, x: u.x, y: u.y });
                }
                *u = FlightUnit::default();
                *slot = None;
            }
        }
        self.events.extend(ev);
    }

    /// Once a second: sites, guns and fighters pick their AI targets; sites and fighters fire.
    fn campaign_acquire(&mut self) {
        let fs = self.fs.max(1);
        let sides: Vec<Option<bool>> = (0..self.flights.len()).map(|i| self.ai_side(i)).collect();
        let pos: Vec<[f64; 3]> = self.flights.iter().map(flight_metres).collect();
        let xy: Vec<(i32, i32)> = self.flights.iter().map(|u| (u.x, u.y)).collect();
        let Some(mut c) = self.campaign.take() else { return };
        let mut rng = c.rng;
        let near = |x: i32, y: i32, blue: bool, reach: i32, min_alt: f64| {
            (0..sides.len())
                .filter(|&j| sides[j] == Some(blue) && pos[j][2] > min_alt)
                .map(|j| (j, approx(xy[j].0 - x, xy[j].1 - y)))
                .filter(|&(_, d)| d < reach)
                .min_by_key(|&(_, d)| d)
        };
        let mut launches: Vec<AiMissile> = Vec::new();
        // Enemy SAM sites against the friendly aircraft (radar horizon, intelligence).
        for i in 0..self.ground_units {
            let o = self.objects[i];
            c.site_cool[i] = (c.site_cool[i] - fs).max(0);
            if o.unit_type == 0
                || o.flags & (oflags::DESTROYED | oflags::FRIENDLY | oflags::NEUTRAL) != 0
                || c.site_cool[i] > 0
            {
                continue;
            }
            let spec = self.site_missile(o.unit_type);
            let lock = (self.missile(spec).lock_range as i32) << 6;
            let reach = lock * if c.intel[i] { INTEL_DETECTION } else { 10 } / 10;
            let Some((j, d)) = near(o.x, o.y, true, reach, 15.0) else { continue };
            if d > horizon(pos[j][2]) || rng.random(100) > 40 + 15 * self.diff {
                continue;
            }
            c.site_cool[i] = SITE_RELOAD * fs;
            let from = [o.x as f64 * M_PER_MAP, (0x8000 - o.y) as f64 * M_PER_MAP, 5.0];
            launches.push(self.ai_missile(spec, from, [0.0, 0.0, 1.0], j, false));
        }
        // Guns pick a target in reach; MANPADS teams fire.
        for k in 0..self.aaa.len() {
            let u = self.aaa[k];
            let blue = c.aaa_blue[k];
            c.aaa_target[k] = None;
            if u.destroyed || (!blue && u.tracking) {
                continue;
            }
            let (_, reach, ceiling) = gun_envelope(u.kind);
            let Some((j, _)) = near(u.x, u.y, !blue, (reach / M_PER_MAP) as i32, 15.0) else {
                continue;
            };
            if pos[j][2] > ceiling {
                continue;
            }
            c.aaa_target[k] = Some(j);
            if u.kind == AaaKind::Manpads && self.aaa[k].cooldown == 0 && rng.random(100) < 50 {
                self.aaa[k].cooldown = 45 * fs;
                let from = add(u.metres(), [0.0, 0.0, 1.5]);
                let d = sub(pos[j], from);
                let dir = mul(d, 1.0 / len(d).max(1.0));
                launches.push(self.ai_missile(SA14, from, dir, j, blue));
            }
        }
        // Campaign fighters go for the other side's aircraft and fire when in reach.
        for i in 0..c.flights.len() {
            let Some(mut f) = c.flights[i] else { continue };
            if f.role != Role::Fighter || sides[i].is_none() {
                continue;
            }
            let u = self.flights[i];
            f.cooldown = (f.cooldown - fs).max(0);
            f.target = near(u.x, u.y, !f.blue, FIGHTER_SEARCH, 0.0).map(|t| t.0);
            if let Some(j) = f.target
                && f.missiles > 0
                && f.cooldown == 0
                && u.missile > 0
            {
                let t = self.flights[j];
                let d = approx(t.x - u.x, t.y - u.y);
                let ahead =
                    crate::math::abs16(s16(
                        crate::math::bearing(t.x - u.x, s16(u.y - t.y)) - u.heading
                    )) < 0x2000;
                if ahead && d < (self.missile(u.missile).lock_range as i32) << 6 {
                    f.missiles -= 1;
                    f.cooldown = FIGHTER_RELOAD * fs;
                    let dir = direction(u.heading, u.pitch);
                    launches.push(self.ai_missile(u.missile, pos[i], dir, j, f.blue));
                }
            }
            c.flights[i] = Some(f);
        }
        c.missiles.extend(launches);
        c.rng = rng;
        self.campaign = Some(c);
    }

    fn ai_missile(
        &self,
        spec: i32,
        from: [f64; 3],
        dir: [f64; 3],
        target: usize,
        blue: bool,
    ) -> AiMissile {
        let m = self.missile(spec);
        let speed = (m.max_speed as f64 * 0.5).clamp(300.0, 1200.0);
        let turn = 0.15 + 0.08 * m.turn as f64;
        // Long enough to fly its lock range and a little more.
        let range_m = (m.lock_range as f64 * 1000.0).clamp(3000.0, 120_000.0) * 1.2;
        AiMissile {
            pos: from,
            vel: mul(dir, speed * 0.3),
            spec,
            target,
            speed,
            turn,
            ttl: ((range_m / speed + 3.0) * self.fs as f64) as i32,
            blue,
        }
    }

    /// Guns that have a target fire bursts of tracers at it.
    fn campaign_guns(&mut self) {
        let fs = self.fs.max(1);
        let Some(mut c) = self.campaign.take() else { return };
        let mut rng = c.rng;
        let skill = 1.6 - 0.2 * self.diff as f64;
        for k in 0..self.aaa.len() {
            let Some(j) = c.aaa_target[k] else { continue };
            let u = &mut self.aaa[k];
            if u.destroyed || u.kind == AaaKind::Manpads {
                continue;
            }
            let t = self.flights[j];
            let to = flight_metres(&t);
            let vel = mul(direction(t.heading, t.pitch), t.speed as f64 * 0.514);
            let from = u.metres();
            let (v0, reach, _) = gun_envelope(u.kind);
            let (az, el, time) = lead(from, to, vel, v0);
            u.aim_az = az as f32;
            u.aim_el = el as f32;
            u.cooldown = (u.cooldown - 1).max(0);
            if u.burst == 0 && u.cooldown == 0 {
                u.burst = fs * 3 / 2 + rng.random(fs);
            }
            if u.burst <= 0 {
                continue;
            }
            u.burst -= 1;
            if u.burst == 0 {
                u.cooldown = fs * 2 + rng.random(fs * 3);
            }
            let flak = u.kind == AaaKind::S60;
            if (flak && u.burst % fs != 0) || (!flak && u.burst & 1 != 0) {
                continue;
            }
            let sigma = 0.008 * skill;
            let g = |rng: &mut Rng| {
                (0..3).map(|_| rng.random(1000) as f64 / 1000.0).sum::<f64>() * 1.15 - 1.73
            };
            let (a, e) = (az + g(&mut rng) * sigma, el + g(&mut rng) * sigma);
            let dir = [a.sin() * e.cos(), a.cos() * e.cos(), e.sin()];
            let ttl = if flak { time } else { reach * 1.3 / v0 };
            if c.shells.len() < MAX_SHELLS {
                c.shells.push(AiShell {
                    shell: Shell {
                        pos: add(from, [0.0, 0.0, 1.5]),
                        vel: mul(dir, v0),
                        ttl: ((ttl * fs as f64) as i32).max(1),
                        flak,
                    },
                    target: j,
                });
            }
        }
        c.rng = rng;
        self.campaign = Some(c);
    }

    fn campaign_missiles(&mut self) {
        let dt = 1.0 / self.fs.max(1) as f64;
        let Some(mut c) = self.campaign.take() else { return };
        let mut rng = c.rng;
        let mut kills: Vec<(usize, i32, i32, i32)> = Vec::new();
        let mut pops: Vec<(i32, i32, i32)> = Vec::new();
        c.missiles.retain_mut(|m| {
            m.ttl -= 1;
            let t = self.flights.get(m.target).copied().unwrap_or_default();
            let alive = t.flags & fflags::DESTROYED == 0 && t.flags & fflags::AIRBORNE != 0;
            // Accelerate to speed, turn towards the target (pure pursuit), else fly on.
            let v = len(m.vel).max(1.0);
            let speed = (v + m.speed * dt * 1.5).min(m.speed);
            let mut dir = mul(m.vel, 1.0 / v);
            if alive {
                let to = sub(flight_metres(&t), m.pos);
                let want = mul(to, 1.0 / len(to).max(1e-6));
                let k = (m.turn * dt * 4.0).min(1.0);
                dir = add(mul(dir, 1.0 - k), mul(want, k));
                dir = mul(dir, 1.0 / len(dir).max(1e-9));
            }
            let step = mul(dir, speed * dt);
            if alive {
                let rel = sub(m.pos, flight_metres(&t));
                if crate::aaa::closest(rel, step) < FUSE {
                    let (x, y, alt) = metres_map(m.pos);
                    pops.push((x, y, alt));
                    if rng.random(1000) < MISSILE_PK {
                        kills.push((m.target, x, y, alt));
                    }
                    return false;
                }
            }
            m.vel = mul(dir, speed);
            m.pos = add(m.pos, step);
            m.ttl > 0 && m.pos[2] > 0.0
        });
        c.rng = rng;
        self.campaign = Some(c);
        for (x, y, alt) in pops {
            self.emit(SimEvent::Explosion { x, y, alt, kind: -6 });
        }
        for (j, ..) in kills {
            self.ai_kill(j);
        }
    }

    fn campaign_shells(&mut self) {
        let dt = 1.0 / self.fs.max(1) as f64;
        let Some(mut c) = self.campaign.take() else { return };
        let mut rng = c.rng;
        let mut kills = Vec::new();
        let mut bursts = Vec::new();
        c.shells.retain_mut(|s| {
            let t = self.flights.get(s.target).copied().unwrap_or_default();
            let tp = flight_metres(&t);
            let step = mul(s.shell.vel, dt);
            let rel = sub(s.shell.pos, tp);
            s.shell.pos = add(s.shell.pos, step);
            s.shell.vel[2] -= G * dt;
            s.shell.ttl -= 1;
            if s.shell.pos[2] < 0.0 {
                return false;
            }
            let alive = t.flags & fflags::DESTROYED == 0 && t.flags & fflags::AIRBORNE != 0;
            let near = crate::aaa::closest(rel, step);
            if s.shell.flak && s.shell.ttl <= 0 {
                bursts.push(metres_map(s.shell.pos));
                if alive && len(sub(s.shell.pos, tp)) < 30.0 && rng.random(1000) < 400 {
                    kills.push(s.target);
                }
                return false;
            }
            if !s.shell.flak && alive && near < TRACER_RADIUS && rng.random(1000) < TRACER_HIT {
                kills.push(s.target);
                return false;
            }
            s.shell.ttl > 0
        });
        c.rng = rng;
        self.campaign = Some(c);
        for (x, y, alt) in bursts {
            self.emit(SimEvent::Flak { x, y, alt });
        }
        for j in kills {
            self.ai_kill(j);
        }
    }

    /// An AI aircraft destroyed by the AI.
    fn ai_kill(&mut self, j: usize) {
        let Some(u) = self.flights.get(j).copied() else { return };
        if u.flags & fflags::DESTROYED != 0 {
            return;
        }
        if let Some(Some(f)) = self.campaign.as_mut().and_then(|c| c.flights.get_mut(j)) {
            f.killed_by_ai = true;
        }
        self.flights[j].flags |= fflags::DESTROYED;
        let name = self.ac_type(u.spec).name.clone();
        self.msg(format!("{name} shot down"));
        self.emit(SimEvent::Explosion { x: u.x, y: u.y, alt: u.alt, kind: 0x10 });
        self.emit(SimEvent::AircraftDestroyed { unit: j, spec: u.spec });
    }

    /// The AI's missiles in flight (metres east, north, up; velocity).
    pub fn ai_missiles(&self) -> &[AiMissile] {
        self.campaign.as_ref().map_or(&[], |c| &c.missiles)
    }

    /// The AI's tracers and shells in flight.
    pub fn ai_shells(&self) -> impl Iterator<Item = &Shell> {
        self.campaign.as_ref().into_iter().flat_map(|c| c.shells.iter().map(|s| &s.shell))
    }

    /// Campaign frame without the player: the world goes on.
    pub(crate) fn step_absent(&mut self) -> Vec<SimEvent> {
        if self.frame & 7 == 0 {
            self.update_base_traffic();
        }
        if self.frame & 0x7F == 0x40 && self.traffic_base >= 0 {
            self.spawn_traffic();
        }
        self.update_aircraft();
        self.update_campaign();
        self.update_effects();
        self.tick_frames += 1;
        if self.tick_frames >= self.fs {
            self.tick_frames = 0;
            self.mission_tick += 1;
        }
        self.frame = (self.frame + 1) & 0xFFFF;
        std::mem::take(&mut self.events)
    }
}

/// Unit vector (east, north, up) of a heading and pitch (BAM).
fn direction(heading: i32, pitch: i32) -> [f64; 3] {
    let (h, p) = (bam(heading), bam(pitch));
    [h.sin() * p.cos(), h.cos() * p.cos(), p.sin()]
}

fn bam(a: i32) -> f64 {
    s16(a) as f64 / 65536.0 * std::f64::consts::TAU
}

/// Muzzle velocity (m/s), slant range (m) and highest target (m) of a gun.
fn gun_envelope(kind: AaaKind) -> (f64, f64, f64) {
    match kind {
        AaaKind::Zsu => (970.0, 2500.0, 2000.0),
        AaaKind::S60 => (1000.0, 6000.0, 4500.0),
        AaaKind::Manpads => (0.0, 5000.0, 3500.0),
    }
}

/// Metres (east, north, up) to map x, y and altitude (world units).
fn metres_map(p: [f64; 3]) -> (i32, i32, i32) {
    ((p[0] / M_PER_MAP) as i32, 0x8000 - (p[1] / M_PER_MAP) as i32, (p[2] / M_PER_WU) as i32)
}
