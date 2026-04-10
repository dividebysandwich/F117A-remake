use bevy::prelude::*;
use bevy_rapier3d::prelude::*;
use std::collections::HashMap;
use lazy_static::lazy_static;

use crate::definitions::*;
use crate::player::*;
use crate::missile::*;
use crate::targeting::SensorTarget;
use crate::targeting::Targetable;

#[derive(Debug, Eq, PartialEq, Hash)]
pub enum AircraftType {
    F117A,
    MIG29
}

lazy_static! {
    pub static ref MAXTHRUST: HashMap<AircraftType, f32> = {
        let mut m = HashMap::new();
        m.insert(AircraftType::F117A, 200.0);
        m.insert(AircraftType::MIG29, 230.0);
        m
    };
    pub static ref STALL_SPEED: HashMap<AircraftType, f32> = {
        let mut m = HashMap::new();
        m.insert(AircraftType::F117A, 10.0);
        m.insert(AircraftType::MIG29, 9.0);
        m
    };
    pub static ref MAXFORCES_ROLL: HashMap<AircraftType, f32> = {
        let mut m = HashMap::new();
        m.insert(AircraftType::F117A, 4.0);
        m.insert(AircraftType::MIG29, 6.0);
        m
    };
    pub static ref MAXFORCES_PITCH: HashMap<AircraftType, f32> = {
        let mut m = HashMap::new();
        m.insert(AircraftType::F117A, 2.5);
        m.insert(AircraftType::MIG29, 3.5);
        m
    };
    pub static ref MAXFORCES_YAW: HashMap<AircraftType, f32> = {
        let mut m = HashMap::new();
        m.insert(AircraftType::F117A, 1.5);
        m.insert(AircraftType::MIG29, 2.5);
        m
    };
}

// ===============================================================
// Flight model constants
// ===============================================================

/// Custom gravity force (applied manually since GravityScale = 0).
const WEIGHT: f32 = 98.0;

/// Polhamus Leading-Edge Suction Analogy constants.
const POTENTIAL_LIFT_FACTOR: f32 = 1.65;
const VORTEX_LIFT_FACTOR: f32 = 3.05;

/// Oswald span efficiency factor for induced drag.
const OSWALD_EFFICIENCY: f32 = 0.85;

/// Ground effect ceiling (game units altitude) and boost fraction.
const GE_CEIL: f32 = 5.0;
const GE_BOOST: f32 = 0.20;

/// Vertical velocity damping — prevents excessive phugoid oscillation.
const VERTICAL_DAMPING: f32 = 8.0;

/// Adverse yaw: roll input induces slight yaw.
const ADVERSE_YAW_FACTOR: f32 = 0.15;

/// Force-at-point steering factor (scales control-surface torque by airspeed).
const STEERING_FACTOR: f32 = 0.02;

/// Sideslip drag factor (extra drag when aircraft is not flying straight).
const SIDESLIP_DRAG_FACTOR: f32 = 1.0;

/// Sea-level air density (kg/m^3), used as reference for density ratio.
const RHO_SEA_LEVEL: f32 = 1.2041;

/// Control input parameters.
const CONTROL_CENTER_RATE: f32 = 10.0;
const INPUT_RAMP: f32 = 6.0;

// ===============================================================
// Per-aircraft aerodynamic configuration
// ===============================================================

struct AeroConfig {
    wing_area: f32,
    wingspan: f32,
    cd_zero: f32, // zero-lift drag coefficient
}

fn aero_config(aircraft_type: &AircraftType) -> AeroConfig {
    match aircraft_type {
        AircraftType::F117A => AeroConfig {
            wing_area: 2.8,
            wingspan: 3.5,
            cd_zero: 0.030,
        },
        AircraftType::MIG29 => AeroConfig {
            wing_area: 2.2,
            wingspan: 3.0,
            cd_zero: 0.025,
        },
    }
}

// ===============================================================
// Aircraft component
// ===============================================================

#[derive(Component)]
pub struct Aircraft {
    pub name: String,
    pub aircraft_type: AircraftType,
    pub fuel: f32,
    pub health: f32,
    pub throttle: f32,
    pub thrust_force: f32,
    pub speed: f32,
    pub speed_knots: f32,
    pub altitude: f32,
    pub roll_force: f32,
    pub yaw_force: f32,
    pub pitch_force: f32,
}

