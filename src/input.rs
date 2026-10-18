//! Input: original key bindings (docs/re/keys.md) mapped to semantic actions, plus the
//! original keyboard "virtual stick" (also gamepad stick). Game logic only reads
//! `FlightControls` and `ActionEvent`s, so other sources (HOTAS, VR controllers) can be added.

use bevy::prelude::*;

pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FlightControls>()
            .init_resource::<VirtualStick>()
            .add_message::<ActionEvent>()
            .add_systems(PreUpdate, (read_keyboard, read_gamepad).chain().in_set(InputSystemsSet).after(bevy::input::InputSystems));
    }
}

/// Keyboard and gamepad reading (other input sources run after it and override).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct InputSystemsSet;

/// Discrete actions (one message per key press).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    ThrottleUp,
    ThrottleDown,
    ThrottleMax,
    ThrottleIdle,
    SelectWeapon,
    FireWeapon,
    Flare,
    Chaff,
    IrJammer,
    Ecm,
    Decoy,
    Gear,
    Autopilot,
    BayDoors,
    Flaps,
    Brakes,
    Eject,
    HudMode,
    HudDeclutter,
    Maps,
    DayNightHud,
    Ordnance,
    Flir,
    Ils,
    MissionOrders,
    CamAhead,
    CamRear,
    CamRight,
    CamLeft,
    DesignateTarget,
    SelectTarget,
    SelectWaypoint,
    ChangeWaypoint,
    ResetWaypoint,
    LastWaypoint,
    NextWaypoint,
    StickSensitivity,
    ViewCockpit,
    LookAhead,
    LookRear,
    LookLeft,
    LookRight,
    ViewSlot,
    ViewChase,
    ViewSide,
    ViewMissile,
    ViewTactical,
    ViewInverseTactical,
    Zoom,
    Unzoom,
    WideAngle,
    TimeAccel,
    TimeNormal,
    Pause,
    Quit,
    Detail,
    Volume,
    Training,
    Resupply,
    DayNight,
}

#[derive(Message, Debug, Clone, Copy)]
pub struct ActionEvent(pub Action);

/// Looks up an action by its name (e.g. "ThrottleMax"), for scripts and config files.
pub fn action_by_name(name: &str) -> Option<Action> {
    BINDINGS.iter().map(|b| b.2).find(|a| format!("{a:?}").eq_ignore_ascii_case(name))
}

