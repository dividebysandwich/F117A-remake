//! The modelled 3D cockpit (desktop and VR).
//!
//! The cockpit is a Blender model (`model`: `assets-src/blend/cockpit_modern.blend` and
//! `cockpit_classic.blend`, exported to `assets/models/cockpit_*.glb`): the classic panel
//! arrangement or a modern one with high-resolution MFDs and bezel buttons. Lamps,
//! readouts and labels are faces of a live instrument atlas whose contents come from the panel
//! layouts (`layout`); the MFD screens, the attitude ball and the HUD are live canvases.
//! The HUD is shown through the combiner glass collimated to infinity, so it overlays the
//! world correctly from any head position. Around it: glare shield, canopy frame, consoles,
//! seat with ejection handle, side stick (right) and throttle (left).
//!
//! The panel's lit buttons are physical caps showing their lamp face, so their state comes
//! from the panel drawing code (`controls`). Controls are touched by VR fingertips (which
//! highlights them) and pressed with the trigger, clicked with the mouse on the desktop, or by scripts (`press=Gear`).
//! The stick and throttle are grabbed with the VR controllers (`levers`). On the desktop the
//! mouse points at the nearest part under the cursor (`pick`), and resting on one shows its
//! tooltip (`tooltips`).
//!
//! VR input arrives through `Hands` (filled by the XR module, in the cockpit frame).

mod controls;
mod displays;
pub(crate) mod hud_glass;
pub mod layout;
pub mod levers;
mod model;
mod pick;
mod shading;
pub mod tooltips;

use bevy::camera::visibility::NoFrustumCulling;
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions};

pub use self::shading::CockpitMaterial;
use self::shading::{cockpit_material, cockpit_material_bright};
use crate::camera::{EyeCamera, HeadAnchor, HeadLook, Orbit, View, ViewState};
use crate::flight::Player;
use crate::input::{Action, ActionEvent};
use crate::settings::{AppState, Settings};

pub use controls::PressControl;
pub use pick::Solid;
pub use tooltips::Tip;

pub struct Cockpit3dPlugin;

impl Plugin for Cockpit3dPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            MaterialPlugin::<hud_glass::HudGlassMaterial>::default(),
            MaterialPlugin::<hud_glass::HmdMaterial>::default(),
            MaterialPlugin::<shading::CockpitMaterial>::default(),
            MaterialPlugin::<tooltips::TipScreenMaterial>::default(),
        ))
        .init_resource::<Hands>()
        .init_resource::<levers::CockpitAnchors>()
        .init_resource::<pick::Shapes>()
        .init_resource::<tooltips::TipAnchor>()
        .add_message::<PressControl>()
        .add_systems(Startup, (setup_materials, initial_look, model::load_models))
        .add_systems(OnEnter(AppState::Flight), tooltips::spawn_box)
        .add_systems(
            Update,
            (
                respawn_on_style_change,
                spawn_cockpit,
                follow_eye,
                sync_materials,
                sync_shading,
                cockpit_visibility,
                pick::forget_shapes,
                tooltips::hover,
                tooltips::place_screen,
            )
                .chain()
                .run_if(in_state(AppState::Flight)),
        )
        .add_systems(
            PreUpdate,
            ((mouse_look, controls::interact, levers::grab_levers)
                .chain()
                .after(crate::input::InputSystemsSet)
                .after(HandsUpdate),)
                .run_if(in_state(AppState::Flight)),
        )
        .add_systems(
            PostUpdate,
            (
                set_projection,
                displays::restyle_hud,
                displays::update_displays,
                controls::animate,
                levers::animate_levers,
                update_hand_proxies,
                damage_flash,
                cockpit_lamps,
            )
                .after(crate::camera::RigSystems)
                .before(TransformSystems::Propagate)
                .run_if(in_state(AppState::Flight)),
        )
        .add_systems(OnExit(AppState::Flight), release_cursor);
    }
}

/// The XR module writes `Hands` in this set (PreUpdate).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct HandsUpdate;

/// VR controller state in the cockpit frame (index 0 = left, 1 = right).
#[derive(Resource, Default)]
pub struct Hands {
    pub hands: [Hand; 2],
}