impl Default for Aircraft {
    fn default() -> Self {
        Aircraft {
            name: String::from("Default"),
            aircraft_type: AircraftType::F117A,
            fuel: 20000.0, health: 100.0,
            throttle: 0.0, thrust_force: 0.0,
            speed: 0.0, speed_knots: 0.0, altitude: 0.0,
            roll_force: 0.0, yaw_force: 0.0, pitch_force: 0.0,
        }
    }
}

// ===============================================================
// US Standard Atmosphere 1976
// ===============================================================

#[allow(unused)]
#[derive(Debug)]
pub struct AtmosphereProperties {
    pub pressure: f64,    // Pascals
    pub density: f64,     // kg/m^3
    pub temperature: f64, // Kelvin
}

/// Compute atmospheric properties at a given geometric altitude in meters.
pub fn atmosphere(altitude_m: f64) -> AtmosphereProperties {
    const G0: f64 = 9.80665;      // gravity at sea level (m/s^2)
    const M: f64 = 0.0289644;     // molar mass of air (kg/mol)
    const R: f64 = 8.31432;       // universal gas constant (J/(mol*K))
    const EARTH_RADIUS: f64 = 6356766.0;

    // Atmospheric layers: (base geopotential altitude, base pressure, base temp, lapse rate)
    const LAYERS: [(f64, f64, f64, f64); 6] = [
        (0.0,     101325.0,  288.15, -0.0065),  // Troposphere
        (11000.0, 22632.1,   216.65,  0.0),     // Tropopause
        (20000.0, 5474.89,   216.65,  0.001),   // Lower Stratosphere
        (32000.0, 868.019,   228.65,  0.0028),  // Upper Stratosphere
        (47000.0, 110.906,   270.65,  0.0),     // Stratopause
        (51000.0, 66.9388,   270.65, -0.0028),  // Lower Mesosphere
    ];

    // Convert geometric altitude to geopotential altitude
    let h = (EARTH_RADIUS * altitude_m) / (EARTH_RADIUS + altitude_m);

    // Find the correct atmospheric layer
    let mut base_h = LAYERS[0].0;
    let mut base_p = LAYERS[0].1;
    let mut base_t = LAYERS[0].2;
    let mut lapse_rate = LAYERS[0].3;

    for layer in LAYERS.iter().rev() {
        if h >= layer.0 {
            base_h = layer.0;
            base_p = layer.1;
            base_t = layer.2;
            lapse_rate = layer.3;
            break;
        }
    }

    let temperature = base_t + lapse_rate * (h - base_h);

    let pressure = if lapse_rate == 0.0 {
        // Isothermal layer
        base_p * (-G0 * M * (h - base_h) / (R * base_t)).exp()
    } else {
        // Gradient layer
        base_p * (base_t / temperature).powf(G0 * M / (R * lapse_rate))
    };

    let density = (pressure * M) / (R * temperature);

    AtmosphereProperties { pressure, density, temperature }
}

// ===============================================================
// Aerodynamic helpers
// ===============================================================

/// Lift coefficient using the Polhamus Leading-Edge Suction Analogy.
/// Suitable for delta-wing and highly-swept aircraft (like the F-117A).
fn lift_coeff(alpha_deg: f32) -> f32 {
    let alpha = alpha_deg.to_radians();
    let sin_a = alpha.sin();
    let cos_a = alpha.cos();

    let cl_potential = POTENTIAL_LIFT_FACTOR * sin_a * cos_a.powi(2);
    let cl_vortex = VORTEX_LIFT_FACTOR * sin_a.powi(2) * cos_a;

    cl_potential + cl_vortex
}

// ===============================================================
// Steering — force-at-point control surfaces
// ===============================================================

/// Control surface attachment points in local space.
/// Coordinate system: +X = forward, +Y = up, +Z = starboard.
struct ControlSurfaceConfig {
    pitch_point: Vec3,           // elevator / V-tail (behind CG)
    yaw_point: Vec3,             // rudder / V-tail (behind and above CG)
    roll_port_point: Vec3,       // port aileron (-Z = port)
    roll_starboard_point: Vec3,  // starboard aileron (+Z = starboard)
}

