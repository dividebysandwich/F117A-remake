//! Side stick (right console) and throttle (left console).
//!
//! VR: squeeze the grip near the stick to take it. Tilting the controller (relative to how it
//! was held when grabbed) deflects pitch (tilt back = nose up) and roll; twisting it about the
//! stick's axis is the rudder. Letting go re-centres the stick. A long stick (a helicopter's
//! centre cyclic) is moved by the hand instead: its grip follows the hand about the pivot, so it
//! doesn't swing further than the hand holding it.
//! Squeeze near the throttle handle and slide it along its rails: forward = more thrust; it stays where it is
//! released, with a haptic tick every 10 %.
//! The controller buttons are mapped in the input mapping (`InputConfig::vr_buttons`); a trigger
//! only counts while its hand holds the stick or throttle.
//! The ejection handle is pulled up with either hand. A hand carrying the kneeboard grabs none.
//! Without VR both animate from the keyboard/gamepad controls.

use bevy::prelude::*;

use super::{
    BUTTON_PRIMARY, BUTTON_SECONDARY, BUTTON_STICK_DOWN, BUTTON_STICK_LEFT, BUTTON_STICK_RIGHT,
    BUTTON_STICK_UP, BUTTON_THUMB, Hands,
};
use crate::flight::{Aircraft, Player};
use crate::hotas::{ButtonTarget, InputConfig, VrHand, VrInput};
use crate::input::{Action, ActionEvent, FlightControls};

/// Left thumbstick deflection below which it does not slew the cursor.
const SLEW_DEADZONE: f32 = 0.15;
/// Grip centre above the pivot, unless the model says (the stick's `grip`).
const GRIP_HEIGHT: f32 = 0.11;
/// Grip heights over which pitch and roll go from the controller's tilt (a short side stick) to
/// the hand's position about the pivot (a long cyclic).
const LONG_STICK: (f32, f32) = (0.2, 0.5);
/// Full deflection (radians).
const STICK_TILT: f32 = 0.35;
const STICK_TWIST: f32 = 0.45;
/// Rudder deadzone (fraction of full twist); a haptic tick on leaving and re-entering it.
const TWIST_DEADZONE: f32 = 0.15;
/// Re-entering needs this much less twist, so hand jitter at the edge doesn't buzz.
const TWIST_HYSTERESIS: f32 = 0.03;

/// Positions in the cockpit frame taken from the cockpit model: the stick pivot (`stick`
/// object), the throttle handle at idle (`throttle` object) and at full thrust (the
/// `throttle_full` anchor), and the ejection handle (the `eject` control). A helicopter's
/// collective is a throttle lever swinging up about its pivot (the `throttle_pivot` anchor).
#[derive(Resource, Clone, Copy, Debug)]
pub struct CockpitAnchors {
    pub stick_pivot: Vec3,
    /// The stick's grip centre above the pivot.
    pub grip_height: f32,
    pub throttle_idle: Vec3,
    pub throttle_full: Vec3,
    /// The collective's pivot (none: the throttle slides along its rail).
    pub throttle_pivot: Option<Vec3>,
    pub eject: Vec3,
}

impl Default for CockpitAnchors {
    fn default() -> Self {
        Self {
            stick_pivot: Vec3::new(0.40, -0.48, -0.22),
            grip_height: GRIP_HEIGHT,
            throttle_idle: Vec3::new(-0.40, -0.40, -0.12),
            throttle_full: Vec3::new(-0.40, -0.40, -0.30),
            throttle_pivot: None,
            eject: Vec3::new(0.0, -0.74, -0.20),
        }
    }
}

impl CockpitAnchors {
    fn throttle_handle(&self, t: f32) -> Vec3 {
        match self.throttle_pivot {
            Some(p) => p + self.throttle_rotation(t) * (self.throttle_idle - p),
            None => self.throttle_idle.lerp(self.throttle_full, t.clamp(0.0, 1.0)),
        }
    }

