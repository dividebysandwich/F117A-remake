//! Side stick (right console) and throttle (left console).
//!
//! VR: squeeze the grip near the stick to take it. Tilting the controller (relative to how it
//! was held when grabbed) deflects pitch (tilt back = nose up) and roll; twisting it about the
//! stick's axis is the rudder. Letting go re-centres the stick.
//! Squeeze near the throttle handle and slide it along its rails: forward = more thrust; it stays where it is
//! released, with a haptic tick every 10 %.
//! The controller buttons are mapped in the input mapping (`InputConfig::vr_buttons`); a trigger
//! only counts while its hand holds the stick or throttle.
//! The ejection handle is pulled up with either hand.
//! Without VR both animate from the keyboard/gamepad controls.

use bevy::camera::visibility::NoFrustumCulling;
use bevy::prelude::*;

use super::{
    BUTTON_PRIMARY, BUTTON_SECONDARY, BUTTON_STICK_DOWN, BUTTON_STICK_LEFT, BUTTON_STICK_RIGHT, BUTTON_STICK_UP, BUTTON_THUMB, CockpitMaterials, Hands,
};
use crate::flight::{Aircraft, Player};
use crate::hotas::{ButtonTarget, InputConfig, VrHand, VrInput};
use crate::input::{Action, ActionEvent, FlightControls};

const STICK_PIVOT: Vec3 = Vec3::new(0.40, -0.48, -0.22);
/// Grip centre above the pivot.
const GRIP_HEIGHT: f32 = 0.11;
/// Full deflection (radians).
const STICK_TILT: f32 = 0.35;
const STICK_TWIST: f32 = 0.45;
/// Rudder deadzone (fraction of full twist); a haptic tick on leaving and re-entering it.
const TWIST_DEADZONE: f32 = 0.15;
/// Re-entering needs this much less twist, so hand jitter at the edge doesn't buzz.
const TWIST_HYSTERESIS: f32 = 0.03;

/// Throttle rail on the left console: idle and full positions of the handle.
const THROTTLE_IDLE: Vec3 = Vec3::new(-0.40, -0.40, -0.12);
const THROTTLE_FULL: Vec3 = Vec3::new(-0.40, -0.40, -0.30);
/// Console top under the throttle rail.
const THROTTLE_RAIL_Y: f32 = -0.4805;

/// Reach for grabbing (metres from the grip centre).
const GRAB_REACH: f32 = 0.10;

#[derive(Component)]
pub(super) struct Stick;

#[derive(Component)]
pub(super) struct ThrottleLever;

#[derive(Default)]
pub(super) struct Grabs {
    stick: Option<StickGrab>,
    throttle: Option<ThrottleGrab>,
    eject: Option<(usize, f32)>,
}

struct StickGrab {
    rot0: Quat,
    /// Twist outside the rudder deadzone.
    twisted: bool,
}

struct ThrottleGrab {
    z0: f32,
    t0: f32,
    /// Last 10 % detent passed (for the haptic ticks).
    notch: i32,
}

/// Throttle detents per full travel (one haptic tick each).
const THROTTLE_NOTCHES: f32 = 10.0;