const CONTROL_SURFACES: ControlSurfaceConfig = ControlSurfaceConfig {
    pitch_point:           Vec3::new(-4.0, 0.0, 0.0),
    yaw_point:             Vec3::new(-4.0, 0.5, 0.0),
    roll_port_point:       Vec3::new(-0.5, 0.0, -2.0),
    roll_starboard_point:  Vec3::new(-0.5, 0.0, 2.0),
};

/// Compute control-surface torques (and small net forces) using force-at-point.
/// Each control surface applies a force at its offset from the center of mass.
/// The cross product of offset x force gives the torque in local space, which
/// is then scaled by airspeed (control authority depends on airflow) and
/// transformed to world space.
fn steering(rot: Quat, airspeed: f32, ac: &Aircraft, ef: &mut ExternalForce) {
    let cfg = &CONTROL_SURFACES;

    // Roll: opposite vertical forces at wing tips.
    // Port wing up, starboard wing down for positive roll_force (right bank).
    let roll_port_force = Vec3::new(0.0, ac.roll_force, 0.0);
    let roll_starboard_force = Vec3::new(0.0, -ac.roll_force, 0.0);

    // Pitch: vertical force at tail — downforce at tail for positive pitch_force (nose up).
    let pitch_force = Vec3::new(0.0, -ac.pitch_force, 0.0);

    // Yaw: lateral force at rudder, including adverse yaw from roll.
    let speed_ratio = (ac.speed / 25.0).clamp(0.0, 1.0);
    let adverse_yaw = -ac.roll_force * ADVERSE_YAW_FACTOR * speed_ratio;
    let yaw_force = Vec3::new(0.0, 0.0, ac.yaw_force + adverse_yaw);

    // Compute torques via cross product in local space
    let torque =
        cfg.roll_port_point.cross(roll_port_force)
        + cfg.roll_starboard_point.cross(roll_starboard_force)
        + cfg.pitch_point.cross(pitch_force)
        + cfg.yaw_point.cross(yaw_force);

    // Scale by airspeed (with a minimum floor for low-speed controllability)
    let effective_airspeed = airspeed.max(3.0);
    let scaled_torque = torque * effective_airspeed * STEERING_FACTOR;
    ef.torque += rot * scaled_torque;

    // Net force contribution from control surfaces (small but physical).
    // Roll forces cancel out; pitch and yaw contribute small forces.
    let net_force = roll_port_force + roll_starboard_force + pitch_force + yaw_force;
    ef.force += rot * net_force * airspeed * STEERING_FACTOR;
}

// ===============================================================
// Flight physics
//
// Replaces the old split-lift model with a physically-based approach:
//
//  - Lift from actual angle of attack via the Polhamus analogy.
//    No separate "base lift" — the AoA between the aircraft nose
//    and the velocity vector determines lift magnitude.
//    Roll + pull-back = banked turn (aircraft-up tilts → lift
//    has horizontal component). Roll alone → altitude loss
//    (reduced vertical lift component).
//
//  - Air density from the US Standard Atmosphere 1976.
//    Higher altitude = thinner air = less lift, drag, and thrust.
//
//  - Drag breakdown: zero-lift parasitic drag + induced drag from
//    lift + sideslip drag. All density-dependent.
//
//  - Control authority via force-at-point on control surfaces.
//    Torque scales with airspeed — sluggish at low speed,
//    responsive at cruise.
//
//  - Vertical damping retained as a gameplay aid to prevent
//    excessive phugoid oscillation.
// ===============================================================

