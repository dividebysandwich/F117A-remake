//! The campaign's unit view: a unit picked on the war map watched live from close by. An
//! offscreen camera follows it (the world is streamed in around it like around the player);
//! on the desktop the picture is a movable, resizable window over the Ops screen, in the
//! headset a panel of its own that the controllers carry (grip) and close (trigger on the red
//! box). Dragging in the picture (or a thumbstick on the panel) turns around the unit, the
//! wheel (thumbstick up and down) goes closer or further.

use bevy::camera::RenderTarget;
use bevy::camera::visibility::RenderLayers;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::math::DVec3;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy_egui::{EguiContexts, EguiTextureHandle, egui};
use stealth_sim::{AircraftKind, fflags, oflags};

use super::ActiveCampaign;
use crate::camera::{OffscreenCamera, TRACKCAM_LOD_LAYER, UNIT_BACKGROUND_LAYER};
use crate::combat::{Combat, unit_position};
use crate::menu::{AMBER, BRIGHT, DIM};
use crate::world::{StreamSites, WorldPos, wu_to_bevy};

/// Picture size (pixels).
const W: u32 = 960;
const H: u32 = 540;
/// Seconds a lost unit (shot down, landed) stays in view before the view closes.
const LOST_SECONDS: f32 = 8.0;

/// What is watched.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnitRef {
    /// Flight unit, with its type (the slot may be reused by another aircraft).
    Flight { index: usize, spec: i32 },
    /// Low-level air defence unit.
    Aaa(usize),
    /// World object (SAM site).
    Site(usize),
}

/// The offscreen camera following the unit.
#[derive(Component)]
pub struct UnitCamera;

/// The headset's panel showing the picture, and its close box.
#[derive(Component)]
pub struct UnitPanel;

#[derive(Resource)]
pub struct UnitView {
    pub target: Option<UnitRef>,
    pub title: String,
    image: Option<Handle<Image>>,
    camera: Option<Entity>,
    panel: Option<Entity>,
    /// Where the unit was last, its heading (radians), and seconds since it was lost.
    last: Option<(DVec3, f32)>,
    lost: f32,
    /// Camera around the unit: yaw from behind, pitch above it (radians), distance (m).
    orbit: f32,
    pitch: f32,
    distance: f32,
    /// VR: the hand carrying the panel and the panel in that hand's aim frame.
    grab: Option<(usize, Transform)>,
    placed: bool,
    /// Altitude (m) and speed (kt) of an aircraft, for the caption.
    pub info: Option<(f32, f32)>,
}

impl Default for UnitView {
    fn default() -> Self {
        Self {
            target: None,
            title: String::new(),
            image: None,
            camera: None,
            panel: None,
            last: None,
            lost: 0.0,
            orbit: 0.0,
            pitch: 0.25,
            distance: 60.0,
            grab: None,
            placed: false,
            info: None,
        }
    }
}

impl UnitView {
    /// Watch a unit (the camera starts behind it).
    pub fn watch(&mut self, unit: UnitRef, title: String) {
        let aircraft = matches!(unit, UnitRef::Flight { .. });
        self.target = Some(unit);
        self.title = title;
        self.last = None;
        self.lost = 0.0;
        self.orbit = if aircraft { 0.0 } else { 0.6 };
        self.pitch = if aircraft { 0.15 } else { 0.35 };
        self.distance = if aircraft { 45.0 } else { 90.0 };
    }

    pub fn close(&mut self) {
        self.target = None;
    }
}

/// Where a unit is: Bevy metres, heading (radians clockwise from north), and an aircraft's
/// altitude (m) and speed (kt).
type Located = (DVec3, f32, Option<(f32, f32)>);