pub(super) fn spawn_levers(commands: &mut Commands, root: Entity, mats: &CockpitMaterials, meshes: &mut Assets<Mesh>) {
    // Stick: boot, shaft and grip under a pivot entity rotated by the deflection.
    let boot = commands
        .spawn((
            Mesh3d(meshes.add(Cylinder::new(0.035, 0.02))),
            MeshMaterial3d(mats.rubber.clone()),
            Transform::from_translation(STICK_PIVOT + Vec3::new(0.0, -0.005, 0.0)),
            NoFrustumCulling,
        ))
        .id();
    commands.entity(root).add_child(boot);
    let shaft = commands
        .spawn((
            Mesh3d(meshes.add(Cylinder::new(0.008, 0.07))),
            MeshMaterial3d(mats.metal.clone()),
            Transform::from_xyz(0.0, 0.035, 0.0),
            NoFrustumCulling,
        ))
        .id();
    let grip = commands
        .spawn((
            Mesh3d(meshes.add(Capsule3d::new(0.017, 0.07))),
            MeshMaterial3d(mats.rubber.clone()),
            Transform::from_xyz(0.0, GRIP_HEIGHT, 0.0).with_rotation(Quat::from_rotation_x(-0.15)),
            NoFrustumCulling,
        ))
        .id();
    let trigger = commands
        .spawn((
            Mesh3d(mats.unit_cube.clone()),
            MeshMaterial3d(mats.metal.clone()),
            Transform::from_xyz(0.0, GRIP_HEIGHT + 0.02, -0.02).with_scale(Vec3::new(0.008, 0.02, 0.008)),
            NoFrustumCulling,
        ))
        .id();
    let hat = commands
        .spawn((
            Mesh3d(meshes.add(Sphere::new(0.006))),
            MeshMaterial3d(mats.metal.clone()),
            Transform::from_xyz(0.0, GRIP_HEIGHT + 0.055, -0.005),
            NoFrustumCulling,
        ))
        .id();
    let stick = commands
        .spawn((Stick, Transform::from_translation(STICK_PIVOT), Visibility::default()))
        .add_children(&[shaft, grip, trigger, hat])
        .id();
    commands.entity(root).add_child(stick);

    // Throttle: slot plate between two guide rails; a carriage slides along them carrying a
    // straight stem and the handle.
    let slot_len = THROTTLE_IDLE.z - THROTTLE_FULL.z + 0.05;
    let slot_z = (THROTTLE_IDLE.z + THROTTLE_FULL.z) / 2.0;
    let slot = commands
        .spawn((
            Mesh3d(mats.unit_cube.clone()),
            MeshMaterial3d(mats.black.clone()),
            Transform::from_xyz(THROTTLE_IDLE.x, THROTTLE_RAIL_Y, slot_z).with_scale(Vec3::new(0.02, 0.002, slot_len)),
            NoFrustumCulling,
        ))
        .id();
    commands.entity(root).add_child(slot);
    for side in [-1.0, 1.0] {
        let rail = commands
            .spawn((
                Mesh3d(mats.unit_cube.clone()),
                MeshMaterial3d(mats.metal.clone()),
                Transform::from_xyz(THROTTLE_IDLE.x + side * 0.014, THROTTLE_RAIL_Y + 0.003, slot_z)
                    .with_scale(Vec3::new(0.005, 0.006, slot_len)),
                NoFrustumCulling,
            ))
            .id();
        commands.entity(root).add_child(rail);
    }
    // Heights relative to the handle centre.
    let rail_top = THROTTLE_RAIL_Y + 0.006 - THROTTLE_IDLE.y;
    let carriage = commands
        .spawn((
            Mesh3d(mats.unit_cube.clone()),
            MeshMaterial3d(mats.metal.clone()),
            Transform::from_xyz(0.0, rail_top + 0.004, 0.0).with_scale(Vec3::new(0.036, 0.008, 0.035)),
            NoFrustumCulling,
        ))
        .id();
    let stem_bottom = rail_top + 0.008;
    let stem_top = -0.025;
    let arm = commands
        .spawn((
            Mesh3d(mats.unit_cube.clone()),
            MeshMaterial3d(mats.metal.clone()),
            Transform::from_xyz(0.0, (stem_bottom + stem_top) / 2.0, 0.0).with_scale(Vec3::new(0.014, stem_top - stem_bottom, 0.014)),
            NoFrustumCulling,
        ))
        .id();
    let handle = commands
        .spawn((
            Mesh3d(meshes.add(Cuboid::new(0.045, 0.05, 0.06))),
            MeshMaterial3d(mats.panel_grey.clone()),
            Transform::from_xyz(-0.005, 0.0, 0.0),
            NoFrustumCulling,
        ))
        .id();
    let buttons = commands
        .spawn((
            Mesh3d(mats.unit_cube.clone()),
            MeshMaterial3d(mats.rubber.clone()),
            Transform::from_xyz(0.018, 0.02, -0.01).with_scale(Vec3::new(0.012, 0.012, 0.03)),
            NoFrustumCulling,
        ))
        .id();
    let lever = commands
        .spawn((ThrottleLever, Transform::from_translation(THROTTLE_IDLE), Visibility::default()))
        .add_children(&[carriage, arm, handle, buttons])
        .id();
    commands.entity(root).add_child(lever);
}