pub const BUTTON_PRIMARY: u8 = 1; // A / X
pub const BUTTON_SECONDARY: u8 = 2; // B / Y
pub const BUTTON_THUMB: u8 = 4; // thumbstick click
pub const BUTTON_MENU: u8 = 8;
/// Thumbstick pushed up / down / left / right.
pub const BUTTON_STICK_UP: u8 = 16;
pub const BUTTON_STICK_DOWN: u8 = 32;
pub const BUTTON_STICK_LEFT: u8 = 64;
pub const BUTTON_STICK_RIGHT: u8 = 128;

#[derive(Default, Clone)]
pub struct Hand {
    /// Tracked this frame (and in a cockpit view).
    pub active: bool,
    /// Grip pose in the cockpit frame.
    pub grip: Transform,
    /// Index fingertip in the cockpit frame.
    pub finger: Vec3,
    /// Thumbstick deflection (x right, y up).
    pub thumb: Vec2,
    /// Aim pose in the headset's tracking space (not the cockpit frame; the VR menu's pointer),
    /// while tracked.
    #[cfg_attr(not(feature = "xr"), allow(dead_code))]
    pub aim_raw: Option<Transform>,
    pub squeeze: f32,
    pub trigger: f32,
    pub buttons: u8,
    pub prev_buttons: u8,
    pub prev_trigger: f32,
    /// Haptic pulse request 0..1 (consumed by the XR module).
    pub haptic: f32,
    /// The fingertip touches a cockpit control (its trigger presses that control).
    pub touching: bool,
    /// Holds the stick, the throttle or the ejection handle (`levers`).
    pub lever: bool,
    /// Carries the kneeboard (the levers are not grabbed meanwhile).
    pub carrying: bool,
    /// The thumbstick scrolls the kneeboard's key guide: it neither slews nor presses its
    /// mapped stick directions.
    pub scrolling: bool,
}

impl Hand {
    pub fn held(&self, b: u8) -> bool {
        self.buttons & b != 0
    }
    pub fn just_pressed(&self, b: u8) -> bool {
        self.buttons & b != 0 && self.prev_buttons & b == 0
    }
    pub fn trigger_pulled(&self) -> bool {
        self.trigger > 0.7 && self.prev_trigger <= 0.7
    }
}

/// The HUD combiner glass (things are placed next to it).
#[derive(Component)]
pub struct HudGlass;

