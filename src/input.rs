//! Input: key bindings (docs/keys.md) mapped to semantic actions, plus the
//! keyboard "virtual stick" (also gamepad stick). Game logic only reads
//! `FlightControls` and `ActionEvent`s, so other sources (HOTAS, VR controllers) can be added.

use bevy::prelude::*;

pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FlightControls>()
            .init_resource::<VirtualStick>()
            .init_resource::<InputBlock>()
            .add_message::<ActionEvent>()
            .add_plugins(crate::hotas::build)
            .add_systems(
                PreUpdate,
                (read_keyboard, read_gamepad, crate::hotas::read_hotas)
                    .chain()
                    .in_set(InputSystemsSet)
                    .after(bevy::input::InputSystems),
            );
    }
}

/// While set (the in-flight menu is open), keys, buttons and sticks only open and close the menu.
#[derive(Resource, Default)]
pub struct InputBlock(pub bool);

/// Keyboard, gamepad and joystick reading (other input sources run after it and override).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct InputSystemsSet;

/// Discrete actions (one message per key press).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
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
    /// Bombing mode CCIP / CCRP (also the A-G button again in AG).
    BombMode,
    HudDeclutter,
    Maps,
    /// Night vision goggles (F4).
    #[serde(alias = "DayNightHud")]
    Nvg,
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
    /// Locks the nearest target ahead: AIR mode with an air-to-air missile selected, else GND.
    NearestTarget,
    /// Drops both target locks and stops the automatic acquisition until a target key.
    ClearTarget,
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
    /// The free camera, moved with the keys and turned with the mouse (`camera::FreeView`).
    ViewFree,
    Zoom,
    Unzoom,
    /// Map display only (bezel buttons; Z/X also zoom the external views).
    MapZoomIn,
    MapZoomOut,
    /// Modern map: north up / heading up.
    MapOrientation,
    WideAngle,
    TimeAccel,
    TimeNormal,
    /// In-flight settings menu (pauses the flight).
    #[serde(alias = "Pause")]
    Menu,
    Quit,
    Detail,
    Volume,
    Training,
    Resupply,
    DayNight,
    /// Modern avionics: lock what is under the sensor of interest's cursor.
    LockCursor,
    /// Modern avionics: the sensor of interest moves to the other MFD.
    SwapSoi,
    /// Kneeboard: desktop, shows it over the view and turns its pages; VR, shows or hides the
    /// board on the thigh.
    Kneeboard,
    /// Multiplayer: type a chat message.
    Chat,
    /// Tracking camera / targeting pod zoom.
    CameraZoomIn,
    CameraZoomOut,
    /// Keyboard stick (held, not a discrete action): the arrow keys.
    PitchDown,
    PitchUp,
    RollLeft,
    RollRight,
    /// Rudder keys (unbound by default).
    YawLeft,
    YawRight,
    /// Trim switch (held, not a discrete action): moves the stick trim.
    TrimNoseDown,
    TrimNoseUp,
    TrimLeft,
    TrimRight,
    /// Centres the trim.
    TrimReset,
    /// Master caution: silences the alarms sounding.
    MasterCaution,
}

impl Action {
    /// The (roll, pitch, yaw) direction of a held stick action; None for the others. Up is nose
    /// down, as for the stick.
    pub fn stick_direction(self) -> Option<(f32, f32, f32)> {
        match self {
            Action::PitchDown => Some((0.0, -1.0, 0.0)),
            Action::PitchUp => Some((0.0, 1.0, 0.0)),
            Action::RollLeft => Some((-1.0, 0.0, 0.0)),
            Action::RollRight => Some((1.0, 0.0, 0.0)),
            Action::YawLeft => Some((0.0, 0.0, -1.0)),
            Action::YawRight => Some((0.0, 0.0, 1.0)),
            _ => None,
        }
    }

    /// The (roll, pitch) direction of a held trim switch action; None for the others.
    pub fn trim_direction(self) -> Option<Vec2> {
        match self {
            Action::TrimNoseDown => Some(Vec2::new(0.0, -1.0)),
            Action::TrimNoseUp => Some(Vec2::new(0.0, 1.0)),
            Action::TrimLeft => Some(Vec2::new(-1.0, 0.0)),
            Action::TrimRight => Some(Vec2::new(1.0, 0.0)),
            _ => None,
        }
    }
}

/// Stick deflection of a held key or button: 0x5A of 0x7F.
pub const KEY_DEFLECTION: f32 = 0x5A as f32 / 0x7F as f32;

#[derive(Message, Debug, Clone, Copy)]
pub struct ActionEvent(pub Action);

