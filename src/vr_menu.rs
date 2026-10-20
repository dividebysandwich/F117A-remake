//! The menu and the debrief in the headset (`--vr`): the egui menu is drawn into an image shown on
//! a panel floating in front of the player (and on the desktop window as a mirror); a
//! controller's aim is the mouse: the trigger clicks, the thumbstick scrolls.

use bevy::camera::RenderTarget;
use bevy::camera::visibility::RenderLayers;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy_egui::{EguiInput, EguiPreUpdateSet, PrimaryEguiContext, egui};

use crate::cockpit3d::{BUTTON_MENU, Hands};
use crate::settings::{AppState, Settings};

pub struct VrMenuPlugin;

impl Plugin for VrMenuPlugin {
    fn build(&self, app: &mut App) {
        let in_menu = |s: Option<Res<State<AppState>>>| s.is_some_and(|s| matches!(s.get(), AppState::Menu | AppState::Debrief));
        let vr = |s: Res<Settings>| s.vr;
        app.add_systems(Update, (place_panel, pointer).chain().run_if(in_menu).run_if(vr)).add_systems(
            PreUpdate,
            feed_egui
                .after(EguiPreUpdateSet::ProcessInput)
                .before(EguiPreUpdateSet::BeginPass)
                .after(crate::cockpit3d::HandsUpdate)
                .run_if(in_menu)
                .run_if(vr),
        );
    }
}

/// Menu image size (pixels) and the panel's size (metres) and distance from the head.
const W: u32 = 1600;
const H: u32 = 1000;
const PANEL: Vec2 = Vec2::new(1.6, 1.0);
const DISTANCE: f32 = 1.3;

/// The panel showing the menu (child of the XR tracking root, in its tracking space).
#[derive(Component, Default)]
pub struct VrMenuPanel {
    placed: bool,
}

/// The laser from the pointing controller to the panel.
#[derive(Component)]
struct Laser;

/// Where the pointer is on the menu image (pixels) and whether the trigger was down.
#[derive(Resource, Default)]
struct PointerState {
    pos: Option<egui::Pos2>,
    pressed: bool,
    scroll: f32,
}

/// Spawns the menu's egui context drawing into an image, the panel showing it in the headset
/// and the desktop mirror; every entity gets `tag` (despawned with the menu camera).
pub fn spawn(commands: &mut Commands, images: &mut Assets<Image>, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>, root: Option<Entity>, tag: impl Bundle + Clone) {
    let mut image = Image::new_fill(
        Extent3d { width: W, height: H, depth_or_array_layers: 1 },
        TextureDimension::D2,
        &[0, 0, 0, 255],
        TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::default(),
    );
    image.texture_descriptor.usage |= TextureUsages::RENDER_ATTACHMENT;
    let image = images.add(image);
    // The egui context renders into the image (the primary one: the menu code draws into it).
    commands.spawn((
        Camera2d,
        Camera { order: -1, clear_color: ClearColorConfig::Custom(Color::BLACK), ..default() },
        RenderTarget::Image(image.clone().into()),
        RenderLayers::none(),
        PrimaryEguiContext,
        tag.clone(),
    ));
    // The panel in the headset.
    let panel = commands
        .spawn((
            VrMenuPanel::default(),
            Mesh3d(meshes.add(Rectangle::new(PANEL.x, PANEL.y))),
            MeshMaterial3d(materials.add(StandardMaterial { base_color_texture: Some(image.clone()), unlit: true, cull_mode: None, fog_enabled: false, ..default() })),
            Transform::from_xyz(0.0, 1.2, -DISTANCE),
            NotShadowCaster,
            tag.clone(),
        ))
        .id();
    let laser = commands
        .spawn((
            Laser,
            Mesh3d(meshes.add(Cuboid::new(0.003, 0.003, 1.0))),
            MeshMaterial3d(materials.add(StandardMaterial { base_color: Color::srgb(0.3, 1.0, 0.45), unlit: true, ..default() })),
            Transform::default(),
            Visibility::Hidden,
            NotShadowCaster,
            tag.clone(),
        ))
        .id();
    if let Some(r) = root {
        commands.entity(r).add_children(&[panel, laser]);
    }
    // The desktop window mirrors the menu.
    let window_cam = commands.spawn((Camera2d, Camera { clear_color: ClearColorConfig::Custom(Color::BLACK), ..default() }, tag.clone())).id();
    commands.spawn((
        ImageNode::new(image),
        Node { width: percent(100), height: percent(100), ..default() },
        UiTargetCamera(window_cam),
        tag,
    ));
    commands.init_resource::<PointerState>();
}

