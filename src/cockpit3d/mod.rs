//! The modelled 3D cockpit (desktop and VR).
//!
//! The instrument panel is modelled on a tilted plane laid out in the original screen's pixels
//! (`geometry`), from one of two layouts (`layout`): the original arrangement (classic) or a
//! modern one with high-resolution MFDs and bezel buttons. Lamps, readouts and labels are faces
//! of a live instrument atlas; the MFD screens are recessed into the panel.
//! Around it: glare shield, HUD combiner (the HUD is a separate canvas shown through the
//! combiner glass collimated to infinity, at the original 256 px/rad, so it overlays the
//! world correctly from any head position), canopy frame,
//! consoles, seat with ejection handle, side stick (right) and throttle (left).
//!
//! The panel's lit buttons are physical caps showing their lamp face, so their state comes
//! from the panel drawing code (`controls`). Controls are touched by VR fingertips (which
//! highlights them) and pressed with the trigger, clicked with the mouse on the desktop, or by scripts (`press=Gear`).
//! The stick and throttle are grabbed with the VR controllers (`levers`).
//!
//! VR input arrives through `Hands` (filled by the XR module, in the cockpit frame).

mod controls;
mod displays;
pub mod geometry;
mod hud_glass;
pub mod layout;
mod levers;

use bevy::camera::visibility::NoFrustumCulling;
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions};

use crate::camera::{EyeCamera, HeadAnchor, HeadLook, Orbit, View, ViewState};
use crate::flight::Player;
use crate::input::{Action, ActionEvent};
use crate::settings::{AppState, Settings};

pub use controls::PressControl;

pub struct Cockpit3dPlugin;

impl Plugin for Cockpit3dPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<hud_glass::HudGlassMaterial>::default())
            .init_resource::<Hands>()
            .add_message::<PressControl>()
            .add_systems(Startup, (setup_materials, initial_look))
            .add_systems(
                Update,
                (respawn_on_style_change, spawn_cockpit, sync_materials, cockpit_visibility).chain().run_if(in_state(AppState::Flight)),
            )
            .add_systems(
                PreUpdate,
                (
                    (mouse_look, controls::interact, levers::grab_levers)
                        .chain()
                        .after(crate::input::InputSystemsSet)
                        .after(HandsUpdate),
                )
                    .run_if(in_state(AppState::Flight)),
            )
            .add_systems(
                PostUpdate,
                (set_projection, displays::restyle_hud, displays::update_displays, controls::animate, levers::animate_levers, update_hand_proxies, damage_flash, cockpit_lamps)
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
    pub squeeze: f32,
    pub trigger: f32,
    pub buttons: u8,
    pub prev_buttons: u8,
    pub prev_trigger: f32,
    /// Haptic pulse request 0..1 (consumed by the XR module).
    pub haptic: f32,
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

/// Root of the cockpit model (child of the aircraft at the design eye point), with the panel
/// layout it was built with and whether it has its lamps (local lights option).
#[derive(Component)]
pub struct CockpitRoot(pub layout::PanelStyle, pub bool);

/// Rebuilds the cockpit when the panel layout or the local lights setting changes.
fn respawn_on_style_change(mut commands: Commands, settings: Res<Settings>, roots: Query<(Entity, &CockpitRoot)>) {
    let want = if settings.classic_cockpit { layout::PanelStyle::Classic } else { layout::PanelStyle::Modern };
    for (e, root) in &roots {
        if root.0 != want || root.1 != settings.local_lights {
            commands.entity(e).despawn();
        }
    }
}

/// Shared cockpit materials.
#[derive(Resource)]
pub struct CockpitMaterials {
    pub panel_grey: Handle<StandardMaterial>,
    pub dark: Handle<StandardMaterial>,
    pub black: Handle<StandardMaterial>,
    pub frame: Handle<StandardMaterial>,
    pub seat: Handle<StandardMaterial>,
    pub cushion: Handle<StandardMaterial>,
    pub yellow: Handle<StandardMaterial>,
    pub rubber: Handle<StandardMaterial>,
    pub metal: Handle<StandardMaterial>,
    pub hand: Handle<StandardMaterial>,
    pub flash: Handle<StandardMaterial>,
    /// Frame around the control a VR fingertip touches.
    pub highlight: Handle<StandardMaterial>,
    pub unit_cube: Handle<Mesh>,
}

fn setup_materials(mut commands: Commands, mut materials: ResMut<Assets<StandardMaterial>>, mut meshes: ResMut<Assets<Mesh>>) {
    // Structure materials are always lit; the panel picture and labels follow the options.
    let mut m = |c: Color, rough: f32, metal: f32| {
        materials.add(StandardMaterial {
            base_color: c,
            perceptual_roughness: rough,
            metallic: metal,
            fog_enabled: false,
            ..default()
        })
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
    let flash = materials.add(StandardMaterial {
        base_color: Color::srgba(0.9, 0.0, 0.0, 0.45),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        cull_mode: None,
        fog_enabled: false,
        ..default()
    });
    let highlight = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.9, 0.4),
        emissive: LinearRgba::rgb(2.0, 1.6, 0.6),
        unlit: true,
        fog_enabled: false,
        ..default()
    });
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
        unit_cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
    });
}

