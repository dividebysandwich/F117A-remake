//! Tooltips (desktop): with the mouse still over a cockpit control, display, instrument or
//! warning light, a box by the cursor names it, says what it does and which keys do the same.
//! It follows the view: whatever comes under the cursor while the head turns, unless something
//! in front hides it (`pick`). No tooltips when switched off (in-flight menu), outside the
//! cockpit views, while looking around (right button held), with the menu or the kneeboard over
//! the view.
//!
//! In VR the part is the control a fingertip touches, else one just ahead of the fingertip; the
//! box is drawn into an image shown on a small screen above the finger, facing the eye point,
//! drawn over the cockpit (readable where it goes into the panel).
//!
//! Every tooltip text is here (to translate them one day; the aviation terms stay English).

use std::sync::LazyLock;

use bevy::camera::RenderTarget;
use bevy::camera::visibility::RenderLayers;
use bevy::light::NotShadowCaster;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{
    ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline,
};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, CompareFunction, Extent3d, RenderPipelineDescriptor, SpecializedMeshPipelineError,
    TextureDimension, TextureFormat, TextureUsages,
};

use super::controls::Control;
use super::displays::{CanvasRole, Face};
use super::layout::{Dial, Effect, Elem, PanelStyle};
use super::pick::{Picker, Pointed};
use super::{CockpitRoot, Hands};
use crate::camera::{View, ViewState};
use crate::hotas::{ButtonTarget, InputConfig};
use crate::input::{Action, key_label, qwerty_key_label};
use crate::mfd::{CamDir, LeftMfd, MfdCmd, MfdState, OsbFn, RightMfd, osb};
use crate::panel::{LAMPS, LampId, LedId};
use crate::settings::{AppState, Settings};

/// Seconds the mouse rests on a part before its tooltip shows.
const DELAY: f32 = 0.4;
/// A tooltip shown this recently: the next one shows at once (moving along the panel).
const LINGER: f32 = 0.3;
/// Longest tooltip line (characters): longer texts are wrapped here, not by the text layout
/// (which would size the box for one line).
const LINE_CHARS: usize = 46;

/// VR: the box's image (pixels; the box is drawn twice as large as on the desktop), the screen's
/// width (metres), how far a part ahead of the fingertip is pointed at (metres) and where the
/// screen is from the fingertip (metres up and towards the eye).
const VR_IMAGE: UVec2 = UVec2::new(1024, 512);
const VR_SCALE: f32 = 2.0;
const VR_WIDTH: f32 = 0.26;
const VR_REACH: f32 = 0.05;
const VR_RAISE: f32 = 0.07;
const VR_TOWARDS_EYE: f32 = 0.04;

/// Tooltip of a cockpit part (the controls' come from their effect): its name, what it does
/// and the actions whose keys do the same.
#[derive(Component, Clone, Debug)]
pub struct Tip {
    title: String,
    detail: String,
    actions: Vec<Action>,
}

impl Tip {
    pub fn new(title: impl Into<String>, detail: impl Into<String>, actions: &[Action]) -> Self {
        Self { title: title.into(), detail: detail.into(), actions: actions.to_vec() }
    }
}