/// Puts the panel in front of the head once it is tracked (again on the left menu button or
/// Home: recentring).
fn place_panel(
    mut panels: Query<(&mut VrMenuPanel, &mut Transform)>,
    cams: Query<&Transform, (With<bevy_mod_xr::camera::XrCamera>, Without<VrMenuPanel>)>,
    hands: Res<Hands>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    let Ok((mut panel, mut tf)) = panels.single_mut() else { return };
    if keys.just_pressed(KeyCode::Home) || hands.hands[0].just_pressed(BUTTON_MENU) {
        panel.placed = false;
    }
    let n = cams.iter().len();
    if panel.placed || n == 0 {
        return;
    }
    let head = cams.iter().map(|t| t.translation).sum::<Vec3>() / n as f32;
    if head == Vec3::ZERO {
        return; // not tracked yet
    }
    let fwd = cams.iter().next().map_or(Vec3::NEG_Z, |t| t.forward().as_vec3());
    let flat = Vec3::new(fwd.x, 0.0, fwd.z).normalize_or(Vec3::NEG_Z);
    let centre = head + flat * DISTANCE - Vec3::Y * 0.1;
    *tf = Transform::from_translation(centre).looking_to(flat, Vec3::Y);
    panel.placed = true;
}

/// The controller pointing at the panel: the laser, the pointer position, trigger and scroll.
fn pointer(
    hands: Res<Hands>,
    panels: Query<&Transform, With<VrMenuPanel>>,
    mut laser: Query<(&mut Transform, &mut Visibility), (With<Laser>, Without<VrMenuPanel>)>,
    mut state: ResMut<PointerState>,
) {
    let Ok(panel) = panels.single() else { return };
    let Ok((mut ltf, mut lvis)) = laser.single_mut() else { return };
    // The right hand points, the left one if the right is not tracked.
    let Some(hand) = [1, 0].into_iter().map(|i| &hands.hands[i]).find(|h| h.aim_raw.is_some()) else {
        state.pos = None;
        lvis.set_if_neq(Visibility::Hidden);
        return;
    };
    let aim = hand.aim_raw.unwrap_or_default();
    let (o, d) = (aim.translation, aim.forward().as_vec3());
    // The panel's plane: its local XY, facing +Z.
    let inv = panel.compute_affine().inverse();
    let (lo, ld) = (inv.transform_point3(o), inv.transform_vector3(d));
    let hit = (ld.z.abs() > 1e-6).then(|| -lo.z / ld.z).filter(|t| *t > 0.0).map(|t| (t, lo + ld * t));
    let on_panel = hit.filter(|(_, p)| p.x.abs() <= PANEL.x / 2.0 && p.y.abs() <= PANEL.y / 2.0);
    state.pos = on_panel.map(|(_, p)| egui::pos2((p.x / PANEL.x + 0.5) * W as f32, (0.5 - p.y / PANEL.y) * H as f32));
    state.pressed = hand.trigger > 0.6;
    state.scroll = if on_panel.is_some() { hand.thumb.y } else { 0.0 };
    let len = on_panel.map_or(3.0, |(t, _)| t);
    *ltf = Transform::from_translation(o + d * len / 2.0).looking_to(d, Vec3::Y).with_scale(Vec3::new(1.0, 1.0, len));
    lvis.set_if_neq(Visibility::Inherited);
}

/// The pointer into the menu's egui input: moves, trigger presses and releases, scrolling.
fn feed_egui(mut state: ResMut<PointerState>, mut input: Query<&mut EguiInput, With<PrimaryEguiContext>>, mut down: Local<bool>, mut last: Local<Option<egui::Pos2>>, time: Res<Time>) {
    let Ok(mut input) = input.single_mut() else { return };
    let ev = &mut input.0.events;
    let modifiers = egui::Modifiers::default();
    match state.pos {
        Some(pos) => {
            if *last != Some(pos) {
                ev.push(egui::Event::PointerMoved(pos));
            }
            if state.pressed != *down {
                ev.push(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: state.pressed, modifiers });
                *down = state.pressed;
            }
            if state.scroll.abs() > 0.2 {
                let delta = egui::vec2(0.0, state.scroll * 900.0 * time.delta_secs());
                ev.push(egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta, phase: egui::TouchPhase::Move, modifiers });
            }
        }
        None => {
            if let (true, Some(pos)) = (*down, *last) {
                ev.push(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, modifiers });
                *down = false;
            }
            if last.is_some() {
                ev.push(egui::Event::PointerGone);
            }
        }
    }
    *last = state.pos;
    state.scroll = 0.0;
}