    /// The collective's rotation about its pivot at setting `t` (none for a sliding throttle).
    fn throttle_rotation(&self, t: f32) -> Quat {
        let Some(p) = self.throttle_pivot else { return Quat::IDENTITY };
        let (a, b) = (self.throttle_idle - p, self.throttle_full - p);
        if a.length_squared() < 1e-6 || b.length_squared() < 1e-6 {
            return Quat::IDENTITY;
        }
        Quat::IDENTITY
            .slerp(Quat::from_rotation_arc(a.normalize(), b.normalize()), t.clamp(0.0, 1.0))
    }

    /// Throttle setting change for a hand movement (along the rail).
    fn throttle_travel(&self, d: Vec3) -> f32 {
        let rail = self.throttle_full - self.throttle_idle;
        d.dot(rail) / rail.length_squared().max(1e-6)
    }
}

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
    /// A held VR button set the wheel brakes (they come off when it is let go).
    brakes: bool,
}

struct StickGrab {
    rot0: Quat,
    /// Hand position from the pivot when grabbed.
    p0: Vec3,
    /// Twist outside the rudder deadzone.
    twisted: bool,
}

struct ThrottleGrab {
    /// Hand position when grabbed.
    p0: Vec3,
    t0: f32,
    /// Last 10 % detent passed (for the haptic ticks).
    notch: i32,
}

/// Throttle detents per full travel (one haptic tick each).
const THROTTLE_NOTCHES: f32 = 10.0;

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

/// Stick angles of the hand moving about the pivot from `p0` to `p` (as `stick_angles`).
fn stick_angles_moved(p0: Vec3, p: Vec3) -> (f32, f32) {
    (p.z.atan2(p.y) - p0.z.atan2(p0.y), p.x.atan2(p.y) - p0.x.atan2(p0.y))
}

