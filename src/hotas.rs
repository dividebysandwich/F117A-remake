//! HOTAS / joystick input: raw device buttons and axes (by platform event code, so HOTAS
//! buttons that are not part of a standard gamepad layout work too), mapped to flight axes and
//! actions by an `InputConfig` that the menu's input mapping dialog edits and saves as JSON in
//! the user config directory.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;

use bevy::platform::cell::SyncCell;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::input::{Action, ActionEvent, FlightControls};

pub fn build(app: &mut App) {
    app.insert_resource(InputConfig::load()).insert_resource(Hotas::new());
}

/// Continuous controls a device axis can drive.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AxisControl {
    Roll,
    Pitch,
    Yaw,
    Throttle,
}

impl AxisControl {
    pub const ALL: [AxisControl; 4] = [AxisControl::Roll, AxisControl::Pitch, AxisControl::Yaw, AxisControl::Throttle];

    pub fn name(self) -> &'static str {
        match self {
            AxisControl::Roll => "Roll",
            AxisControl::Pitch => "Pitch",
            AxisControl::Yaw => "Rudder",
            AxisControl::Throttle => "Throttle",
        }
    }
}

/// What a device button does: the held gun trigger or a discrete action.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ButtonTarget {
    Gun,
    Action(Action),
}

/// A device input usable as a button: a real button, or an axis (hat, slider) pushed past half
/// way in one direction.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Source {
    Button(u32),
    AxisPositive(u32),
    AxisNegative(u32),
}

