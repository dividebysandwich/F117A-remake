use serde::{Deserialize, Serialize};

/// Weapon index of the special equipment (supplies, cargo).
pub const SPECIAL_EQUIPMENT: i32 = 18;

/// World-object flags.
pub mod oflags {
    pub const AIRFIELD: i32 = 0x0001;
    pub const SWEPT: i32 = 0x0002;
    pub const PAINTED: i32 = 0x0004;
    pub const SHIP: i32 = 0x0008;
    pub const TRACKING: i32 = 0x0010;
    pub const DESTROYED: i32 = 0x0080;
    pub const NEUTRAL: i32 = 0x0100;
    pub const CARRIER: i32 = 0x0200;
    pub const FRIENDLY: i32 = 0x0400;
    pub const DISABLED: i32 = 0x0800;
    pub const RADAR_ID: i32 = 0x1000;
}

/// Flight-unit flags.
pub mod fflags {
    pub const EXISTS: i32 = 0x0001;
    pub const AIRBORNE: i32 = 0x0002;
    pub const RTB: i32 = 0x0004;
    pub const CONTACT: i32 = 0x0008;
    pub const HIT: i32 = 0x0010;
    pub const DESTROYED: i32 = 0x0020;
    pub const ALERTED_PATROL: i32 = 0x0040;
    pub const SCRAMBLE: i32 = 0x0080;
    pub const ESCORT: i32 = 0x0100;
    pub const FINAL: i32 = 0x0200;
    pub const TAKEOFF: i32 = 0x0400;
    pub const VISUAL_ID: i32 = 0x0800;
    pub const ROLLOUT: i32 = 0x1000;
    pub const TERRAIN: i32 = 0x2000;
    pub const SAW_PLAYER: i32 = 0x4000;
}

/// Player flags.
pub mod pflags {
    pub const GEAR_UP: i32 = 0x0001;
    pub const FLAPS: i32 = 0x0002;
    pub const BAY_OPEN: i32 = 0x0004;
    pub const BRAKES: i32 = 0x0008;
    pub const ECM: i32 = 0x0010;
    pub const IRJ: i32 = 0x0020;
    pub const DESIGNATE: i32 = 0x0100;
    pub const CAMERA_VIEW: i32 = 0x0200;
    pub const TRAINING: i32 = 0x1000;
    pub const SECONDARY_DONE: i32 = 0x2000;
    pub const PRIMARY_DONE: i32 = 0x4000;
}

/// Damage bits.
pub mod dmg {
    /// Missile warning: RAD/IR lights and klaxon off.
    pub const MW: u8 = 0x01;
    /// Engine: thrust capped at (25 - hits) * 4 %.
    pub const ENG: u8 = 0x02;
    /// Flight control: stick jitter.
    pub const FC: u8 = 0x04;
    /// Avionics: autopilot off, no INS waypoint cue.
    pub const AV: u8 = 0x08;
    /// Bay doors jammed open.
    pub const BD: u8 = 0x10;
    /// Fuel leak (lights the FIRE lamp in the original).
    pub const FIRE: u8 = 0x20;
    /// Jammers dead, decoys blocked.
    pub const JAM: u8 = 0x40;
    /// No effect.
    pub const UNUSED: u8 = 0x80;
}

/// World object in flight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct GroundObject {
    pub name: i32,
    /// Map units (x east, y south).
    pub x: i32,
    pub y: i32,
    /// Radar / SAM type (0 = none; set to 0 when destroyed).
    pub unit_type: i32,
    pub flags: i32,
    /// Alert 0..255 for sites; for airfields the stationed aircraft type (same field).
    pub alert: i32,
    /// Sweep timer for sites; squadron count for airfields.
    pub timer: i32,
    /// Shape: low 7 bits model; bit 8 = placed / wreck state.
    pub shape: i32,
}

/// Flight unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct FlightUnit {
    /// Waypoint / base object.
    pub wp: i32,
    /// Map position.
    pub x: i32,
    pub y: i32,
    pub alt: i32,
    /// Map << 5 (y grows south).
    pub wx: i32,
    pub wy: i32,
    pub heading: i32,
    pub pitch: i32,
    pub bank: i32,
    pub spec: i32,
    pub flags: i32,
    pub speed: i32,
    /// Endurance timer (frames).
    pub timer: i32,
    /// Missile type.
    pub missile: i32,
    /// Unused, kept for completeness.
    pub reserved: i32,
    /// Radar track strength 0..255.
    pub heat: i32,
    /// AI mode of the last frame (1 intercept, 2 RTB, 3 patrol; 0 inactive). Not in the original record.
    pub mode: i32,
}

