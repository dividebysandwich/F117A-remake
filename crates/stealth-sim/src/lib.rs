pub mod aaa;
pub mod ballistics;
pub mod campaign;
pub mod data;
pub mod math;
pub mod rng;
pub mod types;

mod ai;
pub mod csar;
mod damage;
pub mod drones;
pub mod guidance;
pub mod lights;
mod missiles;
mod objectives;
mod pilot;
mod radar;
pub mod score;
mod targeting;
mod tiles;
mod weapons;
mod weather;

pub use data::{
    AircraftKind, AircraftType, MissileType, SimData, TheaterTiles, ThreatType, WeaponType,
};
pub use pilot::{Pilot, PilotEvent};
pub use radar::emv;
pub use rng::Rng;
pub use types::*;

use math::{s16, view_z};
use serde::{Deserialize, Serialize};
use stealth_mission::Mission;

/// Number of projectile slots (0..7 enemy, 8..11 the active pilot's).
pub const N_PROJ: usize = 12;
/// Enemy missile slots (shared by all the pilots they go for) and each pilot's own.
pub const ENEMY_SLOTS: usize = 8;
pub const PILOT_SLOTS: usize = N_PROJ - ENEMY_SLOTS;
/// Pilots a simulation takes at most (coop players).
pub const MAX_PILOTS: usize = 8;

/// Player-derived values of the current frame (in the original game's units).
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct PlayerView {
    /// World units, y north.
    pub wx: i32,
    pub wy: i32,
    /// Map units, y south.
    pub mx: i32,
    pub my: i32,
    /// True altitude ft and compressed viewZ.
    pub alt: i32,
    pub view_z: i32,
    pub heading: i32,
    pub pitch: i32,
    pub roll: i32,
    /// knots * 27 and knots.
    pub velocity: i32,
    pub knots: i32,
    /// Climb rate (ft/s).
    #[serde(default)]
    pub climb: i32,
}

/// The complete simulation state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sim {
    // ---- static data ----
    pub(crate) threats: Vec<ThreatType>,
    /// Aircraft types, including the running kill counters.
    pub aircraft_types: Vec<AircraftType>,
    pub(crate) missile_types: Vec<MissileType>,
    pub(crate) weapon_types: Vec<WeaponType>,
    pub(crate) compat: Vec<[i8; 16]>,
    pub(crate) maneuver: Vec<i16>,
    pub(crate) trig: math::Trig,
    pub(crate) tiles: Option<TheaterTiles>,
    pub opts: SimOptions,

    // ---- mission settings ----
    pub(crate) diff: i32,
    pub(crate) roe: i32,
    pub(crate) night: bool,
    pub theater: usize,
    /// Runway direction of the theater (+1 / -1).
    pub(crate) ns: i32,

    pub(crate) rng: Rng,
    /// Frame counter (16-bit).
    pub(crate) frame: i32,
    /// Mission clock (+1 every fs frames).
    pub(crate) mission_tick: i32,
    pub(crate) tick_frames: i32,
    pub(crate) fs: i32,

    // ---- player ----
    /// The pilot whose turn it is (one per player; see `pilot.rs`).
    pub(crate) pl: Pilot,
    /// All the pilots by id; the active one's entry is a placeholder.
    #[serde(default)]
    pub(crate) parked: Vec<Pilot>,
    /// The pilot each enemy missile slot, flight unit, ground site and AAA unit goes for.
    #[serde(default)]
    pub(crate) proj_pilot: [u8; ENEMY_SLOTS],
    #[serde(default)]
    pub(crate) flight_pilot: Vec<u8>,
    #[serde(default)]
    pub(crate) site_pilot: Vec<u8>,
    #[serde(default)]
    pub(crate) aaa_pilot: Vec<u8>,
    /// The pilot the theater alarm is about.
    #[serde(default)]
    pub(crate) alarm_pilot: u8,
    /// The pilot whose weapon is being flown (kills are his).
    #[serde(skip)]
    pub(crate) credit: Option<u8>,

    // ---- world ----
    pub(crate) objects: Vec<GroundObject>,
    pub(crate) ground_units: usize,
    pub(crate) woc: usize,
    pub(crate) buf7: Vec<u8>,
    pub(crate) buf8: Vec<u8>,
    pub(crate) grid: Vec<u8>,
    pub(crate) names: Vec<String>,
    pub(crate) land_rubble: i32,
    pub(crate) water_rubble: i32,
    /// Shape to draw for each world object (changes when destroyed).
    pub(crate) display_shape: Vec<i32>,
    pub(crate) flights: Vec<FlightUnit>,
    /// Flight unit the escorts follow, -1 none.
    pub(crate) escort_leader: i32,
    pub(crate) proj: [Projectile; N_PROJ],
    pub(crate) bullets: Vec<Bullet>,
    /// Number of player gun-round slots (enemy rounds follow them).
    pub(crate) n_bullets: usize,
    pub(crate) smoke: [Smoke; 16],
    /// Smoke source object (-1 aircraft / none).
    pub(crate) smoke_src: i32,
    pub(crate) wreck: Wreck,
    pub(crate) flash: Flash,
    pub(crate) tile_damage: Vec<TileDamage>,

    // ---- threat state ----
    /// Theater alarm timer, frames left.
    pub(crate) alarm_timer: i32,
    /// Alarm seen in the current 128-frame window / the previous one.
    pub(crate) alarm_window: i32,
    pub(crate) alarm_last: i32,
    /// Where the defences think the player is: map position, viewZ, heading.
    pub(crate) ref_x: i32,
    pub(crate) ref_y: i32,
    pub(crate) ref_alt: i32,
    pub(crate) ref_heading: i32,
    pub(crate) traffic_base: i32,
    pub(crate) traffic_toggle: bool,
    /// Flight units of the mission (its last 4 are the base traffic); units added later
    /// (drones, rescue helicopters) follow them.
    #[serde(default)]
    pub(crate) mission_flights: usize,
    /// Attack drones of the mission (their flight units follow the mission's own).
    #[serde(default)]
    pub(crate) drones: Option<drones::Drones>,
    /// Rescue of the pilot after an ejection over enemy territory.
    #[serde(default)]
    pub(crate) csar: Option<csar::Csar>,
    /// Map positions of everything destroyed on the ground (campaign damage).
    #[serde(default)]
    pub(crate) destroyed_at: Vec<(i32, i32)>,
    /// Light for visual identification, 0 (moonless night) .. 255 (day), from the renderer's
    /// continuous time of day; None: the mission's day or night, as in the original.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) visual_light: Option<u8>,
    /// Low-level air defences (`SimOptions::air_defence`), their shells and random stream.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) aaa: Vec<aaa::AaaUnit>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) aaa_shells: Vec<aaa::Shell>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) aaa_rng: Option<Rng>,
    /// Apron parking spots for the parked base traffic (not in the original): world x, y and
    /// heading; empty = parked on the base object as in the original.
    #[serde(default)]
    pub(crate) parking: Vec<(i32, i32, i32)>,
    /// Realtime dynamic campaign (`SimOptions::campaign`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) campaign: Option<Box<campaign::CampaignSim>>,
    /// Per-frame seeker acquisition results (range / aim bearing).
    pub(crate) acq_range: i32,
    pub(crate) acq_aim: i32,

    // ---- targeting ----

    // ---- objectives / end ----
    pub(crate) ended: Option<MissionEnd>,
    /// Clouds (not in the original; clear = original behaviour).
    #[serde(default)]
    pub(crate) weather: stealth_mission::Weather,

    #[serde(skip)]
    pub(crate) events: Vec<SimEvent>,
    /// Per event: the active pilot when it happened.
    #[serde(skip)]
    pub(crate) event_pilot: Vec<u8>,
    /// Per event: the pilot whose weapon caused it.
    #[serde(skip)]
    pub(crate) event_credit: Vec<Option<u8>>,
}

