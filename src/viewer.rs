//! `--viewer [--file stflt] [--model N] [--yaw deg] [--pitch deg] [--distance d] [--no-label]`:
//! browse every converted model.
//! PgUp/PgDn model, Tab file, drag orbit, wheel zoom, P palette, R day/night remap table,
//! L lights (state slots 10/12, flashing 18-20), D destroyed variant (slot 1), A animate parts.

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;

use crate::models::{ModelAssets, ModelLibrary, ModelState};
use crate::palette::{Palette, VgaPalette};
use crate::settings::{AppState, SetupSet};

pub struct ViewerPlugin;

impl Plugin for ViewerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ViewerState>()
            .add_systems(OnEnter(AppState::Viewer), setup.in_set(SetupSet::Spawn))
            .add_systems(
                Update,
                (navigate, respawn, orbit, label).chain().run_if(in_state(AppState::Viewer)),
            );
    }
}

#[derive(Resource, Default)]
struct ViewerState {
    files: Vec<String>,
    file: usize,
    model: usize,
    names: Vec<String>,
    current: Option<Entity>,
    dirty: bool,
    yaw: f32,
    pitch: f32,
    distance: f32,
    palettes: Vec<(String, VgaPalette)>,
    palette: usize,
    lights: bool,
    destroyed: bool,
    animate: bool,
}

#[derive(Component)]
struct ViewerCamera;

#[derive(Component)]
struct ViewerLabel;