/// An action's name and what it does.
pub fn action_text(a: Action) -> (String, &'static str) {
    use Action::*;
    let (title, detail) = match a {
        Gear => ("Landing gear", "Gear up / down (the lever shows the position)"),
        Autopilot => ("Autopilot", "Flies to the current waypoint"),
        BayDoors => ("Weapon bay doors", "Open / close; weapons are released with open bays only"),
        Flaps => ("Flaps", "More lift, about 20 % lower stall speed"),
        Brakes => ("Brakes", "Wheel brakes on the ground, speed brake in the air"),
        Eject => ("Ejection handle", "Eject"),
        HudMode => ("HUD mode", "NAV, AIR, GND"),
        BombMode => ("Bombing mode", "CCIP / CCRP"),
        HudDeclutter => ("HUD declutter", ""),
        Maps => ("Left MFD page", "Map / tactical"),
        Nvg => ("Night vision goggles", ""),
        Ordnance => ("Weapons page", "Right MFD"),
        Flir => ("FLIR", "Infrared picture on / off"),
        Ils => ("ILS", "Instrument landing system: landing guidance and the HUD steering bars"),
        MissionOrders => ("Mission orders", ""),
        CamAhead => ("Tracking camera", "Looking ahead"),
        CamRear => ("Tracking camera", "Looking to the rear"),
        CamRight => ("Tracking camera", "Looking right"),
        CamLeft => ("Tracking camera", "Looking left"),
        DesignateTarget => {
            ("Designate target", "Blinks while a target is designated, steady within weapon range")
        }
        SelectTarget => ("Next target", "Select the next target"),
        NearestTarget => ("Nearest target", "Lock the nearest target ahead"),
        ClearTarget => ("Clear target", "No automatic acquisition until the next target key"),
        SelectWeapon => ("Select weapon", "The next weapon, after the last one the gun"),
        FireWeapon => ("Fire", "Release the selected weapon"),
        Flare => ("Flare", "Release a flare (against infrared missiles)"),
        Chaff => ("Chaff", "Release chaff (against radar missiles)"),
        Decoy => ("Decoy", "Release a decoy"),
        Ecm => ("ECM", "Radar jammer on / off (lit when on)"),
        IrJammer => ("IR jammer", "On / off; switches itself off after 20 s and must cool down"),
        SelectWaypoint => ("Waypoints", "Waypoint page"),
        ChangeWaypoint => ("Change waypoint", ""),
        ResetWaypoint => ("Reset waypoint", ""),
        LastWaypoint => ("Previous waypoint", ""),
        NextWaypoint => ("Next waypoint", ""),
        Menu => ("Settings menu", "Opens or closes the in-flight menu"),
        Volume => ("Sound volume", ""),
        Detail => ("Detail level", ""),
        TimeAccel => {
            ("Time acceleration", "Ends when a radar detects the aircraft or a weapon is released")
        }
        TimeNormal => ("Normal time", ""),
        MapZoomIn => ("Map zoom in", ""),
        MapZoomOut => ("Map zoom out", ""),
        MapOrientation => ("Map orientation", "North up / heading up"),
        LockCursor => ("Lock cursor target", "Lock what is under the sensor's cursor"),
        SwapSoi => ("Sensor of interest", "Moves to the other MFD"),
        Kneeboard => ("Kneeboard", "Show it over the view, next page, hide"),
        CameraZoomIn => ("Camera zoom in", ""),
        CameraZoomOut => ("Camera zoom out", ""),
        _ => return (crate::hotas::action_name(a), ""),
    };
    (title.to_string(), detail)
}

/// What a display command does.
fn cmd_text(c: MfdCmd) -> String {
    use crate::avionics::Format;
    match c {
        MfdCmd::LeftPage(LeftMfd::Map) => "Map page".into(),
        MfdCmd::LeftPage(LeftMfd::Tac) => "Tactical page".into(),
        MfdCmd::RightPage(RightMfd::Camera) => "Tracking camera page".into(),
        MfdCmd::RightPage(RightMfd::Way) => "Waypoint page".into(),
        MfdCmd::RightPage(RightMfd::Wpn) => "Weapons page".into(),
        MfdCmd::RightPage(RightMfd::Ord) => "Mission orders page".into(),
        MfdCmd::ZoomIn | MfdCmd::MapZoom(1..) => "Map zoom in".into(),
        MfdCmd::ZoomOut | MfdCmd::MapZoom(..=0) => "Map zoom out".into(),
        MfdCmd::ToggleOrient => "Map north up / heading up".into(),
        MfdCmd::Flir => "FLIR: infrared picture on / off".into(),
        MfdCmd::Cam(dir) => format!(
            "Tracking camera looks {}",
            match dir {
                CamDir::Ahead => "ahead",
                CamDir::Right => "right",
                CamDir::Rear => "to the rear",
                CamDir::Left => "left",
            }
        ),
        MfdCmd::Ils => "ILS on / off".into(),
        MfdCmd::Format(_, f) => match f {
            Format::Hsd => "Horizontal situation display",
            Format::Harm => "HARM targeting",
            Format::Tgp => "Targeting pod",
            Format::Map => "Map",
            Format::Sms => "Stores management",
            Format::Hsi => "Horizontal situation indicator",
        }
        .into(),
        MfdCmd::Soi(_) => "Make this display the sensor of interest (the slew keys move it)".into(),
        MfdCmd::SwapSoi => "Sensor of interest to the other MFD".into(),
        MfdCmd::HsdRange(1..) => "HSD range up".into(),
        MfdCmd::HsdRange(..=0) => "HSD range down".into(),
        MfdCmd::HsdCentre => "HSD centred / depressed".into(),
        MfdCmd::MapWay => "Waypoints on the map".into(),
        MfdCmd::Sms(true) => "Mission orders".into(),
        MfdCmd::Sms(false) => "Weapon list".into(),
        MfdCmd::HarmFilter => "HARM threat filter: all, SAM, air".into(),
        MfdCmd::CamZoom(1..) => "Camera zoom in".into(),
        MfdCmd::CamZoom(..=0) => "Camera zoom out".into(),
        MfdCmd::CamZoomCycle => "Camera zoom: auto, 1X to 6X".into(),
        MfdCmd::TgpSp => "Targeting pod snowplow: locks dropped, fixed ahead".into(),
        MfdCmd::TgpSlave => "Targeting pod back to slaved".into(),
    }
}

