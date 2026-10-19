//! Player flight model: the original's F-117A flight step, integrated per second instead of
//! per frame. State is kept in the original units so the constants can be compared directly:
//! velocity = knots * 27, angles in BAM
//! (0x10000 = 360 degrees), altitude and positions in world units ("feet"), thrust 0..100,
//! g in 1/16 g. Per-frame terms that the original does not divide by fs (stall nose drop) are
//! scaled with the original's nominal 15 frames per sim second.

use std::f32::consts::TAU;

use bevy::prelude::*;
use serde::Deserialize;

use crate::input::{Action, ActionEvent, FlightControls};
use crate::world::{Theater, WorldPos, bevy_to_wu, wu_to_bevy};

pub struct FlightPlugin;

impl Plugin for FlightPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(FlightConstants::default()).add_systems(
            Update,
            (handle_actions, step_flight)
                .chain()
                .in_set(FlightSystems)
                .run_if(in_state(crate::settings::AppState::Flight)),
        );
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct FlightSystems;

/// BAM -> radians.
pub fn bam(v: f32) -> f32 {
    v / 65536.0 * TAU
}

/// Radians -> BAM.
pub fn to_bam(r: f32) -> f32 {
    r / TAU * 65536.0
}

/// Flight tables from the original game (assets/sim/tables.ron).
#[derive(Resource, Deserialize)]
pub struct SimTables {
    pub roll_gee: Vec<u8>,
    /// Runway direction per theater.
    #[serde(default)]
    pub start_dir: Vec<i16>,
}

impl SimTables {
    pub fn load() -> Self {
        let path = crate::asset_path::asset("sim/tables.ron");
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        ron::from_str(&text).expect("tables.ron")
    }
}

/// F-117A constants (F-15 SE II values in comments).
#[derive(Resource, Debug, Clone)]
pub struct FlightConstants {
    /// Gravity term in the target speed: sinMul(pitch, 120) (F-15: 80).
    pub gravity_term: f32,
    /// Thrust to knots: *720/100 (F-15: *8).
    pub thrust_to_knots: f32,
    pub max_knots: f32,
    /// 1 g stall speed at sea level, knots (F-15: 100).
    pub stall_knots: f32,
    /// Velocity lag time constant (s).
    pub speed_lag: f32,
    /// Engine spool: thrust += (set - thrust) / spool_div per second.
    pub spool_div: f32,
    /// Fuel burn every 2 s: set^2 / fuel_div + 2 (F-15: 750).
    pub fuel_div: f32,
    /// Original frames per sim second, for per-frame terms.
    pub fs: f32,
    /// Landing skill L: 0 no crashes, 1 easy, 2 realistic.
    pub landing: u8,
    /// Starting fuel (lbs).
    pub fuel: f32,
}

impl Default for FlightConstants {
    fn default() -> Self {
        Self {
            gravity_term: 120.0,
            thrust_to_knots: 7.2,
            max_knots: 899.0,
            stall_knots: 120.0,
            speed_lag: 16.0,
            spool_div: 4.0,
            fuel_div: 1000.0,
            fs: 15.0,
            landing: 1,
            fuel: 10000.0,
        }
    }
}

/// Why the flight ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlightEnd {
    Crash,
    BadLanding,
    Landed,
}

/// Effects of combat damage on the flight model (set by the combat simulation).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FlightDamage {
    /// ENG: thrust capped at this percentage.
    pub throttle_cap: Option<f32>,
    /// FC: extra stick jitter added to the turbulence term.
    pub stick_jitter: f32,
    /// AV: autopilot off and unavailable.
    pub autopilot_failed: bool,
    /// FIRE (fuel leak): extra fuel units lost per 2 s while the throttle is not idle.
    pub fuel_leak: f32,
    /// More than 8 hits: the stall nose drop is doubled.
    pub heavy_stall: bool,
    /// More than 4 hits: any fast ground contact outside a runway is fatal.
    pub fragile: bool,
}