/// Looks up an action by its name (e.g. "ThrottleMax"), for scripts and config files.
pub fn action_by_name(name: &str) -> Option<Action> {
    all_actions().into_iter().find(|a| format!("{a:?}").eq_ignore_ascii_case(name))
}

/// A key with the modifier held with it, as text: "Shift+F1", "Z". Character keys are named as
/// the keyboard layout prints them (bindings are by position).
pub fn key_label(mods: Mods, key: KeyCode) -> String {
    with_mods(mods, crate::keyboard_layout::key_name(key).unwrap_or_else(|| qwerty_name(key)))
}

/// As `key_label`, the key named as on a US keyboard (for a font that lacks the layout's
/// characters).
pub fn qwerty_key_label(mods: Mods, key: KeyCode) -> String {
    with_mods(mods, qwerty_name(key))
}

/// "KeyB" -> "B", "Digit2" -> "2", "F10" -> "F10".
fn qwerty_name(key: KeyCode) -> String {
    let name = format!("{key:?}");
    name.strip_prefix("Key").or_else(|| name.strip_prefix("Digit")).unwrap_or(&name).to_string()
}

/// The name with the modifier held with the key.
fn with_mods(mods: Mods, name: String) -> String {
    match mods {
        Mods::None => name,
        Mods::Shift => format!("Shift+{name}"),
        Mods::Alt => format!("Alt+{name}"),
    }
}

/// A key by its `KeyCode` variant name (e.g. "KeyB", "F10").
pub fn key_from_name(name: &str) -> Option<KeyCode> {
    use bevy::reflect::{FromReflect, enums::DynamicEnum};
    KeyCode::from_reflect(&DynamicEnum::new(name, ()))
}

/// Every action, in the key summary's order.
pub fn all_actions() -> Vec<Action> {
    let mut out: Vec<Action> = Vec::new();
    for &(_, _, a) in BINDINGS {
        if !out.contains(&a) {
            out.push(a);
        }
    }
    out
}

/// Default keyboard bindings: the manual's key summary, plus Backspace for the gun.
pub fn default_key_mappings() -> Vec<crate::hotas::KeyMapping> {
    use crate::hotas::{ButtonTarget, KeyMapping};
    std::iter::once(KeyMapping {
        target: ButtonTarget::Gun,
        mods: Mods::None,
        key: KeyCode::Backspace,
    })
    .chain(BINDINGS.iter().map(|&(mods, key, action)| KeyMapping {
        target: ButtonTarget::Action(action),
        mods,
        key,
    }))
    .collect()
}

/// Continuous controls, in the sim's stick units.
#[derive(Resource, Debug, Default, Clone)]
pub struct FlightControls {
    /// Stick roll, -1 (left) .. 1 (right).
    pub roll: f32,
    /// Stick pitch, -1 (push, nose down) .. 1 (pull, nose up).
    pub pitch: f32,
    /// Rudder, -1 (nose left) .. 1 (nose right); 0 = yaw from the bank only.
    pub yaw: f32,
    /// Absolute throttle 0..1 while an analog throttle is held (VR throttle lever); None = the
    /// throttle keys set the thrust. A helicopter's collective.
    pub throttle: Option<f32>,
    /// Helicopters: the collective moving while its keys (the throttle keys) or the gamepad
    /// triggers are held, -1 (down) .. 1 (up).
    pub collective_rate: f32,
    /// Trim switch held: x roll right, y nose up (-1..1).
    pub trim: Vec2,
    /// Gun trigger held.
    pub gun: bool,
    /// Fire button held (fires the gun when it is the selected weapon).
    pub fire: bool,
    /// Sensor/cursor slew, -1..1 (x right, y up): moves the sensor of interest's cursor.
    pub slew: Vec2,
    /// "Lock cursor target" held (held on the HSD or HARM display: a zoomed view to aim with).
    pub mark: bool,
}

/// Modifier held with a key (Alt wins over Shift).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Mods {
    None,
    Shift,
    Alt,
}