/// A control's tooltip (bezel buttons: on the current page; None for a blank one).
fn control_tip(effect: Effect, mfd: &MfdState) -> Option<Tip> {
    let (title, detail) = match effect {
        Effect::Action(a) => {
            let (title, detail) = action_text(a);
            (title, detail.to_string())
        }
        Effect::HudMode(m) => {
            let (title, detail) = match m {
                0 => ("NAV mode", "HUD navigation mode: steering to the current waypoint"),
                1 => ("AIR mode (A-A)", "HUD air-to-air mode: aircraft targets"),
                _ => {
                    ("GND mode (A-G)", "HUD air-to-ground mode: ground targets; again: CCIP / CCRP")
                }
            };
            (title.to_string(), detail.to_string())
        }
        Effect::LeftPage(p) => (cmd_text(MfdCmd::LeftPage(p)), "Left MFD".to_string()),
        Effect::CamCycle => (
            "Tracking camera direction".to_string(),
            "Next direction: ahead, right, rear, left".to_string(),
        ),
        Effect::TimeToggle => {
            let (title, detail) = action_text(Action::TimeAccel);
            (title, format!("On / off. {detail}"))
        }
        Effect::Osb(side, slot) => {
            let (label, f, _) = osb(side, slot, mfd)?;
            let detail = match f {
                OsbFn::Cmd(c) => cmd_text(c),
                OsbFn::Action(a) => {
                    let (title, detail) = action_text(a);
                    if detail.is_empty() { title } else { format!("{title}: {detail}") }
                }
                // The only readout: the HSD's range between its RNG- and RNG+ buttons.
                OsbFn::Readout => "HSD range in km".to_string(),
            };
            (label.to_string(), detail)
        }
    };
    // The key a script `press=` names; both time keys; none for the camera cycle (one key per
    // direction).
    let actions = match effect {
        Effect::TimeToggle => vec![Action::TimeAccel, Action::TimeNormal],
        Effect::CamCycle => Vec::new(),
        _ => effect.action(mfd).into_iter().collect(),
    };
    Some(Tip { title, detail, actions })
}