fn throttle_handle(t: f32) -> Vec3 {
    THROTTLE_IDLE.lerp(THROTTLE_FULL, t.clamp(0.0, 1.0))
}

/// Twist of a rotation about an axis (swing-twist decomposition), radians.
fn twist_about(q: Quat, axis: Vec3) -> f32 {
    let v = Vec3::new(q.x, q.y, q.z);
    let p = axis * v.dot(axis);
    let t = Quat::from_xyzw(p.x, p.y, p.z, q.w);
    if t.length_squared() < 1e-9 {
        return 0.0;
    }
    let t = t.normalize();
    let angle = 2.0 * t.w.clamp(-1.0, 1.0).acos();
    let sign = if Vec3::new(t.x, t.y, t.z).dot(axis) >= 0.0 { 1.0 } else { -1.0 };
    let a = sign * angle;
    // Wrap to -pi..pi.
    (a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

/// Stick angles of a hand rotation since the grab: (pitch: back = +, roll: right = +).
/// The stick axis is tilted as the controller was; twist about the axis is ignored here.
fn stick_angles(delta: Quat) -> (f32, f32) {
    let v = delta * Vec3::Y;
    (v.z.atan2(v.y), v.x.atan2(v.y))
}

/// `Hand::buttons` bit of a controller input (the trigger is analog).
fn vr_bit(input: VrInput) -> u8 {
    match input {
        VrInput::Trigger => 0,
        VrInput::Primary => BUTTON_PRIMARY,
        VrInput::Secondary => BUTTON_SECONDARY,
        VrInput::ThumbClick => BUTTON_THUMB,
        VrInput::ThumbUp => BUTTON_STICK_UP,
        VrInput::ThumbDown => BUTTON_STICK_DOWN,
        VrInput::ThumbLeft => BUTTON_STICK_LEFT,
        VrInput::ThumbRight => BUTTON_STICK_RIGHT,
    }
}

/// VR grabbing: writes `FlightControls` (after the keyboard/gamepad) and the mapped controller
/// buttons.
pub(super) fn grab_levers(
    mut hands: ResMut<Hands>,
    config: Res<InputConfig>,
    mut grabs: Local<Grabs>,
    mut controls: ResMut<FlightControls>,
    mut actions: MessageWriter<ActionEvent>,
    aircraft: Query<&Aircraft, With<Player>>,
) {
    let set_thrust = aircraft.single().map_or(0.0, |a| a.set_thrust / 100.0);
    let [left, right] = &mut hands.hands;

    // --- stick (right hand)
    let grip_centre = STICK_PIVOT + Vec3::Y * GRIP_HEIGHT;
    if !right.active || right.squeeze < 0.4 {
        if grabs.stick.take().is_some() {
            right.haptic = right.haptic.max(0.2);
        }
    } else if grabs.stick.is_none() && right.squeeze > 0.6 && right.grip.translation.distance(grip_centre) < GRAB_REACH {
        grabs.stick = Some(StickGrab { rot0: right.grip.rotation, twisted: false });
        right.haptic = right.haptic.max(0.4);
    }
    if let Some(g) = &mut grabs.stick {
        let delta = right.grip.rotation * g.rot0.inverse();
        let (p, r) = stick_angles(delta);
        controls.pitch = (p / STICK_TILT).clamp(-1.0, 1.0);
        controls.roll = (r / STICK_TILT).clamp(-1.0, 1.0);
        // Twisting clockwise (seen from above) = nose right.
        let twist = twist_about(delta, Vec3::Y);
        let yaw = -twist / STICK_TWIST;
        let edge = if g.twisted { TWIST_DEADZONE - TWIST_HYSTERESIS } else { TWIST_DEADZONE };
        if (yaw.abs() > edge) != g.twisted {
            g.twisted = !g.twisted;
            right.haptic = right.haptic.max(0.15);
        }
        // Rescaled past the deadzone so the rudder starts from zero at its edge.
        controls.yaw = if g.twisted {
            (yaw.signum() * (yaw.abs() - TWIST_DEADZONE).max(0.0) / (1.0 - TWIST_DEADZONE)).clamp(-1.0, 1.0)
        } else {
            0.0
        };
    }

    // --- throttle (left hand)
    let handle = throttle_handle(set_thrust);
    if !left.active || left.squeeze < 0.4 {
        if grabs.throttle.take().is_some() {
            left.haptic = left.haptic.max(0.2);
        }
    } else if grabs.throttle.is_none() && left.squeeze > 0.6 && left.grip.translation.distance(handle) < GRAB_REACH {
        let notch = (set_thrust * THROTTLE_NOTCHES).floor() as i32;
        grabs.throttle = Some(ThrottleGrab { z0: left.grip.translation.z, t0: set_thrust, notch });
        left.haptic = left.haptic.max(0.4);
    }
    if let Some(g) = &mut grabs.throttle {
        let t = (g.t0 + (g.z0 - left.grip.translation.z) / (THROTTLE_IDLE.z - THROTTLE_FULL.z)).clamp(0.0, 1.0);
        // A light tick at each detent, a firm one at the idle and full stops.
        let before = set_thrust;
        if (t >= 1.0 && before < 1.0) || (t <= 0.0 && before > 0.0) {
            left.haptic = left.haptic.max(0.5);
        }
        // Hysteresis so hand jitter on a detent doesn't buzz.
        let pos = t * THROTTLE_NOTCHES;
        if pos >= (g.notch + 1) as f32 + 0.1 || pos < g.notch as f32 - 0.1 {
            g.notch = pos.floor() as i32;
            left.haptic = left.haptic.max(0.15);
        }
        controls.throttle = Some(t);
    }

    // --- mapped buttons; a trigger only while its hand holds the throttle / stick (otherwise it
    // presses the cockpit control under the fingertip)
    for (side, hand, holding) in [(VrHand::Left, &*left, grabs.throttle.is_some()), (VrHand::Right, &*right, grabs.stick.is_some())] {
        if !hand.active {
            continue;
        }
        for m in config.vr_buttons.iter().filter(|m| m.hand == side) {
            let (held, pressed) = match m.input {
                VrInput::Trigger if !holding => continue,
                VrInput::Trigger => (hand.trigger > 0.7, hand.trigger_pulled()),
                input => (hand.held(vr_bit(input)), hand.just_pressed(vr_bit(input))),
            };
            match m.target {
                ButtonTarget::Gun => controls.gun |= held,
                ButtonTarget::Action(a) if pressed => {
                    actions.write(ActionEvent(a));
                }
                ButtonTarget::Action(_) => {}
            }
        }
    }

    // --- ejection handle: grab with either hand and pull up 6 cm
    let handle = super::controls::EJECT_HANDLE;
    for (i, h) in [&mut *left, &mut *right].into_iter().enumerate() {
        let is_grabbing_other = (i == 1 && grabs.stick.is_some()) || (i == 0 && grabs.throttle.is_some());
        match grabs.eject {
            Some((who, y0)) if who == i => {
                if !h.active || h.squeeze < 0.4 {
                    grabs.eject = None;
                } else if h.grip.translation.y - y0 > 0.06 {
                    actions.write(ActionEvent(Action::Eject));
                    h.haptic = 1.0;
                    grabs.eject = None;
                }
            }
            None if h.active && !is_grabbing_other && h.squeeze > 0.6 && h.grip.translation.distance(handle) < 0.07 => {
                grabs.eject = Some((i, h.grip.translation.y));
                h.haptic = h.haptic.max(0.3);
            }
            _ => {}
        }
    }
}

/// Stick and throttle follow the controls (whatever the input source).
pub(super) fn animate_levers(
    controls: Res<FlightControls>,
    aircraft: Query<&Aircraft, With<Player>>,
    mut stick: Query<&mut Transform, (With<Stick>, Without<ThrottleLever>)>,
    mut throttle: Query<&mut Transform, With<ThrottleLever>>,
) {
    if let Ok(mut tf) = stick.single_mut() {
        tf.rotation = Quat::from_rotation_x(controls.pitch * STICK_TILT)
            * Quat::from_rotation_z(-controls.roll * STICK_TILT)
            * Quat::from_rotation_y(-controls.yaw * STICK_TWIST);
    }
    if let (Ok(mut tf), Ok(a)) = (throttle.single_mut(), aircraft.single()) {
        let t = controls.throttle.unwrap_or(a.set_thrust / 100.0);
        tf.translation = throttle_handle(t);
    }
}
