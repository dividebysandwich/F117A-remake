pub mod ballistics;
pub mod data;
pub mod math;
pub mod rng;
pub mod types;

mod ai;
pub mod csar;
pub mod drones;
pub mod lights;
mod missiles;
mod objectives;
mod radar;
pub mod score;
mod targeting;
mod tiles;
mod weapons;
mod weather;

pub use data::{AircraftKind, AircraftType, MissileType, SimData, TheaterTiles, ThreatType, WeaponType};
pub use radar::emv;
pub use rng::Rng;
pub use types::*;

use stealth_mission::Mission;
use math::{s16, view_z};
use serde::{Deserialize, Serialize};

/// Number of projectile slots (0..7 enemy, 8..11 player).
pub const N_PROJ: usize = 12;

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
    pub(crate) lockheed: bool,
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
    pub(crate) p: PlayerView,
    /// Player flags (`pflags`).
    pub(crate) pflags: i32,
    pub(crate) emv: i32,
    pub(crate) damage: u8,
    pub(crate) hits: i32,
    pub(crate) irj_heat: i32,
    pub(crate) door_anim: i32,
    pub(crate) bay_req: bool,
    /// Bays or pylon stations (weapon type, count; type -1 empty).
    pub(crate) bays: Vec<(i32, i32)>,
    /// Selected station: the first one carrying the selected weapon.
    pub(crate) cur_bay: usize,
    /// The cannon is the selected weapon (not in the original: Space steps to it after the
    /// stores, and the fire button fires it).
    #[serde(default)]
    pub(crate) gun_selected: bool,
    /// Countermeasure stores ([1] flare, [2] chaff, [3] decoy).
    pub(crate) stores: [i32; 4],
    pub(crate) gun_ammo: i32,
    pub(crate) gun_firing: bool,
    pub(crate) fire_cooldown: i32,
    pub(crate) prev_fire: bool,
    /// CCRP: ground distance (ft) the bomb's impact point still had to go to the target last
    /// frame (None: no CCRP solution).
    #[serde(default)]
    pub(crate) ccrp_last: Option<f64>,
    pub(crate) prev_hud_mode: HudMode,
    pub(crate) hud_mode: HudMode,
    pub(crate) camera_mfd: bool,
    pub(crate) external_view: bool,
    pub(crate) prev_on_ground: bool,
    pub(crate) on_ground: bool,

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
    pub(crate) cms: [Countermeasure; 4],
    pub(crate) smoke: [Smoke; 16],
    /// Smoke source object (-1 aircraft / none).
    pub(crate) smoke_src: i32,
    pub(crate) wreck: Wreck,
    pub(crate) flash: Flash,
    pub(crate) tile_damage: Vec<TileDamage>,
    /// Result of the last tile search.
    pub(crate) last_tile: Option<tiles::TileHit>,

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
    /// Frames left on the radar sweep animation, -1 idle.
    pub(crate) sweep_anim: i32,
    /// Aircraft engaging the player: last frame / this frame.
    pub(crate) threat_count_last: i32,
    pub(crate) threat_count: i32,
    /// TRAK count, radar IDs and visual IDs held against the player.
    pub(crate) tracking: i32,
    pub(crate) radar_ids: i32,
    pub(crate) visual_ids: i32,
    /// Nearest base object and its distance; base used for traffic.
    pub(crate) nearest_base: i32,
    pub(crate) nearest_base_dist: i32,
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
    /// Apron parking spots for the parked base traffic (not in the original): world x, y and
    /// heading; empty = parked on the base object as in the original.
    #[serde(default)]
    pub(crate) parking: Vec<(i32, i32, i32)>,
    /// Lamp timers: chaff / flare / EMV.
    pub(crate) chaff_lamp: i32,
    pub(crate) flare_lamp: i32,
    pub(crate) emv_lamp: i32,
    pub(crate) egr: Option<SignalBar>,
    pub(crate) ear: Option<SignalBar>,
    pub(crate) arc: Option<RadarArc>,
    pub(crate) lights: Lights,
    /// Per-frame seeker acquisition results (range / aim bearing).
    pub(crate) acq_range: i32,
    pub(crate) acq_aim: i32,
    /// Missile launches against the player.
    pub launches: i32,

    // ---- targeting ----
    /// Air / ground target locks (bit 0x80 = reselect, -1 none).
    pub(crate) air_lock: i32,
    pub(crate) gnd_lock: i32,
    pub(crate) targeting: TargetingState,
    pub(crate) gnd_cooldown: i32,
    /// Frames left for a nearest-target request to wait for the HUD mode (`Commands::nearest_target`).
    pub(crate) nearest_req: u8,
    /// Locks cleared on request (`Commands::clear_target`): no automatic acquisition.
    pub(crate) target_hold: bool,
    /// Camera view direction relative to the nose.
    pub(crate) cam_dir: i32,
    pub(crate) last_missile_slot: i32,
    pub(crate) maverick_view: bool,

    // ---- objectives / end ----
    pub(crate) targets: [TargetSlot; 2],
    pub(crate) waypoint: i32,
    pub(crate) deadline: i32,
    pub(crate) cue_timers: [i32; 2],
    pub(crate) photo_timer: i32,
    pub(crate) photo_frames: i32,
    pub(crate) deadline_done: [bool; 2],
    pub(crate) time_expired_shown: bool,
    pub(crate) target_escaped: bool,
    pub(crate) corridor: bool,
    /// Landing state: 0 until airborne, 1 once airborne, then counts frames on a friendly runway.
    pub(crate) armed_landing: i32,
    /// Frames since ejection (counting from 1), 0 = not ejected.
    pub(crate) eject: i32,
    pub(crate) chute: (i32, i32, i32),
    pub(crate) landing_type: u8,
    pub(crate) ended: Option<MissionEnd>,
    /// Clouds (not in the original; clear = original behaviour).
    #[serde(default)]
    pub(crate) weather: stealth_mission::Weather,
    pub(crate) log: Vec<LogEvent>,

    #[serde(skip)]
    pub(crate) events: Vec<SimEvent>,
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
        let bays = m.loadout.bays.iter().take(n_bays).map(|b| (b.weapon as i32, b.count as i32)).collect();
        let display_shape = objects.iter().map(|o| o.shape).collect();
        let theater = s.theater;
        let seed = opts.seed.unwrap_or(m.rng.holdrand_exit);
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
            lockheed,
            theater,
            // Remake theaters use the original airbase models, laid out like Central Europe's.
            ns: data.start_dir.get(theater).copied().unwrap_or(1) as i32,
            rng: Rng::new(seed),
            frame: 0,
            mission_tick: 0,
            tick_frames: 0,
            fs: 15,
            p: PlayerView::default(),
            pflags: if m.invulnerable { pflags::TRAINING } else { 0 },
            weather: m.weather,
            emv: 0,
            damage: 0,
            hits: 2,
            irj_heat: 0,
            door_anim: 0,
            bay_req: false,
            bays,
            cur_bay: 0,
            gun_selected: false,
            stores: [data.stores_init[0] as i32, data.stores_init[1] as i32, data.stores_init[2] as i32, data.stores_init[3] as i32],
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
            cms: [Countermeasure::default(); 4],
            smoke: [Smoke::default(); 16],
            smoke_src: -1,
            wreck: Wreck::default(),
            flash: Flash::default(),
            tile_damage: Vec::new(),
            last_tile: None,
            alarm_timer: 0,
            alarm_window: 0,
            alarm_last: 0,
            ref_x: 0,
            ref_y: 0,
            ref_alt: 0,
            ref_heading: 0,
            sweep_anim: -1,
            threat_count_last: 0,
            threat_count: 0,
            tracking: 0,
            radar_ids: 0,
            visual_ids: 0,
            nearest_base: -1,
            nearest_base_dist: 0x7FFF,
            traffic_base: -1,
            traffic_toggle: false,
            mission_flights: w.flights.len() - m.drones.as_ref().map_or(0, |d| d.count.min(w.flights.len())),
            csar: None,
            destroyed_at: Vec::new(),
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
            chaff_lamp: 0,
            flare_lamp: 0,
            emv_lamp: 0,
            egr: None,
            ear: None,
            arc: None,
            lights: Lights::default(),
            acq_range: 0,
            acq_aim: 0,
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
            ended: None,
            log: Vec::new(),
            events: Vec::new(),
        };
        sim.set_fs(15);
        if let Some(d) = sim.opts.prior_damage.clone() {
            sim.apply_damage(&d);
        }
        if sim.opts.external_stores {
            sim.bay_req = true;
            sim.door_anim = 2;
            sim.pflags |= pflags::BAY_OPEN;
        }
        let st = sim.start_state();
        sim.ingest(&st);
        sim.find_nearest_base();
        sim.log_event(8, 0);
        sim.events.clear();
        sim
    }

    /// The player's start state: at the takeoff base, on the ground.
    pub fn start_state(&self) -> PlayerState {
        let b = self.targets[0].base.clamp(0, self.objects.len() as i32 - 1).max(0) as usize;
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
        self.cms[0].ttl > 0
    }

    pub(crate) fn ecm_on(&self) -> bool {
        self.pflags & pflags::ECM != 0
    }

    pub(crate) fn irj_on(&self) -> bool {
        self.pflags & pflags::IRJ != 0
    }

    pub(crate) fn training(&self) -> bool {
        self.pflags & pflags::TRAINING != 0
    }

    pub(crate) fn emit(&mut self, e: SimEvent) {
        self.events.push(e);
    }

    pub(crate) fn msg(&mut self, s: impl Into<String>) {
        self.events.push(SimEvent::Message(s.into()));
    }

    pub(crate) fn sound(&mut self, id: u8, priority: u8) {
        self.events.push(SimEvent::Sound { id, priority });
    }

    /// Append an event to the debrief log.
    pub(crate) fn log_event(&mut self, kind: i32, arg: i32) {
        let e = LogEvent {
            tick: self.mission_tick,
            x: (self.p.mx >> 7) as u8,
            y: (self.p.my >> 7) as u8,
            kind: kind as u8,
            arg: arg as u8,
        };
        self.log.push(e);
        self.events.push(SimEvent::Log(e));
    }

    /// WLD name of object i.
    pub fn place_name(&self, i: usize) -> String {
        let o = match self.objects.get(i) {
            Some(o) => *o,
            None => return String::new(),
        };
        let idx = if i == 0 || o.shape & 0x100 != 0 && o.name == 0 { o.shape & 0x7F } else { o.name };
        self.names.get(idx.max(0) as usize).cloned().unwrap_or_default()
    }

    pub(crate) fn obj(&self, i: i32) -> GroundObject {
        if i >= 0 { self.objects.get(i as usize).copied().unwrap_or_default() } else { GroundObject::default() }
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
        self.p = PlayerView {
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
        let keep = pflags::BAY_OPEN | pflags::ECM | pflags::IRJ | pflags::DESIGNATE | pflags::CAMERA_VIEW | pflags::TRAINING | pflags::PRIMARY_DONE | pflags::SECONDARY_DONE;
        let mut f = self.pflags & keep;
        if ps.gear_up {
            f |= pflags::GEAR_UP;
        }
        if ps.flaps {
            f |= pflags::FLAPS;
        }
        if ps.brakes {
            f |= pflags::BRAKES;
        }
        self.pflags = f;
        self.prev_on_ground = self.on_ground;
        self.on_ground = ps.on_ground;
        self.prev_hud_mode = self.hud_mode;
        self.hud_mode = ps.hud_mode;
        self.camera_mfd = ps.camera_mfd;
        self.external_view = ps.external_view;
    }

    /// Advance one frame. `fs` = nominal frames per sim second (normally 15).
    pub fn step(&mut self, ps: &PlayerState, fs: u8) -> Vec<SimEvent> {
        self.events.clear();
        if self.ended.is_some() {
            return Vec::new();
        }
        if fs as i32 != self.fs {
            self.set_fs(fs as i32);
        }
        self.ingest(ps);
        // Key commands and per-frame systems.
        self.handle_keys(ps);
        self.update_systems();
        self.handle_fire(ps);
        // EMV every frame.
        self.compute_emv();
        if self.frame & 7 == 0 {
            self.find_nearest_base();
            self.update_base_traffic();
        }
        if self.frame & 0x7F == 0 {
            self.alarm_last = self.alarm_window;
            self.alarm_window = 0;
        }
        if self.fs > 0 && self.frame % (4 * self.fs) == 0 {
            self.rebuild_tracking();
        }
        if self.frame & 0x7F == 0x40 && self.traffic_base >= 0 {
            self.spawn_traffic();
        }
        // Sites, aircraft, projectiles, countermeasures, gun rounds, lamps.
        self.update_sites();
        self.update_aircraft();
        self.update_missiles();
        self.update_cm_timers();
        self.update_bullets(ps);
        self.update_light_timers();
        if self.alarm_timer > 0 {
            self.alarm_timer -= 1;
        }
        // Draw-list pass and HUD overlay.
        self.update_targeting(ps);
        self.update_effects();
        self.update_objectives(ps);
        // Clock.
        self.tick_frames += 1;
        if self.tick_frames >= self.fs {
            self.tick_frames = 0;
            self.mission_tick += 1;
        }
        self.frame = (self.frame + 1) & 0xFFFF;
        std::mem::take(&mut self.events)
    }

    // ---------------------------------------------------------------- accessors

    /// Player EMV.
    pub fn emv(&self) -> i32 {
        self.emv
    }

    /// EMV gauge bar lengths: (blue from the left, red from the right), rows 187..197.
    pub fn emv_gauge(&self) -> (i32, i32) {
        let e = self.emv.clamp(0, 99);
        ((100 - e) / 2, e / 2)
    }

    /// Ground (EGR) and air (EAR) signal bars of the sweep being displayed.
    pub fn signal_bars(&self) -> (Option<SignalBar>, Option<SignalBar>) {
        (self.egr, self.ear)
    }

    /// MAP radar arc of the sweep being displayed.
    pub fn radar_arc(&self) -> Option<RadarArc> {
        self.arc
    }

    pub fn lights(&self) -> Lights {
        self.lights
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
            .filter(|(_, u)| u.flags & fflags::EXISTS != 0 && (u.flags & fflags::AIRBORNE != 0 || u.speed == 0))
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
        &self.cms
    }

    /// Flare, chaff, decoy stores left.
    pub fn stores(&self) -> (i32, i32, i32) {
        (self.stores[1], self.stores[2], self.stores[3])
    }

    pub fn gun_ammo(&self) -> i32 {
        self.gun_ammo
    }

    pub fn gun_firing(&self) -> bool {
        self.gun_firing
    }

    /// The aircraft has a cannon (the real F-117A has none).
    pub fn has_gun(&self) -> bool {
        !self.lockheed
    }

    /// The cannon is the selected weapon.
    pub fn gun_selected(&self) -> bool {
        self.gun_selected
    }

    /// Bays or stations (weapon index, count; -1 empty) and the selected one.
    pub fn bays(&self) -> (&[(i32, i32)], usize) {
        (&self.bays, self.cur_bay)
    }

    /// The weapons carried (weapon index, rounds on all stations), in station order, and the
    /// selected one's index in this list (None: nothing carried).
    pub fn weapon_groups(&self) -> (Vec<(i32, i32)>, Option<usize>) {
        let mut groups: Vec<(i32, i32)> = Vec::new();
        let mut selected = None;
        for (i, &(w, n)) in self.bays.iter().enumerate() {
            if w < 0 {
                continue;
            }
            match groups.iter().position(|g| g.0 == w) {
                Some(k) => groups[k].1 += n,
                None => {
                    if i == self.cur_bay && !self.gun_selected {
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
        self.bays = stations.iter().map(|&(w, n)| if w < 0 { (-1, 0) } else { (w, n.max(0)) }).collect();
        self.cur_bay = self.bays.iter().position(|b| b.0 >= 0).unwrap_or(0);
        self.gun_selected = self.has_gun() && !self.bays.iter().any(|b| b.0 >= 0);
    }

    pub fn weapon_types(&self) -> &[WeaponType] {
        &self.weapon_types
    }

    pub fn bay_doors_open(&self) -> bool {
        self.pflags & pflags::BAY_OPEN != 0
    }

    /// Door animation 0 (closed) .. 2 (open).
    pub fn bay_door_anim(&self) -> i32 {
        self.door_anim
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
        self.targeting
    }

    /// The right MFD should show the Maverick seeker of this slot.
    pub fn maverick_view_slot(&self) -> Option<usize> {
        let s = self.last_missile_slot;
        (self.maverick_view && s > 8 && (s as usize) < N_PROJ && self.proj[s as usize].ttl != 0 && self.proj[s as usize].spec == 27).then_some(s as usize)
    }

    pub fn objectives(&self) -> Objectives {
        Objectives {
            primary_done: self.pflags & pflags::PRIMARY_DONE != 0,
            secondary_done: self.pflags & pflags::SECONDARY_DONE != 0,
            waypoint: self.waypoint,
            tick: self.mission_tick,
            deadline: self.deadline,
            target_escaped: self.target_escaped,
        }
    }

    pub fn damage_effects(&self) -> DamageEffects {
        let d = self.damage;
        DamageEffects {
            bits: d,
            hits: self.hits,
            throttle_cap: (d & dmg::ENG != 0).then(|| ((25 - self.hits) * 4).max(0)),
            stick_jitter: if d & dmg::FC != 0 { (self.hits * 4).clamp(0, 32) } else { 0 },
            autopilot_failed: d & dmg::AV != 0,
            doors_jammed: d & dmg::BD != 0,
            fuel_leak: if d & dmg::FIRE != 0 { self.hits } else { 0 },
            jammers_failed: d & dmg::JAM != 0,
            heavy_stall: self.hits > 8,
            fragile: self.hits > 4,
        }
    }

    /// Everything destroyed on the ground so far, earlier missions included.
    pub fn world_damage(&self) -> WorldDamage {
        WorldDamage {
            tiles: self.tile_damage.clone(),
            objects: (3..self.objects.len()).filter(|&i| self.objects[i].flags & oflags::DESTROYED != 0).map(|i| i as u16).collect(),
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

    /// Debrief event log (for END).
    pub fn event_log(&self) -> &[LogEvent] {
        &self.log
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
        (self.radar_ids, self.visual_ids, self.tracking)
    }

    /// Parachute position (map x, map y, alt) while ejected.
    pub fn chute(&self) -> Option<(i32, i32, i32)> {
        (self.eject != 0).then_some(self.chute)
    }

    pub fn frame(&self) -> i32 {
        self.frame
    }

    pub fn mission_tick(&self) -> i32 {
        self.mission_tick
    }

    pub fn player(&self) -> PlayerView {
        self.p
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
        self.damage = bits;
        self.hits = hits;
    }

    /// Test / tool hook: load a bay.
    pub fn set_bay(&mut self, bay: usize, weapon: i32, count: i32) {
        if bay < self.bays.len() {
            self.bays[bay] = (weapon, count);
        }
    }

    /// Test / tool hook: select the current bay.
    pub fn select_bay(&mut self, bay: usize) {
        self.cur_bay = bay.min(self.bays.len().saturating_sub(1));
        self.gun_selected = false;
    }

    /// Test / tool hook: set the ground lock.
    pub fn set_ground_lock(&mut self, obj: Option<usize>) {
        self.gnd_lock = obj.map_or(-1, |o| o as i32);
    }

    /// Test / tool hook: switch invulnerability.
    pub fn set_training(&mut self, on: bool) {
        if on {
            self.pflags |= pflags::TRAINING;
        } else {
            self.pflags &= !pflags::TRAINING;
        }
    }

    /// Test / tool hook: the objective slots.
    pub fn target_slots(&self) -> [TargetSlot; 2] {
        self.targets
    }

    pub fn target_slots_mut(&mut self) -> &mut [TargetSlot; 2] {
        &mut self.targets
    }

    /// Test / tool hook: set the mission deadline (ticks).
    pub fn set_deadline(&mut self, ticks: i32) {
        self.deadline = ticks;
    }

    /// Test / tool hook: raise the theater alarm now.
    pub fn raise_alarm(&mut self) {
        self.alarm();
    }
}