/// The tooltip of an instrument atlas face that is not a button (None: a plain label).
pub(super) fn face_tip(face: Face) -> Option<Tip> {
    let Face::Item(elem) = face else {
        return matches!(face, Face::LedBars).then(radar_bars);
    };
    let (title, detail): (String, &str) = match elem {
        Elem::Lamp(id, label) => {
            let detail = match id {
                LampId::Mw => "Missile warning",
                LampId::Stall => "Airspeed below stall speed (blinks)",
                LampId::Trak => "An enemy radar tracks the aircraft",
                LampId::Ir => "An infrared missile is in flight",
                LampId::Rad => "A radar-guided missile is in flight",
                LampId::Fire => "Fire on board",
                LampId::Eng => "Engine damage",
                LampId::Fc => "Fire control damage",
                LampId::Jam => "Jammer damage: ECM and IRJ fail",
                LampId::Av => "Avionics damage",
                LampId::Fuel => "Fuel system damage",
                LampId::Bd => "Bay door damage: the doors stay open",
                LampId::ChaffAct => "Lights when chaff is released",
                LampId::FlareAct => "Lights when a flare is released",
                LampId::DecoyAct => "Lights when a decoy is released",
                LampId::EmvWarn => "EMV: lit for 2 s after each radar detection",
                LampId::LowFuel => "Blinks below 1500 lbs of fuel",
                _ => return None,
            };
            let name = label.unwrap_or(LAMPS[id as usize].1);
            let title = match id {
                LampId::ChaffAct => "Chaff released",
                LampId::FlareAct => "Flare released",
                LampId::DecoyAct => "Decoy released",
                LampId::EmvWarn => "EMV warning",
                _ => name,
            };
            (title.replace('\n', " "), detail)
        }
        Elem::Led(led) => match led {
            LedId::Chaff => ("Chaff".into(), "Chaff left"),
            LedId::Flares => ("Flares".into(), "Flares left"),
            LedId::Decoys => ("Decoys".into(), "Decoys left"),
            LedId::Fuel => ("Fuel".into(), "Fuel in lbs divided by 10"),
            LedId::Thrust => ("Thrust".into(), "Thrust in percent"),
        },
        Elem::Gauge => return Some(radar_bars()),
        Elem::GearLights => {
            ("Gear lights".into(), "A green lamp at each wheel while the gear is down")
        }
        Elem::Dial(dial) => {
            let (title, detail) = match dial {
                Dial::Airspeed => ("Airspeed indicator", ""),
                Dial::Altimeter => ("Altimeter", ""),
                Dial::Aoa => ("AOA indicator", "Angle of attack; the approach band is green"),
                Dial::Vvi => ("Vertical velocity indicator", ""),
                Dial::Rpm => ("RPM", "Engine speed"),
                Dial::Ftit => ("FTIT", "Fan turbine inlet temperature"),
                Dial::Oil => ("Oil pressure", ""),
                Dial::Nozzle => ("Nozzle position", ""),
                Dial::Fuel => ("Fuel quantity", ""),
                Dial::Clock => ("Clock", "Local time"),
            };
            (title.into(), detail)
        }
        Elem::Label(..) | Elem::Hazard => return None,
    };
    Some(Tip::new(title, detail, &[]))
}

fn radar_bars() -> Tip {
    Tip::new("EGR / EAR", "Radar bar graph: enemy radar signal against the aircraft's EMV", &[])
}

/// The tooltip of a live display (None: the HUD and the atlases).
pub(super) fn display_tip(role: CanvasRole, style: PanelStyle) -> Option<Tip> {
    let (title, detail) = match role {
        CanvasRole::Left => ("Left MFD", "Multifunction display"),
        CanvasRole::Right if style == PanelStyle::Modern => {
            ("Centre display", "The right MFD's pages")
        }
        CanvasRole::Right => ("Right MFD", "Multifunction display"),
        CanvasRole::Third => ("Third MFD", "Starts on the HSI page"),
        CanvasRole::Adi => ("Attitude indicator", "Pitch and bank"),
        CanvasRole::Approach => (
            "AOA indexer",
            "Slow: red top chevron, on speed: green circle, fast: amber bottom chevron",
        ),
        CanvasRole::Rwr => ("RWR", "Radar warning receiver: the radars that see the aircraft"),
        CanvasRole::Hsi => {
            ("HSI", "Horizontal situation indicator: heading, bearing and distance to the waypoint")
        }
        CanvasRole::Ded => ("DED", "Data entry display: radio, steerpoint, target, time and fuel"),
        CanvasRole::Atlas | CanvasRole::Glow | CanvasRole::Hud => return None,
    };
    Some(Tip::new(title, detail, &[]))
}

pub(super) fn stick_tip() -> Tip {
    use Action::*;
    Tip::new("Side stick", "Pitch and roll", &[PitchDown, PitchUp, RollLeft, RollRight])
}

pub(super) fn throttle_tip() -> Tip {
    use Action::*;
    Tip::new("Throttle", "Thrust", &[ThrottleUp, ThrottleDown, ThrottleMax, ThrottleIdle])
}

/// The tooltips' font: egui's Hack (as the menus), which has the accented letters of the
/// keyboard layouts (Bevy's built-in font only has ASCII: "é" would show as a box).
const FONT: &[u8] = epaint_default_fonts::HACK_REGULAR;