/// Default bindings, from the manual's key summary (IBM version); the input mapping can change
/// them.
const BINDINGS: &[(Mods, KeyCode, Action)] = {
    use Action::*;
    use KeyCode as K;
    use Mods::*;
    &[
        (None, K::ArrowUp, PitchDown),
        (None, K::ArrowDown, PitchUp),
        (None, K::ArrowLeft, RollLeft),
        (None, K::ArrowRight, RollRight),
        // Remake: the rudder (the helicopters' pedals), left hand.
        (None, K::KeyQ, YawLeft),
        (None, K::KeyE, YawRight),
        // Remake: the trim switch.
        (Alt, K::ArrowUp, TrimNoseDown),
        (Alt, K::ArrowDown, TrimNoseUp),
        (Alt, K::ArrowLeft, TrimLeft),
        (Alt, K::ArrowRight, TrimRight),
        (Alt, K::Home, TrimReset),
        (None, K::Equal, ThrottleUp),
        (None, K::NumpadAdd, ThrottleUp),
        (None, K::Minus, ThrottleDown),
        (None, K::NumpadSubtract, ThrottleDown),
        (Shift, K::Equal, ThrottleMax),
        (Shift, K::Minus, ThrottleIdle),
        (None, K::Space, SelectWeapon),
        (None, K::Enter, FireWeapon),
        (None, K::NumpadEnter, LockCursor),
        (Alt, K::KeyS, SwapSoi),
        (None, K::BracketRight, CameraZoomIn),
        (None, K::BracketLeft, CameraZoomOut),
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
        (Alt, K::KeyC, MasterCaution),
        (None, K::F2, HudMode),
        (Alt, K::F2, BombMode),
        (None, K::KeyV, HudDeclutter),
        (None, K::F3, Maps),
        (None, K::F4, Nvg),
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
        (None, K::KeyT, NearestTarget),
        (Shift, K::KeyB, ClearTarget),
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
        (Shift, K::F7, ViewFree),
        (None, K::KeyZ, Zoom),
        (None, K::KeyX, Unzoom),
        (Alt, K::KeyZ, MapZoomIn),
        (Alt, K::KeyX, MapZoomOut),
        (Alt, K::F3, MapOrientation),
        (None, K::KeyC, WideAngle),
        (Shift, K::KeyZ, TimeAccel),
        (Shift, K::KeyX, TimeNormal),
        (Alt, K::KeyP, Menu),
        (None, K::Pause, Menu),
        (None, K::Escape, Menu),
        (Alt, K::KeyQ, Quit),
        (Alt, K::KeyD, Detail),
        (Alt, K::KeyV, Volume),
        (Alt, K::KeyT, Training),
        (Alt, K::KeyR, Resupply),
        (Alt, K::KeyN, DayNight),
        (None, K::KeyK, Kneeboard),
        (None, K::KeyY, Chat),
    ]
};

/// Keyboard stick: a direction key deflects the stick (0x5A of 0x7F);
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

/// Keyboard sensor/cursor slew (fixed): the numeric keypad.
const SLEW_KEYS: &[(KeyCode, f32, f32)] = &[
    // (key, x, y); up = cursor up.
    (KeyCode::Numpad8, 0.0, 1.0),
    (KeyCode::Numpad2, 0.0, -1.0),
    (KeyCode::Numpad4, -1.0, 0.0),
    (KeyCode::Numpad6, 1.0, 0.0),
    (KeyCode::Numpad7, -1.0, 1.0),
    (KeyCode::Numpad9, 1.0, 1.0),
    (KeyCode::Numpad1, -1.0, -1.0),
    (KeyCode::Numpad3, 1.0, -1.0),
];