/// Continuous controls, in the original's stick units.
#[derive(Resource, Debug, Default, Clone)]
pub struct FlightControls {
    /// Stick roll, -1 (left) .. 1 (right).
    pub roll: f32,
    /// Stick pitch, -1 (push, nose down) .. 1 (pull, nose up).
    pub pitch: f32,
    /// Rudder, -1 (nose left) .. 1 (nose right). Not in the original (yaw came from the bank
    /// only); 0 keeps the original behaviour.
    pub yaw: f32,
    /// Absolute throttle 0..1 while an analog throttle is held (VR throttle lever); None = the
    /// throttle keys set the thrust as in the original.
    pub throttle: Option<f32>,
    /// Gun trigger held.
    pub gun: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mods {
    None,
    Shift,
    Alt,
}

/// Default bindings, from the manual's key summary (IBM version).
const BINDINGS: &[(Mods, KeyCode, Action)] = {
    use Action::*;
    use KeyCode as K;
    use Mods::*;
    &[
        (None, K::Equal, ThrottleUp),
        (None, K::NumpadAdd, ThrottleUp),
        (None, K::Minus, ThrottleDown),
        (None, K::NumpadSubtract, ThrottleDown),
        (Shift, K::Equal, ThrottleMax),
        (Shift, K::Minus, ThrottleIdle),
        (None, K::Space, SelectWeapon),
        (None, K::Enter, FireWeapon),
        (None, K::NumpadEnter, FireWeapon),
        (None, K::Digit1, Flare),
        (None, K::Digit2, Chaff),
        (None, K::Digit3, IrJammer),
        (None, K::Digit4, Ecm),
        (None, K::Digit5, Decoy),
        (None, K::Digit6, Gear),
        (None, K::Digit7, Autopilot),
        (None, K::Digit8, BayDoors),
        (None, K::Digit9, Flaps),
        (None, K::Digit0, Brakes),
        (Shift, K::F10, Eject),
        (None, K::F2, HudMode),
        (None, K::KeyV, HudDeclutter),
        (None, K::F3, Maps),
        (None, K::F4, DayNightHud),
        (None, K::F5, Ordnance),
        (None, K::F6, Flir),
        (None, K::F9, Ils),
        (None, K::F10, MissionOrders),
        (None, K::Slash, CamAhead),
        (None, K::Period, CamRear),
        (None, K::Comma, CamRight),
        (None, K::KeyM, CamLeft),
        (None, K::KeyN, DesignateTarget),
        (None, K::KeyB, SelectTarget),
        (None, K::F7, SelectWaypoint),
        (None, K::F8, ChangeWaypoint),
        (Shift, K::F8, ResetWaypoint),
        (None, K::PageUp, LastWaypoint),
        (None, K::PageDown, NextWaypoint),
        (None, K::Insert, StickSensitivity),
        (None, K::F1, ViewCockpit),
        (Shift, K::Slash, LookAhead),
        (Shift, K::Period, LookRear),
        (Shift, K::Comma, LookLeft),
        (Shift, K::KeyM, LookRight),
        (Shift, K::F1, ViewSlot),
        (Shift, K::F2, ViewChase),
        (Shift, K::F3, ViewSide),
        (Shift, K::F4, ViewMissile),
        (Shift, K::F5, ViewTactical),
        (Shift, K::F6, ViewInverseTactical),
        (None, K::KeyZ, Zoom),
        (None, K::KeyX, Unzoom),
        (None, K::KeyC, WideAngle),
        (Shift, K::KeyZ, TimeAccel),
        (Shift, K::KeyX, TimeNormal),
        (Alt, K::KeyP, Pause),
        (None, K::Pause, Pause),
        (Alt, K::KeyQ, Quit),
        (Alt, K::KeyD, Detail),
        (Alt, K::KeyV, Volume),
        (Alt, K::KeyT, Training),
        (Alt, K::KeyR, Resupply),
        (Alt, K::KeyN, DayNight),
    ]
};

/// Keyboard stick, as in the original: a direction key deflects the stick (0x5A of 0x7F);
/// tapping the same key again quickly gives full deflection; releasing re-centres.
/// Ins cycles the sensitivity (tiny, small, medium).
#[derive(Resource)]
struct VirtualStick {
    sensitivity: u8,
    last_key: Option<(KeyCode, f64)>,
    full: bool,
}

impl Default for VirtualStick {
    fn default() -> Self {
        Self { sensitivity: 2, last_key: None, full: false }
    }
}

/// Double-tap window: 5 BIOS ticks.
const DOUBLE_TAP_S: f64 = 5.0 / 18.2;

const STICK_KEYS: &[(KeyCode, f32, f32)] = &[
    // (key, roll, pitch); up = nose down.
    (KeyCode::ArrowUp, 0.0, -1.0),
    (KeyCode::ArrowDown, 0.0, 1.0),
    (KeyCode::ArrowLeft, -1.0, 0.0),
    (KeyCode::ArrowRight, 1.0, 0.0),
    (KeyCode::Numpad8, 0.0, -1.0),
    (KeyCode::Numpad2, 0.0, 1.0),
    (KeyCode::Numpad4, -1.0, 0.0),
    (KeyCode::Numpad6, 1.0, 0.0),
    (KeyCode::Numpad7, -1.0, -1.0),
    (KeyCode::Numpad9, 1.0, -1.0),
    (KeyCode::Numpad1, -1.0, 1.0),
    (KeyCode::Numpad3, 1.0, 1.0),
];

fn read_keyboard(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time<Real>>,
    mut stick: ResMut<VirtualStick>,
    mut controls: ResMut<FlightControls>,
    mut actions: MessageWriter<ActionEvent>,
) {
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let alt = keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
    let mods = if alt { Mods::Alt } else if shift { Mods::Shift } else { Mods::None };
    for &(m, key, action) in BINDINGS {
        if m == mods && keys.just_pressed(key) {
            if action == Action::StickSensitivity {
                stick.sensitivity = (stick.sensitivity + 1) % 3;
            }
            actions.write(ActionEvent(action));
        }
    }

    // Virtual stick (unmodified direction keys only; Shift+arrows move waypoints).
    let now = time.elapsed_secs_f64();
    let (mut roll, mut pitch) = (0.0, 0.0);
    let mut held = false;
    if mods == Mods::None {
        for &(key, r, p) in STICK_KEYS {
            if keys.just_pressed(key) {
                stick.full = matches!(stick.last_key, Some((k, t)) if k == key && now - t < DOUBLE_TAP_S);
                stick.last_key = Some((key, now));
            }
            if keys.pressed(key) {
                roll += r;
                pitch += p;
                held = true;
            }
        }
    }
    let deflection = if stick.full { 1.0 } else { 0x5A as f32 / 0x7F as f32 };
    // Sensitivity scales the deflection like the original: (sens+1)/3.
    let scale = deflection * (stick.sensitivity as f32 + 1.0) / 3.0;
    controls.roll = (roll as f32).clamp(-1.0, 1.0) * scale;
    controls.pitch = (pitch as f32).clamp(-1.0, 1.0) * scale;
    if !held {
        stick.full = false;
    }
    controls.gun = keys.pressed(KeyCode::Backspace);
    controls.yaw = 0.0;
    controls.throttle = None;
}

/// Gamepad: left stick = stick, right stick X = rudder, triggers = throttle, A = fire, B = gun.
fn read_gamepad(gamepads: Query<&Gamepad>, mut controls: ResMut<FlightControls>, mut actions: MessageWriter<ActionEvent>) {
    for pad in &gamepads {
        let x = pad.get(GamepadAxis::LeftStickX).unwrap_or(0.0);
        let y = pad.get(GamepadAxis::LeftStickY).unwrap_or(0.0);
        if x.abs() > 0.1 || y.abs() > 0.1 {
            controls.roll = x;
            controls.pitch = -y;
        }
        let rx = pad.get(GamepadAxis::RightStickX).unwrap_or(0.0);
        if rx.abs() > 0.15 {
            controls.yaw = rx;
        }
        controls.gun |= pad.pressed(GamepadButton::East);
        for (b, a) in [
            (GamepadButton::South, Action::FireWeapon),
            (GamepadButton::RightTrigger, Action::ThrottleUp),
            (GamepadButton::LeftTrigger, Action::ThrottleDown),
            (GamepadButton::North, Action::SelectWeapon),
            (GamepadButton::West, Action::BayDoors),
        ] {
            if pad.just_pressed(b) {
                actions.write(ActionEvent(a));
            }
        }
    }
}