/// The player's aircraft state.
#[derive(Component, Debug, Clone)]
pub struct Aircraft {
    /// Orientation (Bevy frame: forward -Z, up Y).
    pub orientation: Quat,
    /// knots * 27
    pub velocity: f32,
    pub thrust: f32,
    pub set_thrust: f32,
    /// World units ("feet") above sea level.
    pub altitude: f32,
    pub ground_altitude: f32,
    /// wu per second along the flight path.
    pub climb_rate: f32,
    /// 1/16 g
    pub gees: f32,
    /// velocity units
    pub stall_speed: f32,
    pub fuel: f32,
    pub gear_down: bool,
    pub flaps: bool,
    pub brakes: bool,
    pub bay_open: bool,
    /// Autopilot target altitude (viewZ), 0 = off.
    pub autopilot_alt: f32,
    pub on_ground: bool,
    pub in_landing_corridor: bool,
    /// Has been airborne since the start (landing only counts afterwards).
    pub has_flown: bool,
    pub ended: Option<FlightEnd>,
    /// Angle of attack trim (BAM).
    pub trim: f32,
    /// Lift force (0..0x2000), as used for the HUD flight path marker.
    pub lift: f32,
    pub roll_input: f32,
    pub pitch_input: f32,
    /// Bearing to the current waypoint (BAM), for the autopilot.
    pub waypoint_bearing: f32,
    /// Seconds of simulated time since the start.
    pub time: f32,
    pub damage: FlightDamage,
    fuel_timer: f32,
    stopped_timer: f32,
}

impl Aircraft {
    pub fn new_on_runway(fuel: f32) -> Self {
        Self {
            orientation: Quat::IDENTITY,
            velocity: 0.0,
            thrust: 0.0,
            set_thrust: 0.0,
            altitude: 0.0,
            ground_altitude: 0.0,
            climb_rate: 0.0,
            gees: 16.0,
            stall_speed: 0.0,
            fuel,
            gear_down: true,
            flaps: false,
            brakes: false,
            bay_open: false,
            autopilot_alt: 0.0,
            on_ground: true,
            in_landing_corridor: true,
            has_flown: false,
            ended: None,
            trim: 0.0,
            lift: 0.0,
            roll_input: 0.0,
            pitch_input: 0.0,
            waypoint_bearing: 0.0,
            time: 0.0,
            damage: FlightDamage::default(),
            fuel_timer: 0.0,
            stopped_timer: 0.0,
        }
    }

    pub fn knots(&self) -> f32 {
        self.velocity / 27.0
    }

    /// Autopilot engaged?
    pub fn autopilot(&self) -> bool {
        self.autopilot_alt > 0.0
    }

    /// Compressed altitude used for rendering, AI and detection.
    pub fn view_z(&self) -> f32 {
        view_z(self.altitude)
    }

    /// (heading, pitch, roll) in radians; heading clockwise from north, roll positive = right wing down.
    pub fn euler(&self) -> (f32, f32, f32) {
        let f = self.orientation * Vec3::NEG_Z;
        let r = self.orientation * Vec3::X;
        let u = self.orientation * Vec3::Y;
        let heading = f.x.atan2(-f.z).rem_euclid(TAU);
        let pitch = f.y.clamp(-1.0, 1.0).asin();
        let roll = (-r.y).atan2(u.y);
        (heading, pitch, roll)
    }

    pub fn from_euler(heading: f32, pitch: f32, roll: f32) -> Quat {
        Quat::from_rotation_y(-heading) * Quat::from_rotation_x(pitch) * Quat::from_rotation_z(-roll)
    }
}

/// Altitude compression (above 8192 halved, above 16384 quartered).
pub fn view_z(alt: f32) -> f32 {
    if alt < 8192.0 {
        alt
    } else if alt < 16384.0 {
        8192.0 + (alt - 8192.0) / 2.0
    } else {
        12288.0 + (alt - 16384.0) / 4.0
    }
}

/// Marks the player's aircraft.
#[derive(Component)]
pub struct Player;

fn handle_actions(mut actions: MessageReader<ActionEvent>, mut q: Query<&mut Aircraft, With<Player>>) {
    let Ok(mut a) = q.single_mut() else {
        actions.clear();
        return;
    };
    for ActionEvent(action) in actions.read() {
        match action {
            // '=' / '-' / '+' / '_' (they do not release the brakes in the F-117A).
            Action::ThrottleUp => a.set_thrust = (a.set_thrust + if a.set_thrust < 10.0 { 5.0 } else { 10.0 }).min(100.0),
            Action::ThrottleDown => a.set_thrust = (a.set_thrust - 10.0).max(0.0),
            Action::ThrottleMax => a.set_thrust = 100.0,
            Action::ThrottleIdle => a.set_thrust = 0.0,
            Action::Gear => {
                if !a.on_ground {
                    a.gear_down = !a.gear_down;
                }
            }
            Action::Flaps => a.flaps = !a.flaps,
            Action::Brakes => a.brakes = !a.brakes,
            // Bay doors (8) are run by the combat simulation, which mirrors them into `bay_open`.
            Action::Autopilot if !a.damage.autopilot_failed => {
                // Holds the current altitude, at least 500 ft.
                a.autopilot_alt = if a.autopilot() { 0.0 } else { a.view_z().max(500.0) };
            }
            _ => {}
        }
    }
}