impl Sim {
    /// Build the runtime world from a generated mission (the mission-generator hand-off).
    pub fn new(data: &SimData, m: &Mission) -> Self {
        Self::with_options(data, m, SimOptions::default())
    }

    pub fn with_options(data: &SimData, m: &Mission, opts: SimOptions) -> Self {
        let s = &m.settings;
        let w = &m.world;
        let objects: Vec<GroundObject> = w
            .objects
            .iter()
            .map(|o| GroundObject {
                name: o.name as i32,
                x: o.x as u16 as i32,
                y: o.y as u16 as i32,
                unit_type: o.unit_type as i32,
                flags: o.flags as u16 as i32,
                alert: o.occupant_type as i32,
                timer: o.patrol_count as i32,
                shape: o.object_idx as i32,
            })
            .collect();
        let flights: Vec<FlightUnit> = w
            .flights
            .iter()
            .map(|f| FlightUnit {
                wp: f.waypoint as i32,
                x: f.x as u16 as i32,
                y: f.y as u16 as i32,
                alt: f.alt as i32,
                wx: f.xp,
                wy: f.yp,
                heading: f.heading as i32,
                pitch: f.pitch as i32,
                bank: f.roll as i32,
                spec: f.plane_type as i32,
                flags: f.flags as u16 as i32,
                speed: f.speed as i32,
                timer: f.fuel as i32,
                missile: f.reserved[0] as i32,
                reserved: f.reserved[1] as i32,
                heat: f.reserved[2] as i32,
                mode: 0,
            })
            .collect();
        let lockheed = s.plane == stealth_mission::Plane::Lockheed;
        let n_bays = if lockheed { 2 } else { 4 };
        let bays =
            m.loadout.bays.iter().take(n_bays).map(|b| (b.weapon as i32, b.count as i32)).collect();
        let display_shape = objects.iter().map(|o| o.shape).collect();
        let theater = s.theater;
        let seed = opts.seed.unwrap_or(m.rng.holdrand_exit);
        let external_stores = opts.external_stores;
        let mut sim = Self {
            threats: data.threats.clone(),
            aircraft_types: data.aircraft.clone(),
            missile_types: data.missiles.clone(),
            weapon_types: data.weapons.clone(),
            compat: data.compat.clone(),
            maneuver: data.maneuver.clone(),
            trig: data.trig.clone(),
            tiles: data.theaters.get(theater).cloned(),
            opts,
            diff: s.opponents.min(3) as i32,
            roe: s.roe.min(2) as i32,
            night: m.night,
            theater,
            // Remake theaters use the original airbase models, laid out like Central Europe's.
            ns: data.start_dir.get(theater).copied().unwrap_or(1) as i32,
            rng: Rng::new(seed),
            frame: 0,
            mission_tick: 0,
            tick_frames: 0,
            fs: 15,
            parked: vec![Pilot::default()],
            proj_pilot: [0; ENEMY_SLOTS],
            flight_pilot: Vec::new(),
            site_pilot: Vec::new(),
            aaa_pilot: Vec::new(),
            alarm_pilot: 0,
            credit: None,
            pl: Pilot {
                id: 0,
                absent: false,
                external_stores,
                scratch0: GroundObject::default(),
                rounds: Vec::new(),
                missiles: Default::default(),
                last_input: PlayerState::default(),
                lockheed,
                p: PlayerView::default(),
                pflags: if m.invulnerable { pflags::TRAINING } else { 0 },
                emv: 0,
                damage: 0,
                hits: 2,
                control_fault: ControlFault::default(),
                fire_frames: 0,
                breakup_frames: 0,
                irj_heat: 0,
                door_anim: 0,
                bay_req: false,
                bays,
                cur_bay: 0,
                gun_selected: false,
                stores: [
                    data.stores_init[0] as i32,
                    data.stores_init[1] as i32,
                    data.stores_init[2] as i32,
                    data.stores_init[3] as i32,
                ],
                gun_ammo: data.gun_ammo_init as i32,
                gun_firing: false,
                fire_cooldown: 0,
                prev_fire: false,
                ccrp_last: None,
                prev_hud_mode: HudMode::Nav,
                hud_mode: HudMode::Nav,
                camera_mfd: false,
                external_view: false,
                prev_on_ground: true,
                on_ground: true,
                cms: [Countermeasure::default(); 4],
                last_tile: None,
                sweep_anim: -1,
                threat_count_last: 0,
                threat_count: 0,
                tracking: 0,
                radar_ids: 0,
                visual_ids: 0,
                nearest_base: -1,
                nearest_base_dist: 0x7FFF,
                aaa_track: None,
                chaff_lamp: 0,
                flare_lamp: 0,
                emv_lamp: 0,
                egr: None,
                ear: None,
                arc: None,
                lights: Lights::default(),
                launches: 0,
                air_lock: -1,
                gnd_lock: -1,
                targeting: TargetingState::default(),
                gnd_cooldown: 0,
                nearest_req: 0,
                target_hold: false,
                cam_dir: 0,
                last_missile_slot: -1,
                maverick_view: false,
                targets: [m.primary.clone(), m.secondary.clone()].map(|t| TargetSlot {
                    mission_type: t.mission_type as i32,
                    object: t.object_index as i32,
                    base: t.base_index as i32,
                    code: t.code as i32,
                }),
                waypoint: 0,
                deadline: m.time.deadline_ticks as i32,
                cue_timers: [0; 2],
                photo_timer: 0,
                photo_frames: 0,
                deadline_done: [false; 2],
                time_expired_shown: false,
                target_escaped: false,
                corridor: true,
                armed_landing: 0,
                eject: 0,
                chute: (0, 0, 0),
                landing_type: 1,
                log: Vec::new(),
            },
            weather: m.weather,
            ground_units: (w.ground_unit_count as usize).min(objects.len()),
            woc: (w.world_object_count as usize).min(objects.len()),
            objects,
            buf7: w.buf7.clone(),
            buf8: w.buf8.clone(),
            grid: w.grid.clone(),
            names: w.names.clone(),
            land_rubble: (w.header & 0xFF) as i32,
            water_rubble: (w.header >> 8) as i32,
            display_shape,
            flights,
            escort_leader: m.escort as i32,
            proj: [Projectile::default(); N_PROJ],
            bullets: Vec::new(),
            n_bullets: 0,
            smoke: [Smoke::default(); 16],
            smoke_src: -1,
            wreck: Wreck::default(),
            flash: Flash::default(),
            tile_damage: Vec::new(),
            alarm_timer: 0,
            alarm_window: 0,
            alarm_last: 0,
            ref_x: 0,
            ref_y: 0,
            ref_alt: 0,
            ref_heading: 0,
            traffic_base: -1,
            traffic_toggle: false,
            mission_flights: w.flights.len()
                - m.drones.as_ref().map_or(0, |d| d.count.min(w.flights.len())),
            csar: None,
            destroyed_at: Vec::new(),
            visual_light: None,
            aaa: Vec::new(),
            aaa_shells: Vec::new(),
            aaa_rng: None,
            drones: m.drones.as_ref().map(|d| drones::Drones {
                defend: d.defend,
                target: d.target as i32,
                nuclear: d.nuclear,
                first: d.first_unit,
                count: d.count,
                allowance: d.allowance as i32,
                ..Default::default()
            }),
            parking: Vec::new(),
            campaign: None,
            acq_range: 0,
            acq_aim: 0,
            ended: None,
            events: Vec::new(),
            event_pilot: Vec::new(),
            event_credit: Vec::new(),
        };
        sim.set_fs(15);
        if let Some(d) = sim.opts.prior_damage.clone() {
            sim.apply_damage(&d);
        }
        if sim.pl.external_stores {
            sim.pl.bay_req = true;
            sim.pl.door_anim = 2;
            sim.pl.pflags |= pflags::BAY_OPEN;
        }
        if sim.opts.campaign {
            let st = data.stores_init.map(|v| v as i32);
            sim.init_campaign(st, data.gun_ammo_init as i32);
        } else if sim.opts.air_defence {
            sim.place_aaa(m);
        }
        let st = sim.start_state();
        sim.ingest(&st);
        sim.find_nearest_base();
        sim.log_event(8, 0);
        sim.events.clear();
        sim.event_pilot.clear();
        sim.event_credit.clear();
        sim
    }