/// How much the hand's position (rather than the controller's tilt) moves a stick this long.
fn stick_follow(grip_height: f32) -> f32 {
    ((grip_height - LONG_STICK.0) / (LONG_STICK.1 - LONG_STICK.0)).clamp(0.0, 1.0)
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
#[allow(clippy::too_many_arguments)]
pub(super) fn grab_levers(
    mut hands: ResMut<Hands>,
    config: Res<InputConfig>,
    mut grabs: Local<Grabs>,
    mut controls: ResMut<FlightControls>,
    mut actions: MessageWriter<ActionEvent>,
    aircraft: Query<&Aircraft, With<Player>>,
    block: Res<crate::input::InputBlock>,
    anchors: Res<CockpitAnchors>,
    hide: Option<Res<crate::camera::HideOwnAircraft>>,
) {
    // The cockpit is not there (VR, the campaign's Ops screen): nothing is held, and of the
    // mapped buttons only the in-flight menu's works.
    if hide.is_some_and(|h| h.0) {
        *grabs = Grabs::default();
        for hand in hands.hands.iter_mut() {
            hand.lever = false;
        }
        let [left, right] = &hands.hands;
        for (side, hand) in [(VrHand::Left, left), (VrHand::Right, right)] {
            for m in config.vr_buttons.iter().filter(|m| m.hand == side) {
                if hand.active
                    && m.target == ButtonTarget::Action(Action::Menu)
                    && m.input != VrInput::Trigger
                    && hand.just_pressed(vr_bit(m.input))
                {
                    actions.write(ActionEvent(Action::Menu));
                }
            }
        }
        return;
    }
    let set_thrust = aircraft.single().map_or(0.0, |a| a.set_thrust / 100.0);
    let brakes_on = aircraft.single().is_ok_and(|a| a.brakes);
    let [left, right] = &mut hands.hands;

    // --- stick (right hand)
    let grip_centre = anchors.stick_pivot + Vec3::Y * anchors.grip_height;
    if !right.active || right.squeeze < 0.4 {
        if grabs.stick.take().is_some() {
            right.haptic = right.haptic.max(0.2);
        }
    } else if grabs.stick.is_none()
        && !right.carrying
        && right.squeeze > 0.6
        && right.grip.translation.distance(grip_centre) < GRAB_REACH
    {
        grabs.stick = Some(StickGrab {
            rot0: right.grip.rotation,
            p0: right.grip.translation - anchors.stick_pivot,
            twisted: false,
        });
        right.haptic = right.haptic.max(0.4);
    }
    if let Some(g) = &mut grabs.stick {
        let delta = right.grip.rotation * g.rot0.inverse();
        let (tp, tr) = stick_angles(delta);
        let (mp, mr) = stick_angles_moved(g.p0, right.grip.translation - anchors.stick_pivot);
        let w = stick_follow(anchors.grip_height);
        let (p, r) = (tp + (mp - tp) * w, tr + (mr - tr) * w);
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
            (yaw.signum() * (yaw.abs() - TWIST_DEADZONE).max(0.0) / (1.0 - TWIST_DEADZONE))
                .clamp(-1.0, 1.0)
        } else {
            0.0
        };
    }

    // --- throttle (left hand)
    let handle = anchors.throttle_handle(set_thrust);
    if !left.active || left.squeeze < 0.4 {
        if grabs.throttle.take().is_some() {
            left.haptic = left.haptic.max(0.2);
        }
    } else if grabs.throttle.is_none()
        && !left.carrying
        && left.squeeze > 0.6
        && left.grip.translation.distance(handle) < GRAB_REACH
    {
        let notch = (set_thrust * THROTTLE_NOTCHES).floor() as i32;
        grabs.throttle = Some(ThrottleGrab { p0: left.grip.translation, t0: set_thrust, notch });
        left.haptic = left.haptic.max(0.4);
    }
    if let Some(g) = &mut grabs.throttle {
        let t = (g.t0 + anchors.throttle_travel(left.grip.translation - g.p0)).clamp(0.0, 1.0);
        // A light tick at each detent, a firm one at the idle and full stops.
        let before = set_thrust;
        if (t >= 1.0 && before < 1.0) || (t <= 0.0 && before > 0.0) {
            left.haptic = left.haptic.max(0.5);
        }
        // Hysteresis so hand jitter on a detent doesn't buzz. A collective has none.
        let pos = t * THROTTLE_NOTCHES;
        if anchors.throttle_pivot.is_none()
            && (pos >= (g.notch + 1) as f32 + 0.1 || pos < g.notch as f32 - 0.1)
        {
            g.notch = pos.floor() as i32;
            left.haptic = left.haptic.max(0.15);
        }
        controls.throttle = Some(t);
    }

    // --- sensor/cursor slew: the left thumbstick
    if left.active && !block.0 && !left.scrolling && left.thumb.length() > SLEW_DEADZONE {
        controls.slew = left.thumb.clamp(Vec2::splat(-1.0), Vec2::ONE);
    }

    // --- mapped buttons; a trigger only while its hand holds the throttle / stick (otherwise it
    // presses the cockpit control under the fingertip), but for the brakes
    let mut brakes_held = false;
    for (side, hand, holding) in [
        (VrHand::Left, &*left, grabs.throttle.is_some()),
        (VrHand::Right, &*right, grabs.stick.is_some()),
    ] {
        if !hand.active {
            continue;
        }
        for m in config.vr_buttons.iter().filter(|m| m.hand == side) {
            // The brakes also on the trigger of a free hand that touches no control.
            let brakes = m.target == ButtonTarget::Action(Action::Brakes)
                && (!hand.touching || grabs.brakes);
            let (held, pressed) = match m.input {
                VrInput::Trigger if !holding && !brakes => continue,
                VrInput::ThumbUp
                | VrInput::ThumbDown
                | VrInput::ThumbLeft
                | VrInput::ThumbRight
                    if hand.scrolling =>
                {
                    continue;
                }
                VrInput::Trigger => (hand.trigger > 0.7, hand.trigger_pulled()),
                input => (hand.held(vr_bit(input)), hand.just_pressed(vr_bit(input))),
            };
            match m.target {
                ButtonTarget::Gun => controls.gun |= held,
                ButtonTarget::Action(Action::FireWeapon) if held && !block.0 => {
                    controls.fire = true;
                    if pressed {
                        actions.write(ActionEvent(Action::FireWeapon));
                    }
                }
                // The wheel brakes are on while the button is held (released only if it set them).
                ButtonTarget::Action(Action::Brakes) if !block.0 => {
                    brakes_held |= held;
                    if pressed && !brakes_on {
                        actions.write(ActionEvent(Action::Brakes));
                        grabs.brakes = true;
                    }
                }
                ButtonTarget::Action(Action::LockCursor) if held && !block.0 => {
                    controls.mark = true;
                    if pressed {
                        actions.write(ActionEvent(Action::LockCursor));
                    }
                }
                ButtonTarget::Action(a) if a.trim_direction().is_some() => {
                    if held && !block.0 {
                        controls.trim += a.trim_direction().unwrap_or_default();
                    }
                }
                ButtonTarget::Action(a) if pressed && (!block.0 || a == Action::Menu) => {
                    actions.write(ActionEvent(a));
                }
                ButtonTarget::Action(_) => {}
            }
        }
    }
    if grabs.brakes && !brakes_held {
        grabs.brakes = false;
        if brakes_on {
            actions.write(ActionEvent(Action::Brakes));
        }
    }

    // --- ejection handle: grab with either hand and pull up 6 cm
    let handle = anchors.eject;
    for (i, h) in [&mut *left, &mut *right].into_iter().enumerate() {
        let is_grabbing_other =
            (i == 1 && grabs.stick.is_some()) || (i == 0 && grabs.throttle.is_some());
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
            None if h.active
                && !is_grabbing_other
                && !h.carrying
                && h.squeeze > 0.6
                && h.grip.translation.distance(handle) < 0.07 =>
            {
                grabs.eject = Some((i, h.grip.translation.y));
                h.haptic = h.haptic.max(0.3);
            }
            _ => {}
        }
    }
    left.lever = grabs.throttle.is_some() || grabs.eject.is_some_and(|(who, _)| who == 0);
    right.lever = grabs.stick.is_some() || grabs.eject.is_some_and(|(who, _)| who == 1);
}