/// Whether the tooltip font draws every character of `s`.
fn drawable(s: &str) -> bool {
    static FACE: LazyLock<Option<ttf_parser::Face<'static>>> =
        LazyLock::new(|| ttf_parser::Face::parse(FONT, 0).ok());
    FACE.as_ref().is_some_and(|f| s.chars().all(|c| f.glyph_index(c).is_some()))
}

/// The keys bound to the actions: "Key: 9", "Keys: Shift+Z, Shift+X", and "Keys: Up/Z, Down/S"
/// when an action has several keys (those of one action joined by "/", the actions' by ", "). A
/// key whose label on the layout the font can't draw (a non-Latin layout) is named as on a US
/// keyboard.
fn keys_text(actions: &[Action], input: &InputConfig) -> String {
    let groups = actions.iter().map(|&a| {
        input
            .keys_for(ButtonTarget::Action(a))
            .map(|m| {
                let k = key_label(m.mods, m.key);
                if drawable(&k) { k } else { qwerty_key_label(m.mods, m.key) }
            })
            .collect()
    });
    join_keys(groups)
}

/// The keys of each action as text (see `keys_text`); a key already shown is not repeated.
fn join_keys(groups: impl IntoIterator<Item = Vec<String>>) -> String {
    let mut seen: Vec<String> = Vec::new();
    let mut shown: Vec<String> = Vec::new();
    for group in groups {
        let start = seen.len();
        for k in group {
            if !seen.contains(&k) {
                seen.push(k);
            }
        }
        if seen.len() > start {
            shown.push(seen[start..].join("/"));
        }
    }
    match seen.as_slice() {
        [] => String::new(),
        [key] => format!("Key: {key}"),
        _ => format!("Keys: {}", shown.join(", ")),
    }
}

/// The text broken into lines of at most `LINE_CHARS` (at spaces; a longer word on its own).
fn wrap(text: &str) -> String {
    let mut out = String::new();
    let mut len = 0;
    for word in text.split(' ').filter(|w| !w.is_empty()) {
        let n = word.chars().count();
        if len > 0 && len + 1 + n > LINE_CHARS {
            out.push('\n');
            len = 0;
        } else if len > 0 {
            out.push(' ');
            len += 1;
        }
        out.push_str(word);
        len += n;
    }
    out
}

/// The tooltip box (title, detail and keys lines).
#[derive(Component)]
pub(super) struct TipBox;

#[derive(Component)]
pub(super) struct TipLine(usize);

/// VR: the camera drawing the box into its image.
#[derive(Component)]
pub(super) struct TipCamera;

/// VR: the screen showing the box's image (a child of the cockpit root).
#[derive(Component)]
pub(super) struct TipScreen;

/// VR: the screen's material, drawn over whatever is in front of it.
pub(super) type TipScreenMaterial = ExtendedMaterial<StandardMaterial, OnTop>;

#[derive(Asset, AsBindGroup, TypePath, Clone, Debug, Default)]
pub(super) struct OnTop {}

impl MaterialExtension for OnTop {
    fn specialize(
        _pipeline: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.depth_compare = Some(CompareFunction::Always);
            depth.depth_write_enabled = Some(false);
        }
        Ok(())
    }
}

/// VR: the screen's mesh and material (the screen goes with the cockpit when it is rebuilt).
#[derive(Resource)]
pub(super) struct TipScreenLook {
    mesh: Handle<Mesh>,
    material: Handle<TipScreenMaterial>,
}

/// VR: the fingertip (cockpit frame) whose part's tooltip shows.
#[derive(Resource, Default)]
pub(super) struct TipAnchor(Option<Vec3>);