    /// The player's start state: at the takeoff base, on the ground.
    pub fn start_state(&self) -> PlayerState {
        let b = self.pl.targets[0].base.clamp(0, self.objects.len() as i32 - 1).max(0) as usize;
        self.start_state_at(b)
    }

    /// The start state on the runway (or carrier deck) of base object `b`.
    pub fn start_state_at(&self, b: usize) -> PlayerState {
        let o = self.objects.get(b).copied().unwrap_or_default();
        let t = self.ns;
        let mut x = (o.x << 5) + 2;
        let mut y = (0x8000 - o.y) << 5;
        let mut heading = if t == 1 { 0 } else { 0x8000 };
        let carrier = o.flags & oflags::CARRIER != 0;
        if carrier {
            x -= t << 7;
            heading += 0x400;
        } else {
            y -= t * 0x708;
        }
        PlayerState {
            x,
            y,
            altitude: if carrier { 0x80 } else { 0 },
            heading: heading as u16,
            on_ground: true,
            fuel: 10000,
            hud_visible: true,
            ..Default::default()
        }
    }

    /// Update the fs-derived constants.
    fn set_fs(&mut self, fs: i32) {
        let fs = fs.clamp(1, 60);
        self.fs = fs;
        let nb = (2 * fs + 2).clamp(3, 64) as usize;
        if nb != self.n_bullets {
            self.n_bullets = nb;
            self.bullets = vec![Bullet::default(); nb + 4];
        }
    }