/// Projectile. Slots 0..7 enemy, 8..11 player.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Projectile {
    pub x: i32,
    pub y: i32,
    /// viewZ units.
    pub alt: i32,
    pub speed: i32,
    pub heading: i32,
    pub pitch: i32,
    pub roll: i32,
    /// Frames, 0 = free.
    pub ttl: i32,
    /// Missile table index.
    pub spec: i32,
    /// Player weapon index.
    pub weapon: i32,
    /// Target lock (object index; -1 none).
    pub target: i32,
    /// Launcher (site index > 2, -unit for aircraft, 1 SA-14) or bomb pitch floor.
    pub launcher: i32,
    pub tgt_x: i32,
    pub tgt_y: i32,
    /// Locked on its target this frame (not in the original record).
    pub locked: bool,
    /// Ballistic bombs (not in the original record): their exact flight.
    #[serde(default)]
    pub fall: Option<BombFall>,
    /// Missiles under realistic guidance (not in the original record): their exact flight.
    #[serde(default)]
    pub flight: Option<MissileFlight>,
}

impl Projectile {
    /// Map position (x, y) and altitude (viewZ) with fractions: a ballistic bomb's exact point,
    /// else the record's (+ 0.5 for the middle of the map unit).
    pub fn map_pos(&self) -> (f64, f64, f64) {
        let fine = ROUND_FINE as f64;
        if let Some(f) = self.flight {
            // True altitude, like the aircraft are drawn.
            return (f.x as f64 / (32.0 * fine), f.y as f64 / (32.0 * fine), f.alt as f64 / fine);
        }
        match self.fall {
            Some(f) => {
                let alt = f.alt as f64 / fine;
                (f.x as f64 / (32.0 * fine), f.y as f64 / (32.0 * fine), crate::math::view_z_f(alt))
            }
            None => (self.x as f64 + 0.5, self.y as f64 + 0.5, self.alt as f64),
        }
    }
}

/// A missile's flight under realistic guidance ([`crate::guidance`]): position and velocity in
/// fine units ([`ROUND_FINE`] per world unit: x east, y south like the map, alt up, the true
/// altitude) and per second.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MissileFlight {
    pub x: i32,
    pub y: i32,
    pub alt: i32,
    pub vx: i32,
    pub vy: i32,
    pub vz: i32,
    /// Frames since launch, and frames the motor still trails smoke.
    pub t: i32,
    pub smoke: i32,
    /// Climb angle (BAM) of a lofted SAM, climbing before it steers for the target; 0 = none.
    pub loft: i32,
}

/// A falling bomb's position (fine units, [`ROUND_FINE`] per world unit: x east, y south like
/// the map, and true altitude in feet) and velocity (fine units per second: east, south, up);
/// see [`crate::ballistics`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct BombFall {
    pub x: i32,
    pub y: i32,
    pub alt: i32,
    pub vx: i32,
    pub vy: i32,
    pub vz: i32,
}

/// What emits a radar signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EmitterSource {
    /// Ground site (object index).
    Site(usize),
    /// Aircraft (flight unit index).
    Aircraft(usize),
    /// ZSU-23-4 Gun Dish radar (`Sim::aaa_units` index; not in the original).
    Gun(usize),
}

/// A radar painting the player, for a radar warning receiver (not in the original).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Emitter {
    pub source: EmitterSource,
    /// Map position (y south) and altitude (viewZ; 0 for sites).
    pub x: i32,
    pub y: i32,
    pub alt: i32,
    /// Radar type (threat table index).
    pub radar_type: i32,
    /// Range (km, map >> 6) and bearing from the player (BAM, north = 0).
    pub range: i32,
    pub bearing: i32,
    /// Its missile's lock range (km).
    pub lock_range: i32,
    /// Detected the player (site: PAINTED; aircraft: radar contact).
    pub painted: bool,
    /// Tracking the player (site: TRACKING or alert past 0xC0; aircraft: radar track past 0xC0).
    pub tracking: bool,
    /// One of its missiles is in the air.
    pub launch: bool,
    /// Higher = more dangerous.
    pub priority: i32,
}

/// Fine units of a gun round's position and velocity per world unit (1/256 wu).
pub const ROUND_FINE: i32 = 256;

