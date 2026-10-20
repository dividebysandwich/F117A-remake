//! Pilot camera rig and views.
//!
//! aircraft entity -> `HeadAnchor` (pilot eye point, rotated by look-around views) -> eye camera.
//! In VR the eye camera entity is the XR tracking root (the headset cameras are its children),
//! so the same rig carries both. External views move the camera off the aircraft instead.
//! Cockpit views add a free look (`HeadLook`, mouse on the desktop) and `RigOffset` (VR
//! recentring) to the head.

use bevy::math::DVec3;
use bevy::prelude::*;

use crate::flight::{Aircraft, Player};
use crate::input::{Action, ActionEvent};
use crate::world::{WorldAnchor, WorldPos};

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ViewState>()
            .init_resource::<HeadLook>()
            .init_resource::<Orbit>()
            .init_resource::<RigOffset>()
            .init_resource::<XrRoot>()
            .add_systems(Update, (switch_view, update_rig).chain().in_set(RigSystems).after(crate::flight::FlightSystems).run_if(in_state(crate::settings::AppState::Flight)));
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct RigSystems;

/// Desktop forward cockpit view tilts down slightly so the lower panel stays in view (radians).
const COCKPIT_DOWN_PITCH: f32 = -0.1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum View {
    #[default]
    Cockpit,
    LookLeft,
    LookRight,
    LookRear,
    Chase,
    Side,
    Tactical,
    /// After ejecting: the pilot under the chute and on the ground (set by the combat
    /// simulation; the view keys do nothing until the mission ends).
    Pilot,
}

#[derive(Resource, Default)]
pub struct ViewState {
    pub view: View,
    pub zoom: i32,
    pub wide: bool,
}

#[derive(Component)]
pub struct HeadAnchor;

/// Free head look in the 3D cockpit (radians, relative to the view's direction).
#[derive(Resource, Default)]
pub struct HeadLook {
    pub yaw: f32,
    pub pitch: f32,
}

/// Mouse orbit in the external views (radians, added to the view's direction to the camera).
#[derive(Resource, Default)]
pub struct Orbit {
    pub yaw: f32,
    pub pitch: f32,
}

/// Transform of the eye camera / XR tracking root relative to the head anchor in cockpit
/// views (VR recentring puts the headset's rest pose at the design eye point).
#[derive(Resource, Default)]
pub struct RigOffset(pub Transform);

impl RigOffset {
    /// The offset with the headset's tracking space scaled by `1 / cockpit_scale`: a larger
    /// tracking space makes the cockpit look smaller.
    pub fn scaled(&self, cockpit_scale: f32) -> Transform {
        Transform::from_scale(Vec3::splat(1.0 / cockpit_scale)) * self.0
    }
}

/// The XR tracking root while a headset session exists (set by the XR module).
#[derive(Resource, Default)]
pub struct XrRoot(pub Option<Entity>);

#[derive(Component)]
pub struct EyeCamera;

/// Radius of the background sphere carrying the sky and ground gradient (inside the far plane).
const BACKGROUND_RADIUS: f32 = 590_000.0;

/// Render layers of the per-camera background spheres (everything else is on layer 0).
pub const MAIN_BACKGROUND_LAYER: usize = 2;
pub const TRACKCAM_BACKGROUND_LAYER: usize = 1;

/// Mip level bias of the cockpit's textured screens and labels (StandardMaterial samples with
/// the view's bias): negative picks sharper levels. The headset's eye buffers pick coarser
/// levels for the same apparent size, so they get more.
pub const DESKTOP_MIP_BIAS: f32 = -0.5;
pub const VR_MIP_BIAS: f32 = -1.25;

/// Render layers of model detail levels picked by only one view (`models::LodShown`); levels
/// picked by both views stay on layer 0.
pub const MAIN_LOD_LAYER: usize = 3;
pub const TRACKCAM_LOD_LAYER: usize = 4;

/// Spawns the background sphere (sky bands and ground gradient) as a child of a camera, on
/// its own render layer so each camera only sees its own sphere.
pub fn spawn_background(
    commands: &mut Commands,
    camera: Entity,
    meshes: &mut Assets<Mesh>,
    palette: &crate::palette::Palette,
    layer: usize,
) -> Entity {
    let sphere = commands
        .spawn((
            // Seen from inside: invert the winding so back-face culling keeps the inner side.
            Mesh3d(meshes.add(
                Mesh::from(Sphere::new(BACKGROUND_RADIUS).mesh().uv(64, 64))
                    .with_inverted_winding()
                    .expect("sphere mesh has indices"),
            )),
            MeshMaterial3d(palette.background.clone()),
            Transform::default(),
            bevy::camera::visibility::NoFrustumCulling,
            bevy::camera::visibility::RenderLayers::layer(layer),
            bevy::light::NotShadowCaster,
        ))
        .id();
    commands.entity(camera).add_child(sphere);
    sphere
}