/// Stick and throttle follow the controls (whatever the input source).
pub(super) fn animate_levers(
    controls: Res<FlightControls>,
    aircraft: Query<&Aircraft, With<Player>>,
    mut stick: Query<&mut Transform, (With<Stick>, Without<ThrottleLever>)>,
    mut throttle: Query<&mut Transform, With<ThrottleLever>>,
    anchors: Res<CockpitAnchors>,
) {
    if let Ok(mut tf) = stick.single_mut() {
        tf.rotation = Quat::from_rotation_x(controls.pitch * STICK_TILT)
            * Quat::from_rotation_z(-controls.roll * STICK_TILT)
            * Quat::from_rotation_y(-controls.yaw * STICK_TWIST);
    }
    if let (Ok(mut tf), Ok(a)) = (throttle.single_mut(), aircraft.single()) {
        let t = controls.throttle.unwrap_or(a.set_thrust / 100.0);
        tf.translation = anchors.throttle_handle(t);
        tf.rotation = anchors.throttle_rotation(t);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A long cyclic's grip stays in the hand: moving the hand 10 cm back tilts the stick so its
    /// grip moves 10 cm back. A short side stick still goes by the controller's tilt.
    #[test]
    fn long_stick_follows_the_hand() {
        let height = 0.59;
        assert_eq!(stick_follow(height), 1.0);
        assert_eq!(stick_follow(0.11), 0.0);
        let p0 = Vec3::Y * height;
        let (pitch, roll) = stick_angles_moved(p0, p0 + Vec3::new(0.0, 0.0, 0.10));
        let grip = Quat::from_rotation_x(pitch) * p0;
        assert!((grip.z - 0.10).abs() < 0.005, "grip at {grip}");
        assert!(roll.abs() < 1e-6);
        let (_, roll) = stick_angles_moved(p0, p0 + Vec3::X * 0.05);
        assert!(roll > 0.0, "hand right = roll right");
    }
}