    pub(crate) fn alarm_init(&self) -> i32 {
        250 * self.fs
    }

    pub(crate) fn alarm_recent(&self) -> i32 {
        200 * self.fs
    }

    pub(crate) fn decoy_active(&self) -> bool {
        self.pl.cms[0].ttl > 0
    }

    pub(crate) fn ecm_on(&self) -> bool {
        self.pl.pflags & pflags::ECM != 0
    }

    pub(crate) fn irj_on(&self) -> bool {
        self.pl.pflags & pflags::IRJ != 0
    }

    pub(crate) fn training(&self) -> bool {
        self.pl.pflags & pflags::TRAINING != 0
    }

    pub(crate) fn emit(&mut self, e: SimEvent) {
        self.events.push(e);
        self.event_pilot.push(self.pl.id);
        self.event_credit.push(self.credit);
    }

    pub(crate) fn msg(&mut self, s: impl Into<String>) {
        self.emit(SimEvent::Message(s.into()));
    }

    pub(crate) fn sound(&mut self, id: u8, priority: u8) {
        self.emit(SimEvent::Sound { id, priority });
    }

    /// Append an event to the debrief log.
    pub(crate) fn log_event(&mut self, kind: i32, arg: i32) {
        let e = LogEvent {
            tick: self.mission_tick,
            x: (self.pl.p.mx >> 7) as u8,
            y: (self.pl.p.my >> 7) as u8,
            kind: kind as u8,
            arg: arg as u8,
        };
        self.pl.log.push(e);
        self.emit(SimEvent::Log(e));
    }

    /// WLD name of object i.
    pub fn place_name(&self, i: usize) -> String {
        let o = match self.objects.get(i) {
            Some(o) => *o,
            None => return String::new(),
        };
        let idx =
            if i == 0 || o.shape & 0x100 != 0 && o.name == 0 { o.shape & 0x7F } else { o.name };
        self.names.get(idx.max(0) as usize).cloned().unwrap_or_default()
    }

    pub(crate) fn obj(&self, i: i32) -> GroundObject {
        if i >= 0 {
            self.objects.get(i as usize).copied().unwrap_or_default()
        } else {
            GroundObject::default()
        }
    }

    /// Weapon category of an object (`buf7[shape & 0x7F] & 0xF`).
    pub(crate) fn category(&self, i: i32) -> i32 {
        let o = self.obj(i);
        self.buf7.get((o.shape & 0x7F) as usize).map_or(0, |&b| (b & 0xF) as i32)
    }

    /// Effectiveness of weapon `w` against object `obj`.
    pub(crate) fn compat(&self, w: i32, obj: i32) -> i32 {
        let cat = self.category(obj);
        self.compat.get(w.max(0) as usize).map_or(0, |r| r[cat as usize] as i32)
    }

    pub(crate) fn is_water(&self, i: i32) -> bool {
        matches!(self.category(i), 9 | 11 | 12 | 13)
    }

    pub(crate) fn is_ship(&self, i: i32) -> bool {
        matches!(self.category(i), 12 | 13)
    }

    pub(crate) fn missile(&self, spec: i32) -> &MissileType {
        &self.missile_types[(spec.max(0) as usize).min(self.missile_types.len() - 1)]
    }

