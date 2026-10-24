//! HOTAS / joystick input: raw device buttons and axes (by platform event code, so HOTAS
//! buttons that are not part of a standard gamepad layout work too), mapped to flight axes and
//! actions by an `InputConfig` that the menu's input mapping dialog edits and saves as JSON in
//! the user config directory.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use bevy::platform::cell::SyncCell;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::input::{Action, ActionEvent, FlightControls, Mods};

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
    /// Sensor/cursor slew (modern avionics): right and up.
    SlewX,
    SlewY,
}

impl AxisControl {
    pub const ALL: [AxisControl; 6] = [
        AxisControl::Roll,
        AxisControl::Pitch,
        AxisControl::Yaw,
        AxisControl::Throttle,
        AxisControl::SlewX,
        AxisControl::SlewY,
    ];

    pub fn name(self) -> &'static str {
        match self {
            AxisControl::Roll => "Roll",
            AxisControl::Pitch => "Pitch",
            AxisControl::Yaw => "Rudder",
            AxisControl::Throttle => "Throttle",
            AxisControl::SlewX => "Sensor/cursor slew X",
            AxisControl::SlewY => "Sensor/cursor slew Y",
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
    /// The device's `Device::key` (older mappings: its name, which only counts while one
    /// connected device has it).
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

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VrHand {
    Left,
    Right,
}

/// A VR controller input usable as a button (thumbstick directions count when pushed most of
/// the way).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VrInput {
    Trigger,
    /// A / X.
    Primary,
    /// B / Y.
    Secondary,
    ThumbClick,
    ThumbUp,
    ThumbDown,
    ThumbLeft,
    ThumbRight,
}

impl VrInput {
    pub const ALL: [VrInput; 8] = [
        VrInput::Trigger,
        VrInput::Primary,
        VrInput::Secondary,
        VrInput::ThumbClick,
        VrInput::ThumbUp,
        VrInput::ThumbDown,
        VrInput::ThumbLeft,
        VrInput::ThumbRight,
    ];