/// Gun round, in fine units ([`ROUND_FINE`] per world unit, x east and y south like the map,
/// velocities per frame); x == 0 is a free slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Bullet {
    pub x: i32,
    pub y: i32,
    pub alt: i32,
    pub vx: i32,
    pub vy: i32,
    pub vz: i32,
}

impl Bullet {
    /// Map position (x, y) and altitude.
    pub fn map_pos(&self) -> (f64, f64, f64) {
        let f = ROUND_FINE as f64;
        (self.x as f64 / (32.0 * f), self.y as f64 / (32.0 * f), self.alt as f64 / f)
    }

    /// Map units (x, y).
    pub(crate) fn map_xy(&self) -> (i32, i32) {
        (self.x / (32 * ROUND_FINE), self.y / (32 * ROUND_FINE))
    }
}

/// The player's cannon (not the original's, whose rounds flew straight along the nose at a
/// fixed speed): muzzle speed (m/s, added to the aircraft's speed along the nose), gravity
/// (m/s^2) and metres per world unit.
pub const GUN_MUZZLE_SPEED: f64 = 1050.0;
pub const GUN_GRAVITY: f64 = 9.81;
pub const WU_METRES: f64 = 512_000.0 / 1_048_576.0;
/// Hit radius of a player round per opponent quality (m): a round stands for a burst.
pub const GUN_HIT_RADIUS: [f64; 4] = [150.0, 100.0, 75.0, 60.0];

/// Countermeasure cartridge. Slot 0 = decoy, 1..3 flares / chaff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Countermeasure {
    pub x: i32,
    pub y: i32,
    /// 1 flare, 2 chaff, 3 decoy.
    pub kind: i32,
    pub ttl: i32,
}

/// Smoke puff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Smoke {
    pub x: i32,
    pub y: i32,
    pub alt: i32,
    pub age: i32,
}

/// Falling wreck: position and fall speed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Wreck {
    pub x: i32,
    pub y: i32,
    pub alt: i32,
    pub vel: i32,
}

/// Impact flash: position and timer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Flash {
    pub x: i32,
    pub y: i32,
    pub alt: i32,
    pub timer: i32,
}

/// Destroyed / replaced tile object (an entry of the tile damage list).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TileDamage {
    pub level: u8,
    pub index: u16,
    pub col: i32,
    pub row: i32,
    /// New shape: either the tile's own shape (draw its wreck state, `| 0x100`) or a rubble shape.
    pub shape: i32,
}

/// Objective slot `targets[k]` (0 primary, 1 secondary).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TargetSlot {
    pub mission_type: i32,
    pub object: i32,
    pub base: i32,
    pub code: i32,
}

/// HUD mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum HudMode {
    #[default]
    Nav = 0,
    Air = 1,
    Gnd = 2,
}

/// One-shot commands of this frame (keys).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Commands {
    /// Enter / joystick button 1 (fire the selected weapon, camera shutter). Holding repeats every 4 frames.
    pub fire: bool,
    /// Backspace / button 0 held: cannon.
    pub gun: bool,
    /// The fire button held (not in the original): fires the cannon when it is selected.
    #[serde(default)]
    pub trigger: bool,
    /// Key 1.
    pub flare: bool,
    /// Key 2.
    pub chaff: bool,
    /// Key 5.
    pub decoy: bool,
    /// Key 3.
    pub toggle_irj: bool,
    /// Key 4.
    pub toggle_ecm: bool,
    /// Key 8.
    pub toggle_bay: bool,
    /// Space: next bay.
    pub next_bay: bool,
    /// 'n': designate the point under the nose.
    pub designate: bool,
    /// 'b': next target.
    pub next_target: bool,
    /// Lock the nearest enemy ahead (not in the original): turns the tracking camera ahead, and
    /// once the HUD is in the selected weapon's mode (AIR for air-to-air missiles, else GND)
    /// picks the nearest hostile flight or ground object in sight.
    #[serde(default)]
    pub nearest_target: bool,
    /// Drop both locks and hold off the automatic acquisition until the next target or camera
    /// key (not in the original).
    #[serde(default)]
    pub clear_target: bool,
    /// Camera keys `/` `,` `.` `m`: 0 / 0x4000 / 0x8000 / 0xC000.
    pub camera_dir: Option<u16>,
    /// Shift-F10.
    pub eject: bool,
    /// Alt-T: toggle invulnerability (marks the mission as training).
    pub toggle_training: bool,
    /// Alt-R (training only): resupply.
    pub resupply: bool,
    /// Lock this world object (a display's cursor lock, not in the original): kept until
    /// another target key.
    #[serde(default)]
    pub lock_object: Option<u16>,
    /// Lock this flight unit (airborne).
    #[serde(default)]
    pub lock_unit: Option<u16>,
    /// Lock what is at this map point (the targeting pod's point track, not in the original):
    /// the object there, or the plain ground as object 0.
    #[serde(default)]
    pub designate_at: Option<(i32, i32)>,
}