fn setup(
    mut commands: Commands,
    mut state: ResMut<ViewerState>,
    mut library: ResMut<ModelLibrary>,
    server: Res<AssetServer>,
    palette: Res<Palette>,
) {
    let mut files: Vec<String> = std::fs::read_dir(crate::asset_path::asset("models"))
        .map(|d| {
            d.filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().to_str()?.strip_suffix(".glb").map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    for f in &files {
        library.load(&server, f);
    }
    state.pitch = 0.3;
    state.distance = 300.0;
    let args: Vec<String> = std::env::args().collect();
    let arg = |f: &str| args.iter().position(|a| a == f).and_then(|i| args.get(i + 1)).cloned();
    if let Some(f) = arg("--file") {
        state.file = files.iter().position(|x| *x == f).unwrap_or(0);
    }
    state.model = arg("--model").and_then(|m| m.parse().ok()).unwrap_or(0);
    if let Some(d) = arg("--pitch").and_then(|m| m.parse::<f32>().ok()) {
        state.pitch = d.to_radians();
    }
    if let Some(d) = arg("--yaw").and_then(|m| m.parse::<f32>().ok()) {
        state.yaw = d.to_radians();
    }
    if let Some(d) = arg("--distance").and_then(|m| m.parse().ok()) {
        state.distance = d;
    }
    state.lights = args.iter().any(|a| a == "--lights");
    state.files = files;
    state.dirty = true;
    state.palettes = vec![
        ("day".into(), palette.theater.day.clone()),
        ("dusk".into(), palette.theater.dusk.clone()),
        ("night".into(), palette.theater.night.clone()),
    ];
    let _ = VgaPalette::load;

    commands.spawn((
        Camera3d::default(),
        ViewerCamera,
        bevy::core_pipeline::tonemapping::Tonemapping::None,
        Projection::Perspective(PerspectiveProjection { near: 0.5, far: 200_000.0, ..default() }),
        Transform::from_xyz(0.0, 10.0, 60.0),
    ));
    commands.insert_resource(ClearColor(palette.color(1)));
    commands.spawn((
        DirectionalLight { illuminance: 3000.0, ..default() },
        Transform::from_xyz(1.0, 2.0, 1.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        Text::new(""),
        ViewerLabel,
        Node { position_type: PositionType::Absolute, top: px(8), left: px(8), ..default() },
    ));
}

fn navigate(
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<ViewerState>,
    mut palette: ResMut<Palette>,
    mut models: Query<&mut ModelState>,
    time: Res<Time>,
) {
    if keys.just_pressed(KeyCode::KeyR) {
        palette.remap = (palette.remap + 1) % palette.remaps.len();
    }
    if keys.just_pressed(KeyCode::KeyL) {
        state.lights = !state.lights;
    }
    if keys.just_pressed(KeyCode::KeyD) {
        state.destroyed = !state.destroyed;
    }
    if keys.just_pressed(KeyCode::KeyA) {
        state.animate = !state.animate;
    }
    for mut m in &mut models {
        m.0[10] = state.lights as i16;
        m.0[12] = state.lights as i16;
        for (k, v) in crate::sim_render::anticollision(time.elapsed_secs(), 0.0, state.lights) {
            m.0[k] = v;
        }
        m.0[1] = state.destroyed as i16;
        if state.animate {
            let a = (time.elapsed_secs() * 20000.0) as i32 as i16;
            m.0[16] = a;
            m.0[17] = a.wrapping_mul(4);
        }
    }
    if state.files.is_empty() {
        return;
    }
    if keys.just_pressed(KeyCode::Tab) {
        state.file = (state.file + 1) % state.files.len();
        state.model = 0;
        state.dirty = true;
    }
    let n = state.names.len().max(1);
    if keys.just_pressed(KeyCode::PageDown) {
        state.model = (state.model + 1) % n;
        state.dirty = true;
    }
    if keys.just_pressed(KeyCode::PageUp) {
        state.model = (state.model + n - 1) % n;
        state.dirty = true;
    }
    if keys.just_pressed(KeyCode::KeyP) {
        state.palette = (state.palette + 1) % state.palettes.len();
        palette.active = state.palettes[state.palette].1.clone();
    }
}

fn respawn(
    mut commands: Commands,
    mut state: ResMut<ViewerState>,
    library: Res<ModelLibrary>,
    assets: ModelAssets,
) {
    if !state.dirty || state.files.is_empty() {
        return;
    }
    let Some(file) = library.files.get(&state.files[state.file]) else { return };
    let names = assets.model_names(file);
    if names.is_empty() {
        return; // still loading
    }
    state.names = names;
    state.model = state.model.min(state.names.len() - 1);
    if let Some(e) = state.current.take() {
        commands.entity(e).despawn();
    }
    state.current = assets.spawn(&mut commands, file, &state.names[state.model], 1, true);
    state.dirty = false;
}

fn orbit(
    mut state: ResMut<ViewerState>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    mut cam: Single<&mut Transform, With<ViewerCamera>>,
) {
    if buttons.pressed(MouseButton::Left) {
        state.yaw -= motion.delta.x * 0.01;
        state.pitch = (state.pitch + motion.delta.y * 0.01).clamp(-1.5, 1.5);
    }
    state.distance = (state.distance * (1.0 - scroll.delta.y * 0.1)).clamp(1.0, 100_000.0);
    let rot = Quat::from_euler(EulerRot::YXZ, state.yaw, -state.pitch, 0.0);
    **cam = Transform::from_translation(rot * Vec3::new(0.0, 0.0, state.distance))
        .looking_at(Vec3::ZERO, Vec3::Y);
}

fn label(
    state: Res<ViewerState>,
    palette: Res<Palette>,
    mut text: Single<&mut Text, With<ViewerLabel>>,
) {
    if std::env::args().any(|a| a == "--no-label") {
        text.0.clear();
        return;
    }
    let file = state.files.get(state.file).map_or("(no models)", String::as_str);
    let model = state.names.get(state.model).map_or("", String::as_str);
    text.0 = format!(
        "{file}  [{}/{}] {model}   palette: {}  remap: {}  lights: {}  destroyed: {}\n\
         Tab file  PgUp/PgDn model  P palette  R remap  L lights  D destroyed  A animate  drag orbit  wheel zoom",
        state.model + 1,
        state.names.len(),
        state.palettes.get(state.palette).map_or("", |p| p.0.as_str()),
        palette.remaps.get(palette.remap).map_or("", |r| r.name.as_str()),
        state.lights,
        state.destroyed,
    );
}