    pub fn name(self, hand: VrHand) -> &'static str {
        match (self, hand) {
            (VrInput::Trigger, _) => "Trigger",
            (VrInput::Primary, VrHand::Left) => "X",
            (VrInput::Primary, VrHand::Right) => "A",
            (VrInput::Secondary, VrHand::Left) => "Y",
            (VrInput::Secondary, VrHand::Right) => "B",
            (VrInput::ThumbClick, _) => "Stick click",
            (VrInput::ThumbUp, _) => "Stick up",
            (VrInput::ThumbDown, _) => "Stick down",
            (VrInput::ThumbLeft, _) => "Stick left",
            (VrInput::ThumbRight, _) => "Stick right",
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct VrButtonMapping {
    pub hand: VrHand,
    pub input: VrInput,
    pub target: ButtonTarget,
}

/// A keyboard key (with a modifier) bound to the gun or an action.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct KeyMapping {
    pub target: ButtonTarget,
    pub mods: Mods,
    #[serde(with = "key_name")]
    pub key: KeyCode,
}

impl KeyMapping {
    pub fn label(&self) -> String {
        crate::input::key_label(self.mods, self.key)
    }
}

/// Keys saved by their `KeyCode` variant name.
mod key_name {
    use bevy::prelude::KeyCode;
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub fn serialize<S: Serializer>(key: &KeyCode, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format!("{key:?}"))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<KeyCode, D::Error> {
        let name = String::deserialize(d)?;
        crate::input::key_from_name(&name)
            .ok_or_else(|| D::Error::custom(format!("unknown key {name}")))
    }
}

/// Default VR controller buttons, as a HOTAS: the right hand flies and fights (trigger fires,
/// A the gun, B the next weapon), its thumbstick only targets (up: nearest target ahead, down:
/// drop the locks, right: next target, left: designate, click: bay doors); the left hand runs
/// the aircraft (trigger brakes, X flare, Y chaff) and its thumbstick slews the cursor, its click
/// locking what is under it. The decoy is on the cockpit's DECOY button. The triggers only count
/// while the hand holds the stick or throttle (otherwise they press the cockpit control under the
/// fingertip). Wheel brakes on a VR button are on while it is held.
pub fn default_vr_buttons() -> Vec<VrButtonMapping> {
    use Action as A;
    use VrHand::*;
    use VrInput::*;
    [
        (Right, Trigger, ButtonTarget::Action(A::FireWeapon)),
        (Right, Primary, ButtonTarget::Gun),
        (Right, Secondary, ButtonTarget::Action(A::SelectWeapon)),
        (Right, ThumbUp, ButtonTarget::Action(A::NearestTarget)),
        (Right, ThumbDown, ButtonTarget::Action(A::ClearTarget)),
        (Right, ThumbRight, ButtonTarget::Action(A::SelectTarget)),
        (Right, ThumbLeft, ButtonTarget::Action(A::DesignateTarget)),
        (Right, ThumbClick, ButtonTarget::Action(A::BayDoors)),
        (Left, Trigger, ButtonTarget::Action(A::Brakes)),
        (Left, Primary, ButtonTarget::Action(A::Flare)),
        (Left, Secondary, ButtonTarget::Action(A::Chaff)),
        (Left, ThumbClick, ButtonTarget::Action(A::LockCursor)),
    ]
    .into_iter()
    .map(|(hand, input, target)| VrButtonMapping { hand, input, target })
    .collect()
}

/// The default VR buttons from mapping version 5 to 8 (`migrate` replaces them, unchanged, by
/// the current ones).
fn vr_buttons_v8() -> Vec<VrButtonMapping> {
    use Action as A;
    use VrHand::*;
    use VrInput::*;
    [
        (Right, Trigger, ButtonTarget::Action(A::FireWeapon)),
        (Right, Primary, ButtonTarget::Gun),
        (Right, Secondary, ButtonTarget::Action(A::SelectWeapon)),
        (Right, ThumbUp, ButtonTarget::Action(A::NearestTarget)),
        (Right, ThumbDown, ButtonTarget::Action(A::ClearTarget)),
        (Right, ThumbRight, ButtonTarget::Action(A::SelectTarget)),
        (Right, ThumbLeft, ButtonTarget::Action(A::DesignateTarget)),
        (Right, ThumbClick, ButtonTarget::Action(A::LockCursor)),
        (Left, Trigger, ButtonTarget::Action(A::Brakes)),
        (Left, Primary, ButtonTarget::Action(A::Flare)),
        (Left, Secondary, ButtonTarget::Action(A::Chaff)),
        (Left, ThumbClick, ButtonTarget::Action(A::BayDoors)),
    ]
    .into_iter()
    .map(|(hand, input, target)| VrButtonMapping { hand, input, target })
    .collect()
}

/// The default VR buttons up to mapping version 4 (`migrate` replaces them, unchanged, by the
/// current ones).
fn vr_buttons_v4() -> Vec<VrButtonMapping> {
    use Action as A;
    use VrHand::*;
    use VrInput::*;
    [
        (Right, Trigger, ButtonTarget::Action(A::FireWeapon)),
        (Right, Primary, ButtonTarget::Gun),
        (Right, Secondary, ButtonTarget::Action(A::SelectWeapon)),
        (Right, ThumbClick, ButtonTarget::Action(A::NearestTarget)),
        (Right, ThumbRight, ButtonTarget::Action(A::SelectTarget)),
        (Right, ThumbDown, ButtonTarget::Action(A::DesignateTarget)),
        (Left, Trigger, ButtonTarget::Action(A::Brakes)),
        (Right, ThumbLeft, ButtonTarget::Action(A::BayDoors)),
        (Left, Primary, ButtonTarget::Action(A::Flare)),
        (Left, Secondary, ButtonTarget::Action(A::Chaff)),
        (Left, ThumbClick, ButtonTarget::Action(A::LockCursor)),
        (Right, ThumbUp, ButtonTarget::Action(A::Decoy)),
    ]
    .into_iter()
    .map(|(hand, input, target)| VrButtonMapping { hand, input, target })
    .collect()
}

const FILE: &str = "input.json";

/// Saved input mapping (`<config dir>/stealth-mission-89/input.json`).
#[derive(Resource, Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(default)]
pub struct InputConfig {
    /// The built-in gamepad layout (left stick, triggers, face buttons) on top of the mapping.
    pub gamepad_defaults: bool,
    pub axes: Vec<AxisMapping>,
    pub buttons: Vec<ButtonMapping>,
    pub vr_buttons: Vec<VrButtonMapping>,
    /// Keyboard keys for the gun, the actions and the keyboard stick.
    pub keys: Vec<KeyMapping>,
    /// Mapping version (`migrate`); files from before the versions read 0.
    #[serde(default)]
    pub version: u32,
}

/// 1: the cursor lock (NumpadEnter, left thumbstick click) and SOI keys. 2: camera zoom keys.
/// 3: the left VR trigger holds the brakes, the bay doors move to the right thumbstick left;
/// the bombsight mode key. 4: the stick keys (the arrows) became bindable. 5: the rudder keys
/// (Q / E); the VR thumbsticks as a HOTAS (the right one targets only). 6: the trim keys
/// (Alt + arrows, Alt + Home). 7: the master caution key (Alt + C). 8: the free view key
/// (Shift + F7). 9: the VR stick clicks swap (left locks, right bay doors).
const VERSION: u32 = 9;

impl Default for InputConfig {
    fn default() -> Self {
        Self {
            gamepad_defaults: true,
            axes: Vec::new(),
            buttons: Vec::new(),
            vr_buttons: default_vr_buttons(),
            keys: crate::input::default_key_mappings(),
            version: VERSION,
        }
    }
}

impl InputConfig {
    pub fn load() -> Self {
        let Some(mut c) = crate::prefs::load_json::<Self>(FILE) else { return Self::default() };
        if c.migrate()
            && let Err(e) = c.save()
        {
            warn!("saving {FILE}: {e}");
        }
        c
    }