#[allow(clippy::too_many_arguments)]
pub(super) fn spawn_box(
    mut commands: Commands,
    settings: Res<Settings>,
    mut fonts: ResMut<Assets<Font>>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<TipScreenMaterial>>,
    mut font: Local<Option<Handle<Font>>>,
) {
    let font = font.get_or_insert_with(|| fonts.add(Font::from_bytes(FONT.to_vec()))).clone();
    let k = if settings.vr { VR_SCALE } else { 1.0 };
    let line = |i: usize, size: f32, color: Color| {
        (
            TipLine(i),
            Text::new(""),
            TextFont {
                font: font.clone().into(),
                font_size: bevy::text::FontSize::Px(size * k),
                ..default()
            },
            TextColor(color),
            TextLayout::no_wrap(),
            Node::default(),
        )
    };
    let tip_box = commands
        .spawn((
            TipBox,
            Node {
                position_type: PositionType::Absolute,
                flex_direction: FlexDirection::Column,
                row_gap: px(2.0 * k),
                padding: UiRect::axes(px(8.0 * k), px(5.0 * k)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.03, 0.04, 0.88)),
            Outline::new(px(k), Val::ZERO, Color::srgba(1.0, 0.85, 0.3, 0.5)),
            GlobalZIndex(10),
            Visibility::Hidden,
            DespawnOnExit(AppState::Flight),
            children![
                line(0, 17.0, Color::srgb(1.0, 0.85, 0.3)),
                line(1, 14.0, Color::srgb(0.85, 0.87, 0.9)),
                line(2, 13.0, Color::srgb(0.55, 0.6, 0.65)),
            ],
        ))
        .id();
    if !settings.vr {
        return;
    }
    // VR: the box is drawn into an image (only while it shows) for the screen by the finger.
    let mut image = Image::new_fill(
        Extent3d { width: VR_IMAGE.x, height: VR_IMAGE.y, depth_or_array_layers: 1 },
        TextureDimension::D2,
        &[0, 0, 0, 0],
        TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::default(),
    );
    image.texture_descriptor.usage |= TextureUsages::RENDER_ATTACHMENT;
    let image = images.add(image);
    let camera = commands
        .spawn((
            TipCamera,
            Camera2d,
            Camera { order: -2, is_active: false, clear_color: Color::NONE.into(), ..default() },
            RenderTarget::Image(image.clone().into()),
            RenderLayers::none(),
            DespawnOnExit(AppState::Flight),
        ))
        .id();
    commands.entity(tip_box).insert(UiTargetCamera(camera));
    let size = Vec2::new(VR_WIDTH, VR_WIDTH * VR_IMAGE.y as f32 / VR_IMAGE.x as f32);
    commands.insert_resource(TipScreenLook {
        mesh: meshes.add(Rectangle::from_size(size)),
        material: materials.add(ExtendedMaterial {
            base: StandardMaterial {
                base_color_texture: Some(image),
                alpha_mode: AlphaMode::Blend,
                unlit: true,
                fog_enabled: false,
                ..default()
            },
            extension: OnTop {},
        }),
    });
}

pub(super) struct Hover {
    pointed: Option<Pointed>,
    /// Seconds on the same part.
    resting: f32,
    /// Seconds since a tooltip was up.
    since_shown: f32,
}

impl Default for Hover {
    fn default() -> Self {
        Self { pointed: None, resting: 0.0, since_shown: f32::INFINITY }
    }
}