    pub(crate) fn ac_type(&self, spec: i32) -> &AircraftType {
        &self.aircraft_types[(spec.max(0) as usize).min(self.aircraft_types.len() - 1)]
    }

    /// Sets the apron parking spots (world x, y, heading) and re-places the base traffic.
    pub fn set_parking(&mut self, spots: Vec<(i32, i32, i32)>) {
        if self.parking != spots {
            self.parking = spots;
            self.traffic_base = -1;
        }
    }

    pub fn has_parking(&self) -> bool {
        !self.parking.is_empty()
    }

    /// How aircraft spec `spec` flies (plane, helicopter, drone).
    pub fn aircraft_kind(&self, spec: i32) -> AircraftKind {
        self.ac_type(spec).kind
    }

    /// Whether aircraft spec `spec` is friendly (radar type -1).
    pub fn friendly_aircraft(&self, spec: i32) -> bool {
        self.ac_type(spec).radar == -1
    }

    pub(crate) fn threat_type(&self, t: i32) -> Option<&ThreatType> {
        if t <= 0 { None } else { self.threats.get(t as usize) }
    }

    /// Missile type fired by a site of unit type `t`.
    pub fn site_missile(&self, t: i32) -> i32 {
        self.threat_type(t).and_then(|th| th.missile).map_or(t, i32::from)
    }

    /// Copy the flight model's state into the per-frame player view and flags.
    fn ingest(&mut self, ps: &PlayerState) {
        let alt = ps.altitude as i32;
        self.pl.p = PlayerView {
            wx: ps.x,
            wy: ps.y,
            mx: (ps.x >> 5) & 0xFFFF,
            my: (0x8000 - (ps.y >> 5)) & 0xFFFF,
            alt,
            view_z: view_z(ps.altitude),
            heading: s16(ps.heading as i32),
            pitch: ps.pitch as i32,
            roll: ps.roll as i32,
            velocity: ps.velocity as i32,
            knots: ps.velocity as i32 / 27,
            climb: ps.climb_rate,
        };
        let keep = pflags::BAY_OPEN
            | pflags::ECM
            | pflags::IRJ
            | pflags::DESIGNATE
            | pflags::CAMERA_VIEW
            | pflags::TRAINING
            | pflags::PRIMARY_DONE
            | pflags::SECONDARY_DONE;
        let mut f = self.pl.pflags & keep;
        if ps.gear_up {
            f |= pflags::GEAR_UP;
        }
        if ps.flaps {
            f |= pflags::FLAPS;
        }
        if ps.brakes {
            f |= pflags::BRAKES;
        }
        self.pl.pflags = f;
        self.pl.prev_on_ground = self.pl.on_ground;
        self.pl.on_ground = ps.on_ground;
        self.pl.prev_hud_mode = self.pl.hud_mode;
        self.pl.hud_mode = ps.hud_mode;
        self.pl.camera_mfd = ps.camera_mfd;
        self.pl.external_view = ps.external_view;
    }

    /// Advance one frame. `fs` = nominal frames per sim second (normally 15).
    pub fn step(&mut self, ps: &PlayerState, fs: u8) -> Vec<SimEvent> {
        let id = self.pl.id;
        self.step_pilots(&[(id, ps.clone())], fs).into_iter().map(|e| e.event).collect()
    }

    /// Advance one frame with every pilot in the world: `inputs` are their flight models'
    /// reports (a pilot without one this frame flies on with his last). Each pilot takes his
    /// turn in each phase, so with one pilot this is the original's frame.
    pub fn step_pilots(&mut self, inputs: &[(u8, PlayerState)], fs: u8) -> Vec<PilotEvent> {
        self.events.clear();
        self.event_pilot.clear();
        self.event_credit.clear();
        if self.ended.is_some() {
            return Vec::new();
        }
        if fs as i32 != self.fs {
            self.set_fs(fs as i32);
        }
        let present = self.present();
        if present.is_empty() {
            self.step_absent();
            return self.take_pilot_events();
        }
        let first = present[0];
        let input = |sim: &Sim, k: usize| -> PlayerState {
            match inputs.iter().find(|(id, _)| *id as usize == k) {
                Some((_, ps)) => ps.clone(),
                None => PlayerState { cmd: Commands::default(), ..sim.pl.last_input.clone() },
            }
        };
        for &k in &present {
            self.activate(k);
            let ps = input(self, k);
            self.ingest(&ps);
            // Key commands and per-frame systems.
            self.handle_keys(&ps);
            self.update_systems();
            self.handle_fire(&ps);
            // EMV every frame.
            self.compute_emv();
            self.pl.last_input = ps;
        }
        if self.frame & 7 == 0 {
            for &k in &present {
                self.activate(k);
                self.find_nearest_base();
            }
            self.activate(first);
            self.update_base_traffic();
        }
        if self.frame & 0x7F == 0 {
            self.alarm_last = self.alarm_window;
            self.alarm_window = 0;
        }
        if self.fs > 0 && self.frame % (4 * self.fs) == 0 {
            for &k in &present {
                self.activate(k);
                self.rebuild_tracking();
            }
        }
        self.activate(first);
        if self.frame & 0x7F == 0x40 && self.traffic_base >= 0 {
            self.spawn_traffic();
        }
        // Sites, aircraft, projectiles, countermeasures, gun rounds, lamps.
        self.update_sites(&present);
        if self.aaa_rng.is_some() {
            self.update_aaa(&present);
        }
        self.update_aircraft(&present);
        if self.campaign.is_some() {
            self.activate(first);
            self.update_campaign();
        }
        self.update_missiles(&present);
        for &k in &present {
            self.activate(k);
            self.update_cm_timers();
        }
        for &k in &present {
            self.activate(k);
            let ps = self.pl.last_input.clone();
            self.update_own_rounds(&ps);
        }
        self.update_enemy_rounds(&present);
        if self.aaa_rng.is_some() {
            self.aaa_damage();
        }
        for &k in &present {
            self.activate(k);
            self.update_light_timers();
            self.update_damage();
        }
        if self.alarm_timer > 0 {
            self.alarm_timer -= 1;
        }
        // Draw-list pass and HUD overlay.
        for &k in &present {
            self.activate(k);
            let ps = self.pl.last_input.clone();
            self.update_targeting(&ps);
        }
        self.activate(first);
        self.update_effects();
        for &k in &present {
            self.activate(k);
            let ps = self.pl.last_input.clone();
            self.update_objectives(&ps);
        }
        // Clock.
        self.tick_frames += 1;
        if self.tick_frames >= self.fs {
            self.tick_frames = 0;
            self.mission_tick += 1;
        }
        self.frame = (self.frame + 1) & 0xFFFF;
        self.activate(0);
        self.take_pilot_events()
    }