    /// Brings an older saved mapping up to date; true if it changed. Every version: new actions
    /// get their default keys where those are free. Version 1: NumpadEnter locks the cursor
    /// target instead of firing, the left thumbstick click locks (the decoy moves to the right
    /// stick up).
    fn migrate(&mut self) -> bool {
        if self.version >= VERSION {
            return false;
        }
        use crate::input::{Action, Mods};
        if self.version < 1 {
            for k in &mut self.keys {
                if k.mods == Mods::None
                    && k.key == KeyCode::NumpadEnter
                    && k.target == ButtonTarget::Action(Action::FireWeapon)
                {
                    k.target = ButtonTarget::Action(Action::LockCursor);
                }
            }
            let slot = |c: &Self, hand: VrHand, input: VrInput| {
                c.vr_buttons.iter().position(|m| m.hand == hand && m.input == input)
            };
            if let Some(i) = slot(self, VrHand::Left, VrInput::ThumbClick)
                && self.vr_buttons[i].target == ButtonTarget::Action(Action::Decoy)
            {
                self.vr_buttons[i].target = ButtonTarget::Action(Action::LockCursor);
                if slot(self, VrHand::Right, VrInput::ThumbUp).is_none() {
                    self.vr_buttons.push(VrButtonMapping {
                        hand: VrHand::Right,
                        input: VrInput::ThumbUp,
                        target: ButtonTarget::Action(Action::Decoy),
                    });
                }
            }
        }
        if self.version < 3 {
            let slot = |c: &Self, hand: VrHand, input: VrInput| {
                c.vr_buttons.iter().position(|m| m.hand == hand && m.input == input)
            };
            if let Some(i) = slot(self, VrHand::Left, VrInput::Trigger)
                && self.vr_buttons[i].target == ButtonTarget::Action(Action::BayDoors)
            {
                self.vr_buttons[i].target = ButtonTarget::Action(Action::Brakes);
                if slot(self, VrHand::Right, VrInput::ThumbLeft).is_none() {
                    self.vr_buttons.push(VrButtonMapping {
                        hand: VrHand::Right,
                        input: VrInput::ThumbLeft,
                        target: ButtonTarget::Action(Action::BayDoors),
                    });
                }
            }
        }
        // The old default VR buttons, unchanged, become the new ones (a customised set stays).
        if self.version < 5 {
            let old = vr_buttons_v4();
            if self.vr_buttons.len() == old.len() && old.iter().all(|m| self.vr_buttons.contains(m))
            {
                self.vr_buttons = default_vr_buttons();
            }
        }
        if self.version < 9 {
            let old = vr_buttons_v8();
            if self.vr_buttons.len() == old.len() && old.iter().all(|m| self.vr_buttons.contains(m))
            {
                self.vr_buttons = default_vr_buttons();
            }
        }
        for d in crate::input::default_key_mappings() {
            let free = !self.keys.iter().any(|k| k.mods == d.mods && k.key == d.key);
            if free && self.keys_for(d.target).next().is_none() {
                self.keys.push(d);
            }
        }
        self.version = VERSION;
        true
    }