/// What the flight model / game reports about the player this frame.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerState {
    /// World units, x east.
    pub x: i32,
    /// World units, y north.
    pub y: i32,
    /// True altitude in feet. viewZ is computed inside.
    pub altitude: u16,
    /// BAM.
    pub heading: u16,
    pub pitch: i16,
    pub roll: i16,
    /// knots * 27.
    pub velocity: u16,
    pub gear_up: bool,
    pub flaps: bool,
    pub brakes: bool,
    pub on_ground: bool,
    pub autopilot: bool,
    /// Climb rate in altitude units per second, for the touchdown test.
    pub climb_rate: i32,
    /// Fuel (original game units).
    pub fuel: i32,
    /// The renderer found the eye inside an object.
    pub collided: bool,
    pub hud_mode: HudMode,
    /// Right MFD shows the tracking camera.
    pub camera_mfd: bool,
    /// External view.
    pub external_view: bool,
    /// Cockpit / HUD drawn.
    pub hud_visible: bool,
    /// Bombing in CCRP (else CCIP; not in the original): with a ground target locked, holding
    /// the fire button releases the bomb when the target passes the release point.
    #[serde(default)]
    pub ccrp: bool,
    pub cmd: Commands,
}

/// Landing option.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LandingMode {
    NoCrashes = 0,
    #[default]
    Easy = 1,
    Realistic = 2,
}

/// Options that are not part of the mission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimOptions {
    pub landing: LandingMode,
    /// Implement the intended rule where the original is evidently broken: the unreachable
    /// supply-drop test, and an air-to-air missile that keeps hitting the wreck it just downed.
    pub fix_original_bugs: bool,
    /// RNG seed override; default is the mission generator's final RNG state.
    pub seed: Option<u32>,
    /// Damage done in earlier missions of a campaign (not in the original game).
    #[serde(default)]
    pub prior_damage: Option<WorldDamage>,
    /// The aircraft carries its weapons outside (no bay): the "doors" are always open, with
    /// their radar signature (not in the original).
    #[serde(default)]
    pub external_stores: bool,
    /// Realistic weapon damage (not in the original): a missile hit can destroy the aircraft
    /// outright, damages more systems, starts fires more often (a fire burns out or the aircraft
    /// explodes) and damaged control surfaces put the aircraft out of trim.
    #[serde(default)]
    pub realistic_damage: bool,
    /// Realistic missile guidance (not in the original): proportional navigation, motors that
    /// burn out (the missile glides on), lofted SAM shots, the AIM-120 flying straight for 3 s.
    #[serde(default)]
    pub realistic_guidance: bool,
    /// Low-level air defences (not in the original): ZSU-23-4 and S-60 guns around airfields
    /// and targets, MANPADS in the towns ([`crate::aaa`]).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub air_defence: bool,
}

impl Default for SimOptions {
    fn default() -> Self {
        Self {
            landing: LandingMode::Easy,
            fix_original_bugs: true,
            seed: None,
            prior_damage: None,
            external_stores: false,
            realistic_damage: false,
            realistic_guidance: false,
            air_defence: false,
        }
    }
}

/// What the player destroyed on the ground, carried from mission to mission in a campaign.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct WorldDamage {
    /// Changed tile objects.
    pub tiles: Vec<TileDamage>,
    /// Destroyed world objects (sites, bases, landmarks; never the scratch slots 0..2).
    pub objects: Vec<u16>,
    /// Map positions of everything destroyed (also tile objects), to keep new targets off them.
    pub positions: Vec<(i32, i32)>,
}

impl WorldDamage {
    /// Adds a later mission's damage.
    pub fn merge(&mut self, later: &WorldDamage) {
        for t in &later.tiles {
            match self
                .tiles
                .iter_mut()
                .find(|d| (d.level, d.index, d.col, d.row) == (t.level, t.index, t.col, t.row))
            {
                Some(d) => d.shape = t.shape,
                None => self.tiles.push(*t),
            }
        }
        for &o in &later.objects {
            if !self.objects.contains(&o) {
                self.objects.push(o);
            }
        }
        for &p in &later.positions {
            if !self.positions.contains(&p) {
                self.positions.push(p);
            }
        }
    }

