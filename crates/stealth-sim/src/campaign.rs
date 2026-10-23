//! Realtime dynamic campaign (`SimOptions::campaign`). One simulation
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
//!   (the classic projectile slots stay the player's business).
//! * Recon intelligence ([`Sim::set_intel`]) shortens a site's detection range and lets the
//!   player spot the object from twice as far.
//!
//! Everything lives in [`CampaignSim`] and draws from its own random stream, so the classic
//! behaviour is untouched while the option is off.

use serde::{Deserialize, Serialize};

use crate::aaa::{AaaKind, AaaUnit, M_PER_MAP, PATRIOT, SA14, STINGER, Shell, lead};
use crate::ai_loadout::{AiLoadout, FighterTask};
use crate::csar::{BLACKHAWK, HIND};
use crate::math::{approx, s16};
use crate::rng::Rng;
use crate::types::*;
use crate::{AircraftKind, ENEMY_SLOTS, Sim};

const M_PER_WU: f64 = M_PER_MAP / 32.0;
const G: f64 = 9.81;
/// Detection range factor of a site with recon intelligence against it (tenths).
pub const INTEL_DETECTION: i32 = 7;
/// Seconds between a site's launches at AI aircraft, and a fighter's.
const SITE_RELOAD: i32 = 25;
const FIGHTER_RELOAD: i32 = 12;
/// A fighter goes for enemy aircraft this close (80 km, its radar).
const FIGHTER_SEARCH: i32 = 0x1400;
/// A SAM site engages a helicopter this close at most (8 km): low and slow, it hides in the
/// terrain and the ground clutter from a far radar.
const HELO_SAM_REACH: i32 = 0x200;
/// A SAM site engages a strike aircraft on its low-level run in and out this close at most (16 km),
/// inside the reach of its anti-radiation missiles (`HARM`): a SEAD flight gets its shot off
/// at the site it goes for, the sites it passes on the way still shoot.
const STRIKE_SAM_REACH: i32 = 0x400;
/// A fighter's gun: muzzle velocity (m/s), reach (m), and how far off its nose (rad) the
/// target may be for it to fire.
const FIGHTER_GUN_V0: f64 = 1000.0;
const FIGHTER_GUN_REACH: f64 = 900.0;
const FIGHTER_GUN_CONE: f64 = 0.1;
/// A CAP fighter engages aircraft this close to its airfield (40 km), drops them once it is this
/// far from it itself (50 km: it does not chase into enemy territory), and lands after this long
/// (s) without a target.
pub const CAP_BUBBLE: i32 = 0xA00;
pub const CAP_LEASH: i32 = 0xC80;
pub const CAP_LINGER: i32 = 60;
/// Patriot: how far it engages (35 km: it guards the rear, the front stays the players' and
/// the CAPs') and its lowest target (m); seconds between a launcher's missiles and to reload
/// its four; missiles in the air at one aircraft at most.
const PATRIOT_REACH: i32 = 0x8C0;
const PATRIOT_FLOOR: f64 = 60.0;
const PATRIOT_RELOAD: i32 = 120;
const PATRIOT_INTERVAL: i32 = 8;
const PATRIOT_SALVO: usize = 2;
pub const PATRIOT_ROUNDS: i32 = 4;
/// A launcher fires on the track of a radar this close (1 km); its fixed elevation (rad).
const PATRIOT_LINK: i32 = 0x40;
pub const PATRIOT_ELEVATION: f32 = 0.66;
/// A missile's fuse (m) and the chance (per mille) it kills what it reaches.
const FUSE: f64 = 25.0;
const MISSILE_PK: i32 = 700;
/// A tracer this close to an aircraft hits it, with this chance (per mille).
const TRACER_RADIUS: f64 = 10.0;
const TRACER_HIT: i32 = 300;
const MAX_SHELLS: usize = 200;
/// Low-level air defences in a campaign at most.
const MAX_UNITS: usize = 400;
/// A crashed pilot's aircraft below this (ft) leaves a burning wreck where it came down.
const CRASH_SITE_ALT: i32 = 200;
/// The attack drone's aircraft type (Geran-2), and the height it cruises at (~150 m).
const DRONE_ALT: i32 = 300;
/// A recalled AWACS this close to its base (map units, ~4 km) has landed.
const AWACS_HOME: i32 = 0x100;
/// The attack drone's aircraft type.
const GERAN: usize = 29;
/// What a campaign strike aircraft fires at its target and from how far (map units): a SAM
/// site's radar gets an anti-radiation missile from its lock range (20 km), anything else a
/// salvo of air-to-ground rockets (the AGM-65's flight, ~5 km).
const HARM: i32 = 24;
const ROCKETS: i32 = 27;
const ROCKET_RELEASE: i32 = 0x140;
/// The chance (per mille) a strike aircraft's weapon that reaches its target destroys it.
const GROUND_PK: i32 = 850;
/// Altitude (world units, ~590 m) a strike aircraft flies in at.
pub(crate) const STRIKE_ALT: i32 = 1200;