    pub fn save(&self) -> anyhow::Result<()> {
        crate::prefs::save_json(FILE, self)
    }

    pub fn button(&self, target: ButtonTarget) -> Option<&ButtonMapping> {
        self.buttons.iter().find(|m| m.target == target)
    }

    /// Binds an axis, taking it away from any other control.
    pub fn bind_axis(&mut self, control: AxisControl, device: String, code: u32) {
        self.axes.retain(|m| m.control != control && !(m.device == device && m.code == code));
        let invert = control == AxisControl::Throttle;
        let deadzone = match control {
            AxisControl::Throttle => 0.0,
            AxisControl::SlewX | AxisControl::SlewY => 0.1,
            _ => 0.05,
        };
        self.axes.push(AxisMapping { control, device, code, invert, deadzone });
    }

    /// Binds a button, taking it away from any other target.
    pub fn bind_button(&mut self, target: ButtonTarget, device: String, source: Source) {
        self.buttons.retain(|m| m.target != target && !(m.device == device && m.source == source));
        self.buttons.push(ButtonMapping { target, device, source });
    }

    /// Keys bound to a target.
    pub fn keys_for(&self, target: ButtonTarget) -> impl Iterator<Item = &KeyMapping> {
        self.keys.iter().filter(move |m| m.target == target)
    }

    /// Adds a key to a target, taking it away from any other target.
    pub fn bind_key(&mut self, target: ButtonTarget, mods: Mods, key: KeyCode) {
        self.keys.retain(|m| !(m.mods == mods && m.key == key));
        self.keys.push(KeyMapping { target, mods, key });
    }

    pub fn vr_button(&self, hand: VrHand, input: VrInput) -> Option<ButtonTarget> {
        self.vr_buttons.iter().find(|m| m.hand == hand && m.input == input).map(|m| m.target)
    }

    /// Sets (or with None clears) what a VR controller input does.
    pub fn bind_vr(&mut self, hand: VrHand, input: VrInput, target: Option<ButtonTarget>) {
        self.vr_buttons.retain(|m| !(m.hand == hand && m.input == input));
        if let Some(target) = target {
            self.vr_buttons.push(VrButtonMapping { hand, input, target });
        }
    }
}

/// A connected joystick, HOTAS part, pedals or gamepad.
#[derive(Debug, Default, Clone)]
pub struct Device {
    pub name: String,
    /// The USB product name, where the platform's name is a generic one (Windows calls every
    /// stick, throttle and pedal set "HID-compliant game controller" or "USB HID device").
    product: Option<String>,
    /// What mappings name the device by: the product name (else the name), the USB vendor and
    /// product ids, and "#2", "#3" for further devices of the same model.
    pub key: String,
    /// The key by the platform's name, as older mappings named a device with a product name.
    alias: String,
    usb_id: Option<(u16, u16)>,
    pub axes: BTreeMap<u32, f32>,
    pub buttons: BTreeSet<u32>,
    prev_buttons: BTreeSet<u32>,
    prev_axes: BTreeMap<u32, f32>,
}

impl Device {
    fn new(pad: gilrs::Gamepad) -> Self {
        let usb_id = pad.vendor_id().zip(pad.product_id());
        #[cfg(windows)]
        let product = usb_id.and_then(|(v, p)| windows_hid::product(v, p));
        #[cfg(not(windows))]
        let product = None;
        Self { name: pad.name().to_string(), product, usb_id, ..default() }
    }