    /// The events so far with their pilots.
    pub fn take_pilot_events(&mut self) -> Vec<PilotEvent> {
        let pilots = std::mem::take(&mut self.event_pilot);
        self.event_credit.clear();
        std::mem::take(&mut self.events)
            .into_iter()
            .enumerate()
            .map(|(i, event)| PilotEvent { pilot: pilots.get(i).copied().unwrap_or(0), event })
            .collect()
    }

    // ---------------------------------------------------------------- accessors

    /// Player EMV.
    pub fn emv(&self) -> i32 {
        self.pl.emv
    }

    /// EMV gauge bar lengths: (blue from the left, red from the right), rows 187..197.
    pub fn emv_gauge(&self) -> (i32, i32) {
        let e = self.pl.emv.clamp(0, 99);
        ((100 - e) / 2, e / 2)
    }

    /// Ground (EGR) and air (EAR) signal bars of the sweep being displayed.
    pub fn signal_bars(&self) -> (Option<SignalBar>, Option<SignalBar>) {
        (self.pl.egr, self.pl.ear)
    }

    /// MAP radar arc of the sweep being displayed.
    pub fn radar_arc(&self) -> Option<RadarArc> {
        self.pl.arc
    }

    /// Missile launches against the player.
    pub fn launches(&self) -> i32 {
        self.pl.launches
    }

    pub fn lights(&self) -> Lights {
        self.pl.lights
    }

    /// World objects (sites, bases, ships, targets) with their live state.
    pub fn objects(&self) -> &[GroundObject] {
        &self.objects
    }

    /// Shape to draw for each world object (`| 0x100` = the model's wreck state).
    pub fn object_shapes(&self) -> &[i32] {
        &self.display_shape
    }

    /// World object `i` is a ship.
    pub fn object_is_ship(&self, i: usize) -> bool {
        self.is_ship(i as i32)
    }

    /// Number of radar/SAM ground units (objects 0..n processed by the site loop).
    pub fn ground_unit_count(&self) -> usize {
        self.ground_units
    }

    /// Site alert 0..255 (sites only).
    pub fn site_alert(&self, i: usize) -> i32 {
        self.objects.get(i).map_or(0, |o| if i < self.ground_units { o.alert } else { 0 })
    }

    /// Detection score a site would produce against the player right now (no side effects).
    pub fn site_score(&self, i: usize) -> i32 {
        let o = self.obj(i as i32);
        if o.flags & oflags::DESTROYED != 0 {
            return 0;
        }
        self.detect_pure(o.x, o.y, 0, o.unit_type).0
    }

    pub fn flights(&self) -> &[FlightUnit] {
        &self.flights
    }

    /// Active aircraft for drawing: (unit index, record, STFLT model index).
    pub fn active_aircraft(&self) -> Vec<(usize, FlightUnit, i32)> {
        self.flights
            .iter()
            .enumerate()
            .filter(|(_, u)| {
                u.flags & fflags::EXISTS != 0 && (u.flags & fflags::AIRBORNE != 0 || u.speed == 0)
            })
            .map(|(i, u)| (i, *u, self.ac_type(u.spec).model as i32))
            .collect()
    }

    pub fn projectiles(&self) -> &[Projectile; N_PROJ] {
        &self.proj
    }

    /// Live projectiles: (slot, record, model, class).
    pub fn active_projectiles(&self) -> Vec<(usize, Projectile, i32, i32)> {
        self.proj
            .iter()
            .enumerate()
            .filter(|(_, m)| m.ttl != 0)
            .map(|(s, m)| {
                let t = self.missile(m.spec);
                (s, *m, t.model as i32, t.class as i32)
            })
            .collect()
    }

    /// Gun rounds (0..n player, n..n+3 enemy); x == 0 = free.
    pub fn bullets(&self) -> (&[Bullet], usize) {
        (&self.bullets, self.n_bullets)
    }