/// What a campaign aircraft is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    /// Helicopter with troops, landing at its goal object.
    Insertion,
    /// Fighter: patrols around its goal, goes for the other side's aircraft.
    Fighter,
    /// Strike aircraft: flies low to its goal, fires at the object there and flies home (an
    /// attack drone dives into it).
    Strike,
    /// AWACS: orbits its goal and fights nobody.
    Awacs,
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
    /// Its missiles (the gun is always there).
    #[serde(default)]
    pub loadout: AiLoadout,
    #[serde(default)]
    pub task: FighterTask,
    /// The base it took off from (object index) and that base's position: a CAP defends it.
    #[serde(default)]
    pub home: i32,
    #[serde(default)]
    pub anchor: (i32, i32),
    /// CAP: seconds without anything to engage.
    #[serde(default)]
    pub quiet: i32,
    /// Gun: frames left of a burst, or (negative) until the next.
    #[serde(default)]
    pub burst: i32,
    /// Shot down by the AI (else by the player), and reported.
    pub killed_by_ai: bool,
    pub reported: bool,
    /// The pilot who shot it down.
    #[serde(default)]
    pub killed_by: Option<u8>,
    /// Strike aircraft: the object it attacks (None once it has fired).
    #[serde(default)]
    pub strike: Option<i32>,
    /// An attack drone: it is destroyed with its target.
    #[serde(default)]
    pub kamikaze: bool,
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
    /// A strike aircraft's weapon: the object it flies at (`target` is then its launcher).
    #[serde(default)]
    pub ground: Option<usize>,
}