pub fn update_aircraft_forces(
    mut query: Query<(&mut ExternalForce, &Velocity, &Transform, &mut Aircraft)>,
    time: Res<Time>,
) {
    for (mut ef, velocity, transform, mut ac) in query.iter_mut() {
        let dt = time.delta_secs();
        ac.altitude = transform.translation.y * 10.0;
        ac.speed = velocity.linvel.length();
        ac.speed_knots = ac.speed * 10.0;

        let rot = transform.rotation;
        let aircraft_up  = rot * Vec3::Y;
        let aircraft_fwd = rot * Vec3::X;
        let aircraft_right = rot * Vec3::Z; // starboard

        let vel = velocity.linvel;
        let vel_dir = vel.normalize_or_zero();
        let speed = vel.length();

        // -- Atmospheric density --
        // Game altitude is in display-feet (Y * 10); convert to metres.
        let altitude_m = (transform.translation.y * 3.048).max(0.0) as f64;
        let rho = atmosphere(altitude_m).density as f32;
        let rho_ratio = rho / RHO_SEA_LEVEL;

        // -- Thrust --
        let max_thr = *MAXTHRUST.get(&ac.aircraft_type).unwrap();
        let target = ac.throttle * max_thr;
        let ramp = 20.0 * dt;
        if ac.thrust_force < target {
            ac.thrust_force = (ac.thrust_force + ramp).min(target);
        } else {
            ac.thrust_force = (ac.thrust_force - ramp).max(target);
        }
        // Jet thrust decreases with altitude (less dense air for the engines).
        let thrust_vec = aircraft_fwd * ac.thrust_force * rho_ratio;

        // -- Weight (custom gravity) --
        let weight_vec = Vec3::new(0.0, -WEIGHT, 0.0);

        // -- Angle of attack --
        let sin_aoa = aircraft_fwd.cross(vel_dir).dot(aircraft_right);
        let cos_aoa = aircraft_fwd.dot(vel_dir);
        let alpha = -sin_aoa.atan2(cos_aoa).to_degrees();

        // -- Lift --
        let cl = lift_coeff(alpha);
        let aero = aero_config(&ac.aircraft_type);
        let aspect_ratio = aero.wingspan * aero.wingspan / aero.wing_area;

        // Airspeed: forward component of velocity (wings need forward airflow).
        let airspeed = aircraft_fwd.dot(vel_dir).clamp(0.0, 1.0) * speed;

        // L = Cl * rho * (v^2 / 2) * S
        let lift_mag = cl * rho * (airspeed.powi(2) * 0.5) * aero.wing_area;

        // Ground effect: extra lift close to the ground.
        let ge = if transform.translation.y < GE_CEIL {
            1.0 + (1.0 - transform.translation.y / GE_CEIL).max(0.0) * GE_BOOST
        } else {
            1.0
        };

        let lift_vec = aircraft_up * lift_mag * ge;

        // -- Drag --
        let q = 0.5 * rho * speed.powi(2); // dynamic pressure

        // Induced drag coefficient: Cd_i = Cl^2 / (pi * AR * e)
        let cd_i = cl.powi(2) / (std::f32::consts::PI * aspect_ratio * OSWALD_EFFICIENCY);

        // Sideslip: cross product magnitude gives sin(sideslip angle).
        let sideslip = aircraft_fwd.cross(vel_dir).length();
        let cd_sideslip = sideslip * SIDESLIP_DRAG_FACTOR;

        // Total drag = dynamic pressure * (Cd_0 + Cd_i + Cd_sideslip) * S
        let drag_mag = q * (aero.cd_zero + cd_i + cd_sideslip) * aero.wing_area;
        let drag_vec = if speed > 0.1 {
            -vel_dir * drag_mag
        } else {
            Vec3::ZERO
        };

        // -- Vertical damping --
        // Damps vertical velocity to prevent phugoid oscillation.
        // Scales with speed so a stalled aircraft can descend freely.
        let stall = *STALL_SPEED.get(&ac.aircraft_type).unwrap();
        let speed_ratio = (speed / stall).clamp(0.0, 1.0);
        let v_damp = Vec3::new(0.0, -vel.y * VERTICAL_DAMPING * speed_ratio, 0.0);

        // -- Sum forces --
        ef.force = thrust_vec + weight_vec + lift_vec + drag_vec + v_damp;
        ef.torque = Vec3::ZERO;

        // -- Control surfaces (force-at-point steering) --
        steering(rot, airspeed, &ac, &mut ef);
    }
}

// ===============================================================
// Weapon controls
// ===============================================================