    fn active(&self, source: Source, prev: bool) -> bool {
        let (buttons, axes) =
            if prev { (&self.prev_buttons, &self.prev_axes) } else { (&self.buttons, &self.axes) };
        match source {
            Source::Button(c) => buttons.contains(&c),
            Source::AxisPositive(c) => axes.get(&c).is_some_and(|v| *v > 0.5),
            Source::AxisNegative(c) => axes.get(&c).is_some_and(|v| *v < -0.5),
        }
    }
}

/// Sets each device's `key`; devices of the same model are numbered in connection order.
fn assign_keys(devices: &mut HashMap<gilrs::GamepadId, Device>) {
    let mut ids: Vec<_> = devices.keys().copied().collect();
    ids.sort_by_key(|id| usize::from(*id));
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut key = |name: &str, usb_id: Option<(u16, u16)>| {
        let base = match usb_id {
            Some((v, p)) => format!("{name} [{v:04x}:{p:04x}]"),
            None => name.to_string(),
        };
        let n = seen.entry(base.clone()).or_default();
        *n += 1;
        if *n == 1 { base } else { format!("{base} #{n}") }
    };
    for id in ids {
        let d = devices.get_mut(&id).unwrap();
        d.key = key(d.product.as_deref().unwrap_or(&d.name), d.usb_id);
        d.alias = if d.product.is_some() { key(&d.name, d.usb_id) } else { d.key.clone() };
    }
}

/// The USB product name of a HID device, from its descriptor.
#[cfg(windows)]
mod windows_hid {
    use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
        DIGCF_DEVICEINTERFACE, DIGCF_PRESENT, SP_DEVICE_INTERFACE_DATA,
        SP_DEVICE_INTERFACE_DETAIL_DATA_W, SetupDiDestroyDeviceInfoList,
        SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsW, SetupDiGetDeviceInterfaceDetailW,
    };
    use windows_sys::Win32::Devices::HumanInterfaceDevice::{
        HIDD_ATTRIBUTES, HidD_GetAttributes, HidD_GetHidGuid, HidD_GetProductString,
    };
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };

    /// The product string of the first present HID interface with these vendor and product ids.
    pub fn product(vendor: u16, product: u16) -> Option<String> {
        // SAFETY: plain SetupAPI / HID calls on structures sized and initialised as the API
        // asks; the device info list and each opened handle are released before returning.
        unsafe {
            let mut guid = std::mem::zeroed();
            HidD_GetHidGuid(&mut guid);
            let set = SetupDiGetClassDevsW(
                &guid,
                std::ptr::null(),
                std::ptr::null_mut(),
                DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
            );
            if set == -1 {
                return None;
            }
            let mut found = None;
            let mut i = 0;
            while found.is_none() {
                let mut iface: SP_DEVICE_INTERFACE_DATA = std::mem::zeroed();
                iface.cbSize = size_of::<SP_DEVICE_INTERFACE_DATA>() as u32;
                if SetupDiEnumDeviceInterfaces(set, std::ptr::null(), &guid, i, &mut iface) == 0 {
                    break;
                }
                i += 1;
                let mut size = 0;
                SetupDiGetDeviceInterfaceDetailW(
                    set,
                    &iface,
                    std::ptr::null_mut(),
                    0,
                    &mut size,
                    std::ptr::null_mut(),
                );
                // u32s: aligned for the detail struct.
                let mut buf = vec![0u32; (size as usize).div_ceil(4).max(2)];
                let detail = buf.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
                (*detail).cbSize = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
                if SetupDiGetDeviceInterfaceDetailW(
                    set,
                    &iface,
                    detail,
                    size,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                ) == 0
                {
                    continue;
                }
                // No access rights: enough to query the attributes and strings.
                let path = std::ptr::addr_of!((*detail).DevicePath) as *const u16;
                let handle = CreateFileW(
                    path,
                    0,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    std::ptr::null(),
                    OPEN_EXISTING,
                    0,
                    std::ptr::null_mut(),
                );
                if handle == INVALID_HANDLE_VALUE {
                    continue;
                }
                let mut attrs: HIDD_ATTRIBUTES = std::mem::zeroed();
                attrs.Size = size_of::<HIDD_ATTRIBUTES>() as u32;
                if HidD_GetAttributes(handle, &mut attrs)
                    && attrs.VendorID == vendor
                    && attrs.ProductID == product
                {
                    // The USB limit: 126 UTF-16 units and the NUL.
                    let mut name = [0u16; 127];
                    if HidD_GetProductString(
                        handle,
                        name.as_mut_ptr().cast(),
                        size_of_val(&name) as u32,
                    ) {
                        let len = name.iter().position(|&c| c == 0).unwrap_or(name.len());
                        let text = String::from_utf16_lossy(&name[..len]).trim().to_string();
                        found = Some(text).filter(|t| !t.is_empty());
                    }
                }
                CloseHandle(handle);
            }
            SetupDiDestroyDeviceInfoList(set);
            found
        }
    }
}