    pub fn countermeasures(&self) -> &[Countermeasure; 4] {
        &self.pl.cms
    }

    /// Flare, chaff, decoy stores left.
    pub fn stores(&self) -> (i32, i32, i32) {
        (self.pl.stores[1], self.pl.stores[2], self.pl.stores[3])
    }

    pub fn gun_ammo(&self) -> i32 {
        self.pl.gun_ammo
    }

    pub fn gun_firing(&self) -> bool {
        self.pl.gun_firing
    }

    /// The aircraft has a cannon (the real F-117A has none).
    pub fn has_gun(&self) -> bool {
        !self.pl.lockheed
    }

    /// The cannon is the selected weapon.
    pub fn gun_selected(&self) -> bool {
        self.pl.gun_selected
    }

    /// Bays or stations (weapon index, count; -1 empty) and the selected one.
    pub fn bays(&self) -> (&[(i32, i32)], usize) {
        (&self.pl.bays, self.pl.cur_bay)
    }

    /// The weapons carried (weapon index, rounds on all stations), in station order, and the
    /// selected one's index in this list (None: nothing carried).
    pub fn weapon_groups(&self) -> (Vec<(i32, i32)>, Option<usize>) {
        let mut groups: Vec<(i32, i32)> = Vec::new();
        let mut selected = None;
        for (i, &(w, n)) in self.pl.bays.iter().enumerate() {
            if w < 0 {
                continue;
            }
            match groups.iter().position(|g| g.0 == w) {
                Some(k) => groups[k].1 += n,
                None => {
                    if i == self.pl.cur_bay && !self.pl.gun_selected {
                        selected = Some(groups.len());
                    }
                    groups.push((w, n));
                }
            }
        }
        (groups, selected)
    }

    /// Replaces the mission's bays with these stations (weapon index, count; -1 empty), e.g.
    /// the player's loadout or an aircraft's pylons.
    pub fn set_loadout(&mut self, stations: &[(i32, i32)]) {
        self.pl.bays =
            stations.iter().map(|&(w, n)| if w < 0 { (-1, 0) } else { (w, n.max(0)) }).collect();
        self.pl.cur_bay = self.pl.bays.iter().position(|b| b.0 >= 0).unwrap_or(0);
        self.pl.gun_selected = self.has_gun() && !self.pl.bays.iter().any(|b| b.0 >= 0);
    }

    pub fn weapon_types(&self) -> &[WeaponType] {
        &self.weapon_types
    }

    pub fn bay_doors_open(&self) -> bool {
        self.pl.pflags & pflags::BAY_OPEN != 0
    }

    /// Door animation 0 (closed) .. 2 (open).
    pub fn bay_door_anim(&self) -> i32 {
        self.pl.door_anim
    }

    pub fn ecm(&self) -> bool {
        self.ecm_on()
    }

    pub fn ir_jammer(&self) -> bool {
        self.irj_on()
    }

    pub fn smoke(&self) -> &[Smoke; 16] {
        &self.smoke
    }

    pub fn wreck(&self) -> Wreck {
        self.wreck
    }

    pub fn tile_damage(&self) -> &[TileDamage] {
        &self.tile_damage
    }

    pub fn targeting(&self) -> TargetingState {
        self.pl.targeting
    }

    /// The right MFD should show the Maverick seeker of this slot.
    pub fn maverick_view_slot(&self) -> Option<usize> {
        let s = self.pl.last_missile_slot;
        (self.pl.maverick_view
            && s > 8
            && (s as usize) < N_PROJ
            && self.proj[s as usize].ttl != 0
            && self.proj[s as usize].spec == 27)
            .then_some(s as usize)
    }

    pub fn objectives(&self) -> Objectives {
        Objectives {
            primary_done: self.pl.pflags & pflags::PRIMARY_DONE != 0,
            secondary_done: self.pl.pflags & pflags::SECONDARY_DONE != 0,
            waypoint: self.pl.waypoint,
            tick: self.mission_tick,
            deadline: self.pl.deadline,
            target_escaped: self.pl.target_escaped,
        }
    }

    pub fn damage_effects(&self) -> DamageEffects {
        let d = self.pl.damage;
        DamageEffects {
            bits: d,
            hits: self.pl.hits,
            throttle_cap: if self.pl.breakup_frames > 0 {
                Some(0)
            } else {
                (d & dmg::ENG != 0).then(|| ((25 - self.pl.hits) * 4).max(0))
            },
            stick_jitter: if d & dmg::FC != 0 { (self.pl.hits * 4).clamp(0, 32) } else { 0 },
            autopilot_failed: d & dmg::AV != 0,
            doors_jammed: d & dmg::BD != 0,
            fuel_leak: if d & dmg::FIRE != 0 { self.pl.hits } else { 0 },
            jammers_failed: d & dmg::JAM != 0,
            heavy_stall: self.pl.hits > 8,
            fragile: self.pl.hits > 4,
            controls: self.pl.control_fault,
            destroyed: self.pl.breakup_frames > 0,
        }
    }