fn read_keyboard(
    keys: Res<ButtonInput<KeyCode>>,
    config: Res<crate::hotas::InputConfig>,
    time: Res<Time<Real>>,
    mut stick: ResMut<VirtualStick>,
    mut controls: ResMut<FlightControls>,
    mut actions: MessageWriter<ActionEvent>,
    block: Res<InputBlock>,
    view: Option<Res<crate::camera::ViewState>>,
) {
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let alt = keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
    let mods = if alt {
        Mods::Alt
    } else if shift {
        Mods::Shift
    } else {
        Mods::None
    };
    // The free view's movement keys move it, not the aircraft (with Alt they still do).
    let free = view.is_some_and(|v| v.view == crate::camera::View::Free) && mods != Mods::Alt;
    let camera_key = |k: KeyCode| free && crate::camera::FREE_KEYS.contains(&k);
    for m in &config.keys {
        let crate::hotas::ButtonTarget::Action(action) = m.target else { continue };
        // The stick keys are held, read below.
        if (block.0 && action != Action::Menu)
            || action.stick_direction().is_some()
            || camera_key(m.key)
        {
            continue;
        }
        if m.mods == mods && keys.just_pressed(m.key) {
            if action == Action::StickSensitivity {
                stick.sensitivity = (stick.sensitivity + 1) % 3;
            }
            actions.write(ActionEvent(action));
        }
    }

    if block.0 {
        // The menu reads the arrow keys.
        controls.roll = 0.0;
        controls.pitch = 0.0;
        controls.gun = false;
        controls.fire = false;
        controls.mark = false;
        controls.yaw = 0.0;
        controls.throttle = None;
        controls.collective_rate = 0.0;
        controls.trim = Vec2::ZERO;
        controls.slew = Vec2::ZERO;
        return;
    }
    // Virtual stick: the keys bound to the stick actions, with the modifier they were bound with
    // (the arrows have none, so Shift+arrows still move waypoints).
    let now = time.elapsed_secs_f64();
    let (mut roll, mut pitch, mut yaw) = (0.0, 0.0, 0.0);
    let mut held = false;
    for m in &config.keys {
        let crate::hotas::ButtonTarget::Action(action) = m.target else { continue };
        let Some((r, p, y)) = action.stick_direction() else { continue };
        if m.mods != mods || camera_key(m.key) {
            continue;
        }
        if keys.just_pressed(m.key) {
            stick.full =
                matches!(stick.last_key, Some((k, t)) if k == m.key && now - t < DOUBLE_TAP_S);
            stick.last_key = Some((m.key, now));
        }
        if keys.pressed(m.key) {
            roll += r;
            pitch += p;
            yaw += y;
            held = true;
        }
    }
    let deflection = if stick.full { 1.0 } else { KEY_DEFLECTION };
    // Sensitivity scales the deflection: (sens+1)/3.
    let scale = deflection * (stick.sensitivity as f32 + 1.0) / 3.0;
    controls.roll = roll.clamp(-1.0, 1.0) * scale;
    controls.pitch = pitch.clamp(-1.0, 1.0) * scale;
    if !held {
        stick.full = false;
    }
    // The gun fires with any modifier unless its key needs one.
    controls.gun = config.keys.iter().any(|m| {
        m.target == crate::hotas::ButtonTarget::Gun
            && keys.pressed(m.key)
            && !camera_key(m.key)
            && (m.mods == Mods::None || m.mods == mods)
    });
    let held = |a: Action| {
        config.keys.iter().any(|m| {
            m.target == crate::hotas::ButtonTarget::Action(a)
                && keys.pressed(m.key)
                && (m.mods == Mods::None || m.mods == mods)
        })
    };
    controls.fire = held(Action::FireWeapon);
    controls.mark = held(Action::LockCursor);
    controls.yaw = yaw.clamp(-1.0, 1.0) * scale;
    controls.throttle = None;
    controls.collective_rate =
        held(Action::ThrottleUp) as i8 as f32 - held(Action::ThrottleDown) as i8 as f32;
    controls.trim = config
        .keys
        .iter()
        .filter_map(|m| {
            let crate::hotas::ButtonTarget::Action(a) = m.target else { return None };
            let d = a.trim_direction()?;
            (m.mods == mods && keys.pressed(m.key)).then_some(d)
        })
        .sum();
    let mut slew = Vec2::ZERO;
    if mods == Mods::None {
        for &(key, x, y) in SLEW_KEYS {
            if keys.pressed(key) {
                slew += Vec2::new(x, y);
            }
        }
    }
    controls.slew = slew.clamp(Vec2::splat(-1.0), Vec2::ONE);
}

/// The larger of two deflections: an analog stick resting a little off centre does not cancel
/// a held stick key, and the stick still overrides the keys when deflected further.
pub fn stronger(current: f32, v: f32) -> f32 {
    if v.abs() > current.abs() { v } else { current }
}