/// Root of the cockpit model (child of the aircraft at the design eye point), with the panel
/// layout it was built with, whether it has its lamps (local lights option), whether its MFDs
/// are the high-resolution ones and its model (another aircraft's may share the layout).
#[derive(Component)]
pub struct CockpitRoot(pub layout::PanelStyle, pub bool, pub bool, pub &'static str);

/// Keeps the cockpit at the eye point (in VR it follows the cockpit size).
fn follow_eye(
    settings: Res<crate::settings::Settings>,
    mut roots: Query<&mut Transform, With<CockpitRoot>>,
) {
    let eye = settings.cockpit_eye();
    for mut tf in &mut roots {
        if tf.translation != eye {
            tf.translation = eye;
        }
    }
}

/// Rebuilds the cockpit when the panel layout, the local lights setting or the MFD resolution
/// changes.
fn respawn_on_style_change(
    mut commands: Commands,
    settings: Res<Settings>,
    roots: Query<(Entity, &CockpitRoot)>,
) {
    let (want, stem) = settings.cockpit();
    for (e, root) in &roots {
        if root.0 != want
            || root.1 != settings.local_lights
            || root.2 != settings.hires_displays()
            || root.3 != stem
        {
            commands.entity(e).despawn();
        }
    }
}

/// Shared cockpit materials.
#[derive(Resource)]
pub struct CockpitMaterials {
    pub panel_grey: Handle<CockpitMaterial>,
    pub dark: Handle<CockpitMaterial>,
    pub black: Handle<CockpitMaterial>,
    pub frame: Handle<CockpitMaterial>,
    pub seat: Handle<CockpitMaterial>,
    pub cushion: Handle<CockpitMaterial>,
    pub yellow: Handle<CockpitMaterial>,
    pub rubber: Handle<CockpitMaterial>,
    pub metal: Handle<CockpitMaterial>,
    pub hand: Handle<CockpitMaterial>,
    pub flash: Handle<CockpitMaterial>,
    /// Frame around the control a VR fingertip touches.
    pub highlight: Handle<CockpitMaterial>,
}

fn setup_materials(mut commands: Commands, mut materials: ResMut<Assets<CockpitMaterial>>) {
    // Structure materials are always lit; the panel picture and labels follow the options.
    let mut m = |c: Color, rough: f32, metal: f32| {
        materials.add(cockpit_material(StandardMaterial {
            base_color: c,
            perceptual_roughness: rough,
            metallic: metal,
            fog_enabled: false,
            ..default()
        }))
    };
    let panel_grey = m(Color::srgb(0.20, 0.22, 0.24), 0.8, 0.0);
    let dark = m(Color::srgb(0.09, 0.10, 0.11), 0.85, 0.0);
    let black = m(Color::srgb(0.02, 0.02, 0.02), 0.9, 0.0);
    let frame = m(Color::srgb(0.13, 0.14, 0.15), 0.7, 0.2);
    let seat = m(Color::srgb(0.16, 0.17, 0.15), 0.9, 0.0);
    let cushion = m(Color::srgb(0.22, 0.21, 0.17), 1.0, 0.0);
    let yellow = m(Color::srgb(0.85, 0.7, 0.05), 0.6, 0.0);
    let rubber = m(Color::srgb(0.05, 0.05, 0.05), 0.95, 0.0);
    let metal = m(Color::srgb(0.45, 0.46, 0.48), 0.4, 0.8);
    let hand = m(Color::srgb(0.25, 0.27, 0.3), 0.8, 0.0);
    let flash = materials.add(cockpit_material_bright(StandardMaterial {
        base_color: Color::srgba(0.9, 0.0, 0.0, 0.45),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        cull_mode: None,
        fog_enabled: false,
        ..default()
    }));
    let highlight = materials.add(cockpit_material(StandardMaterial {
        base_color: Color::srgb(1.0, 0.9, 0.4),
        emissive: LinearRgba::rgb(2.0, 1.6, 0.6),
        unlit: true,
        fog_enabled: false,
        ..default()
    }));
    commands.insert_resource(CockpitMaterials {
        panel_grey,
        dark,
        black,
        frame,
        seat,
        cushion,
        yellow,
        rubber,
        metal,
        hand,
        flash,
        highlight,
    });
}

/// Whether the cockpit is drawn lit (otherwise flat, like the classic panel pictures).
pub fn lighting_on(s: &Settings) -> bool {
    s.shadows || s.local_lights
}

fn sync_materials(
    settings: Res<Settings>,
    extra: Query<&displays::LitMaterial>,
    added: Query<(), Added<displays::LitMaterial>>,
    mut materials: ResMut<Assets<CockpitMaterial>>,
    mut last: Local<Option<bool>>,
) {
    let lit = lighting_on(&settings);
    if *last == Some(lit) && added.is_empty() {
        return;
    }
    *last = Some(lit);
    for h in extra.iter().map(|l| &l.0) {
        if let Some(mut m) = materials.get_mut(h)
            && m.base.unlit == lit
        {
            m.base.unlit = !lit;
        }
    }
}

/// Every cockpit material's shader variant follows its `unlit` and the lighting options, and
/// its darkness the night's (also materials created later).
fn sync_shading(
    settings: Res<Settings>,
    sky: Res<crate::sky::Sky>,
    hud: Option<Res<crate::hud::HudState>>,
    lighting: Res<crate::lighting::LightingState>,
    mut materials: ResMut<Assets<CockpitMaterial>>,
) {
    // Bevy's full lighting (the clustered point light loop) only for local lights: on the
    // Steam Frame it costs about 10 ms of GPU time.
    let full = settings.local_lights;
    let shadows = settings.shadows;
    // Night vision goggles: the cockpit in green too, and as well seen on a moonless night.
    let nvg = hud.is_some_and(|h| h.nvg);
    // In 1/64 steps: a change re-uploads every cockpit material.
    let q = |v: f32| (v * 64.0).round() / 64.0;
    let dark = if sky.on && !nvg { q(sky.dark) } else { 0.0 };
    let daylight = if nvg { q(lighting.daylight.clamp(0.0, 1.0)) } else { 0.0 };
    let night = |m: &CockpitMaterial| {
        Vec4::new(
            if m.extension.bright { 0.0 } else { dark },
            nvg as u32 as f32,
            // Pictures (the tracking camera) are lit by themselves: the goggles' daylight gain,
            // the night's would wash them out.
            if m.extension.bright { 1.0 } else { daylight },
            !settings.modern_shading as u32 as f32,
        )
    };
    let stale: Vec<_> = materials
        .iter()
        .filter(|(_, m)| {
            m.extension.unlit != m.base.unlit
                || m.extension.full != full
                || m.extension.shadows != shadows
                || m.extension.night != night(m)
        })
        .map(|(id, _)| id)
        .collect();
    for id in stale {
        if let Some(mut m) = materials.get_mut(id) {
            m.extension.unlit = m.base.unlit;
            m.extension.full = full;
            m.extension.shadows = shadows;
            m.extension.night = night(&m);
        }
    }
}

#[derive(Component)]
struct HandProxy(usize);

#[derive(Component)]
struct FingerProxy(usize);

#[derive(Component)]
struct FlashQuad;

#[derive(Component)]
struct CockpitLamp;

/// Flood lamps: on with the local lighting option, brighter as it gets dark.
fn cockpit_lamps(
    settings: Res<Settings>,
    lighting: Res<crate::lighting::LightingState>,
    mut q: Query<&mut PointLight, With<CockpitLamp>>,
) {
    let lumens =
        if settings.local_lights { 300.0 + 1500.0 * (1.0 - lighting.daylight) } else { 0.0 };
    for mut l in &mut q {
        if l.intensity != lumens {
            l.intensity = lumens;
        }
    }
}

/// The cockpit is only drawn from the pilot's seat, not in the external views.
fn cockpit_visibility(
    state: Res<ViewState>,
    hide: Option<Res<crate::camera::HideOwnAircraft>>,
    mut roots: Query<&mut Visibility, With<CockpitRoot>>,
) {
    let inside =
        matches!(state.view, View::Cockpit | View::LookLeft | View::LookRight | View::LookRear)
            && !hide.is_some_and(|h| h.0);
    for mut v in &mut roots {
        v.set_if_neq(if inside { Visibility::Inherited } else { Visibility::Hidden });
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_cockpit(
    mut commands: Commands,
    existing: Query<(), With<CockpitRoot>>,
    player: Query<Entity, With<Player>>,
    head: Query<Entity, With<HeadAnchor>>,
    mats: Res<CockpitMaterials>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<CockpitMaterial>>,
    mut hud_materials: ResMut<Assets<hud_glass::HudGlassMaterial>>,
    mut images: ResMut<Assets<Image>>,
    fonts: Res<crate::font::Fonts>,
    tc_image: Option<Res<crate::trackcam::TrackCamImage>>,
    settings: Res<Settings>,
    models: Res<model::CockpitModels>,
    server: Res<AssetServer>,
    assets: model::ModelAssets,
    mut anchors: ResMut<levers::CockpitAnchors>,
) {
    if !existing.is_empty() {
        return;
    }
    let (Ok(aircraft), Ok(head)) = (player.single(), head.single()) else { return };
    let (style, stem) = settings.cockpit();
    let Some(file) = models.0.get(stem) else { return };
    if !server.is_loaded_with_dependencies(file) {
        return;
    }
    let layout = layout::PanelLayout::for_style(style);
    let root = commands
        .spawn((
            CockpitRoot(layout.style, settings.local_lights, settings.hires_displays(), stem),
            Transform::from_translation(settings.cockpit_eye()),
            Visibility::default(),
        ))
        .id();
    commands.entity(aircraft).add_child(root);

    let panel =
        displays::panel_materials(&mut commands, root, &layout, &mut materials, &mut images);
    let mut spawner = model::Spawner::new(
        &mats,
        &panel,
        &layout,
        &fonts,
        tc_image.as_deref(),
        settings.classic_hud,
        settings.local_lights,
        settings.hires_displays(),
        settings.hud_scale(),
        &mut meshes,
        &mut materials,
        &mut hud_materials,
        &mut images,
    );
    spawner.spawn(&mut commands, &assets, file, root);
    *anchors = spawner.anchors;

    for i in 0..2 {
        let palm = commands
            .spawn((
                HandProxy(i),
                Mesh3d(meshes.add(Cuboid::new(0.05, 0.03, 0.09))),
                MeshMaterial3d(mats.hand.clone()),
                Transform::default(),
                Visibility::Hidden,
                NoFrustumCulling,
                bevy::light::NotShadowCaster,
            ))
            .id();
        let finger = commands
            .spawn((
                FingerProxy(i),
                Mesh3d(meshes.add(Sphere::new(0.007))),
                MeshMaterial3d(mats.metal.clone()),
                Transform::default(),
                Visibility::Hidden,
                NoFrustumCulling,
                bevy::light::NotShadowCaster,
            ))
            .id();
        commands.entity(root).add_children(&[palm, finger]);
    }
    let flash = commands
        .spawn((
            FlashQuad,
            Mesh3d(meshes.add(Rectangle::new(2.0, 2.0))),
            MeshMaterial3d(mats.flash.clone()),
            Transform::from_xyz(0.0, 0.0, -0.25),
            Visibility::Hidden,
            NoFrustumCulling,
            bevy::light::NotShadowCaster,
        ))
        .id();
    commands.entity(head).add_child(flash);
}

/// Palm boxes following the VR controllers (the fingertip is the palm's front).
fn update_hand_proxies(
    hands: Res<Hands>,
    mut palms: Query<(&HandProxy, &mut Transform, &mut Visibility), Without<FingerProxy>>,
    mut fingers: Query<(&FingerProxy, &mut Transform, &mut Visibility), Without<HandProxy>>,
) {
    let vis_of = |active: bool| if active { Visibility::Inherited } else { Visibility::Hidden };
    for (p, mut tf, mut vis) in &mut palms {
        let h = &hands.hands[p.0];
        vis.set_if_neq(vis_of(h.active));
        if h.active {
            *tf = h.grip;
        }
    }
    for (p, mut tf, mut vis) in &mut fingers {
        let h = &hands.hands[p.0];
        vis.set_if_neq(vis_of(h.active));
        if h.active {
            tf.translation = h.finger;
        }
    }
}

/// Red flash over the view when hit.
fn damage_flash(
    mut flash: ResMut<crate::hud::DamageFlash>,
    mut q: Query<&mut Visibility, With<FlashQuad>>,
) {
    let on = flash.0 > 0;
    if on {
        flash.0 -= 1;
    }
    for mut v in &mut q {
        v.set_if_neq(if on { Visibility::Inherited } else { Visibility::Hidden });
    }
}

/// Desktop: a normal perspective projection.
fn set_projection(
    settings: Res<Settings>,
    state: Res<ViewState>,
    mut cams: Query<(&mut Camera, &mut Projection), With<EyeCamera>>,
) {
    if settings.vr {
        return;
    }
    for (mut camera, mut projection) in &mut cams {
        if camera.sub_camera_view.is_some() {
            camera.sub_camera_view = None;
        }
        if let Projection::Perspective(p) = projection.as_mut() {
            let base = if state.wide { 85f32 } else { 65f32 }.to_radians();
            let zooms = state.first_person() || state.view == View::Free;
            let magnify = if zooms { state.magnify } else { 1.0 };
            let fov = 2.0 * ((base / 2.0).tan() / magnify).atan();
            if p.fov != fov {
                p.fov = fov;
            }
        }
    }
}

/// Strongest mouse wheel magnification in the cockpit.
const MAX_MAGNIFY: f32 = 6.0;

/// Desktop mouse look: hold the right button and move the mouse. The look-view keys turn the
/// head to that side (and reset the free look). In the external views the mouse orbits the
/// camera around the aircraft (reset when an external view is selected).
#[allow(clippy::too_many_arguments)]
fn mouse_look(
    settings: Res<Settings>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    mut look: ResMut<HeadLook>,
    mut orbit: ResMut<Orbit>,
    mut actions: MessageReader<ActionEvent>,
    mut state: ResMut<ViewState>,
    mut cursor: Query<&mut CursorOptions>,
    scroll: Res<bevy::input::mouse::AccumulatedMouseScroll>,
) {
    for ActionEvent(a) in actions.read() {
        if matches!(
            a,
            Action::ViewCockpit
                | Action::LookAhead
                | Action::LookLeft
                | Action::LookRight
                | Action::LookRear
        ) {
            *look = HeadLook::default();
            state.magnify = 1.0;
        }
        if matches!(
            a,
            Action::ViewChase
                | Action::ViewSlot
                | Action::ViewSide
                | Action::ViewTactical
                | Action::ViewInverseTactical
                | Action::ViewMissile
        ) {
            *orbit = Orbit::default();
            state.wheel_zoom = 0.0;
        }
    }
    if settings.vr {
        return;
    }
    let cockpit =
        matches!(state.view, View::Cockpit | View::LookLeft | View::LookRight | View::LookRear);
    let held = buttons.pressed(MouseButton::Right);
    if let Ok(mut c) = cursor.single_mut() {
        let (grab, visible) =
            if held { (CursorGrabMode::Locked, false) } else { (CursorGrabMode::None, true) };
        if c.grab_mode != grab {
            c.grab_mode = grab;
            c.visible = visible;
        }
    }
    // The free view turns and zooms itself (`camera::place_free_camera`).
    if !held || state.view == View::Free {
        return;
    }
    // The wheel zooms while the button is held: in the cockpit (and under the parachute) it
    // narrows the view, outside it moves the camera in and out.
    let notches = match scroll.unit {
        bevy::input::mouse::MouseScrollUnit::Line => scroll.delta.y,
        bevy::input::mouse::MouseScrollUnit::Pixel => scroll.delta.y / 50.0,
    };
    if notches != 0.0 {
        if state.first_person() {
            state.magnify = (state.magnify * 1.2f32.powf(notches)).clamp(1.0, MAX_MAGNIFY);
        } else {
            state.wheel_zoom = (state.wheel_zoom + notches * 0.25)
                .clamp(-(crate::camera::MAX_UNZOOM + state.zoom) as f32, 4.0 - state.zoom as f32);
        }
    }
    if cockpit {
        // Slower when zoomed in, so the view moves as far on the screen per mouse move.
        let k = 0.004 / state.magnify;
        look.yaw = (look.yaw - motion.delta.x * k).clamp(-2.6, 2.6);
        look.pitch = (look.pitch - motion.delta.y * k).clamp(-1.2, 1.3);
    } else {
        // External views: orbit the camera around the aircraft (under the parachute: look
        // around, slower when zoomed in).
        let k = if state.first_person() { 0.006 / state.magnify } else { 0.006 };
        orbit.yaw -= motion.delta.x * k;
        orbit.pitch = (orbit.pitch + motion.delta.y * k).clamp(-1.5, 1.5);
    }
}

/// `--head-look yaw,pitch` (degrees): initial free look (screenshots of the cockpit).
fn initial_look(mut look: ResMut<HeadLook>) {
    let args: Vec<String> = std::env::args().collect();
    if let Some(v) = args.iter().position(|a| a == "--head-look").and_then(|i| args.get(i + 1)) {
        let mut it = v.split(',').map(|x| x.parse::<f32>().unwrap_or(0.0).to_radians());
        look.yaw = it.next().unwrap_or(0.0);
        look.pitch = it.next().unwrap_or(0.0);
    }
}

fn release_cursor(mut cursor: Query<&mut CursorOptions>) {
    if let Ok(mut c) = cursor.single_mut() {
        c.grab_mode = CursorGrabMode::None;
        c.visible = true;
    }
}