/// Whether the cockpit is drawn lit (otherwise flat, like the original pictures).
pub fn lighting_on(s: &Settings) -> bool {
    s.shadows || s.local_lights
}

fn sync_materials(
    settings: Res<Settings>,
    extra: Query<&displays::LitMaterial>,
    added: Query<(), Added<displays::LitMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut last: Local<Option<bool>>,
) {
    let lit = lighting_on(&settings);
    if *last == Some(lit) && added.is_empty() {
        return;
    }
    *last = Some(lit);
    for h in extra.iter().map(|l| &l.0) {
        if let Some(mut m) = materials.get_mut(h)
            && m.unlit == lit
        {
            m.unlit = !lit;
        }
    }
}

/// Spawns a box between two points, or with a transform, under the cockpit root.
pub(crate) fn spawn_box(commands: &mut Commands, root: Entity, mats: &CockpitMaterials, material: &Handle<StandardMaterial>, tf: Transform) -> Entity {
    let e = commands.spawn((Mesh3d(mats.unit_cube.clone()), MeshMaterial3d(material.clone()), tf, NoFrustumCulling)).id();
    commands.entity(root).add_child(e);
    e
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
fn cockpit_lamps(settings: Res<Settings>, lighting: Res<crate::lighting::LightingState>, mut q: Query<&mut PointLight, With<CockpitLamp>>) {
    let lumens = if settings.local_lights { 300.0 + 1500.0 * (1.0 - lighting.daylight) } else { 0.0 };
    for mut l in &mut q {
        if l.intensity != lumens {
            l.intensity = lumens;
        }
    }
}

/// The cockpit is only drawn from the pilot's seat, not in the external views.
fn cockpit_visibility(state: Res<ViewState>, mut roots: Query<&mut Visibility, With<CockpitRoot>>) {
    let inside = matches!(state.view, View::Cockpit | View::LookLeft | View::LookRight | View::LookRear);
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
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut hud_materials: ResMut<Assets<hud_glass::HudGlassMaterial>>,
    mut images: ResMut<Assets<Image>>,
    fonts: Res<crate::font::Fonts>,
    tc_image: Option<Res<crate::trackcam::TrackCamImage>>,
    settings: Res<Settings>,
) {
    if !existing.is_empty() {
        return;
    }
    let (Ok(aircraft), Ok(head)) = (player.single(), head.single()) else { return };
    let layout = layout::PanelLayout::for_classic(settings.classic_cockpit);
    let root = commands
        .spawn((CockpitRoot(layout.style, settings.local_lights), Transform::from_translation(crate::camera::EYE_OFFSET), Visibility::default()))
        .id();
    commands.entity(aircraft).add_child(root);

    let faces = displays::spawn_panel(&mut commands, root, &layout, &mats, &mut meshes, &mut materials, &mut images, tc_image.as_deref());
    displays::spawn_hud(&mut commands, root, &mut meshes, &mut hud_materials, &mut images, settings.classic_hud);
    displays::spawn_approach_indexer(&mut commands, root, &mut meshes, &mut materials, &mut images);
    displays::spawn_structure(&mut commands, root, &mats, &mut meshes);
    controls::spawn_controls(&mut commands, root, &layout, &faces, &mats, &mut meshes, &mut materials, &mut images, &fonts);
    levers::spawn_levers(&mut commands, root, &mats, &mut meshes);

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
    // Flood lamps: two red side lamps on the canopy sills and a white one under the glare
    // shield lighting the panel. Only with the local lighting option (see `lighting::spawn_sun`:
    // point lights hang the Steam Frame's GPU).
    let lamps: &[(Vec3, Color)] = if settings.local_lights {
        &[
            (Vec3::new(-0.45, -0.18, 0.05), Color::srgb(1.0, 0.35, 0.25)),
            (Vec3::new(0.45, -0.18, 0.05), Color::srgb(1.0, 0.35, 0.25)),
            (Vec3::new(0.0, -0.17, -0.55), Color::srgb(1.0, 0.92, 0.8)),
        ]
    } else {
        &[]
    };
    for &(pos, color) in lamps {
        let lamp = commands
            .spawn((CockpitLamp, PointLight { intensity: 0.0, range: 1.6, color, shadow_maps_enabled: false, ..default() }, Transform::from_translation(pos)))
            .id();
        commands.entity(root).add_child(lamp);
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
fn damage_flash(mut flash: ResMut<crate::hud::DamageFlash>, mut q: Query<&mut Visibility, With<FlashQuad>>) {
    let on = flash.0 > 0;
    if on {
        flash.0 -= 1;
    }
    for mut v in &mut q {
        v.set_if_neq(if on { Visibility::Inherited } else { Visibility::Hidden });
    }
}

/// Desktop: a normal perspective projection.
fn set_projection(settings: Res<Settings>, state: Res<ViewState>, mut cams: Query<(&mut Camera, &mut Projection), With<EyeCamera>>) {
    if settings.vr {
        return;
    }
    for (mut camera, mut projection) in &mut cams {
        if camera.sub_camera_view.is_some() {
            camera.sub_camera_view = None;
        }
        if let Projection::Perspective(p) = projection.as_mut() {
            let fov = if state.wide { 85f32 } else { 65f32 }.to_radians();
            if p.fov != fov {
                p.fov = fov;
            }
        }
    }
}

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
    state: Res<ViewState>,
    mut cursor: Query<&mut CursorOptions>,
) {
    for ActionEvent(a) in actions.read() {
        if matches!(a, Action::ViewCockpit | Action::LookAhead | Action::LookLeft | Action::LookRight | Action::LookRear) {
            *look = HeadLook::default();
        }
        if matches!(a, Action::ViewChase | Action::ViewSlot | Action::ViewSide | Action::ViewTactical | Action::ViewInverseTactical) {
            *orbit = Orbit::default();
        }
    }
    if settings.vr {
        return;
    }
    let cockpit = matches!(state.view, View::Cockpit | View::LookLeft | View::LookRight | View::LookRear);
    let held = buttons.pressed(MouseButton::Right);
    if let Ok(mut c) = cursor.single_mut() {
        let (grab, visible) = if held { (CursorGrabMode::Locked, false) } else { (CursorGrabMode::None, true) };
        if c.grab_mode != grab {
            c.grab_mode = grab;
            c.visible = visible;
        }
    }
    if !held {
        return;
    }
    if cockpit {
        look.yaw = (look.yaw - motion.delta.x * 0.004).clamp(-2.6, 2.6);
        look.pitch = (look.pitch - motion.delta.y * 0.004).clamp(-1.2, 1.3);
    } else {
        // External views: orbit the camera around the aircraft.
        orbit.yaw -= motion.delta.x * 0.006;
        orbit.pitch = (orbit.pitch + motion.delta.y * 0.006).clamp(-1.5, 1.5);
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
