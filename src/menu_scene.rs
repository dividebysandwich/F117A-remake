//! The scene behind the main menu: an F-117A on the runway at Tabor on one side of the screen,
//! the control tower on the other, the menu between them, on a clear full-moon night in the
//! classic palette with the moon over the field. Not in VR (the menu is a panel there).

use bevy::math::DVec3;
use bevy::prelude::*;
use bevy::render::storage::ShaderBuffer;

use crate::models::{ModelAssets, ModelLibrary, ModelState};
use crate::palette::{Palette, TimeOfDay};
use crate::palette_material::PaletteMaterial;
use crate::settings::{AppState, Settings};
use crate::world::{WU, WorldAnchor, WorldPos};

pub struct MenuScenePlugin;

impl Plugin for MenuScenePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::Menu), spawn_scene)
            .add_systems(OnExit(AppState::Menu), (despawn_scene, crate::world::unload_theater))
            .add_systems(
                Update,
                (spawn_aircraft, aircraft_lights, fit_view, match_overlay_msaa)
                    .run_if(resource_exists::<MenuScene>),
            );
    }
}

/// The scene is drawn behind the menu.
#[derive(Resource)]
pub struct MenuScene;

/// Everything spawned for the scene.
#[derive(Component)]
struct SceneEntity;

/// The aircraft on the runway (its model is spawned once the models are loaded).
#[derive(Component)]
struct SceneAircraft;

#[derive(Component)]
struct SceneCamera;

const THEATER: &str = "ce";
/// Tabor's control tower (world units east, north): the scene is laid out from it. The runway
/// runs north and south 254 wu (124 m) west of it.
const TOWER: (f64, f64) = (567_552.0, 173_056.0);
/// The camera (metres east, up and north of the tower's foot), its heading (degrees clockwise
/// from north) and pitch.
const CAMERA: [f32; 5] = [-80.0, 4.5, 265.0, 190.0, 7.0];
/// The aircraft (metres east and north of the tower's foot) and its heading (degrees).
const AIRCRAFT: [f32; 3] = [-101.0, 231.0, 345.0];
/// The moon's azimuth (degrees clockwise from north) and elevation: between the tower and the
/// menu.
const MOON: [f32; 2] = [171.0, 18.0];
/// The classic night palette's seed (`TimeOfDay::seed`): a few steps from dusk towards night,
/// the purple horizon under a dark blue sky.
const TOD_SEED: u8 = 3;
/// The narrowest horizontal field of view (degrees), so the tower and the aircraft stay on the
/// screen in narrow windows.
const MIN_WIDTH_FOV: f32 = 72.0;

/// The scene is drawn behind the menu (on the screen, not in the headset).
pub fn shown(settings: &Settings) -> bool {
    !settings.vr
}

/// A point `east`, `up`, `north` metres from the tower's foot, in Bevy metres.
fn at_tower(east: f32, up: f32, north: f32) -> DVec3 {
    crate::world::wu_to_bevy(TOWER.0, TOWER.1, 0.0)
        + DVec3::new(east as f64, up as f64, -(north as f64))
}

/// Direction (Bevy: x east, y up, z south) at an azimuth and elevation in degrees.
fn direction(az: f32, el: f32) -> Vec3 {
    let (az, el) = (az.to_radians(), el.to_radians());
    Vec3::new(az.sin() * el.cos(), el.sin(), -az.cos() * el.cos())
}

#[allow(clippy::too_many_arguments)]
fn spawn_scene(
    mut commands: Commands,
    settings: Res<Settings>,
    mut streamed: ResMut<crate::world::StreamedCells>,
    mut overrides: ResMut<crate::world::TileOverrides>,
    mut materials: ResMut<Assets<PaletteMaterial>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut tod: ResMut<TimeOfDay>,
    mut sky: ResMut<crate::sky::Sky>,
    mut viewpoint: ResMut<crate::palette::Viewpoint>,
) {
    if !shown(&settings) {
        return;
    }
    let (cam, moon) = (CAMERA, MOON);
    commands.insert_resource(MenuScene);
    crate::world::insert_theater(&mut commands, THEATER, None, &mut streamed, &mut overrides);
    // A night in the classic palette, held still.
    *tod = TimeOfDay::default();
    tod.night_mission = true;
    tod.seed = TOD_SEED;
    tod.frozen = true;
    let palette = Palette::new(THEATER, &tod, &mut materials, &mut buffers);
    // The full moon: the sun stands opposite, below the horizon.
    *sky = crate::sky::Sky::default();
    let m = direction(moon[0], moon[1]);
    (sky.moon, sky.sun, sky.stars, sky.classic_bodies) = (m, -m, 0.4, true);
    crate::lighting::spawn_lights(&mut commands, &settings);
    viewpoint.altitude_wu = (cam[1] as f64 / WU) as f32;
    let camera = commands
        .spawn((
            Camera3d::default(),
            Camera { order: 0, ..default() },
            bevy::core_pipeline::tonemapping::Tonemapping::None,
            Projection::Perspective(PerspectiveProjection {
                near: 0.5,
                far: 600_000.0,
                ..default()
            }),
            SceneCamera,
            SceneEntity,
            WorldAnchor,
            WorldPos(at_tower(cam[0], cam[1], cam[2])),
            Transform::from_rotation(Quat::from_euler(
                EulerRot::YXZ,
                -cam[3].to_radians(),
                cam[4].to_radians(),
                0.0,
            )),
            bevy::camera::visibility::RenderLayers::from_layers(&[
                0,
                crate::camera::MAIN_BACKGROUND_LAYER,
                crate::camera::MAIN_LOD_LAYER,
            ]),
        ))
        .id();
    crate::camera::spawn_background(
        &mut commands,
        camera,
        &mut meshes,
        &palette,
        crate::camera::MAIN_BACKGROUND_LAYER,
    );
    commands.insert_resource(palette);
}