    /// Something was destroyed within `r` map units of (x, y).
    pub fn near(&self, x: i32, y: i32, r: i32) -> bool {
        self.positions.iter().any(|&(px, py)| (px - x).abs() < r && (py - y).abs() < r)
    }
}

/// Mission end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissionEnd {
    /// 0 safe stop / chute landed / rescued, 1 crash, 2 collision, 5 bad landing, 6 ejection
    /// failed, 7 quit, 8 captured after ejecting.
    pub outcome: u8,
    /// Landing type: 1 default, 2 ejected, 3 landed, 4 ejected and rescued.
    pub landing_type: u8,
    /// Final map position.
    pub map_x: i32,
    pub map_y: i32,
    pub damage: u8,
    pub hits: i32,
}

/// Debrief event log record (`{tick, x>>7, y>>7, type|0x80 primary|0x40 secondary, arg}`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogEvent {
    pub tick: i32,
    pub x: u8,
    pub y: u8,
    /// Event type with 0x80 primary / 0x40 secondary.
    pub kind: u8,
    pub arg: u8,
}

/// Who fired an enemy missile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Launcher {
    Site(usize),
    Aircraft(usize),
    Player,
    /// A MANPADS team (`Sim::aaa_units` index; not in the original).
    Manpads(usize),
}

/// Radar warning receiver tones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RwrTone {
    /// A radar signal is received but the player is not detected.
    Track,
    /// Detected by a Doppler radar.
    Scan,
    /// Detected by a non-Doppler radar.
    SearchWarning,
    /// A SAM site launched a missile at the player.
    SamLaunch,
    /// An enemy aircraft launched a missile at the player.
    AirLaunch,
}

/// Output events of one frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SimEvent {
    /// HUD message line, as printed by the original.
    Message(String),
    /// Sound effect `id` at `priority`.
    Sound {
        id: u8,
        priority: u8,
    },
    /// Radar warning receiver tone (replaces the original's sounds 8 and 9).
    Rwr(RwrTone),
    /// Time acceleration must be cancelled.
    CancelTimeAccel,
    MissileLaunched {
        slot: usize,
        spec: i32,
        launcher: Launcher,
    },
    /// Player damaged: the bits that were rolled this time.
    PlayerHit {
        bits: u8,
    },
    /// Red damage flash.
    DamageFlash,
    /// Impact flash (flash timer value: 0x10 kill, -6 bomb, -16 crash, -1 gun).
    Explosion {
        x: i32,
        y: i32,
        alt: i32,
        kind: i32,
    },
    /// An S-60 shell burst (map position, altitude ft; not in the original).
    Flak {
        x: i32,
        y: i32,
        alt: i32,
    },
    /// A gun round hit the ground here (a round's fine units: x east, y south).
    RoundImpact {
        x: i32,
        y: i32,
    },
    AircraftDestroyed {
        unit: usize,
        spec: i32,
    },
    AircraftCrashed {
        unit: usize,
    },
    AircraftTakeoff {
        unit: usize,
        base: usize,
    },
    /// The downed pilot fired a signal flare (map position).
    SignalFlare {
        x: i32,
        y: i32,
    },
    /// An attack drone reached its target (map position).
    DroneImpact {
        unit: usize,
        x: i32,
        y: i32,
    },
    /// A world object was destroyed; `shape` = the shape to draw now (`| 0x100` = wreck state).
    GroundDestroyed {
        object: usize,
        shape: i32,
    },
    /// A plain tile object was destroyed or replaced.
    TileDestroyed(TileDamage),
    ObjectiveDone {
        primary: bool,
    },
    PhotoTaken {
        frame: i32,
    },
    /// Extra fuel tank transferred (fuel += lbs).
    FuelTransfer {
        lbs: i32,
    },
    /// Resupply (training): fuel = 5000.
    Resupplied,
    Ejected {
        success: bool,
    },
    /// "No Crashes" collision with a 3D object: the flight model adds 500 ft and drops the autopilot.
    CollisionBounce,
    MissionEnd(MissionEnd),
    Log(LogEvent),
}