/// Where the unit is now; None: gone (shot down, landed, destroyed).
fn locate(combat: &Combat, unit: UnitRef) -> Option<Located> {
    let sim = &combat.sim;
    match unit {
        UnitRef::Flight { index, spec } => {
            let u = *sim.flights().get(index)?;
            if u.spec != spec || u.flags & fflags::EXISTS == 0 || u.flags & fflags::AIRBORNE == 0 {
                return None;
            }
            let (e, n, alt) = unit_position(combat.prev_flights.get(index), &u, combat.alpha);
            let heading = u.heading as u16 as f32 / 65536.0 * std::f32::consts::TAU;
            let m = alt as f32 * crate::world::WU as f32;
            let kt = if sim.aircraft_kind(u.spec) == AircraftKind::Helicopter {
                u.speed as f32 * 0.6
            } else {
                u.speed as f32
            };
            Some((wu_to_bevy(e, n, alt), heading, Some((m, kt))))
        }
        UnitRef::Aaa(k) => {
            let u = sim.aaa_units().get(k)?;
            if u.destroyed {
                return None;
            }
            let p = crate::sim_render::map_point(u.x as f64 + 0.5, u.y as f64 + 0.5, 0.0);
            Some((p + DVec3::Y * 2.0, u.aim_az, None))
        }
        UnitRef::Site(i) => {
            let o = sim.objects().get(i)?;
            if o.flags & oflags::DESTROYED != 0 {
                return None;
            }
            let p = crate::sim_render::map_point(o.x as f64 + 0.5, o.y as f64 + 0.5, 0.0);
            Some((p + DVec3::Y * 4.0, 0.0, None))
        }
    }
}

/// Despawns the camera and the panel (leaving the flight).
pub fn follow_cleanup(commands: &mut Commands, view: &mut UnitView) {
    for e in [view.camera.take(), view.panel.take()].into_iter().flatten() {
        commands.entity(e).try_despawn();
    }
    view.grab = None;
}

/// Moves the camera with the unit; spawns and despawns the camera (and the VR panel).
#[allow(clippy::too_many_arguments)]
pub fn follow(
    mut commands: Commands,
    mut view: ResMut<UnitView>,
    campaign: Option<Res<ActiveCampaign>>,
    combat: Option<Res<Combat>>,
    mut sites: ResMut<StreamSites>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    palette: Option<Res<crate::palette::Palette>>,
    settings: Res<crate::settings::Settings>,
    root: Option<Res<crate::camera::XrRoot>>,
    mut cams: Query<(&mut WorldPos, &mut Transform), With<UnitCamera>>,
    time: Res<Time<Real>>,
) {
    let view = &mut *view;
    // The view lives with the Ops screen.
    if !campaign.as_ref().is_some_and(|c| c.screen_open() && !c.exit) {
        view.target = None;
    }
    let (Some(unit), Some(combat)) = (view.target, combat) else {
        follow_cleanup(&mut commands, view);
        sites.follow = None;
        return;
    };
    match locate(&combat, unit) {
        Some((p, heading, info)) => {
            view.last = Some((p, heading));
            view.info = info;
            view.lost = 0.0;
        }
        None => {
            view.lost += time.delta_secs();
            if view.lost > LOST_SECONDS || view.last.is_none() {
                view.target = None;
                return;
            }
        }
    }
    let Some((target, heading)) = view.last else { return };
    sites.follow = Some(target);
    // Behind the unit (its heading turned by the orbit), above it by the pitch.
    let yaw = heading + view.orbit;
    let back = DVec3::new(-(yaw as f64).sin(), 0.0, (yaw as f64).cos());
    let (sp, cp) = (view.pitch as f64).sin_cos();
    let eye = target + (back * cp + DVec3::Y * sp) * view.distance as f64;
    let look = Transform::IDENTITY.looking_to((target - eye).as_vec3(), Vec3::Y);
    if let Some(cam) = view.camera
        && let Ok((mut pos, mut tf)) = cams.get_mut(cam)
    {
        pos.0 = eye;
        tf.rotation = look.rotation;
    } else if view.camera.is_none()
        && let Some(palette) = palette
    {
        let image = view
            .image
            .get_or_insert_with(|| {
                images.add(Image::new_target_texture(
                    W,
                    H,
                    TextureFormat::Rgba8Unorm,
                    Some(TextureFormat::Rgba8UnormSrgb),
                ))
            })
            .clone();
        let cam = commands
            .spawn((
                UnitCamera,
                OffscreenCamera,
                Camera3d::default(),
                Camera {
                    order: -3,
                    clear_color: ClearColorConfig::Custom(Color::BLACK),
                    ..default()
                },
                RenderTarget::Image(image.clone().into()),
                Projection::Perspective(PerspectiveProjection {
                    fov: 50f32.to_radians() * H as f32 / W as f32 * 1.4,
                    aspect_ratio: W as f32 / H as f32,
                    near: 2.0,
                    far: 600_000.0,
                    ..default()
                }),
                Tonemapping::None,
                Msaa::Off,
                WorldPos(eye),
                look,
                RenderLayers::from_layers(&[0, TRACKCAM_LOD_LAYER, UNIT_BACKGROUND_LAYER]),
                DespawnOnExit(crate::settings::AppState::Flight),
            ))
            .id();
        crate::camera::spawn_background(
            &mut commands,
            cam,
            &mut meshes,
            &palette,
            UNIT_BACKGROUND_LAYER,
        );
        view.camera = Some(cam);
    }
    // The headset's panel.
    if settings.vr && view.panel.is_none() {
        let image = view.image.clone();
        view.panel = Some(spawn_panel(&mut commands, &mut meshes, &mut materials, image));
        view.placed = false;
        if let Some(r) = root.and_then(|r| r.0)
            && let Some(p) = view.panel
        {
            commands.entity(r).add_child(p);
        }
    }
}