/// Stick axis -1..1 to the original's -7..7 steps (joy = 0x80 + deflection).
fn stick_steps(axis: f32) -> f32 {
    let joy = (128.0 + axis.clamp(-1.0, 1.0) * 127.0).round() as i32;
    let mut r = (joy >> 4) - 8;
    if r < 0 {
        r += 1;
    }
    r as f32
}

/// Signed BAM difference a - b in -0x8000..0x8000.
fn bam_diff(a: f32, b: f32) -> f32 {
    (a - b + 32768.0).rem_euclid(65536.0) - 32768.0
}

fn step_flight(
    time: Res<Time>,
    controls: Res<FlightControls>,
    k: Res<FlightConstants>,
    tables: Res<SimTables>,
    theater: Option<Res<Theater>>,
    mut q: Query<(&mut Aircraft, &mut WorldPos, &mut Transform), With<Player>>,
) {
    // Sub-steps of at most 1/60 s keep accelerated test runs accurate.
    let total = time.delta_secs().min(1.0);
    for (mut a, mut pos, mut tf) in &mut q {
        let mut remaining = total;
        while remaining > 1e-6 {
            let dt = remaining.min(1.0 / 60.0);
            remaining -= dt;
            step_one(&mut a, &mut pos, &mut tf, dt, &controls, &k, &tables, theater.as_deref());
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn step_one(
    a: &mut Aircraft,
    pos: &mut WorldPos,
    tf: &mut Transform,
    dt: f32,
    controls: &FlightControls,
    k: &FlightConstants,
    tables: &SimTables,
    theater: Option<&Theater>,
) {
    let l = k.landing as f32;
    {
        // The clock keeps running after the end (scripts and screenshots use it).
        a.time += dt;
        if a.ended.is_some() {
            return;
        }
        let (_, pitch, roll) = a.euler();
        let (head_b, pitch_b, roll_b) = {
            let (h, p, r) = a.euler();
            (to_bam(h), to_bam(p), to_bam(r))
        };
        let air = !a.on_ground;
        let vz = a.view_z();
        let knots = a.knots();

        // --- stick
        let r = stick_steps(controls.roll);
        let p = stick_steps(controls.pitch);
        let mut roll_input = (r.abs() + 2.0) * r * 2.0;
        let mut pitch_input = if p < 0.0 { (6.0 * p / 2.0).trunc() } else { 6.0 * p };
        if !air && pitch_input < 0.0 && pitch_b <= 0.0 {
            pitch_input = 0.0;
        }
        if roll_input != 0.0 || pitch_input != 0.0 || a.damage.autopilot_failed {
            a.autopilot_alt = 0.0;
        }

        // --- autopilot: steer to the waypoint bearing, hold altitude
        if a.autopilot() {
            let e = bam_diff(a.waypoint_bearing, head_b).clamp(-5120.0, 5120.0) * 2.0;
            roll_input = ((e - roll_b) / 64.0).clamp(-24.0, 24.0);
            let t = ((a.autopilot_alt - vz) * 16.0 - a.trim).clamp(-5120.0, 3072.0);
            pitch_input = ((t - pitch_b) / 128.0).clamp(-8.0, 8.0);
        }

        // --- turbulence (stronger below 500 ft and with the gear down)
        let mut turb = if k.landing > 0 { ((500.0 - vz) * knots / 8192.0).floor() } else { 0.0 };
        if a.gear_down {
            turb += ((knots - 200.0) / 16.0).clamp(0.0, 32.0);
        }
        if a.flaps {
            turb = (turb / 2.0).floor();
        }
        turb += a.damage.stick_jitter;
        if turb > 0.0 && air {
            roll_input += rand::random_range(0.0..turb) - turb / 2.0;
            pitch_input += (rand::random_range(0.0..turb) - turb / 2.0) / 2.0;
        }

        // --- auto pull-up ("barometric altimeter", no-crash and easy only)
        if !a.gear_down && pitch_input <= 0.0 && a.velocity > a.stall_speed && k.landing < 2 && roll_b.abs() < 12288.0 {
            let kk = ((a.trim - pitch_b) / 8.0 - vz + 200.0) / 8.0;
            if kk > 0.0 {
                pitch_input = kk.clamp(0.0, 32.0);
            }
        }

        // --- thrust and fuel (engine damage caps the thrust)
        if let Some(t) = controls.throttle {
            a.set_thrust = (t.clamp(0.0, 1.0) * 100.0).round();
        }
        let set = a.damage.throttle_cap.map_or(a.set_thrust, |cap| a.set_thrust.min(cap));
        a.thrust += (set - a.thrust) / k.spool_div * dt;
        if set > a.thrust {
            a.thrust = (a.thrust + k.fs * dt).min(set);
        } else {
            a.thrust = set;
        }
        a.fuel_timer += dt;
        while a.fuel_timer >= 2.0 {
            a.fuel_timer -= 2.0;
            if a.set_thrust != 0.0 {
                a.fuel -= (a.set_thrust * a.set_thrust / k.fuel_div).floor() + 2.0 + a.damage.fuel_leak;
            }
        }
        if a.fuel <= 0.0 {
            a.fuel = 0.0;
            a.thrust = 0.0;
        }

        // --- g-load: bank angle table plus the full pull
        let idx = ((roll_b.abs() as i32 >> 8) & 0x7F) as usize;
        let table_g = tables.roll_gee.get(idx).copied().unwrap_or(16) as f32;
        a.gees = table_g;
        if air {
            a.gees += pitch_input;
        }
        if a.gees > 128.0 {
            a.gees = 128.0;
            pitch_input = (128.0 - table_g).clamp(0.0, pitch_input.max(0.0));
        }

        // --- speed, drag, stall, lift
        let mut s = (a.thrust - pitch.sin() * k.gravity_term) * k.thrust_to_knots;
        let mut cs = k.stall_knots * ((a.altitude / 64.0).floor() + 1024.0) / 1024.0;
        s *= ((vz / 128.0).floor() + 1024.0) / 1024.0;
        s *= (100.0 - (a.fuel / 512.0).floor()) / 90.0;
        s *= (128.0 - a.gees) / 128.0;
        cs = ((a.gees.max(0.0) * 4.0).sqrt().floor() * cs / 8.0).abs();
        if a.flaps {
            cs -= cs / 4.0;
            s -= s / 16.0;
        }
        if a.gear_down {
            s -= s / 8.0;
        }
        a.stall_speed = cs * 27.0;
        let target = s.clamp(0.0, k.max_knots) * 27.0;
        a.velocity += (target - a.velocity) / k.speed_lag * dt;
        let x = ((target - a.velocity) / 4.0).clamp(0.0, a.stall_speed);
        a.lift = ((a.stall_speed - x) * 3072.0 / (a.velocity.abs() + 1.0)).min(8192.0);
        a.trim = roll.cos() * (a.lift - 512.0);

        // --- brakes (ground brakes depend on the landing skill; airborne speed brake)
        if a.brakes {
            if !air {
                a.velocity -= (4.0 - l) * 216.0 * dt;
            } else {
                a.velocity -= a.velocity / 16.0 * dt;
            }
        }
        a.velocity = a.velocity.max(0.0);

        // --- rotation
        let mut yaw = (roll.sin() * a.gees * 16.0 * 128.0) / ((a.velocity / 256.0).floor() + 32.0) * pitch.cos();
        // Rudder (extension): up to 10 deg/s in the air.
        let rudder = controls.yaw.clamp(-1.0, 1.0);
        yaw += rudder * 1820.0;
        if !air {
            // Nosewheel steering: by the stick as in the original, or by the rudder.
            yaw = if rudder != 0.0 { rudder * 8064.0 } else { roll_input * 64.0 };
            roll_input = 0.0;
            if a.knots() < cs {
                pitch_input = 0.0;
            }
        }
        let d_roll = bam(roll_input * 128.0 * dt);
        let d_pitch = bam(pitch_input * 128.0 * dt);
        let d_yaw = bam(yaw * dt);
        a.orientation = (Quat::from_rotation_y(-d_yaw) * a.orientation * Quat::from_rotation_x(d_pitch) * Quat::from_rotation_z(-d_roll)).normalize();

        // Stall: the nose drops (per original frame, scaled with fs).
        if air && a.stall_speed > a.velocity {
            let shift = (if k.landing == 2 { 2.0 } else { 4.0 }) / if a.damage.heavy_stall { 2.0 } else { 1.0 };
            let drop = bam((a.stall_speed - a.velocity) / shift * k.fs * dt);
            let (h, p, r) = a.euler();
            a.orientation = Aircraft::from_euler(h, (p - drop).max(-1.5), r);
        }
        if !air {
            let (h, p, _) = a.euler();
            let p = if p < 0.0 || a.knots() < cs { 0.0 } else { p };
            a.orientation = Aircraft::from_euler(h, p, 0.0);
        }

        // --- integration
        let (head2, pitch2, roll2) = a.euler();
        let prev_alt = a.altitude;
        a.climb_rate = a.velocity / 16.0 * (pitch2 - bam(a.trim)).sin();
        a.altitude += a.climb_rate * dt;
        let horiz = pitch2.cos() * a.velocity / 16.0;
        let (mut east, mut north, _) = bevy_to_wu(pos.0);
        east += (horiz * head2.sin() * dt) as f64;
        north += (horiz * head2.cos() * dt) as f64;

        // --- landing corridor at the nearest friendly base
        let (corridor, ground) = theater.map(|t| landing_corridor(t, east, north, k.landing)).unwrap_or((false, 0.0));
        a.in_landing_corridor = corridor;
        a.ground_altitude = ground;
        a.altitude = a.altitude.clamp(a.ground_altitude, 60000.0);
        let on_ground_now = a.altitude <= a.ground_altitude;
        if on_ground_now && prev_alt > a.ground_altitude {
            touchdown(a, roll2, l);
        }
        if on_ground_now {
            a.climb_rate = 0.0;
            if !a.in_landing_corridor && a.knots() > 50.0 && (k.landing != 0 || a.damage.fragile) {
                a.ended = Some(FlightEnd::Crash);
                info!("crash: ground contact outside a runway at {:.0} kt", a.knots());
            }
        } else {
            a.has_flown = true;
        }
        a.on_ground = on_ground_now;

        // Stopped on a friendly runway after flying: "Safe Landing" ends the mission.
        if a.on_ground && a.in_landing_corridor && a.has_flown && a.knots() <= 1.0 {
            a.stopped_timer += dt;
            if a.stopped_timer > 2.0 && a.ended.is_none() {
                a.ended = Some(FlightEnd::Landed);
                info!("safe landing");
            }
        } else {
            a.stopped_timer = 0.0;
        }

        a.roll_input = roll_input;
        a.pitch_input = pitch_input;
        // The 3D view uses the compressed altitude, like the original.
        pos.0 = wu_to_bevy(east, north, a.view_z() as f64);
        tf.rotation = a.orientation;
    }
}

/// Touchdown in a landing corridor.
fn touchdown(a: &mut Aircraft, roll: f32, l: f32) {
    if !a.in_landing_corridor {
        return;
    }
    let sink = (-a.climb_rate).max(0.0);
    let limit = if a.ground_altitude > 0.0 { 256.0 } else { 128.0 };
    let max_roll = if l > 0.0 { bam(24.0 / l * 256.0) } else { f32::MAX };
    if sink * l > limit || (l > 0.0 && (!a.gear_down || roll.abs() > max_roll)) {
        a.ended = Some(FlightEnd::BadLanding);
        info!("bad landing: sink {sink:.0} gear {} roll {:.1}", a.gear_down, roll.to_degrees());
    }
}

/// (in corridor, ground altitude): runway box around friendly bases (flags 0x500, not 0x800):
/// |dx| <= 5, |dy| <= 64 map units (+4/+8 on easy); carriers 8 x 30 at deck height 128.
fn landing_corridor(t: &Theater, east: f64, north: f64, landing: u8) -> (bool, f32) {
    let (mx, my) = (east / 32.0, 32768.0 - north / 32.0);
    let (ex, ey) = if landing == 1 { (4.0, 8.0) } else { (0.0, 0.0) };
    for o in &t.world.objects {
        let flags = o.target_flags as u16;
        if flags & 0x500 == 0 || flags & 0x800 != 0 {
            continue;
        }
        let carrier = flags & 0x200 != 0;
        let (bx, by) = if carrier { (8.0, 30.0) } else { (5.0, 64.0) };
        if (mx - o.x as f64).abs() <= bx + ex && (my - o.y as f64).abs() <= by + ey {
            return (true, if carrier { 128.0 } else { 0.0 });
        }
    }
    (false, 0.0)
}