/// Drops gilrs' fallback Xbox layout from a device without an SDL mapping (every HOTAS): it
/// turns four axis slots into triggers, which arrive as button events, so a throttle there
/// couldn't be bound as an axis.
fn raw_mapping(g: &mut gilrs::Gilrs, id: gilrs::GamepadId) {
    if g.gamepad(id).mapping_source() != gilrs::MappingSource::Driver {
        return;
    }
    if let Err(e) = g.set_mapping(usize::from(id), &gilrs::Mapping::new(), None) {
        warn!("joystick {}: can't clear the default mapping: {e}", g.gamepad(id).name());
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
        let mut gilrs = match gilrs::GilrsBuilder::new()
            .with_default_filters(false)
            .set_update_state(false)
            .build()
        {
            Ok(g) => Some(g),
            Err(e) => {
                warn!("joystick input unavailable: {e}");
                None
            }
        };
        let mut devices = HashMap::new();
        if let Some(g) = &mut gilrs {
            let ids: Vec<_> = g.gamepads().map(|(id, _)| id).collect();
            for id in ids {
                raw_mapping(g, id);
                devices.insert(id, Device::new(g.gamepad(id)));
            }
        }
        assign_keys(&mut devices);
        for d in devices.values() {
            info!("joystick: {}", d.key);
        }
        Self { gilrs: gilrs.map(SyncCell::new), devices, throttle_applied: None }
    }

    /// Connected devices, sorted by key.
    pub fn devices(&self) -> Vec<&Device> {
        let mut v: Vec<_> = self.devices.values().collect();
        v.sort_by(|a, b| a.key.cmp(&b.key));
        v
    }

    /// The device a mapping names, by key (or alias) or (older mappings) by a name no other
    /// device has.
    fn mapped<'a>(&'a self, device: &'a str) -> impl Iterator<Item = &'a Device> {
        let by_key = self.devices.values().find(|d| d.key == device || d.alias == device);
        let by_name = || {
            let mut named = self.devices.values().filter(|d| d.name == device);
            named.next().filter(|_| named.next().is_none())
        };
        by_key.or_else(by_name).into_iter()
    }

    /// Current raw value of a device axis (None until it has moved).
    pub fn axis(&self, device: &str, code: u32) -> Option<f32> {
        self.mapped(device).find_map(|d| d.axes.get(&code).copied())
    }

    fn active(&self, device: &str, source: Source, prev: bool) -> bool {
        self.mapped(device).any(|d| d.active(source, prev))
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
                raw_mapping(g, ev.id);
                self.devices.entry(ev.id).or_insert_with(|| Device::new(g.gamepad(ev.id)));
                assign_keys(&mut self.devices);
                info!("joystick connected: {}", self.devices[&ev.id].key);
                continue;
            }
            if matches!(ev.event, E::Disconnected) {
                self.devices.remove(&ev.id);
                assign_keys(&mut self.devices);
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
pub fn read_hotas(
    mut hotas: ResMut<Hotas>,
    config: Res<InputConfig>,
    mut controls: ResMut<FlightControls>,
    mut actions: MessageWriter<ActionEvent>,
    block: Res<crate::input::InputBlock>,
) {
    hotas.poll();
    if block.0 {
        for m in &config.buttons {
            if m.target == ButtonTarget::Action(Action::Menu)
                && hotas.active(&m.device, m.source, false)
                && !hotas.active(&m.device, m.source, true)
            {
                actions.write(ActionEvent(Action::Menu));
            }
        }
        return;
    }
    for m in &config.axes {
        let Some(raw) = hotas.axis(&m.device, m.code) else { continue };
        let v = m.output(raw);
        match m.control {
            AxisControl::Roll => controls.roll = crate::input::stronger(controls.roll, v),
            AxisControl::Pitch => controls.pitch = crate::input::stronger(controls.pitch, v),
            AxisControl::Yaw => controls.yaw = crate::input::stronger(controls.yaw, v),
            AxisControl::SlewX => controls.slew.x = crate::input::stronger(controls.slew.x, v),
            AxisControl::SlewY => controls.slew.y = crate::input::stronger(controls.slew.y, v),
            AxisControl::Throttle
                if hotas.throttle_applied.is_none_or(|t| (t - v).abs() > 0.01) =>
            {
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
                if let Some((r, p, y)) = a.stick_direction() {
                    if now {
                        let d = crate::input::KEY_DEFLECTION;
                        controls.roll = crate::input::stronger(controls.roll, r * d);
                        controls.pitch = crate::input::stronger(controls.pitch, p * d);
                        controls.yaw = crate::input::stronger(controls.yaw, y * d);
                    }
                    continue;
                }
                if let Some(d) = a.trim_direction() {
                    if now {
                        controls.trim += d;
                    }
                    continue;
                }
                controls.fire |= now && a == Action::FireWeapon;
                controls.mark |= now && a == Action::LockCursor;
                if now && !hotas.active(&m.device, m.source, true) {
                    actions.write(ActionEvent(a));
                }
            }
        }
    }
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

    /// A mapping saved before the cursor lock: NumpadEnter fired, the left thumbstick click
    /// dropped a decoy.
    #[test]
    fn old_mapping_migrates() {
        use crate::input::Mods;
        let mut c = InputConfig { version: 0, ..InputConfig::default() };
        c.keys.retain(|k| {
            !matches!(
                k.target,
                ButtonTarget::Action(
                    Action::LockCursor
                        | Action::SwapSoi
                        | Action::CameraZoomIn
                        | Action::CameraZoomOut
                )
            )
        });
        c.keys.push(KeyMapping {
            target: ButtonTarget::Action(Action::FireWeapon),
            mods: Mods::None,
            key: KeyCode::NumpadEnter,
        });
        c.vr_buttons = vr_buttons_v4();
        c.vr_buttons.retain(|m| !(m.hand == VrHand::Right && m.input == VrInput::ThumbUp));
        for m in &mut c.vr_buttons {
            if m.hand == VrHand::Left && m.input == VrInput::ThumbClick {
                m.target = ButtonTarget::Action(Action::Decoy);
            }
        }
        let json = serde_json::to_string(&c).unwrap().replace(",\"version\":0", "");
        let mut c: InputConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(c.version, 0);
        assert!(c.migrate());
        let key =
            |t| c.keys_for(ButtonTarget::Action(t)).map(|k| (k.mods, k.key)).collect::<Vec<_>>();
        assert_eq!(key(Action::LockCursor), vec![(Mods::None, KeyCode::NumpadEnter)]);
        assert_eq!(key(Action::SwapSoi), vec![(Mods::Alt, KeyCode::KeyS)]);
        assert_eq!(key(Action::CameraZoomIn), vec![(Mods::None, KeyCode::BracketRight)]);
        assert_eq!(key(Action::FireWeapon), vec![(Mods::None, KeyCode::Enter)]);
        // Brought up to the version 4 defaults, then on to the current ones.
        assert_eq!(c.vr_buttons, default_vr_buttons());
        assert!(!c.migrate());
    }

    /// The old default VR buttons become the new ones; a customised set stays as it is.
    #[test]
    fn vr_buttons_migrate() {
        let mut c = InputConfig { version: 4, vr_buttons: vr_buttons_v4(), ..default() };
        c.vr_buttons.reverse();
        assert!(c.migrate());
        assert_eq!(c.vr_buttons, default_vr_buttons());

        let mut custom = vr_buttons_v4();
        custom[0].target = ButtonTarget::Action(Action::Kneeboard);
        let mut c = InputConfig { version: 4, vr_buttons: custom.clone(), ..default() };
        assert!(c.migrate());
        assert_eq!(c.vr_buttons, custom);

        let mut c = InputConfig { version: 8, vr_buttons: vr_buttons_v8(), ..default() };
        assert!(c.migrate());
        assert_eq!(c.vr_buttons, default_vr_buttons());
    }

    /// A mapping saved while the stick keys were fixed gets the arrows back as bindings, and
    /// keeps a key the player already moved them to.
    #[test]
    fn stick_keys_migrate() {
        use crate::input::Mods;
        let mut c = InputConfig { version: 3, ..InputConfig::default() };
        c.keys.retain(|k| {
            !matches!(
                k.target,
                ButtonTarget::Action(
                    Action::PitchUp | Action::PitchDown | Action::RollLeft | Action::RollRight
                )
            )
        });
        assert!(c.migrate());
        let key = |c: &InputConfig, t| {
            c.keys_for(ButtonTarget::Action(t)).map(|k| (k.mods, k.key)).collect::<Vec<_>>()
        };
        assert_eq!(key(&c, Action::PitchUp), vec![(Mods::None, KeyCode::ArrowDown)]);
        assert_eq!(key(&c, Action::RollLeft), vec![(Mods::None, KeyCode::ArrowLeft)]);
        assert_eq!(key(&c, Action::YawLeft), vec![(Mods::None, KeyCode::KeyQ)]);

        let mut c = InputConfig { version: 3, ..InputConfig::default() };
        c.keys.retain(|k| {
            !matches!(
                k.target,
                ButtonTarget::Action(
                    Action::PitchUp | Action::PitchDown | Action::RollLeft | Action::RollRight
                )
            )
        });
        c.bind_key(ButtonTarget::Action(Action::RollLeft), Mods::None, KeyCode::KeyA);
        c.migrate();
        assert_eq!(key(&c, Action::RollLeft), vec![(Mods::None, KeyCode::KeyA)]);
    }

    #[test]
    fn binding_replaces_and_round_trips() {
        let mut c = InputConfig::default();
        c.bind_axis(AxisControl::Roll, "Stick".into(), 0x30000);
        c.bind_axis(AxisControl::Pitch, "Stick".into(), 0x30000);
        assert_eq!(c.axes.len(), 1, "an axis drives one control");
        c.bind_button(ButtonTarget::Gun, "Stick".into(), Source::Button(0x10120));
        c.bind_button(ButtonTarget::Action(Action::Flare), "Stick".into(), Source::Button(0x10120));
        c.bind_button(
            ButtonTarget::Action(Action::Chaff),
            "Stick".into(),
            Source::AxisNegative(0x30010),
        );
        assert_eq!(c.buttons.len(), 2, "a button does one thing");
        c.bind_vr(VrHand::Left, VrInput::Trigger, Some(ButtonTarget::Gun));
        assert_eq!(c.vr_button(VrHand::Left, VrInput::Trigger), Some(ButtonTarget::Gun));
        c.bind_vr(VrHand::Left, VrInput::Primary, None);
        assert_eq!(c.vr_button(VrHand::Left, VrInput::Primary), None);
        let back: InputConfig = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(back, c);
        // Missing fields fall back to the defaults.
        let partial: InputConfig = serde_json::from_str("{}").unwrap();
        assert!(partial.gamepad_defaults);
        assert_eq!(partial.vr_buttons, default_vr_buttons());
        assert_eq!(partial.keys, crate::input::default_key_mappings());
        c.bind_key(ButtonTarget::Action(Action::Chaff), Mods::None, KeyCode::Digit1);
        assert!(
            c.keys_for(ButtonTarget::Action(Action::Flare)).next().is_none(),
            "a key does one thing"
        );
        assert_eq!(c.keys_for(ButtonTarget::Action(Action::Chaff)).count(), 2);
        let back: InputConfig = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn axis_shaping() {
        let m = AxisMapping {
            control: AxisControl::Roll,
            device: String::new(),
            code: 0,
            invert: false,
            deadzone: 0.1,
        };
        assert_eq!(m.output(0.05), 0.0);
        assert_eq!(m.output(1.0), 1.0);
        assert!((m.output(-0.55) + 0.5).abs() < 1e-6);
        let t = AxisMapping { control: AxisControl::Throttle, invert: true, ..m };
        assert_eq!(t.output(-1.0), 1.0);
        assert_eq!(t.output(1.0), 0.0);
    }
}