/// Whether the eye cameras draw without indirect draws (GPU preprocessing still builds the
/// instance data): in VR, where on the Steam Frame's GPU indirect draws took 8 ms more per
/// frame. `SM89_NO_INDIRECT=0|1` overrides.
pub fn no_indirect(vr: bool) -> bool {
    match std::env::var("SM89_NO_INDIRECT").as_deref() {
        Ok("1") => true,
        Ok("0") => false,
        _ => vr,
    }
}

/// Spawns the rig under the given aircraft entity, the head at the pilot's eye point (`eye`,
/// aircraft frame, metres). `xr_root`: the XR tracking root, used as
/// the eye camera (its headset cameras get the render layers from the XR module).
pub fn spawn_rig(
    commands: &mut Commands,
    aircraft: Entity,
    meshes: &mut Assets<Mesh>,
    palette: &crate::palette::Palette,
    xr_root: Option<Entity>,
    eye: Vec3,
) {
    let head = commands.spawn((HeadAnchor, Transform::from_translation(eye), Visibility::default())).id();
    if let Some(root) = xr_root {
        commands.entity(root).insert((EyeCamera, Transform::IDENTITY));
        spawn_background(commands, root, meshes, palette, MAIN_BACKGROUND_LAYER);
        commands.entity(head).add_child(root);
        commands.entity(aircraft).add_child(head).insert(WorldAnchor);
        return;
    }
    let cam = commands
        .spawn((
            Camera3d::default(),
            EyeCamera,
            Projection::Perspective(PerspectiveProjection { near: 0.5, far: 600_000.0, ..default() }),
            Transform::default(),
            // Palette colours must reach the screen unchanged.
            bevy::core_pipeline::tonemapping::Tonemapping::None,
            bevy::render::camera::MipBias(DESKTOP_MIP_BIAS),
        ))
        .id();
    spawn_background(commands, cam, meshes, palette, MAIN_BACKGROUND_LAYER);
    commands.entity(cam).insert(bevy::camera::visibility::RenderLayers::from_layers(&[0, MAIN_BACKGROUND_LAYER, MAIN_LOD_LAYER]));
    if no_indirect(false) {
        commands.entity(cam).insert(bevy::render::view::NoIndirectDrawing);
    }
    commands.entity(head).add_child(cam);
    commands.entity(aircraft).add_child(head).insert(WorldAnchor);
}