    /// Everything destroyed on the ground so far, earlier missions included.
    pub fn world_damage(&self) -> WorldDamage {
        WorldDamage {
            tiles: self.tile_damage.clone(),
            objects: (3..self.objects.len())
                .filter(|&i| self.objects[i].flags & oflags::DESTROYED != 0)
                .map(|i| i as u16)
                .collect(),
            positions: self.destroyed_at.clone(),
        }
    }

    /// Starts the mission with earlier missions' damage.
    fn apply_damage(&mut self, d: &WorldDamage) {
        self.tile_damage = d.tiles.clone();
        for &i in &d.objects {
            let i = i as usize;
            if (3..self.objects.len()).contains(&i) {
                self.objects[i].flags |= oflags::DESTROYED;
                self.objects[i].unit_type = 0;
                self.display_shape[i] = self.wreck_shape(i as i32);
            }
        }
        self.destroyed_at = d.positions.clone();
    }

    /// Events raised outside a step (by the campaign's calls), handed over once.
    pub fn take_events(&mut self) -> Vec<SimEvent> {
        self.event_pilot.clear();
        self.event_credit.clear();
        std::mem::take(&mut self.events)
    }

    /// Debrief event log (for END).
    pub fn event_log(&self) -> &[LogEvent] {
        &self.pl.log
    }

    pub fn mission_end(&self) -> Option<MissionEnd> {
        self.ended
    }

    /// Theater alarm timer (frames) and whether it is fresh (< 50 s old).
    pub fn alarm_state(&self) -> (i32, bool) {
        (self.alarm_timer, self.alarm_timer > self.alarm_recent())
    }

    /// Where the defences think the player is (map x, map y, viewZ).
    pub fn threat_reference(&self) -> (i32, i32, i32) {
        (self.ref_x, self.ref_y, self.ref_alt)
    }

    /// Radar IDs / visual IDs held against the player and the TRAK count.
    pub fn identification(&self) -> (i32, i32, i32) {
        (self.pl.radar_ids, self.pl.visual_ids, self.pl.tracking)
    }

    /// Parachute position (map x, map y, alt) while ejected.
    pub fn chute(&self) -> Option<(i32, i32, i32)> {
        (self.pl.eject != 0).then_some(self.pl.chute)
    }

    pub fn frame(&self) -> i32 {
        self.frame
    }

    pub fn mission_tick(&self) -> i32 {
        self.mission_tick
    }

    /// Sets the light for visual identification ([`Sim::visual_range`]); None: the mission's
    /// day or night.
    pub fn set_visual_light(&mut self, light: Option<u8>) {
        self.visual_light = light;
    }

    /// Range (km) at which enemy pilots identify the player by sight: 8 km by day, 4 by
    /// night (the original), or with a light level from 2 km on a moonless night to 8 by day;
    /// halved in a storm.
    pub fn visual_range(&self) -> i32 {
        let base = match self.visual_light {
            None => 8 >> self.night as i32,
            Some(l) => 2 + l as i32 * 6 / 255,
        };
        base >> self.visual_shift()
    }

    pub fn player(&self) -> PlayerView {
        self.pl.p
    }

    pub fn invulnerable(&self) -> bool {
        self.training()
    }

    /// Convert a map position to player-style world units (x east, y north).
    pub fn map_to_world(x: i32, y: i32) -> (i32, i32) {
        (x << 5, (0x8000 - y) << 5)
    }

    /// Test / tool hook: mutable access to the world objects.
    pub fn objects_mut(&mut self) -> &mut Vec<GroundObject> {
        &mut self.objects
    }

    /// Test / tool hook: mutable access to the flight units.
    pub fn flights_mut(&mut self) -> &mut Vec<FlightUnit> {
        &mut self.flights
    }

    /// Test / tool hook: set the damage bits and hit counter.
    pub fn set_damage(&mut self, bits: u8, hits: i32) {
        self.pl.damage = bits;
        self.pl.hits = hits;
    }

    /// Test / tool hook: load a bay.
    pub fn set_bay(&mut self, bay: usize, weapon: i32, count: i32) {
        if bay < self.pl.bays.len() {
            self.pl.bays[bay] = (weapon, count);
        }
    }

    /// Test / tool hook: select the current bay.
    pub fn select_bay(&mut self, bay: usize) {
        self.pl.cur_bay = bay.min(self.pl.bays.len().saturating_sub(1));
        self.pl.gun_selected = false;
    }

    /// Test / tool hook: set the ground lock.
    pub fn set_ground_lock(&mut self, obj: Option<usize>) {
        self.pl.gnd_lock = obj.map_or(-1, |o| o as i32);
    }

    /// Test / tool hook: switch invulnerability.
    pub fn set_training(&mut self, on: bool) {
        if on {
            self.pl.pflags |= pflags::TRAINING;
        } else {
            self.pl.pflags &= !pflags::TRAINING;
        }
    }

    /// Test / tool hook: the objective slots.
    pub fn target_slots(&self) -> [TargetSlot; 2] {
        self.pl.targets
    }

    pub fn target_slots_mut(&mut self) -> &mut [TargetSlot; 2] {
        &mut self.pl.targets
    }

    /// Test / tool hook: set the mission deadline (ticks).
    pub fn set_deadline(&mut self, ticks: i32) {
        self.pl.deadline = ticks;
    }

    /// Test / tool hook: raise the theater alarm now.
    pub fn raise_alarm(&mut self) {
        self.alarm();
    }
}