/// A tracer of a gun firing at an AI aircraft.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AiShell {
    pub shell: Shell,
    pub target: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CampaignSim {
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
        self.pl.deadline = 0;
        let base = self.pl.targets[0].base;
        self.pl.targets = [TargetSlot { mission_type: 0, object: -1, base, code: 0 }; 2];
        let seed = self.rng.hold ^ 0xCA4D_A1A1;
        self.campaign = Some(Box::new(CampaignSim {
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

    /// The active pilot is not in the world (campaign, between sorties).
    pub fn player_absent(&self) -> bool {
        self.campaign.is_some() && self.pl.absent
    }

    /// Pilot `id` is not in the world.
    pub fn pilot_absent(&self, id: u8) -> bool {
        let p = self.pilot_ref(id as usize);
        p.id != id || p.absent
    }

    pub fn campaign(&self) -> Option<&CampaignSim> {
        self.campaign.as_deref()
    }

    /// The active pilot leaves the world: what was after him is called off.
    fn leave_world(&mut self) {
        let me = self.pl.id;
        self.pl.absent = true;
        let alone = self.present().is_empty();
        for s in Self::own_slots() {
            self.proj[s] = Projectile::default();
        }
        for s in 0..ENEMY_SLOTS {
            if alone || self.proj_pilot[s] == me {
                self.proj[s] = Projectile::default();
            }
        }
        let nb = if alone { self.bullets.len() } else { self.n_bullets };
        for b in self.bullets[..nb].iter_mut() {
            *b = Bullet::default();
        }
        if alone {
            self.alarm_timer = 0;
            self.aaa_shells.clear();
        } else {
            self.aaa_shells.retain(|s| s.pilot != me);
        }
        for (i, o) in self.objects.iter_mut().enumerate().take(self.ground_units) {
            if !alone && self.site_pilot.get(i).is_some_and(|&k| k == me) {
                o.flags &= !(oflags::PAINTED | oflags::TRACKING);
            }
        }
        self.pl.air_lock = -1;
        self.pl.gnd_lock = -1;
        self.pl.pilot_lock = None;
        self.csar = None;
        self.pl.eject = 0;
        self.pl.threat_count = 0;
        self.pl.tracking = 0;
        self.pl.egr = None;
        self.pl.ear = None;
        self.pl.arc = None;
        self.pl.aaa_track = None;
    }

    /// Campaign: ends the player's sortie from outside (abort, or the flight model's crash) with
    /// the mission outcome codes; nothing if he is not flying.
    pub fn end_sortie(&mut self, outcome: u8) {
        self.end_sortie_pilot(self.pl.id, outcome);
    }

    /// Campaign: ends pilot `id`'s sortie from outside.
    pub fn end_sortie_pilot(&mut self, id: u8, outcome: u8) {
        let back = self.pl.id as usize;
        self.activate(id as usize);
        if self.campaign.is_some() && self.pl.id == id && !self.player_absent() {
            let e = MissionEnd {
                outcome,
                landing_type: self.pl.landing_type,
                map_x: self.pl.p.mx,
                map_y: self.pl.p.my,
                damage: self.pl.damage,
                hits: self.pl.hits,
            };
            self.crash_site(outcome);
            self.emit(SimEvent::SortieEnded(e));
            self.leave_world();
        }
        self.activate(back);
    }

    /// Campaign `finalize`: the sortie ends, the simulation goes on.
    pub(crate) fn finalize_sortie(&mut self, e: MissionEnd) {
        self.log_event(8, 0);
        self.crash_site(e.outcome);
        self.emit(SimEvent::SortieEnded(e));
        self.leave_world();
    }

    /// A pilot who crashed into the ground (outcomes 1, 2) without ejecting leaves a burning
    /// wreck everyone sees, from the next step (one blown up in the air has had his explosion).
    fn crash_site(&mut self, outcome: u8) {
        let p = self.pl.p;
        if matches!(outcome, 1 | 2) && self.pl.eject == 0 && p.alt < CRASH_SITE_ALT {
            self.crash_sites.push((p.mx, p.my));
        }
    }

    /// Campaign: a new sortie from base object `base` with the objective (mission type 1
    /// photo or 2 destroy, object) in the primary slot. Returns the start state for the flight
    /// model.
    pub fn respawn_player(&mut self, base: usize, objective: Option<(i32, usize)>) -> PlayerState {
        let airframe = Airframe {
            lockheed: self.pl.lockheed,
            external_stores: self.pl.external_stores,
            helicopter: self.pl.helicopter,
        };
        self.respawn_pilot(self.pl.id, base, objective, airframe)
    }

    /// Campaign: pilot `id`'s new sortie (as [`Sim::respawn_player`]) in an aircraft with or
    /// without a cannon (the real F-117A), with its weapons in a bay or outside, or a helicopter.
    pub fn respawn_pilot(
        &mut self,
        id: u8,
        base: usize,
        objective: Option<(i32, usize)>,
        airframe: Airframe,
    ) -> PlayerState {
        let Airframe { lockheed, external_stores, helicopter } = airframe;
        if self.campaign.is_none() {
            return self.start_state();
        }
        if id as usize >= self.pilot_count() {
            self.add_pilot(id);
        }
        let back = self.pl.id as usize;
        self.activate(id as usize);
        let Some(c) = &self.campaign else { return self.start_state() };
        let (stores, gun) = (c.stores_init, c.gun_init);
        self.pl.absent = false;
        self.pl.lockheed = lockheed;
        self.pl.external_stores = external_stores;
        self.pl.helicopter = helicopter;
        self.pl.unload = [0; 2];
        self.pl.damage = 0;
        self.pl.hits = 2;
        self.pl.control_fault = ControlFault::default();
        self.pl.fire_frames = 0;
        self.pl.breakup_frames = 0;
        self.pl.irj_heat = 0;
        self.pl.stores = stores;
        self.pl.gun_ammo = gun;
        self.pl.gun_firing = false;
        self.pl.pflags &= pflags::BAY_OPEN;
        if external_stores {
            self.pl.bay_req = true;
            self.pl.door_anim = 2;
            self.pl.pflags |= pflags::BAY_OPEN;
        }
        self.pl.eject = 0;
        self.pl.chute = (0, 0, 0);
        self.pl.landing_type = 1;
        self.pl.armed_landing = 0;
        self.pl.corridor = true;
        self.pl.photo_timer = 0;
        self.pl.photo_frames = 0;
        self.pl.cue_timers = [0; 2];
        self.pl.waypoint = 0;
        self.pl.targeting = TargetingState::default();
        self.pl.target_hold = false;
        self.wreck = Wreck::default();
        self.smoke_src = -1;
        self.pl.cms = [Countermeasure::default(); 4];
        self.pl.log.clear();
        self.pl.targets = [
            TargetSlot {
                mission_type: objective.map_or(0, |o| o.0),
                object: objective.map_or(-1, |o| o.1 as i32),
                base: base as i32,
                code: 0,
            },
            TargetSlot { mission_type: 0, object: -1, base: base as i32, code: 0 },
        ];
        self.leave_world();
        self.pl.absent = false;
        let st = self.start_state();
        self.pl.on_ground = true;
        self.pl.prev_on_ground = true;
        self.pl.last_input = st.clone();
        self.activate(back);
        st
    }

    /// Campaign: rearm and refuel on a friendly base (the game refuels the flight model).
    pub fn rearm(&mut self, stations: &[(i32, i32)]) {
        self.rearm_pilot(self.pl.id, stations);
    }

    /// Campaign: pilot `id` rearms.
    pub fn rearm_pilot(&mut self, id: u8, stations: &[(i32, i32)]) {
        let back = self.pl.id as usize;
        self.activate(id as usize);
        if let Some(c) = &self.campaign {
            self.pl.stores = c.stores_init;
            self.pl.gun_ammo = c.gun_init;
        }
        self.pl.damage = 0;
        self.pl.hits = 2;
        self.pl.control_fault = ControlFault::default();
        self.set_loadout(stations);
        self.pl.armed_landing = 0;
        self.emit(SimEvent::Resupplied);
        self.activate(back);
    }

    /// Pilot `id`'s stores (as [`Sim::set_loadout`]) and the selected station.
    pub fn set_pilot_loadout(&mut self, id: u8, stations: &[(i32, i32)], select: usize) {
        let back = self.pl.id as usize;
        self.activate(id as usize);
        self.set_loadout(stations);
        self.select_bay(select);
        self.activate(back);
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
        self.emit(SimEvent::GroundDestroyed { object: i, shape, by: None });
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
    /// of `SimOptions::air_defence`; friendly ones Gepards at airfields and Stinger teams.
    /// Replaces the other side's guns there.
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
                rounds: 0,
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
                    add(&mut rng, AaaKind::Gepard, x, y, 26, 90);
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

    /// Campaign: `n` of a side's live guns and MANPADS teams around a place are wrecks (lost
    /// before the simulation was built).
    pub fn wreck_defences(&mut self, x: i32, y: i32, blue: bool, n: usize) {
        let Some(c) = self.campaign.as_ref() else { return };
        let mine: Vec<usize> = (0..self.aaa.len())
            .filter(|&k| c.aaa_blue.get(k).copied().unwrap_or(false) == blue)
            .filter(|&k| !self.aaa[k].kind.patriot())
            .filter(|&k| {
                !self.aaa[k].destroyed && approx(self.aaa[k].x - x, self.aaa[k].y - y) < 0x180
            })
            .collect();
        for k in mine.into_iter().rev().take(n) {
            self.aaa[k].destroyed = true;
            self.aaa[k].burst = 0;
        }
    }

    /// Campaign: one of a side's destroyed guns or MANPADS teams around a place is replaced.
    pub fn restore_defence(&mut self, x: i32, y: i32, blue: bool) {
        let Some(c) = self.campaign.as_ref() else { return };
        let k = (0..self.aaa.len()).find(|&k| {
            let u = &self.aaa[k];
            c.aaa_blue.get(k).copied().unwrap_or(false) == blue
                && !u.kind.patriot()
                && u.destroyed
                && approx(u.x - x, u.y - y) < 0x180
        });
        if let Some(k) = k {
            let u = &mut self.aaa[k];
            u.destroyed = false;
            u.tracking = false;
            u.cooldown = 0;
            u.burst = 0;
        }
    }

    /// Campaign: a friendly Patriot battery at (x, y): its radar and three launchers around it.
    pub fn place_patriot(&mut self, x: i32, y: i32) {
        let Some(mut c) = self.campaign.take() else { return };
        if self.aaa.len() + 4 <= MAX_UNITS {
            c.aaa_blue.resize(self.aaa.len(), false);
            let mut rng = c.rng;
            let facing = rng.random(0x4000) as f32 * 4.0 / 65536.0 * std::f32::consts::TAU;
            let unit = |kind: AaaKind, x: i32, y: i32| AaaUnit {
                kind,
                x: x.clamp(0x40, 0x7FC0),
                y: y.clamp(0x40, 0x7FC0),
                aim_az: facing,
                aim_el: PATRIOT_ELEVATION,
                rounds: if kind == AaaKind::PatriotLauncher { PATRIOT_ROUNDS } else { 0 },
                ..Default::default()
            };
            self.aaa.push(unit(AaaKind::PatriotRadar, x, y));
            for k in 0..3 {
                let t = facing as f64 + (k as f64 - 1.0) * 1.2 + std::f64::consts::PI;
                let r = (10 + rng.random(16)) as f64;
                let (lx, ly) = (x + (t.sin() * r) as i32, y - (t.cos() * r) as i32);
                self.aaa.push(unit(AaaKind::PatriotLauncher, lx, ly));
            }
            c.aaa_blue.resize(self.aaa.len(), true);
            c.aaa_target.resize(self.aaa.len(), None);
            c.rng = rng;
        }
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

    /// Campaign: a friendly helicopter with troops lifts off from base `from` to land at
    /// object `target`. Returns its flight unit.
    pub fn spawn_insertion(&mut self, from: usize, target: usize, tag: u32, n: usize) -> usize {
        self.spawn_insertion_for(from, target, tag, n, true)
    }

    /// Campaign: a helicopter of a side with troops (a UH-60, an enemy Mi-24) lifts off from
    /// base `from` to land at object `target`. Returns its flight unit.
    pub fn spawn_insertion_for(
        &mut self,
        from: usize,
        target: usize,
        tag: u32,
        n: usize,
        blue: bool,
    ) -> usize {
        let i = self.campaign_slot();
        let o = self.objects[from];
        let t = self.objects[target];
        let mut u = FlightUnit {
            spec: if blue { BLACKHAWK } else { HIND },
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
            blue,
            goal: (t.x, t.y),
            target: None,
            cooldown: 0,
            loadout: AiLoadout::default(),
            task: FighterTask::Patrol,
            home: from as i32,
            anchor: (0, 0),
            quiet: 0,
            burst: 0,
            killed_by_ai: false,
            killed_by: None,
            reported: false,
            strike: None,
            kamikaze: false,
        });
        self.emit(SimEvent::AircraftTakeoff { unit: i, base: from });
        i
    }

    /// Campaign: a strike aircraft of a side takes off from base `from` to bomb object
    /// `target`, then flies home and lands.
    pub fn spawn_striker(&mut self, from: usize, target: usize, tag: u32, blue: bool) -> usize {
        let pick =
            |c: &mut CampaignSim, list: &[i32]| list[c.rng.random(list.len() as i32) as usize];
        let c = self.campaign.as_mut().expect("campaign");
        let spec = if blue {
            pick(c, &[22, 24])
        } else if self.aircraft_types.len() > 32 && self.theater >= 9 {
            31
        } else {
            0
        };
        self.spawn_strike(from, target, tag, blue, spec, false)
    }

    /// Campaign: an attack drone (a Geran-2) of a side takes off from base `from` and dives
    /// into object `target`.
    pub fn spawn_drone(&mut self, from: usize, target: usize, tag: u32, blue: bool) -> usize {
        let spec = if self.aircraft_types.len() > GERAN { GERAN as i32 } else { 0 };
        self.spawn_strike(from, target, tag, blue, spec, true)
    }

    /// Campaign: an AWACS of a side takes off from base `from` to orbit (x, y).
    pub fn spawn_awacs(&mut self, from: usize, x: i32, y: i32, tag: u32, blue: bool) -> usize {
        let i = self.campaign_slot();
        self.flights[i] = FlightUnit { spec: if blue { 18 } else { 5 }, ..Default::default() };
        self.spawn(i, from);
        let base = self.objects.get(from).map_or((x, y), |o| (o.x, o.y));
        let c = self.campaign.as_mut().expect("campaign");
        c.flights[i] = Some(CampaignFlight {
            tag,
            role: Role::Awacs,
            blue,
            goal: (x, y),
            target: None,
            cooldown: 0,
            loadout: AiLoadout::default(),
            task: FighterTask::Patrol,
            home: from as i32,
            anchor: base,
            quiet: 0,
            burst: 0,
            killed_by_ai: false,
            killed_by: None,
            reported: false,
            strike: None,
            kamikaze: false,
        });
        i
    }

    /// Campaign: the aircraft with campaign tag `tag` flies home and lands.
    pub fn recall_flight(&mut self, tag: u32) {
        let Some(c) = self.campaign.as_ref() else { return };
        let Some(i) = c.flights.iter().position(|f| f.is_some_and(|f| f.tag == tag)) else {
            return;
        };
        let home = c.flights[i].map_or(0, |f| f.home);
        self.flights[i].flags |= fflags::RTB;
        self.flights[i].wp = home;
    }

    fn spawn_strike(
        &mut self,
        from: usize,
        target: usize,
        tag: u32,
        blue: bool,
        spec: i32,
        kamikaze: bool,
    ) -> usize {
        let i = self.campaign_slot();
        self.flights[i] = FlightUnit { spec, ..Default::default() };
        self.spawn(i, from);
        if kamikaze {
            // Launched at cruise height and speed, straight at the target (`fly_drone`).
            let t = self.ac_type(spec).max_speed as i32;
            let u = &mut self.flights[i];
            u.wp = target as i32;
            u.speed = t;
            u.alt = DRONE_ALT;
            u.flags &= !fflags::TAKEOFF;
        }
        let t = self.objects.get(target).map_or((0, 0), |o| (o.x, o.y));
        let base = self.objects.get(from).map_or(t, |o| (o.x, o.y));
        let c = self.campaign.as_mut().expect("campaign");
        c.flights[i] = Some(CampaignFlight {
            tag,
            role: Role::Strike,
            blue,
            goal: t,
            target: None,
            cooldown: 0,
            loadout: AiLoadout::default(),
            task: FighterTask::Patrol,
            home: from as i32,
            anchor: base,
            quiet: 0,
            burst: 0,
            killed_by_ai: false,
            killed_by: None,
            reported: false,
            strike: Some(target as i32),
            kamikaze,
        });
        i
    }

    /// Campaign: a fighter takes off from base `from` for `task`, orbiting (x, y).
    pub fn spawn_fighter(
        &mut self,
        from: usize,
        x: i32,
        y: i32,
        tag: u32,
        blue: bool,
        task: FighterTask,
    ) -> usize {
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
        let loadout = AiLoadout::for_task(spec, missile, task);
        self.flights[i] =
            FlightUnit { spec, missile: loadout.stores[0].missile, ..Default::default() };
        self.spawn(i, from);
        let base = self.objects.get(from).map_or((x, y), |o| (o.x, o.y));
        let c = self.campaign.as_mut().expect("campaign");
        c.flights[i] = Some(CampaignFlight {
            tag,
            role: Role::Fighter,
            blue,
            goal: (x, y),
            target: None,
            cooldown: 0,
            loadout,
            task,
            home: from as i32,
            anchor: base,
            quiet: 0,
            burst: 0,
            killed_by_ai: false,
            killed_by: None,
            reported: false,
            strike: None,
            kamikaze: false,
        });
        i
    }

    /// Campaign: rearm fighter `i` with `loadout` (another task's, or none for its gun only).
    pub fn set_fighter_loadout(&mut self, i: usize, loadout: AiLoadout) {
        if let Some(Some(f)) = self.campaign.as_mut().and_then(|c| c.flights.get_mut(i)) {
            f.loadout = loadout;
        }
    }

    /// The campaign's aircraft of flight unit `i`.
    pub fn campaign_flight(&self, i: usize) -> Option<CampaignFlight> {
        self.campaign.as_ref().and_then(|c| c.flights.get(i).copied().flatten())
    }

    /// Whether flight unit `i` (of aircraft spec `spec`) is friendly: a campaign aircraft's
    /// side, else its type's.
    pub fn friendly_unit(&self, i: usize, spec: i32) -> bool {
        self.campaign_flight(i).map_or_else(|| self.friendly_aircraft(spec), |f| f.blue)
    }

    /// Where a campaign fighter patrols (circling its goal) or a strike aircraft flies to,
    /// None for other aircraft.
    pub(crate) fn campaign_goal(&self, i: usize, u: &FlightUnit) -> Option<(i32, i32)> {
        let f = self.campaign_flight(i).filter(|f| f.role != Role::Insertion)?;
        let (gx, gy) = f.goal;
        if f.role == Role::Strike {
            return Some((gx, gy));
        }
        if approx(u.x - gx, u.y - gy) > 0x200 {
            return Some((gx, gy));
        }
        // On station: a wide circle around the point.
        let a = s16(u.heading + 0x2000);
        Some((u.x + self.trig.sin_mul(a, 0x100), u.y - self.trig.cos_mul(a, 0x100)))
    }

    /// A campaign strike aircraft on its way to the target (it comes in low).
    pub(crate) fn campaign_striking(&self, i: usize) -> bool {
        self.campaign_flight(i).is_some_and(|f| f.role == Role::Strike && f.strike.is_some())
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
        Some(self.friendly_unit(i, u.spec))
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
        self.campaign_bombs();
        let fs = self.fs.max(1);
        if self.frame % fs == 0 {
            self.campaign_acquire();
        }
        self.campaign_guns();
        self.campaign_fighter_guns();
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
                    by: if f.killed_by_ai { None } else { f.killed_by },
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
        for e in ev {
            self.emit(e);
        }
    }

    /// Strike aircraft in reach of their target fire at it and turn for home; a recalled
    /// AWACS over its base lands. (Attack drones hit theirs in `drone_impact`.)
    fn campaign_bombs(&mut self) {
        // Per object: the weapon a strike aircraft fires at it, and from how far.
        let weapon = |sim: &Sim, t: usize| {
            let o = sim.objects.get(t).copied().unwrap_or_default();
            if o.unit_type != 0 && o.flags & oflags::AIRFIELD == 0 {
                (HARM, (sim.missile(HARM).lock_range as i32) << 6)
            } else {
                (ROCKETS, ROCKET_RELEASE)
            }
        };
        let weapons: Vec<(i32, i32)> = (0..self.objects.len()).map(|t| weapon(self, t)).collect();
        let c = self.campaign.as_mut().expect("campaign");
        let mut shots = Vec::new();
        for (i, slot) in c.flights.iter_mut().enumerate() {
            let Some(f) = slot.as_mut() else { continue };
            let u = &mut self.flights[i];
            // A recalled AWACS over its base is down (too heavy for the AI's tight circuit).
            if f.role == Role::Awacs
                && u.flags & fflags::RTB != 0
                && approx(u.x - f.anchor.0, u.y - f.anchor.1) < AWACS_HOME
            {
                u.flags &= !fflags::AIRBORNE;
                continue;
            }
            // Attack drones dive all the way in (`drone_impact`).
            let Some(target) = f.strike.filter(|_| !f.kamikaze) else { continue };
            if u.flags & fflags::DESTROYED != 0 || u.flags & fflags::AIRBORNE == 0 {
                continue;
            }
            let (spec, release) =
                weapons.get(target as usize).copied().unwrap_or((ROCKETS, ROCKET_RELEASE));
            if approx(u.x - f.goal.0, u.y - f.goal.1) < release {
                f.strike = None;
                u.flags |= fflags::RTB;
                u.wp = f.home;
                shots.push((i, spec, target as usize, f.blue));
            }
        }
        for (i, spec, target, blue) in shots {
            let u = self.flights[i];
            let mut m = self.ai_missile(spec, flight_metres(&u), direction(u.heading, 0), i, blue);
            m.ground = Some(target);
            if let Some(c) = self.campaign.as_mut() {
                c.missiles.push(m);
            }
        }
    }

    /// Once a second: sites, guns and fighters pick their AI targets; sites and fighters fire.
    fn campaign_acquire(&mut self) {
        let fs = self.fs.max(1);
        let sides: Vec<Option<bool>> = (0..self.flights.len()).map(|i| self.ai_side(i)).collect();
        let pos: Vec<[f64; 3]> = self.flights.iter().map(flight_metres).collect();
        let xy: Vec<(i32, i32)> = self.flights.iter().map(|u| (u.x, u.y)).collect();
        // How close a SAM site engages each low flyer: helicopters, strike aircraft in and out.
        let low: Vec<Option<i32>> = (0..self.flights.len())
            .map(|i| {
                if self.aircraft_kind(self.flights[i].spec) == AircraftKind::Helicopter {
                    Some(HELO_SAM_REACH)
                } else if self
                    .campaign_flight(i)
                    .is_some_and(|f| f.role == Role::Strike && !f.kamikaze)
                {
                    Some(STRIKE_SAM_REACH)
                } else {
                    None
                }
            })
            .collect();
        let Some(mut c) = self.campaign.take() else { return };
        let mut rng = c.rng;
        // The nearest aircraft of a side in reach (`low` for a SAM radar's low flyers).
        let near = |x: i32, y: i32, blue: bool, reach: i32, sam: bool, min_alt: f64| {
            (0..sides.len())
                .filter(|&j| sides[j] == Some(blue) && pos[j][2] > min_alt)
                .map(|j| (j, approx(xy[j].0 - x, xy[j].1 - y)))
                .filter(|&(j, d)| d < low[j].filter(|_| sam).map_or(reach, |l| reach.min(l)))
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
            let Some((j, d)) = near(o.x, o.y, true, reach, true, 15.0) else {
                continue;
            };
            if d > horizon(pos[j][2]) || rng.random(100) > 40 + 15 * self.diff {
                continue;
            }
            c.site_cool[i] = SITE_RELOAD * fs;
            let from = [o.x as f64 * M_PER_MAP, (0x8000 - o.y) as f64 * M_PER_MAP, 5.0];
            launches.push(self.ai_missile(spec, from, [0.0, 0.0, 1.0], j, false));
        }
        // Guns pick a target in reach; MANPADS teams fire; Patriot radars track.
        for k in 0..self.aaa.len() {
            let u = self.aaa[k];
            let blue = c.aaa_blue[k];
            c.aaa_target[k] = None;
            // The missile teams reload here (the classic air defence only does it while it
            // fights a player).
            if (u.kind == AaaKind::Manpads && (blue || !u.tracking)) || u.kind.patriot() {
                let a = &mut self.aaa[k];
                a.cooldown = (a.cooldown - fs).max(0);
                if a.kind == AaaKind::PatriotLauncher && a.rounds == 0 && a.cooldown == 0 {
                    a.rounds = PATRIOT_ROUNDS;
                }
            }
            if u.destroyed || (!blue && u.tracking) || u.kind == AaaKind::PatriotLauncher {
                continue;
            }
            if u.kind == AaaKind::PatriotRadar {
                let track = near(u.x, u.y, !blue, PATRIOT_REACH, true, PATRIOT_FLOOR)
                    .filter(|&(j, d)| d <= horizon(pos[j][2]));
                c.aaa_target[k] = track.map(|t| t.0);
                let a = &mut self.aaa[k];
                a.tracking = track.is_some();
                if let Some((j, _)) = track {
                    let d = sub(pos[j], u.metres());
                    a.aim_az = d[0].atan2(d[1]) as f32;
                }
                continue;
            }
            let (_, reach, ceiling) = gun_envelope(u.kind);
            let reach = (reach / M_PER_MAP) as i32;
            let Some((j, _)) = near(u.x, u.y, !blue, reach, false, 15.0) else {
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
                launches.push(self.ai_missile(
                    if blue { STINGER } else { SA14 },
                    from,
                    dir,
                    j,
                    blue,
                ));
            }
        }
        // Patriot launchers fire on their battery radar's track.
        for k in 0..self.aaa.len() {
            let u = self.aaa[k];
            if u.kind != AaaKind::PatriotLauncher || u.destroyed {
                continue;
            }
            let blue = c.aaa_blue[k];
            let track = (0..self.aaa.len()).find_map(|r| {
                let radar = self.aaa[r];
                (radar.kind == AaaKind::PatriotRadar
                    && !radar.destroyed
                    && c.aaa_blue[r] == blue
                    && approx(radar.x - u.x, radar.y - u.y) < PATRIOT_LINK)
                    .then_some(c.aaa_target[r])
                    .flatten()
            });
            self.aaa[k].tracking = track.is_some();
            let Some(j) = track else { continue };
            let from = add(u.metres(), [0.0, 0.0, 3.0]);
            let d = sub(pos[j], from);
            let az = d[0].atan2(d[1]);
            self.aaa[k].aim_az = az as f32;
            let in_air = |m: &AiMissile| m.target == j && m.spec == PATRIOT && m.blue == blue;
            let salvo = c.missiles.iter().chain(&launches).filter(|m| in_air(m)).count();
            if u.rounds == 0 || u.cooldown > 0 || salvo >= PATRIOT_SALVO || rng.random(100) >= 50 {
                continue;
            }
            let a = &mut self.aaa[k];
            a.rounds -= 1;
            a.cooldown = if a.rounds == 0 { PATRIOT_RELOAD } else { PATRIOT_INTERVAL } * fs;
            let el = PATRIOT_ELEVATION as f64;
            let dir = [az.sin() * el.cos(), az.cos() * el.cos(), el.sin()];
            launches.push(self.ai_missile(PATRIOT, from, dir, j, blue));
        }
        // Campaign fighters go for the other side's aircraft and fire when in reach. A CAP
        // stays over its airfield and lands when there is nothing left to engage.
        for i in 0..c.flights.len() {
            let Some(mut f) = c.flights[i] else { continue };
            if f.role != Role::Fighter || sides[i].is_none() {
                continue;
            }
            let u = self.flights[i];
            f.cooldown = (f.cooldown - fs).max(0);
            let cap = f.task == FighterTask::Cap;
            let (ax, ay) = f.anchor;
            let leashed = cap && approx(u.x - ax, u.y - ay) > CAP_LEASH;
            let in_bubble = |j: usize| !cap || approx(xy[j].0 - ax, xy[j].1 - ay) < CAP_BUBBLE;
            f.target = (0..sides.len())
                .filter(|&j| !leashed && sides[j] == Some(!f.blue) && pos[j][2] > 0.0)
                .filter(|&j| in_bubble(j))
                .map(|j| (j, approx(xy[j].0 - u.x, xy[j].1 - u.y)))
                .filter(|&(_, d)| d < FIGHTER_SEARCH)
                .min_by_key(|&(_, d)| d)
                .map(|t| t.0);
            if cap {
                let landing = u.flags & (fflags::FINAL | fflags::ROLLOUT) != 0;
                if f.target.is_some() {
                    f.quiet = 0;
                    // Called back to the fight on its way home.
                    if !landing {
                        self.flights[i].flags &= !fflags::RTB;
                    }
                } else if u.flags & fflags::RTB == 0 {
                    f.quiet += 1;
                    if f.quiet >= CAP_LINGER || leashed && f.quiet >= CAP_LINGER / 2 {
                        self.flights[i].flags |= fflags::RTB;
                        self.flights[i].wp = f.home;
                    }
                }
            }
            if let Some(j) = f.target
                && f.cooldown == 0
            {
                let t = self.flights[j];
                let range = len(sub(pos[j], pos[i]));
                let bearing = crate::math::bearing(t.x - u.x, s16(u.y - t.y));
                let off = bam(crate::math::abs16(s16(bearing - u.heading)));
                let reach = |m: i32| self.missile(m).lock_range as f64 * 1000.0;
                if let Some(k) = f.loadout.choose(range, off, reach) {
                    let store = &mut f.loadout.stores[k];
                    store.count -= 1;
                    f.cooldown = FIGHTER_RELOAD * fs;
                    let dir = direction(u.heading, u.pitch);
                    launches.push(self.ai_missile(store.missile, pos[i], dir, j, f.blue));
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
            ground: None,
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
            if u.destroyed || u.kind == AaaKind::Manpads || u.kind.patriot() {
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
            let (a, e) = (az + jitter(&mut rng) * sigma, el + jitter(&mut rng) * sigma);
            let dir = [a.sin() * e.cos(), a.cos() * e.cos(), e.sin()];
            let ttl = if flak { time } else { reach * 1.3 / v0 };
            if c.shells.len() < MAX_SHELLS {
                let shell = Shell {
                    pos: add(from, [0.0, 0.0, 1.5]),
                    vel: mul(dir, v0),
                    ttl: ((ttl * fs as f64) as i32).max(1),
                    flak,
                    pilot: 0,
                };
                if self.record_shells {
                    self.new_ai_shells.push(shell);
                }
                c.shells.push(AiShell { shell, target: j });
            }
        }
        c.rng = rng;
        self.campaign = Some(c);
    }

    /// Fighters with their target close ahead fire bursts of their guns at it.
    fn campaign_fighter_guns(&mut self) {
        let fs = self.fs.max(1);
        let Some(mut c) = self.campaign.take() else { return };
        let mut rng = c.rng;
        let sigma = 0.006 * (1.6 - 0.2 * self.diff as f64);
        for i in 0..c.flights.len() {
            let Some(mut f) = c.flights[i] else { continue };
            if f.role != Role::Fighter {
                continue;
            }
            if f.burst < 0 {
                f.burst += 1;
                c.flights[i] = Some(f);
                continue;
            }
            let u = self.flights[i];
            let t = f.target.and_then(|j| self.flights.get(j).copied()).unwrap_or_default();
            let alive = u.flags & fflags::DESTROYED == 0
                && t.flags & fflags::DESTROYED == 0
                && t.flags & fflags::AIRBORNE != 0;
            let from = flight_metres(&u);
            let to = flight_metres(&t);
            let nose = direction(u.heading, u.pitch);
            let own = mul(nose, u.speed as f64 * 0.514);
            // Lead against the target's motion relative to the fighter.
            let rel = sub(mul(direction(t.heading, t.pitch), t.speed as f64 * 0.514), own);
            let (az, el, _) = lead(from, to, rel, FIGHTER_GUN_V0);
            let aim = [az.sin() * el.cos(), az.cos() * el.cos(), el.sin()];
            let cos = aim[0] * nose[0] + aim[1] * nose[1] + aim[2] * nose[2];
            let sight =
                alive && len(sub(to, from)) < FIGHTER_GUN_REACH && cos > FIGHTER_GUN_CONE.cos();
            if f.burst == 0 && sight {
                f.burst = fs + rng.random(fs);
            }
            if f.burst > 0 {
                let fire = sight && f.burst & 1 == 0;
                f.burst -= 1;
                if f.burst == 0 {
                    f.burst = -(fs * 2 + rng.random(fs * 2));
                }
                if fire && c.shells.len() < MAX_SHELLS {
                    let (a, e) = (az + jitter(&mut rng) * sigma, el + jitter(&mut rng) * sigma);
                    let dir = [a.sin() * e.cos(), a.cos() * e.cos(), e.sin()];
                    let ttl = FIGHTER_GUN_REACH * 1.5 / FIGHTER_GUN_V0;
                    let shell = Shell {
                        pos: add(from, mul(nose, 10.0)),
                        vel: add(mul(dir, FIGHTER_GUN_V0), own),
                        ttl: ((ttl * fs as f64) as i32).max(1),
                        flak: false,
                        pilot: 0,
                    };
                    if self.record_shells {
                        self.new_ai_shells.push(shell);
                    }
                    c.shells.push(AiShell { shell, target: f.target.unwrap_or(0) });
                }
            }
            c.flights[i] = Some(f);
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
        let mut ground_hits = Vec::new();
        c.missiles.retain_mut(|m| {
            m.ttl -= 1;
            // A strike aircraft's weapon flies at its object and hits it there.
            if let Some(g) = m.ground {
                let o = self.objects.get(g).copied().unwrap_or_default();
                let at = [o.x as f64 * M_PER_MAP, (0x8000 - o.y) as f64 * M_PER_MAP, 5.0];
                let v = len(m.vel).max(1.0);
                let speed = (v + m.speed * dt * 1.5).min(m.speed);
                let to = sub(at, m.pos);
                let k = (m.turn * dt * 4.0).min(1.0);
                let dir = add(mul(m.vel, (1.0 - k) / v), mul(to, k / len(to).max(1e-6)));
                let dir = mul(dir, 1.0 / len(dir).max(1e-9));
                let step = mul(dir, speed * dt);
                if crate::aaa::closest(sub(m.pos, at), step) < FUSE {
                    pops.push(metres_map(at));
                    if rng.random(1000) < GROUND_PK {
                        ground_hits.push(g);
                    }
                    return false;
                }
                m.vel = mul(dir, speed);
                m.pos = add(m.pos, step);
                return m.ttl > 0 && m.pos[2] > 0.0;
            }
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
        for g in ground_hits {
            self.remove_object(g);
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
        self.emit(SimEvent::AircraftDestroyed { unit: j, spec: u.spec, by: None });
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
    pub(crate) fn step_absent(&mut self) {
        if self.frame & 7 == 0 {
            self.update_base_traffic();
        }
        if self.frame & 0x7F == 0x40 && self.traffic_base >= 0 {
            self.spawn_traffic();
        }
        self.update_aircraft(&[]);
        self.update_campaign();
        self.update_effects();
        self.tick_frames += 1;
        if self.tick_frames >= self.fs {
            self.tick_frames = 0;
            self.mission_tick += 1;
        }
        self.frame = (self.frame + 1) & 0xFFFF;
        // A server sends the world by its steps (its players on the Ops screen see it move).
        self.steps = self.steps.wrapping_add(1);
    }
}

/// Unit vector (east, north, up) of a heading and pitch (BAM).
fn direction(heading: i32, pitch: i32) -> [f64; 3] {
    let (h, p) = (bam(heading), bam(pitch));
    [h.sin() * p.cos(), h.cos() * p.cos(), p.sin()]
}

/// A gun's aiming error: roughly normal, about unit spread.
fn jitter(rng: &mut Rng) -> f64 {
    (0..3).map(|_| rng.random(1000) as f64 / 1000.0).sum::<f64>() * 1.15 - 1.73
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
        AaaKind::Gepard => (1175.0, 3500.0, 3000.0),
        AaaKind::PatriotLauncher | AaaKind::PatriotRadar => (0.0, 0.0, 0.0),
    }
}

/// Metres (east, north, up) to map x, y and altitude (world units).
fn metres_map(p: [f64; 3]) -> (i32, i32, i32) {
    ((p[0] / M_PER_MAP) as i32, 0x8000 - (p[1] / M_PER_MAP) as i32, (p[2] / M_PER_WU) as i32)
}