fn switch_view(mut actions: MessageReader<ActionEvent>, mut state: ResMut<ViewState>, combat: Option<Res<crate::combat::Combat>>) {
    if combat.is_some_and(|c| c.sim.chute().is_some()) {
        actions.clear();
        state.view = View::Pilot;
        return;
    }
    if state.view == View::Pilot {
        state.view = View::Cockpit;
    }
    for ActionEvent(a) in actions.read() {
        match a {
            Action::ViewCockpit | Action::LookAhead => state.view = View::Cockpit,
            Action::LookLeft => state.view = View::LookLeft,
            Action::LookRight => state.view = View::LookRight,
            Action::LookRear => state.view = View::LookRear,
            Action::ViewChase | Action::ViewSlot => state.view = View::Chase,
            Action::ViewSide => state.view = View::Side,
            Action::ViewTactical | Action::ViewInverseTactical => state.view = View::Tactical,
            Action::Zoom => state.zoom = (state.zoom + 1).min(3),
            Action::Unzoom => state.zoom = (state.zoom - 1).max(-3),
            Action::WideAngle => state.wide = !state.wide,
            _ => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn update_rig(
    state: Res<ViewState>,
    settings: Res<crate::settings::Settings>,
    look: Res<HeadLook>,
    orbit: Res<Orbit>,
    offset: Res<RigOffset>,
    aircraft: Query<(&Aircraft, &WorldPos), With<Player>>,
    mut head: Query<&mut Transform, (With<HeadAnchor>, Without<EyeCamera>)>,
    mut cam: Query<&mut Transform, With<EyeCamera>>,
    mut viewpoint: ResMut<crate::palette::Viewpoint>,
    combat: Option<Res<crate::combat::Combat>>,
) {
    let (Ok(mut head), Ok(mut cam_tf)) = (head.single_mut(), cam.single_mut()) else { return };
    let Ok((a, pos)) = aircraft.single() else { return };
    // Lowest camera height relative to the aircraft origin in external views (1 m above ground).
    let floor = 1.0 - (a.altitude - a.ground_altitude) * crate::world::WU as f32;
    let eye = settings.aircraft.spec().eye;
    viewpoint.altitude_wu = a.altitude + eye.y / crate::world::WU as f32;
    let yaw = match state.view {
        View::LookLeft => 90f32.to_radians(),
        View::LookRight => -90f32.to_radians(),
        View::LookRear => 180f32.to_radians(),
        _ => 0.0,
    };
    match state.view {
        View::Cockpit | View::LookLeft | View::LookRight | View::LookRear => {
            // Looks are head turns in the cockpit (in VR the headset turns).
            let base_pitch = if state.view == View::Cockpit { COCKPIT_DOWN_PITCH } else { 0.0 };
            let (y, p) = if settings.vr { (0.0, 0.0) } else { (yaw + look.yaw, base_pitch + look.pitch) };
            *head = Transform::from_translation(eye).with_rotation(Quat::from_rotation_y(y) * Quat::from_rotation_x(p));
            *cam_tf = if settings.vr { offset.scaled(settings.vr_rig_scale()) } else { offset.0 };
        }
        View::Chase => {
            // Behind and above, looking along the flight direction (camera stays in the aircraft frame).
            let d = 40.0 * 2f32.powi(-state.zoom);
            *head = Transform::IDENTITY;
            *cam_tf = orbited(Vec3::new(0.0, d * 0.25, d), Vec3::new(0.0, 0.0, -20.0), &orbit, a.orientation, floor);
        }
        View::Side => {
            let d = 40.0 * 2f32.powi(-state.zoom);
            *head = Transform::IDENTITY;
            *cam_tf = orbited(Vec3::new(d, 2.0, 0.0), Vec3::ZERO, &orbit, a.orientation, floor);
        }
        View::Pilot => {
            let Some(sim) = combat.as_ref().map(|c| &c.sim) else { return };
            let Some((x, y, alt)) = sim.chute() else { return };
            // Eye height on the ground, just under the canopy in the air.
            let eye = crate::sim_render::map_point(x as f64 + 0.5, y as f64 + 0.5, alt as f64) + DVec3::Y * 1.7;
            viewpoint.altitude_wu = (eye.y / crate::world::WU) as f32;
            // Facing the rescue helicopter once there is one, else north; the mouse looks around.
            let mut yaw = 0.0;
            if let Some(c) = sim.csar()
                && let Some(u) = sim.flights().get(c.rescue)
            {
                yaw = -((u.x - x) as f32).atan2((y - u.y) as f32);
            }
            // The mouse looks up as far as the canopy overhead.
            let world_rot = Quat::from_rotation_y(yaw + orbit.yaw) * Quat::from_rotation_x((0.05 - orbit.pitch).clamp(-1.4, 1.5));
            let local = a.orientation.inverse();
            *head = Transform::from_translation(local * (eye - pos.0).as_vec3()).with_rotation(local * world_rot);
            *cam_tf = offset.0;
        }
        View::Tactical => {
            // Above and behind in world-up terms: undo the aircraft's roll and pitch.
            let d = 120.0 * 2f32.powi(-state.zoom);
            let (h, _, _) = a.euler();
            let world_rot = Quat::from_rotation_y(-h);
            let local = a.orientation.inverse() * world_rot;
            *head = Transform::from_rotation(local);
            *cam_tf = orbited(Vec3::new(0.0, d * 0.5, d), Vec3::ZERO, &orbit, world_rot, floor);
        }
    }
    // Projection (FOV, off-centre frustum) is set by the cockpit module from the original
    // focal length.
}

/// An external camera at `pos` looking at `target` (rig frame), orbited around the aircraft:
/// the orbit yaw turns both about the vertical axis, the pitch raises or lowers the camera.
/// `to_world` rotates the rig frame to the world; the camera stays at least `floor` metres above
/// the aircraft origin (world up).
fn orbited(pos: Vec3, target: Vec3, orbit: &Orbit, to_world: Quat, floor: f32) -> Transform {
    let r = pos.length();
    let yaw = pos.x.atan2(pos.z) + orbit.yaw;
    let pitch = ((pos.y / r).asin() + orbit.pitch).clamp(-1.5, 1.5);
    let mut p = Vec3::new(yaw.sin() * pitch.cos(), pitch.sin(), yaw.cos() * pitch.cos()) * r;
    let height = (to_world * p).y;
    if height < floor {
        p += to_world.inverse() * Vec3::Y * (floor - height);
    }
    Transform::from_translation(p).looking_at(Quat::from_rotation_y(orbit.yaw) * target, Vec3::Y)
}

/// World position of the eye (for systems that need it without the transform hierarchy).
#[allow(dead_code)]
pub fn eye_world(a: &Aircraft, pos: &WorldPos, eye: Vec3) -> DVec3 {
    pos.0 + (a.orientation * eye).as_dvec3()
}