/// The F-117A on the runway, once its model file is loaded.
fn spawn_aircraft(
    mut commands: Commands,
    mut library: ResMut<ModelLibrary>,
    server: Res<AssetServer>,
    assets: ModelAssets,
    existing: Query<(), With<SceneAircraft>>,
) {
    if !existing.is_empty() {
        return;
    }
    let spec = crate::aircraft::AircraftKind::F117.spec();
    let file = library.load(&server, "stflt");
    let Some(model) = assets.spawn(
        &mut commands,
        &file,
        spec.model,
        crate::palette_material::LEVEL_AIRCRAFT,
        true,
    ) else {
        return;
    };
    let plane = AIRCRAFT;
    let scale = crate::player::AIRCRAFT_MODEL_SCALE as f32;
    // Gear down (slots 5 and 6), navigation lights on (12).
    let mut state = ModelState::default();
    (state.0[5], state.0[6], state.0[12]) = (1, 2, 1);
    commands.entity(model).insert((
        Transform::from_xyz(0.0, spec.model_lift, 0.0).with_scale(Vec3::splat(scale)),
        state,
    ));
    commands
        .spawn((
            SceneAircraft,
            SceneEntity,
            WorldPos(at_tower(plane[0], 0.0, plane[1])),
            Transform::from_rotation(Quat::from_rotation_y(-plane[2].to_radians())),
            Visibility::default(),
        ))
        .add_child(model);
}

/// The aircraft's anti-collision lights flash; its lights follow the palette's (slots 10/11).
fn aircraft_lights(
    time: Res<Time>,
    palette: Option<Res<Palette>>,
    aircraft: Query<&Children, With<SceneAircraft>>,
    mut states: Query<&mut ModelState>,
) {
    let lights = palette.is_some_and(|p| p.lights_on()) as i16;
    for children in &aircraft {
        let mut it = states.iter_many_mut(children);
        while let Some(mut s) = it.fetch_next() {
            for (slot, v) in crate::sim_render::anticollision(time.elapsed_secs(), 0.0, true)
                .into_iter()
                .chain([(10, lights), (11, lights)])
            {
                if s.0[slot] != v {
                    s.0[slot] = v;
                }
            }
        }
    }
}

/// A narrow window widens the view (Bevy keeps the vertical field of view).
fn fit_view(
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mut cams: Query<&mut Projection, With<SceneCamera>>,
) {
    let Ok(w) = windows.single() else { return };
    let aspect = w.width() / w.height().max(1.0);
    let half = (MIN_WIDTH_FOV.to_radians() / 2.0).tan();
    let fov = std::f32::consts::FRAC_PI_4.max(2.0 * (half / aspect).atan());
    for mut p in &mut cams {
        if let Projection::Perspective(p) = &mut *p
            && (p.fov - fov).abs() > 1e-4
        {
            p.fov = fov;
        }
    }
}

/// The menu's egui camera draws over the scene: its MSAA has to be the scene camera's (else
/// it gets a picture of its own that nothing clears).
fn match_overlay_msaa(
    scene: Query<&Msaa, (With<SceneCamera>, Without<crate::menu::MenuCamera>)>,
    mut overlay: Query<&mut Msaa, With<crate::menu::MenuCamera>>,
) {
    let Ok(&want) = scene.single() else { return };
    for mut m in &mut overlay {
        if *m != want {
            *m = want;
        }
    }
}

fn despawn_scene(mut commands: Commands, q: Query<Entity, With<SceneEntity>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
    commands.remove_resource::<MenuScene>();
    commands.remove_resource::<Palette>();
}