/// Shows the tooltip of the part under the cursor.
#[allow(clippy::too_many_arguments)]
pub(super) fn hover(
    time: Res<Time<Real>>,
    settings: Res<Settings>,
    view: Res<ViewState>,
    menu: Res<crate::flight_menu::FlightMenu>,
    hide: Option<Res<crate::camera::HideOwnAircraft>>,
    kneeboard: Option<Res<crate::kneeboard::Kneeboard>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut picker: Picker,
    controls: Query<(Entity, &Control, &Transform)>,
    tips: Query<&Tip>,
    mfd: Res<MfdState>,
    input: Res<InputConfig>,
    mut state: Local<Hover>,
    (hands, mut anchor): (Res<Hands>, ResMut<TipAnchor>),
    mut boxes: Query<(&mut Node, &mut Visibility, &ComputedNode), With<TipBox>>,
    mut lines: Query<(&TipLine, &mut Text, &mut Node), Without<TipBox>>,
) {
    let dt = time.delta_secs();
    let cockpit_view =
        matches!(view.view, View::Cockpit | View::LookLeft | View::LookRight | View::LookRear);
    let pressing = if settings.vr {
        hands.hands.iter().any(|h| h.trigger > 0.7)
    } else {
        kneeboard.is_some_and(|k| k.overlay_shown())
            || buttons.pressed(MouseButton::Right)
            || buttons.pressed(MouseButton::Left)
    };
    let active =
        settings.tooltips && cockpit_view && !menu.open && !hide.is_some_and(|h| h.0) && !pressing;
    let all: Vec<_> = if active { controls.iter().collect() } else { Vec::new() };
    let (pointed, finger) = if !active {
        (None, None)
    } else if settings.vr {
        finger_pointed(&hands, &mut picker, &all).unzip()
    } else {
        (picker.cursor_ray().and_then(|ray| picker.pick(ray, &all, |_| true)), None)
    };
    if buttons.just_pressed(MouseButton::Left) {
        // A click puts the tooltip away until the mouse rests again.
        state.since_shown = f32::INFINITY;
    }
    if pointed == state.pointed {
        state.resting += dt;
    } else {
        state.pointed = pointed;
        state.resting = 0.0;
    }
    let tip = pointed.and_then(|p| match p {
        Pointed::Control(e) => {
            controls.get(e).ok().and_then(|(_, c, _)| control_tip(c.effect, &mfd))
        }
        Pointed::Tip(e) => tips.get(e).ok().cloned(),
    });
    let show = tip.is_some() && (state.resting >= DELAY || state.since_shown < LINGER);
    state.since_shown = if show { 0.0 } else { state.since_shown + dt };
    anchor.0 = finger.filter(|_| show);

    let Ok((mut node, mut vis, computed)) = boxes.single_mut() else { return };
    vis.set_if_neq(if show { Visibility::Inherited } else { Visibility::Hidden });
    let (Some(tip), true) = (tip, show) else { return };
    let texts = [wrap(&tip.title), wrap(&tip.detail), wrap(&keys_text(&tip.actions, &input))];
    for (TipLine(i), mut text, mut line) in &mut lines {
        let s = &texts[*i];
        if text.0 != *s {
            text.0.clone_from(s);
        }
        let display = if s.is_empty() { Display::None } else { Display::Flex };
        if line.display != display {
            line.display = display;
        }
    }
    let size = computed.size() * computed.inverse_scale_factor();
    let (x, y) = if settings.vr {
        // In the middle of the image, so in the middle of the screen.
        ((VR_IMAGE.as_vec2() - size) / 2.0).max(Vec2::ZERO).into()
    } else {
        // By the cursor, inside the window (above the cursor near the bottom).
        let (Some(cursor), Some(window)) = (picker.cursor(), picker.window_size()) else { return };
        let x = (cursor.x + 14.0).min(window.x - size.x - 4.0).max(4.0);
        let below = cursor.y + 22.0;
        let y = if below + size.y > window.y - 4.0 { cursor.y - size.y - 8.0 } else { below };
        (x, y.max(4.0))
    };
    if node.left != px(x) || node.top != px(y) {
        node.left = px(x);
        node.top = px(y);
    }
}

/// VR: what a fingertip points at, and the fingertip: the control it touches, else the part
/// just ahead of it (along the controller from the grip to the fingertip). Not while the hand
/// holds a lever or the kneeboard.
fn finger_pointed(
    hands: &Hands,
    picker: &mut Picker,
    controls: &[(Entity, &Control, &Transform)],
) -> Option<(Pointed, Vec3)> {
    let free = || hands.hands.iter().filter(|h| h.active && !h.lever && !h.carrying);
    if let Some(&(e, ..)) = controls.iter().find(|(_, c, _)| c.touched())
        && let Some(hand) = free().find(|h| h.touching)
    {
        return Some((Pointed::Control(e), hand.finger));
    }
    free().find_map(|hand| {
        let along = (hand.finger - hand.grip.translation).normalize_or_zero();
        let (ray, reach) =
            picker.segment(hand.finger - along * VR_REACH, hand.finger + along * VR_REACH)?;
        // Controls are pointed at by touching them only.
        match picker.pick_within(ray, reach, controls, |_| false)? {
            p @ Pointed::Tip(_) => Some((p, hand.finger)),
            Pointed::Control(_) => None,
        }
    })
}