/// Panel size (m): the picture, the title bar above it and its close box.
const PANEL: Vec2 = Vec2::new(0.8, 0.45);
const BAR: f32 = 0.06;

fn spawn_panel(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    image: Option<Handle<Image>>,
) -> Entity {
    let unlit = |materials: &mut Assets<StandardMaterial>, colour: Color, tex| {
        materials.add(StandardMaterial {
            base_color: colour,
            base_color_texture: tex,
            unlit: true,
            cull_mode: None,
            fog_enabled: false,
            ..default()
        })
    };
    let picture = unlit(materials, Color::WHITE, image);
    let bar = unlit(materials, Color::srgb(0.05, 0.25, 0.1), None);
    let close = unlit(materials, Color::srgb(0.8, 0.15, 0.1), None);
    let cross = unlit(materials, Color::WHITE, None);
    let panel = commands
        .spawn((
            UnitPanel,
            Mesh3d(meshes.add(Rectangle::new(PANEL.x, PANEL.y))),
            MeshMaterial3d(picture),
            Transform::from_xyz(0.9, 1.2, -1.1),
            bevy::light::NotShadowCaster,
        ))
        .id();
    let bar_y = PANEL.y / 2.0 + BAR / 2.0;
    let bar = commands
        .spawn((
            Mesh3d(meshes.add(Rectangle::new(PANEL.x, BAR))),
            MeshMaterial3d(bar),
            Transform::from_xyz(0.0, bar_y, 0.0),
            bevy::light::NotShadowCaster,
        ))
        .id();
    let close = commands
        .spawn((
            Mesh3d(meshes.add(Rectangle::new(BAR * 0.9, BAR * 0.9))),
            MeshMaterial3d(close),
            Transform::from_xyz(PANEL.x / 2.0 - BAR / 2.0, bar_y, 0.002),
            bevy::light::NotShadowCaster,
        ))
        .id();
    let stroke = meshes.add(Cuboid::new(BAR * 0.8, 0.006, 0.001));
    let x = [0.785f32, -0.785].map(|a| {
        commands
            .spawn((
                Mesh3d(stroke.clone()),
                MeshMaterial3d(cross.clone()),
                Transform::from_xyz(0.0, 0.0, 0.002).with_rotation(Quat::from_rotation_z(a)),
            ))
            .id()
    });
    commands.entity(close).add_children(&x);
    commands.entity(panel).add_children(&[bar, close]);
    panel
}