pub fn update_player_weapon_controls(
    aircrafts: Query<(&Aircraft, Entity, &Transform, &Velocity), With<Player>>,
    asset_server: Res<AssetServer>,
    mut commands: Commands,
    input: Res<ButtonInput<KeyCode>>,
    targets: Query<(Entity, &Transform), With<SensorTarget>>,
) {
    if input.just_pressed(KeyCode::Space) {
        for (target, target_transform) in targets.iter() {
            info!("Firing missile");
            commands.spawn(AudioPlayer::new(asset_server.load("sounds/internallaunch.ogg")));
            for (_ac, entity, transform, vel) in aircrafts.iter() {
                commands.spawn(SceneRoot(asset_server.load("models/weapons/AGM-65.glb#Scene0")))
                .insert(Missile {
                    launching_vehicle: entity, target: target,
                    target_transform: *target_transform, ..default()
                }).insert(transform.clone())
                .insert(Velocity { linvel: vel.linvel, ..default() })
                .insert(ExternalForce { ..default() })
                .insert(Collider::cuboid(0.2, 0.05, 0.2))
                .insert(ActiveEvents::COLLISION_EVENTS)
                .insert(CollisionGroups::new(
                    Group::from_bits_truncate(COLLISION_MASK_MISSILE),
                    Group::from_bits_truncate(
                        COLLISION_MASK_TERRAIN | COLLISION_MASK_AIRCRAFT |
                        COLLISION_MASK_GROUNDVEHICLE | COLLISION_MASK_MISSILE)))
                .insert(Ccd::enabled())
                .insert(Restitution::coefficient(0.4))
                .insert(RigidBody::Dynamic)
                .insert(GravityScale(1.0))
                .insert(Damping { linear_damping: 0.3, angular_damping: 1.0 })
                .insert(ColliderMassProperties::Density(15.0))
                .insert(Targetable);
            }
        }
    }
}

// ===============================================================
// Player input
// ===============================================================

fn slew_to_zero(v: f32, rate: f32, dt: f32) -> f32 {
    if v > 0.0 { (v - rate * dt).max(0.0) } else { (v + rate * dt).min(0.0) }
}

pub fn update_player_aircraft_controls(
    mut aircrafts: Query<(&mut Aircraft, &mut Transform), With<Player>>,
    input: Res<ButtonInput<KeyCode>>, time: Res<Time>,
) {
    let dt = time.delta_secs();
    for (mut ac, mut transform) in aircrafts.iter_mut() {
        if input.pressed(KeyCode::KeyW) { ac.throttle = (ac.throttle + 0.4 * dt).min(1.0); }
        if input.pressed(KeyCode::KeyS) { ac.throttle = (ac.throttle - 0.4 * dt).max(0.0); }

        let mp = *MAXFORCES_PITCH.get(&ac.aircraft_type).unwrap();
        let mr = *MAXFORCES_ROLL.get(&ac.aircraft_type).unwrap();
        let my = *MAXFORCES_YAW.get(&ac.aircraft_type).unwrap();

        if      input.pressed(KeyCode::ArrowUp)   { ac.pitch_force = (ac.pitch_force - INPUT_RAMP * dt).max(-mp); }
        else if input.pressed(KeyCode::ArrowDown)  { ac.pitch_force = (ac.pitch_force + INPUT_RAMP * dt).min(mp); }
        else { ac.pitch_force = slew_to_zero(ac.pitch_force, CONTROL_CENTER_RATE, dt); }

        if      input.pressed(KeyCode::ArrowLeft)  { ac.roll_force = (ac.roll_force - INPUT_RAMP * dt).max(-mr); }
        else if input.pressed(KeyCode::ArrowRight) { ac.roll_force = (ac.roll_force + INPUT_RAMP * dt).min(mr); }
        else { ac.roll_force = slew_to_zero(ac.roll_force, CONTROL_CENTER_RATE, dt); }

        if      input.pressed(KeyCode::KeyD) { ac.yaw_force = (ac.yaw_force - INPUT_RAMP * dt).max(-my); }
        else if input.pressed(KeyCode::KeyA) { ac.yaw_force = (ac.yaw_force + INPUT_RAMP * dt).min(my); }
        else { ac.yaw_force = slew_to_zero(ac.yaw_force, CONTROL_CENTER_RATE, dt); }

        if input.pressed(KeyCode::KeyL) { transform.rotate_y(-0.01); }
        if input.pressed(KeyCode::KeyJ) { transform.rotate_y(0.01); }
        if input.pressed(KeyCode::KeyI) { transform.rotate_x(-0.01); }
        if input.pressed(KeyCode::KeyK) { transform.rotate_x(0.01); }
    }
}