impl Source {
    pub fn label(self) -> String {
        match self {
            Source::Button(c) => format!("Button {}", c & 0xffff),
            Source::AxisPositive(c) => format!("Axis {} +", c & 0xffff),
            Source::AxisNegative(c) => format!("Axis {} -", c & 0xffff),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct AxisMapping {
    pub control: AxisControl,
    /// Device name as the OS reports it.
    pub device: String,
    /// Platform event code of the axis.
    pub code: u32,
    pub invert: bool,
    /// Centre dead zone, 0..1 (not used for the throttle).
    pub deadzone: f32,
}

impl AxisMapping {
    /// The control's value from the raw axis value (-1..1): -1..1 for the stick and rudder,
    /// 0..1 for the throttle.
    pub fn output(&self, raw: f32) -> f32 {
        let v = if self.invert { -raw } else { raw };
        if self.control == AxisControl::Throttle {
            return ((v + 1.0) / 2.0).clamp(0.0, 1.0);
        }
        let dz = self.deadzone.clamp(0.0, 0.9);
        if v.abs() <= dz { 0.0 } else { v.signum() * ((v.abs() - dz) / (1.0 - dz)).min(1.0) }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ButtonMapping {
    pub target: ButtonTarget,
    pub device: String,
    pub source: Source,
}

/// Saved input mapping (`<config dir>/f117/input.json`).
#[derive(Resource, Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(default)]
pub struct InputConfig {
    /// The built-in gamepad layout (left stick, triggers, face buttons) on top of the mapping.
    pub gamepad_defaults: bool,
    pub axes: Vec<AxisMapping>,
    pub buttons: Vec<ButtonMapping>,
}

impl Default for InputConfig {
    fn default() -> Self {
        Self { gamepad_defaults: true, axes: Vec::new(), buttons: Vec::new() }
    }
}

impl InputConfig {
    pub fn path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("f117").join("input.json"))
    }

    pub fn load() -> Self {
        let Some(path) = Self::path() else { return Self::default() };
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                warn!("{}: {e}; using the default input mapping", path.display());
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> anyhow::Result<PathBuf> {
        let path = Self::path().ok_or_else(|| anyhow::anyhow!("no user config directory"))?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, serde_json::to_string_pretty(self)?)?;
        Ok(path)
    }

    pub fn button(&self, target: ButtonTarget) -> Option<&ButtonMapping> {
        self.buttons.iter().find(|m| m.target == target)
    }

    /// Binds an axis, taking it away from any other control.
    pub fn bind_axis(&mut self, control: AxisControl, device: String, code: u32) {
        self.axes.retain(|m| m.control != control && !(m.device == device && m.code == code));
        let invert = control == AxisControl::Throttle;
        let deadzone = if control == AxisControl::Throttle { 0.0 } else { 0.05 };
        self.axes.push(AxisMapping { control, device, code, invert, deadzone });
    }

    /// Binds a button, taking it away from any other target.
    pub fn bind_button(&mut self, target: ButtonTarget, device: String, source: Source) {
        self.buttons.retain(|m| m.target != target && !(m.device == device && m.source == source));
        self.buttons.push(ButtonMapping { target, device, source });
    }
}

/// A connected joystick, HOTAS part, pedals or gamepad.
#[derive(Debug, Default, Clone)]
pub struct Device {
    pub name: String,
    pub axes: BTreeMap<u32, f32>,
    pub buttons: BTreeSet<u32>,
    prev_buttons: BTreeSet<u32>,
    prev_axes: BTreeMap<u32, f32>,
}

impl Device {
    fn active(&self, source: Source, prev: bool) -> bool {
        let (buttons, axes) = if prev { (&self.prev_buttons, &self.prev_axes) } else { (&self.buttons, &self.axes) };
        match source {
            Source::Button(c) => buttons.contains(&c),
            Source::AxisPositive(c) => axes.get(&c).is_some_and(|v| *v > 0.5),
            Source::AxisNegative(c) => axes.get(&c).is_some_and(|v| *v < -0.5),
        }
    }
}

/// Raw joystick state, read through our own gilrs instance (Bevy's gamepad layer drops the
/// buttons and axes a standard gamepad doesn't have).
#[derive(Resource)]
pub struct Hotas {
    gilrs: Option<SyncCell<gilrs::Gilrs>>,
    devices: HashMap<gilrs::GamepadId, Device>,
    /// Last throttle value written, so the throttle keys work until the lever moves.
    throttle_applied: Option<f32>,
}

impl Hotas {
    fn new() -> Self {
        let gilrs = match gilrs::GilrsBuilder::new().with_default_filters(false).set_update_state(false).build() {
            Ok(g) => Some(g),
            Err(e) => {
                warn!("joystick input unavailable: {e}");
                None
            }
        };
        let mut devices = HashMap::new();
        if let Some(g) = &gilrs {
            for (id, pad) in g.gamepads() {
                info!("joystick: {}", pad.name());
                devices.insert(id, Device { name: pad.name().to_string(), ..default() });
            }
        }
        Self { gilrs: gilrs.map(SyncCell::new), devices, throttle_applied: None }
    }

    /// Connected devices, sorted by name.
    pub fn devices(&self) -> Vec<&Device> {
        let mut v: Vec<_> = self.devices.values().collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }

    /// Current raw value of a device axis (None until it has moved).
    pub fn axis(&self, device: &str, code: u32) -> Option<f32> {
        self.devices.values().filter(|d| d.name == device).find_map(|d| d.axes.get(&code).copied())
    }

    fn active(&self, device: &str, source: Source, prev: bool) -> bool {
        self.devices.values().any(|d| d.name == device && d.active(source, prev))
    }

    fn poll(&mut self) {
        for d in self.devices.values_mut() {
            d.prev_buttons.clone_from(&d.buttons);
            d.prev_axes.clone_from(&d.axes);
        }
        let Some(g) = &mut self.gilrs else { return };
        let g = g.get();
        while let Some(ev) = g.next_event() {
            use gilrs::EventType as E;
            if matches!(ev.event, E::Connected) {
                let name = g.gamepad(ev.id).name().to_string();
                info!("joystick connected: {name}");
                self.devices.entry(ev.id).or_insert_with(|| Device { name, ..default() });
                continue;
            }
            if matches!(ev.event, E::Disconnected) {
                self.devices.remove(&ev.id);
                continue;
            }
            let Some(d) = self.devices.get_mut(&ev.id) else { continue };
            match ev.event {
                E::ButtonPressed(_, code) => {
                    d.buttons.insert(code.into_u32());
                }
                E::ButtonReleased(_, code) => {
                    d.buttons.remove(&code.into_u32());
                }
                E::AxisChanged(_, v, code) => {
                    d.axes.insert(code.into_u32(), v);
                }
                _ => {}
            }
        }
    }
}

/// Reads the joysticks and applies the mapping (after the keyboard and gamepad, overriding them).
pub fn read_hotas(mut hotas: ResMut<Hotas>, config: Res<InputConfig>, mut controls: ResMut<FlightControls>, mut actions: MessageWriter<ActionEvent>) {
    hotas.poll();
    for m in &config.axes {
        let Some(raw) = hotas.axis(&m.device, m.code) else { continue };
        let v = m.output(raw);
        match m.control {
            AxisControl::Roll if v != 0.0 => controls.roll = v,
            AxisControl::Pitch if v != 0.0 => controls.pitch = v,
            AxisControl::Yaw if v != 0.0 => controls.yaw = v,
            AxisControl::Throttle if hotas.throttle_applied.is_none_or(|t| (t - v).abs() > 0.01) => {
                hotas.throttle_applied = Some(v);
                controls.throttle = Some(v);
            }
            _ => {}
        }
    }
    for m in &config.buttons {
        let now = hotas.active(&m.device, m.source, false);
        match m.target {
            ButtonTarget::Gun => controls.gun |= now,
            ButtonTarget::Action(a) => {
                if now && !hotas.active(&m.device, m.source, true) {
                    actions.write(ActionEvent(a));
                }
            }
        }
    }
}

/// Every action with its default key(s), in the key summary's order.
pub fn action_list() -> Vec<(Action, String)> {
    let mut out: Vec<(Action, String)> = Vec::new();
    for (action, key) in crate::input::default_keys() {
        match out.iter_mut().find(|(a, _)| *a == action) {
            Some((_, keys)) => {
                keys.push_str(", ");
                keys.push_str(&key);
            }
            None => out.push((action, key)),
        }
    }
    out
}

/// "ThrottleUp" -> "Throttle up".
pub fn action_name(a: Action) -> String {
    let mut s = String::new();
    for (i, c) in format!("{a:?}").chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            s.push(' ');
            s.push(c.to_ascii_lowercase());
        } else {
            s.push(c);
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binding_replaces_and_round_trips() {
        let mut c = InputConfig::default();
        c.bind_axis(AxisControl::Roll, "Stick".into(), 0x30000);
        c.bind_axis(AxisControl::Pitch, "Stick".into(), 0x30000);
        assert_eq!(c.axes.len(), 1, "an axis drives one control");
        c.bind_button(ButtonTarget::Gun, "Stick".into(), Source::Button(0x10120));
        c.bind_button(ButtonTarget::Action(Action::Flare), "Stick".into(), Source::Button(0x10120));
        c.bind_button(ButtonTarget::Action(Action::Chaff), "Stick".into(), Source::AxisNegative(0x30010));
        assert_eq!(c.buttons.len(), 2, "a button does one thing");
        let back: InputConfig = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(back, c);
        // Missing fields fall back to the defaults.
        let partial: InputConfig = serde_json::from_str("{}").unwrap();
        assert!(partial.gamepad_defaults);
    }

    #[test]
    fn axis_shaping() {
        let m = AxisMapping { control: AxisControl::Roll, device: String::new(), code: 0, invert: false, deadzone: 0.1 };
        assert_eq!(m.output(0.05), 0.0);
        assert_eq!(m.output(1.0), 1.0);
        assert!((m.output(-0.55) + 0.5).abs() < 1e-6);
        let t = AxisMapping { control: AxisControl::Throttle, invert: true, ..m };
        assert_eq!(t.output(-1.0), 1.0);
        assert_eq!(t.output(1.0), 0.0);
    }
}