/// Cockpit lights driven by the threat / weapon code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Lights {
    /// Status light 0 (0x11 while lit).
    pub rad: bool,
    /// Status light 1 (0x29 while lit).
    pub ir: bool,
    /// Light 24: 2*fs frames after a detection.
    pub emv_warning: bool,
    /// Status 8 (blinks on odd frames while something tracks you).
    pub trak: bool,
    pub ecm: bool,
    pub irj: bool,
    /// IRJ still hot (colour 0x2A).
    pub irj_hot: bool,
    pub decoy: bool,
    pub chaff: bool,
    pub flare: bool,
    /// LOCK light (blinks while the oval is up, steady when in range).
    pub lock: bool,
    pub bay: bool,
    /// Missile klaxon this frame.
    pub klaxon: bool,
    /// Steady versions of RAD / IR (a locked radar / IR missile is inbound), without the blink.
    pub rad_threat: bool,
    pub ir_threat: bool,
}

/// EMV gauge signal bar (EGR ground / EAR air).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SignalBar {
    /// min(score, 99).
    pub length: i32,
    /// Palette colour.
    pub colour: u8,
}

/// MAP radar arc of the sweep being displayed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RadarArc {
    /// Ground site index, or -1-unit for aircraft.
    pub source: i32,
    /// Centre (map units).
    pub x: i32,
    pub y: i32,
    /// Coverage radius in km (`(EMV + score) * range / 100`).
    pub radius_km: i32,
    pub doppler: bool,
    /// Arc limits in BAM high bytes; None = full circle.
    pub arc: Option<(i32, i32)>,
    pub colour: u8,
    /// Frames left of the display.
    pub frames_left: i32,
}

/// Targeting cues for the HUD / MFD.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TargetingState {
    /// Ground lock (world object index), None if none.
    pub ground: Option<usize>,
    /// Air lock (flight unit index).
    pub air: Option<usize>,
    /// LOCK oval shown.
    pub lock_valid: bool,
    /// Oval red / contracted (in range).
    pub in_range: bool,
    /// Tracking box colour (HUD colour 0 = default, 6 wrong weapon, 0x29 red).
    pub box_colour: u8,
    /// Box half size.
    pub box_size: i32,
    /// Approximate HUD screen position of the locked target (320x200 cockpit), None off screen.
    pub screen: Option<(i32, i32)>,
    /// The locked target was killed.
    pub target_killed: bool,
    /// CCIP point (map units) for free-fall bombs.
    pub ccip: Option<(i32, i32)>,
    /// Ranging bar half-length (release cue at 0) and whether it flashes.
    pub ranging_bar: Option<(i32, bool)>,
    /// CCRP: frames to the release point (negative past it).
    #[serde(default)]
    pub ccrp_release: Option<i32>,
    /// Range (map units) / bearing (BAM) to the ground lock.
    pub range: i32,
    pub bearing: i32,
}

/// Objective status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Objectives {
    pub primary_done: bool,
    pub secondary_done: bool,
    /// Current waypoint: 1 after the secondary, 2 after the primary, 3 both.
    pub waypoint: i32,
    /// Mission clock (ticks).
    pub tick: i32,
    pub deadline: i32,
    /// The special aircraft landed and escaped (air mission failed).
    pub target_escaped: bool,
}

/// Effects of the player's damage that the flight model must apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DamageEffects {
    pub bits: u8,
    pub hits: i32,
    /// ENG: throttle cap in percent.
    pub throttle_cap: Option<i32>,
    /// FC: extra stick jitter magnitude `clamp(hits*4, 0, 32)` added to the turbulence term.
    pub stick_jitter: i32,
    /// AV: autopilot forced off, INS waypoint cue disabled.
    pub autopilot_failed: bool,
    /// BD: bay doors jammed open.
    pub doors_jammed: bool,
    /// FIRE bit: extra fuel units lost per 2 s while the throttle is not idle.
    pub fuel_leak: i32,
    /// JAM: jammers off, decoys blocked.
    pub jammers_failed: bool,
    /// Stall nose drop doubled (hits > 8).
    pub heavy_stall: bool,
    /// Ground contact is fatal (hits > 4).
    pub fragile: bool,
    /// Damaged control surfaces (realistic damage): the stick input they add.
    pub controls: ControlFault,
    /// The airframe is breaking up (realistic damage): the aircraft explodes within seconds.
    pub destroyed: bool,
}

/// Damaged control surfaces (realistic damage): a constant out-of-trim input and an oscillating
/// one (a flapping surface), in thousandths of full stick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ControlFault {
    pub roll: i32,
    pub pitch: i32,
    /// Oscillation amplitudes and its period (tenths of a second, 0 = none).
    pub roll_swing: i32,
    pub pitch_swing: i32,
    pub period: i32,
}