/// VR: the screen above the fingertip, facing the eye point (the cockpit's origin), drawn while
/// the tooltip shows.
pub(super) fn place_screen(
    mut commands: Commands,
    anchor: Res<TipAnchor>,
    look: Option<Res<TipScreenLook>>,
    roots: Query<Entity, With<CockpitRoot>>,
    mut screens: Query<(Entity, &ChildOf, &mut Transform, &mut Visibility), With<TipScreen>>,
    mut cams: Query<&mut Camera, With<TipCamera>>,
) {
    let Some(look) = look else { return };
    let Ok(root) = roots.single() else { return };
    if let Ok(mut cam) = cams.single_mut()
        && cam.is_active != anchor.0.is_some()
    {
        cam.is_active = anchor.0.is_some();
    }
    let Some(finger) = anchor.0 else {
        for (.., mut vis) in &mut screens {
            vis.set_if_neq(Visibility::Hidden);
        }
        return;
    };
    let to_eye = (-finger).normalize_or_zero();
    let at = finger + to_eye * VR_TOWARDS_EYE + Vec3::Y * VR_RAISE;
    // The screen's front (+Z) faces the eye.
    let place = Transform::from_translation(at).looking_to(at, Vec3::Y);
    match screens.iter_mut().find(|(_, parent, ..)| parent.parent() == root) {
        Some((_, _, mut tf, mut vis)) => {
            *tf = place;
            vis.set_if_neq(Visibility::Inherited);
        }
        // The first time, and after the cockpit was rebuilt (the old screen went with it).
        None => {
            commands.spawn((
                TipScreen,
                Mesh3d(look.mesh.clone()),
                MeshMaterial3d(look.material.clone()),
                place,
                NotShadowCaster,
                ChildOf(root),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_of_one_action_are_joined_by_a_slash() {
        let g = |keys: &[&str]| keys.iter().map(|k| k.to_string()).collect::<Vec<_>>();
        assert_eq!(join_keys(vec![]), "");
        assert_eq!(join_keys(vec![g(&[]), g(&[])]), "");
        assert_eq!(join_keys(vec![g(&["9"])]), "Key: 9");
        assert_eq!(join_keys(vec![g(&["ArrowUp", "Z"])]), "Keys: ArrowUp/Z");
        assert_eq!(
            join_keys(vec![g(&["ArrowUp", "Z"]), g(&[]), g(&["ArrowDown", "S"])]),
            "Keys: ArrowUp/Z, ArrowDown/S"
        );
        // A key shared by two actions, or bound twice to one, shows once.
        assert_eq!(join_keys(vec![g(&["A", "A"]), g(&["B"])]), "Keys: A, B");
        assert_eq!(join_keys(vec![g(&["A", "B"]), g(&["B"]), g(&["C"])]), "Keys: A/B, C");
    }

    /// Every key shows: the US names, the accented letters and symbols of the European layouts
    /// (AZERTY, QWERTZ, Nordic, Spanish...); what the font lacks falls back to the US name.
    #[test]
    fn key_labels_are_drawable() {
        for m in crate::input::default_key_mappings() {
            let k = qwerty_key_label(m.mods, m.key);
            assert!(drawable(&k), "{k}");
        }
        for c in "ÉÈÇÀÙ²µ§£¤¨°ÖÜÄßÅÆØÑ´`^~".chars() {
            assert!(drawable(&c.to_string()), "{c}");
        }
        assert!(!drawable("你"));
    }

    /// Every panel button has a tooltip, and every bezel button on the first pages but the blank
    /// ones.
    #[test]
    fn every_control_has_a_tooltip() {
        use super::super::layout::PanelLayout;
        let mfd = MfdState::default();
        for style in [PanelStyle::Classic, PanelStyle::Modern, PanelStyle::F16] {
            for effect in PanelLayout::for_style(style).items.iter().filter_map(|p| p.cap) {
                let tip = control_tip(effect, &mfd).unwrap_or_else(|| panic!("{effect:?}"));
                assert!(!tip.title.is_empty() && !tip.title.contains('_'), "{effect:?}");
            }
        }
        for side in [crate::mfd::MfdSide::Left, crate::mfd::MfdSide::Right] {
            for slot in 0..crate::mfd::OSB_COUNT {
                let effect = Effect::Osb(side, slot);
                assert_eq!(control_tip(effect, &mfd).is_some(), osb(side, slot, &mfd).is_some());
            }
        }
    }

    #[test]
    fn long_texts_wrap_at_spaces() {
        let text = "Horizontal situation indicator: heading, bearing and distance to the waypoint";
        let wrapped = wrap(text);
        assert!(wrapped.lines().count() > 1);
        assert!(wrapped.lines().all(|l| l.chars().count() <= LINE_CHARS), "{wrapped}");
        assert_eq!(wrapped.replace('\n', " "), text);
        assert_eq!(wrap("Flaps"), "Flaps");
        assert_eq!(wrap(""), "");
    }
}
