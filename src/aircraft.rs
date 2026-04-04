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
        m.insert(AircraftType::F117A, 90.0);
        m.insert(AircraftType::MIG29, 105.0);
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

// ── Flight-model constants ──

const WEIGHT: f32 = 98.0;
/// 2% surplus so numerical noise doesn't cause slow sink in level flight
const LIFT_SPEED_CAP: f32 = 1.02;
/// Extra lift per unit pitch-pull (fraction of WEIGHT).
/// 1.5 means half-pull in a 45° bank just about maintains altitude.
const AOA_LIFT_FRACTION: f32 = 1.5;
const DRAG_COEFF: f32 = 0.012;
const AOA_DRAG_FACTOR: f32 = 0.25;
const MANEUVER_DRAG_FACTOR: f32 = 0.04;
const CONTROL_REF_SPEED: f32 = 25.0;
const MIN_CONTROL_EFF: f32 = 0.05;
const GE_CEIL: f32 = 5.0;
const GE_BOOST: f32 = 0.20;
/// Vertical velocity damping — absorbs the 2% lift surplus so the
/// aircraft flies level, and dampens climb/descent oscillations.
const VERTICAL_DAMPING: f32 = 12.0;
const ADVERSE_YAW_FACTOR: f32 = 0.15;
/// On the ground with throttle applied, engine exhaust over the tail
/// provides this fraction of control effectiveness regardless of airspeed.
const GROUND_PITCH_BOOST: f32 = 0.25;

const CONTROL_CENTER_RATE: f32 = 10.0;
const INPUT_RAMP: f32 = 6.0;

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

// ═══════════════════════════════════════════════════════════════
// Flight model
//
// Lift direction is PURELY along aircraft-up (no world-up blend).
// This eliminates the sideways-slide artifact after banked turns.
//
// To compensate for bank-angle altitude loss, AOA_LIFT_FRACTION is
// high (1.5) so small amounts of back-pressure easily maintain
// altitude in moderate banks.
//
// Takeoff requires pulling back (rotation).  On the ground, base
// lift is suppressed unless the pilot commands pitch-up.  A ground
// pitch-boost gives the elevator enough authority to rotate the
// nose even at low airspeed (simulates engine exhaust over the tail).
// ═══════════════════════════════════════════════════════════════

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
        let aircraft_up = rot * Vec3::Y;
        let aircraft_fwd = rot * Vec3::X;

        // ── Thrust ──
        let max_thr = *MAXTHRUST.get(&ac.aircraft_type).unwrap();
        let target = ac.throttle * max_thr;
        let ramp = 20.0 * dt;
        if ac.thrust_force < target { ac.thrust_force = (ac.thrust_force + ramp).min(target); }
        else { ac.thrust_force = (ac.thrust_force - ramp).max(target); }
        let thrust_vec = aircraft_fwd * ac.thrust_force;

        // ── Weight ──
        let weight_vec = Vec3::new(0.0, -WEIGHT, 0.0);

        // ── Lift ──
        let stall = *STALL_SPEED.get(&ac.aircraft_type).unwrap();
        let speed_ratio = (ac.speed / stall).min(LIFT_SPEED_CAP);

        let ge = if transform.translation.y < GE_CEIL {
            1.0 + (1.0 - transform.translation.y / GE_CEIL).max(0.0) * GE_BOOST
        } else { 1.0 };

        let max_pitch = *MAXFORCES_PITCH.get(&ac.aircraft_type).unwrap();
        let pitch_pull = (-ac.pitch_force / max_pitch).clamp(-0.3, 1.0);

        // On/near the ground, base lift is suppressed — the pilot must
        // pull back to generate takeoff lift.  Once airborne (y > ~0)
        // the auto-trim provides full base lift for level flight.
        // Uses max(airborne, pitch_pull) so EITHER being in the air OR
        // pulling back is sufficient.
        let airborne = ((transform.translation.y + 1.0) / 0.3).clamp(0.0, 1.0);
        let rotation_factor = airborne.max(pitch_pull.max(0.0));

        let base_lift = WEIGHT * LIFT_SPEED_CAP * speed_ratio * ge * rotation_factor;
        let aoa_extra = pitch_pull * AOA_LIFT_FRACTION * WEIGHT * speed_ratio;
        let lift_mag = (base_lift + aoa_extra).max(0.0);

        // Lift is PURELY along aircraft-up.  No world-up blend.
        // This ensures no sideways force artifact after banked turns.
        let lift_vec = aircraft_up * lift_mag;

        // ── Vertical velocity damping ──
        let v_damp = Vec3::new(0.0, -velocity.linvel.y * VERTICAL_DAMPING * speed_ratio, 0.0);

        // ── Drag ──
        let parasitic = DRAG_COEFF * ac.speed * ac.speed;
        let maneuver_load = ac.roll_force.abs() + ac.pitch_force.abs() + ac.yaw_force.abs();
        let maneuver_drag = MANEUVER_DRAG_FACTOR * maneuver_load * ac.speed;
        let aoa_drag = pitch_pull.abs() * ac.speed * AOA_DRAG_FACTOR;
        let drag_vec = if ac.speed > 0.1 {
            -velocity.linvel.normalize() * (parasitic + maneuver_drag + aoa_drag)
        } else { Vec3::ZERO };

        // ── Sum ──
        ef.force = thrust_vec + weight_vec + lift_vec + v_damp + drag_vec;

        // ── Control authority ──
        // Airspeed-based effectiveness for all axes
        let base_eff = (ac.speed / CONTROL_REF_SPEED).clamp(MIN_CONTROL_EFF, 1.3);

        // Pitch gets a ground boost: engine exhaust over the tail gives
        // the elevator authority to rotate the nose even at low airspeed.
        let on_ground = transform.translation.y < 0.0 && ac.thrust_force > 5.0;
        let pitch_eff = if on_ground {
            (base_eff + GROUND_PITCH_BOOST).min(1.3)
        } else {
            base_eff
        };

        // At extreme bank, elevator becomes rudder → reduced pitch authority
        let bank_cos = aircraft_up.y.abs();
        let pitch_bank_factor = bank_cos.clamp(0.2, 1.0);

        let adverse_yaw = -ac.roll_force * ADVERSE_YAW_FACTOR * speed_ratio;

        ef.torque = rot * Vec3::new(
            ac.roll_force  * base_eff,
            (ac.yaw_force + adverse_yaw) * base_eff,
            ac.pitch_force * pitch_eff * pitch_bank_factor,
        );
    }
}

// ═══════════════════════════════════════════════════════════════
// Weapons
// ═══════════════════════════════════════════════════════════════

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

// ═══════════════════════════════════════════════════════════════
// Player input
// ═══════════════════════════════════════════════════════════════

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