/// VR: the panel is placed beside the menu panel, carried with a grip while a controller
/// points at it, closed with the trigger on its close box; a thumbstick on the picture turns
/// around the unit and goes closer.
#[cfg(feature = "xr")]
pub fn vr_panel(
    mut view: ResMut<UnitView>,
    mut hands: ResMut<crate::cockpit3d::Hands>,
    mut panels: Query<&mut Transform, With<UnitPanel>>,
    cams: Query<&Transform, (With<bevy_mod_xr::camera::XrCamera>, Without<UnitPanel>)>,
    time: Res<Time<Real>>,
) {
    let view = &mut *view;
    let Some(panel) = view.panel else { return };
    let Ok(mut tf) = panels.get_mut(panel) else { return };
    if !view.placed {
        let n = cams.iter().len();
        let head = cams.iter().map(|t| t.translation).sum::<Vec3>() / n.max(1) as f32;
        if n == 0 || head == Vec3::ZERO {
            return;
        }
        let fwd = cams.iter().next().map_or(Vec3::NEG_Z, |t| t.forward().as_vec3());
        let flat = Vec3::new(fwd.x, 0.0, fwd.z).normalize_or(Vec3::NEG_Z);
        let right = flat.cross(Vec3::Y);
        let at = head + flat * 1.0 + right * 0.95;
        *tf = Transform::from_translation(at).looking_at(head, Vec3::Y);
        // Rectangle faces +Z: turn it to face the head.
        tf.rotate_local_y(std::f32::consts::PI);
        view.placed = true;
    }
    let dt = time.delta_secs();
    if let Some((i, in_aim)) = view.grab {
        let h = &mut hands.hands[i];
        match h.aim_raw {
            Some(aim) if h.squeeze >= 0.4 => {
                *tf = aim * in_aim;
                return;
            }
            _ => {
                view.grab = None;
                h.haptic = h.haptic.max(0.2);
            }
        }
    }
    let inv = tf.compute_affine().inverse();
    for (i, h) in hands.hands.iter_mut().enumerate() {
        let Some(aim) = h.aim_raw else { continue };
        let (o, d) = (inv.transform_point3(aim.translation), inv.transform_vector3(*aim.forward()));
        if d.z.abs() < 1e-6 {
            continue;
        }
        let t = -o.z / d.z;
        if t <= 0.0 {
            continue;
        }
        let p = o + d * t;
        let on_picture = p.x.abs() <= PANEL.x / 2.0 && p.y.abs() <= PANEL.y / 2.0;
        let on_bar =
            p.x.abs() <= PANEL.x / 2.0 && p.y > PANEL.y / 2.0 && p.y <= PANEL.y / 2.0 + BAR;
        let on_close = on_bar && p.x >= PANEL.x / 2.0 - BAR;
        if on_close && h.trigger_pulled() {
            h.haptic = h.haptic.max(0.5);
            view.target = None;
            return;
        }
        if (on_picture || on_bar) && h.squeeze > 0.6 {
            view.grab =
                Some((i, Transform::from_matrix(aim.to_matrix().inverse() * tf.to_matrix())));
            h.haptic = h.haptic.max(0.4);
            return;
        }
        if on_picture {
            view.orbit += h.thumb.x * dt * 1.5;
            view.distance = (view.distance * (-h.thumb.y * dt * 1.2).exp()).clamp(10.0, 2000.0);
        }
    }
}

/// Desktop: the window with the picture (movable, resizable, closeable).
pub fn window(
    mut contexts: EguiContexts,
    mut view: ResMut<UnitView>,
    settings: Res<crate::settings::Settings>,
) -> Result {
    if settings.vr || view.target.is_none() {
        return Ok(());
    }
    let Some(image) = view.image.clone() else { return Ok(()) };
    let tex = contexts.add_image(EguiTextureHandle::Strong(image));
    let Ok(ctx) = contexts.ctx_mut() else { return Ok(()) };
    let mut open = true;
    let title = format!("{} - LIVE", view.title.to_ascii_uppercase());
    let view = &mut *view;
    egui::Window::new(title)
        .id(egui::Id::new("unit_view"))
        .open(&mut open)
        .resizable(true)
        .collapsible(true)
        .default_size(egui::vec2(560.0, 340.0))
        .default_pos(egui::pos2(60.0, 60.0))
        .show(ctx, |ui| {
            let w = ui.available_width().max(160.0);
            let size = egui::vec2(w, w * H as f32 / W as f32);
            let resp = ui.add(egui::Image::new((tex, size)).sense(egui::Sense::click_and_drag()));
            if resp.dragged() {
                let d = resp.drag_delta();
                view.orbit += d.x * 0.01;
                view.pitch = (view.pitch + d.y * 0.01).clamp(-0.3, 1.5);
            }
            if resp.hovered() {
                let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                view.distance = (view.distance * (-scroll * 0.003).exp()).clamp(10.0, 2000.0);
            }
            ui.horizontal(|ui| {
                if view.lost > 0.0 {
                    ui.label(egui::RichText::new("CONTACT LOST").color(AMBER));
                } else if let Some((alt, kt)) = view.info {
                    ui.label(
                        egui::RichText::new(format!("ALT {:.0} FT  SPD {kt:.0} KT", alt / 0.3048))
                            .color(BRIGHT),
                    );
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new("Drag: orbit  Wheel: zoom").small().color(DIM));
                });
            });
        });
    if !open {
        view.close();
    }
    Ok(())
}