/// Gamepad: left stick = stick, right stick X = rudder, triggers = throttle (a helicopter's
/// collective, moving as far as they are pulled), A = fire, B = gun (unless turned off in the
/// input mapping).
fn read_gamepad(
    gamepads: Query<&Gamepad>,
    config: Res<crate::hotas::InputConfig>,
    mut controls: ResMut<FlightControls>,
    mut actions: MessageWriter<ActionEvent>,
    block: Res<InputBlock>,
) {
    for pad in &gamepads {
        if pad.just_pressed(GamepadButton::Start) {
            actions.write(ActionEvent(Action::Menu));
        }
    }
    if !config.gamepad_defaults || block.0 {
        return;
    }
    for pad in &gamepads {
        let x = pad.get(GamepadAxis::LeftStickX).unwrap_or(0.0);
        let y = pad.get(GamepadAxis::LeftStickY).unwrap_or(0.0);
        if x.abs() > 0.1 || y.abs() > 0.1 {
            controls.roll = stronger(controls.roll, x);
            controls.pitch = stronger(controls.pitch, -y);
        }
        let rx = pad.get(GamepadAxis::RightStickX).unwrap_or(0.0);
        if rx.abs() > 0.15 {
            controls.yaw = rx;
        }
        let pull = |b: GamepadButton| {
            pad.get(b).unwrap_or(0.0).max(if pad.pressed(b) { 1.0 } else { 0.0 })
        };
        let lt = pull(GamepadButton::LeftTrigger2).max(pull(GamepadButton::LeftTrigger));
        let rt = pull(GamepadButton::RightTrigger2).max(pull(GamepadButton::RightTrigger));
        controls.collective_rate = stronger(controls.collective_rate, rt - lt);
        controls.gun |= pad.pressed(GamepadButton::East);
        controls.fire |= pad.pressed(GamepadButton::South);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A held arrow key deflects the stick, and keeps doing so with a joystick axis resting
    /// slightly off centre.
    #[test]
    fn arrow_keys_steer() {
        let mut app = App::new();
        app.init_resource::<FlightControls>()
            .init_resource::<VirtualStick>()
            .init_resource::<InputBlock>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<Time<Real>>()
            .insert_resource(crate::hotas::InputConfig::default())
            .add_message::<ActionEvent>()
            .add_systems(Update, read_keyboard);
        for (key, roll, pitch) in [
            (KeyCode::ArrowLeft, -1.0, 0.0),
            (KeyCode::ArrowRight, 1.0, 0.0),
            (KeyCode::ArrowUp, 0.0, -1.0),
            (KeyCode::ArrowDown, 0.0, 1.0),
        ] {
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(key);
            app.update();
            let c = app.world().resource::<FlightControls>().clone();
            assert!(c.roll * roll > 0.5 || c.pitch * pitch > 0.5, "{key:?}: {c:?}");
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().release(key);
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().clear();
        }
        assert_eq!(stronger(-0.71, 0.06), -0.71);
        assert_eq!(stronger(-0.71, 0.9), 0.9);
        assert_eq!(stronger(0.0, 0.06), 0.06);
    }

    /// The rudder keys are listed with the other stick keys, not at the end.
    #[test]
    fn yaw_actions_follow_roll() {
        let all = all_actions();
        let i = all.iter().position(|a| *a == Action::RollRight).unwrap();
        assert_eq!(all[i + 1..i + 3], [Action::YawLeft, Action::YawRight]);
        assert_eq!(all.iter().filter(|a| **a == Action::YawLeft).count(), 1);
    }

    /// Stick keys moved to the positions of ZQSD and AE on an AZERTY keyboard (KeyW, KeyA, KeyS,
    /// KeyD, KeyQ, KeyE) steer, and the arrows no longer do.
    #[test]
    fn rebound_stick_keys_steer() {
        use crate::hotas::{ButtonTarget, InputConfig};
        let mut config = InputConfig::default();
        for (action, key) in [
            (Action::PitchDown, KeyCode::KeyW),
            (Action::PitchUp, KeyCode::KeyS),
            (Action::RollLeft, KeyCode::KeyA),
            (Action::RollRight, KeyCode::KeyD),
            (Action::YawLeft, KeyCode::KeyQ),
            (Action::YawRight, KeyCode::KeyE),
        ] {
            config.keys.retain(|m| m.target != ButtonTarget::Action(action));
            config.bind_key(ButtonTarget::Action(action), Mods::None, key);
        }
        let mut app = App::new();
        app.init_resource::<FlightControls>()
            .init_resource::<VirtualStick>()
            .init_resource::<InputBlock>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<Time<Real>>()
            .insert_resource(config)
            .add_message::<ActionEvent>()
            .add_systems(Update, read_keyboard);
        for (key, roll, pitch, yaw) in [
            (KeyCode::KeyA, -1.0, 0.0, 0.0),
            (KeyCode::KeyD, 1.0, 0.0, 0.0),
            (KeyCode::KeyW, 0.0, -1.0, 0.0),
            (KeyCode::KeyS, 0.0, 1.0, 0.0),
            (KeyCode::KeyQ, 0.0, 0.0, -1.0),
            (KeyCode::KeyE, 0.0, 0.0, 1.0),
            (KeyCode::ArrowLeft, 0.0, 0.0, 0.0),
        ] {
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(key);
            app.update();
            let c = app.world().resource::<FlightControls>().clone();
            let sign = |v: f32| if v == 0.0 { 0.0 } else { v.signum() };
            assert_eq!(
                (sign(c.roll), sign(c.pitch), sign(c.yaw)),
                (roll, pitch, yaw),
                "{key:?}: {c:?}"
            );
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().release(key);
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().clear();
        }
    }
}
