//! Weather (not in the original game): the mission's clouds (f117_mission::weather) as
//! flat-shaded geometry, the overcast sky, the whiteout inside clouds and storm effects.
//!
//! - Deck (overcast, storm): two huge planes at the deck's base and top following the
//!   viewpoint; past their edges the background sphere draws the same deck (palette.wgsl).
//! - Cumulus: faceted blobs streamed around the viewpoint from the shared cloud field, so
//!   what is drawn is what blocks the simulation's lines of sight.
//! - Shading is done by the palette shader (`FLAG_CLOUD`): the grey ramp by face orientation,
//!   hazed per pixel with the weather's visibility.

use std::collections::HashMap;

use bevy::math::DVec3;
use bevy::mesh::{MeshTag, VertexAttributeValues};
use bevy::prelude::*;
use f117_mission::weather::{CELL, Cloud};
use f117_mission::{Sky, Weather};

use crate::camera::EyeCamera;
use crate::trackcam::TrackCamera;
use crate::mission::ActiveMission;
use crate::palette::Palette;
use crate::palette_material::{FLAG_CLOUD, PaletteMaterial, PaletteParams, mesh_tag};
use crate::settings::{AppState, SetupSet};
use crate::world::{FloatingOrigin, WU, WorldPos, wu_to_bevy};

pub struct WeatherPlugin;

impl Plugin for WeatherPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WeatherState>()
            .add_systems(OnEnter(AppState::Flight), setup_weather.in_set(SetupSet::Spawn))
            .add_systems(OnExit(AppState::Flight), teardown_weather)
            .add_systems(
                Update,
                turbulence.after(crate::flight::FlightSystems).before(crate::camera::RigSystems).run_if(in_state(AppState::Flight)),
            )
            .add_systems(
                PostUpdate,
                (track_eye, follow_deck, stream_clouds, lightning, rain)
                    .chain()
                    .after(bevy::transform::TransformSystems::Propagate)
                    .before(crate::lighting::LightingSystems)
                    .run_if(in_state(AppState::Flight)),
            );
    }
}

/// Cumulus are drawn this far around the viewpoint (map units, 64 per km).
const DRAW_RADIUS: i32 = 40 * 64;
/// Size of the deck planes (metres from the viewpoint to their edge).
const DECK_HALF: f32 = 280_000.0;
/// Distinct cumulus shapes.
const TEMPLATES: u32 = 8;
/// Horizontal period of the cloud texture (metres, `NOISE_PERIOD` in palette.wgsl).
const NOISE_PERIOD: f64 = 81920.0;

#[derive(Resource, Default)]
pub struct WeatherState {
    pub weather: Weather,
    /// Viewpoint: map units and feet.
    pub eye_map: (i32, i32),
    pub eye_ft: f32,
    /// Under an overcast or storm deck.
    pub below_deck: bool,
    /// 0 clear air .. 1 inside a cloud (eased).
    pub whiteout: f32,
    /// Lightning flash brightness.
    pub flash: f32,
    next_flash: f32,
    flash_left: f32,
    /// Seconds until the thunder of the last flash.
    thunder_in: Option<f32>,
    /// Eye position last frame (Bevy metres), for the rain's relative wind.
    prev_eye: Option<DVec3>,
    rain: Option<Entity>,
    drops: Vec<Vec3>,
    rng: u32,
    material: Handle<PaletteMaterial>,
    templates: Vec<Handle<Mesh>>,
    puffs: HashMap<(i32, i32), Entity>,
    streamed_at: Option<(i32, i32)>,
}

impl WeatherState {
    /// Haze distance scale for the world under the clouds (reduced visibility in storms).
    pub fn haze_scale(&self) -> f32 {
        if self.weather.sky == Sky::Storm && self.eye_ft < self.weather.top_ft as f32 {
            (20.0 / self.weather.vis_km.max(1) as f32).max(1.0)
        } else {
            1.0
        }
    }

    /// Shader globals: (deck base m, deck top m, deck present, whiteout),
    /// (haze scale, flash, cloud visibility m, 0) and the cloud texture's origin and seed.
    pub fn globals(&self, origin: DVec3) -> (Vec4, Vec4, Vec4) {
        let w = &self.weather;
        let deck = w.sky.has_deck();
        let clouds = Vec4::new(
            w.base_ft as f32 * WU as f32,
            w.top_ft as f32 * WU as f32,
            deck as u32 as f32,
            self.whiteout,
        );
        let vis_m = (w.vis_km as f32 * 1000.0).max(4000.0);
        let noise = Vec4::new(
            origin.x.rem_euclid(NOISE_PERIOD) as f32,
            origin.z.rem_euclid(NOISE_PERIOD) as f32,
            (w.seed & 0xFFFF) as f32,
            0.0,
        );
        (clouds, Vec4::new(self.haze_scale(), self.flash, vis_m, 0.0), noise)
    }

    fn random(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng >> 8) as f32 / (1u32 << 24) as f32
    }
}

#[derive(Component)]
struct CloudEntity;

/// Deck plane: offset from the viewpoint is kept at zero horizontally.
#[derive(Component)]
struct DeckPlane {
    alt_ft: i32,
}

/// Positions only (the palette material treats UVs as dot corners).
fn positions_only(mut mesh: Mesh) -> Mesh {
    mesh.remove_attribute(Mesh::ATTRIBUTE_NORMAL);
    mesh.remove_attribute(Mesh::ATTRIBUTE_UV_0);
    mesh
}

fn hash(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A cumulus shape: a lumpy faceted dome with a flat base, x/z in about -1..1, y in 0..1.
fn cumulus_mesh(seed: u32) -> Mesh {
    let mut mesh = positions_only(Sphere::new(1.0).mesh().ico(1).expect("icosphere"));
    if let Some(VertexAttributeValues::Float32x3(pos)) = mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION) {
        for (i, p) in pos.iter_mut().enumerate() {
            let h = hash(((seed as u64) << 32) | i as u64);
            let bump = 0.8 + 0.35 * ((h & 0xFFFF) as f32 / 65535.0);
            let v = Vec3::from(*p) * bump;
            // Flat, slightly bulging base; puffy top.
            let y = if v.y < 0.0 { v.y * 0.12 } else { v.y };
            *p = [v.x, (y + 0.12) / 1.27, v.z];
        }
    }
    mesh
}

fn setup_weather(
    mut commands: Commands,
    mission: Option<Res<ActiveMission>>,
    palette: Res<Palette>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<PaletteMaterial>>,
    mut state: ResMut<WeatherState>,
) {
    let weather = mission.map(|m| m.0.weather).unwrap_or_default();
    let material = materials.add(PaletteMaterial {
        params: PaletteParams { word: 0xFFE0, flags: FLAG_CLOUD, ..default() },
        globals: palette.globals.clone(),
    });
    *state = WeatherState {
        weather,
        rng: weather.seed | 1,
        next_flash: 3.0,
        material: material.clone(),
        templates: (0..TEMPLATES).map(|i| meshes.add(cumulus_mesh(weather.seed ^ i))).collect(),
        ..default()
    };
    info!("weather: {}", weather.describe());
    if !weather.sky.has_deck() {
        return;
    }
    // The base seen from below, the top seen from above.
    for (normal, alt_ft) in [(Vec3::NEG_Y, weather.base_ft), (Vec3::Y, weather.top_ft)] {
        commands.spawn((
            CloudEntity,
            DeckPlane { alt_ft },
            Mesh3d(meshes.add(positions_only(Plane3d::new(normal, Vec2::splat(DECK_HALF)).mesh().build()))),
            MeshMaterial3d(material.clone()),
            MeshTag(mesh_tag(1, 0, true)),
            WorldPos(DVec3::new(0.0, alt_ft as f64 * WU, 0.0)),
            Transform::default(),
            bevy::camera::visibility::NoFrustumCulling,
            bevy::light::NotShadowCaster,
        ));
    }
}

fn teardown_weather(mut commands: Commands, q: Query<Entity, With<CloudEntity>>, mut state: ResMut<WeatherState>) {
    for e in &q {
        commands.entity(e).try_despawn();
    }
    *state = WeatherState::default();
}

/// The main viewpoint: the eye camera (XR root in VR), else the free camera.
type Eyes<'w, 's> = Query<'w, 's, (&'static GlobalTransform, Has<EyeCamera>), (Or<(With<EyeCamera>, With<Camera3d>)>, Without<TrackCamera>)>;

fn eye_position(eyes: &Eyes, origin: &FloatingOrigin) -> Option<DVec3> {
    let (tf, _) = eyes.iter().max_by_key(|(_, eye)| *eye)?;
    Some(tf.translation().as_dvec3() + origin.origin)
}

/// Viewpoint position, whiteout and deck state.
fn track_eye(time: Res<Time>, origin: Res<FloatingOrigin>, eyes: Eyes, mut state: ResMut<WeatherState>) {
    let Some(p) = eye_position(&eyes, &origin) else { return };
    let (east, north, alt) = crate::world::bevy_to_wu(p);
    let map = ((east / 32.0) as i32, (0x8000 as f64 - north / 32.0) as i32);
    let ft = alt as f32;
    let w = state.weather;
    let inside = w.in_cloud(map.0, map.1, ft as i32);
    let k = (time.delta_secs() * 3.0).min(1.0);
    state.whiteout += ((inside as u32 as f32) - state.whiteout) * k;
    state.eye_map = map;
    state.eye_ft = ft;
    state.below_deck = w.sky.has_deck() && ft < w.base_ft as f32;
}

fn follow_deck(origin: Res<FloatingOrigin>, eyes: Eyes, mut decks: Query<(&DeckPlane, &mut WorldPos)>) {
    let Some(p) = eye_position(&eyes, &origin) else { return };
    for (deck, mut pos) in &mut decks {
        let want = DVec3::new(p.x.round(), deck.alt_ft as f64 * WU, p.z.round());
        if pos.0 != want {
            pos.0 = want;
        }
    }
}

/// Spawns the cumulus near the viewpoint and drops the ones left behind.
fn stream_clouds(mut commands: Commands, origin: Res<FloatingOrigin>, mut state: ResMut<WeatherState>) {
    let (x, y) = state.eye_map;
    if let Some((sx, sy)) = state.streamed_at
        && (sx - x).abs() < CELL / 2
        && (sy - y).abs() < CELL / 2
    {
        return;
    }
    state.streamed_at = Some((x, y));
    let w = state.weather;
    let keep = (DRAW_RADIUS + CELL) as i64;
    state.puffs.retain(|&(gx, gy), e| {
        let (cx, cy) = ((gx * CELL + CELL / 2 - x) as i64, (gy * CELL + CELL / 2 - y) as i64);
        let near = cx * cx + cy * cy <= keep * keep;
        if !near {
            commands.entity(*e).try_despawn();
        }
        near
    });
    let new: Vec<Cloud> = w.clouds_near(x, y, DRAW_RADIUS).filter(|c| !state.puffs.contains_key(&(c.gx, c.gy))).collect();
    for c in new {
        let h = hash(((w.seed as u64) << 32) ^ ((c.gx as u16 as u64) << 16) ^ c.gy as u16 as u64 ^ 0xC10D);
        let template = state.templates[(h % TEMPLATES as u64) as usize].clone();
        let r_m = c.r as f32 * 32.0 * WU as f32;
        let pos = wu_to_bevy(c.x as f64 * 32.0, (0x8000 - c.y) as f64 * 32.0, c.base_ft as f64);
        let h_m = (c.top_ft - c.base_ft) as f32 * WU as f32;
        let e = commands
            .spawn((
                CloudEntity,
                Mesh3d(template),
                MeshMaterial3d(state.material.clone()),
                MeshTag(mesh_tag(1, 0, true)),
                WorldPos(pos),
                Transform::from_translation((pos - origin.origin).as_vec3())
                    .with_rotation(Quat::from_rotation_y((h >> 16 & 0xFFFF) as f32 / 65535.0 * std::f32::consts::TAU))
                    .with_scale(Vec3::new(r_m, h_m, r_m)),
            ))
            .id();
        state.puffs.insert((c.gx, c.gy), e);
    }
}

/// Storms: random lightning flashes (a double flicker), more often under the deck, and
/// the thunder a moment later.
fn lightning(time: Res<Time>, mut state: ResMut<WeatherState>, mut sounds: MessageWriter<crate::sound::PlaySound>) {
    if state.weather.sky != Sky::Storm {
        state.flash = 0.0;
        return;
    }
    let dt = time.delta_secs();
    state.next_flash -= dt;
    if state.next_flash <= 0.0 {
        let near = state.below_deck || state.whiteout > 0.5;
        state.next_flash = if near { 3.0 + 9.0 * state.random() } else { 8.0 + 20.0 * state.random() };
        state.flash_left = 0.35;
        state.thunder_in = Some(0.5 + 3.0 * state.random());
    }
    if let Some(t) = state.thunder_in.as_mut() {
        *t -= dt;
        if *t <= 0.0 {
            state.thunder_in = None;
            // No thunder among the original sounds: the explosion rumble stands in.
            sounds.write(crate::sound::PlaySound { id: 5, priority: 0 });
        }
    }
    if state.flash_left > 0.0 {
        state.flash_left -= dt;
        let t = 0.35 - state.flash_left;
        // Two strokes: 0..0.08 s and 0.16..0.3 s.
        state.flash = if t < 0.08 || (0.16..0.3).contains(&t) { if state.below_deck { 0.6 } else { 0.35 } } else { 0.0 };
    } else {
        state.flash = 0.0;
    }
}

/// Rain streaks around the eye (storms, under the deck top).
const DROPS: usize = 500;
/// Half size of the rain box around the eye (metres).
const RAIN_BOX: f32 = 30.0;

/// Rain: streaks in a box around the eye, drawn along the wind the eye sees (falling rain
/// plus the aircraft's own speed), on the main view's layer only. The box is world aligned:
/// it is a child of the eye camera, counter-rotated.
#[allow(clippy::too_many_arguments)]
fn rain(
    mut commands: Commands,
    time: Res<Time>,
    origin: Res<FloatingOrigin>,
    eyes: Eyes,
    eye_cams: Query<(Entity, &GlobalTransform), With<EyeCamera>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<PaletteMaterial>>,
    palette: Res<Palette>,
    mut tfs: Query<(&mut Transform, &Mesh3d)>,
    mut state: ResMut<WeatherState>,
) {
    let eye = eye_position(&eyes, &origin);
    let dt = time.delta_secs().max(1e-4);
    let eye_vel = match (eye, state.prev_eye) {
        (Some(p), Some(q)) if (p - q).length() < 2000.0 => ((p - q) / dt as f64).as_vec3(),
        _ => Vec3::ZERO,
    };
    state.prev_eye = eye;
    let raining = state.weather.sky == Sky::Storm && state.eye_ft < state.weather.top_ft as f32;
    let Ok((cam, cam_tf)) = eye_cams.single() else { return };
    if !raining {
        if let Some(e) = state.rain.take() {
            commands.entity(e).try_despawn();
        }
        return;
    }
    if state.drops.is_empty() {
        let drops: Vec<Vec3> = (0..DROPS)
            .map(|_| Vec3::new(state.random(), state.random(), state.random()) * 2.0 * RAIN_BOX - Vec3::splat(RAIN_BOX))
            .collect();
        state.drops = drops;
    }
    // Wind relative to the eye: rain falls at 9 m/s.
    let wind = Vec3::new(0.0, -9.0, 0.0) - eye_vel;
    let streak = wind * 0.012 + wind.normalize_or_zero() * 0.4;
    let width = 0.012;
    let mut pos = Vec::with_capacity(DROPS * 4);
    for d in state.drops.iter_mut() {
        *d += wind * dt;
        *d = (*d + Vec3::splat(RAIN_BOX)).rem_euclid(Vec3::splat(2.0 * RAIN_BOX)) - Vec3::splat(RAIN_BOX);
        let side = streak.cross(-*d).normalize_or_zero() * width * (1.0 + d.length() * 0.05);
        let (a, b) = (*d, *d - streak);
        pos.extend([a - side, a + side, b + side, b - side]);
    }
    let rot = cam_tf.compute_transform().rotation.inverse();
    match state.rain {
        Some(e) => {
            if let Ok((mut tf, mesh)) = tfs.get_mut(e) {
                tf.rotation = rot;
                if let Some(mut m) = meshes.get_mut(&mesh.0) {
                    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
                }
            }
        }
        None => {
            // Both windings: the streaks are seen from either side.
            let indices: Vec<u32> = (0..DROPS as u32)
                .flat_map(|i| {
                    let v = i * 4;
                    [v, v + 1, v + 2, v, v + 2, v + 3, v, v + 2, v + 1, v, v + 3, v + 2]
                })
                .collect();
            let mesh = Mesh::new(
                bevy::mesh::PrimitiveTopology::TriangleList,
                bevy::asset::RenderAssetUsages::default(),
            )
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
            .with_inserted_indices(bevy::mesh::Indices::U32(indices));
            let material = materials.add(PaletteMaterial {
                params: PaletteParams { word: 0xFFE4, flags: crate::palette_material::FLAG_LINE, ..default() },
                globals: palette.globals.clone(),
            });
            let e = commands
                .spawn((
                    CloudEntity,
                    Mesh3d(meshes.add(mesh)),
                    MeshMaterial3d(material),
                    MeshTag(mesh_tag(1, 0, true)),
                    Transform::from_rotation(rot),
                    bevy::camera::visibility::NoFrustumCulling,
                    bevy::camera::visibility::RenderLayers::layer(crate::camera::MAIN_BACKGROUND_LAYER),
                    bevy::light::NotShadowCaster,
                    ChildOf(cam),
                ))
                .id();
            state.rain = Some(e);
        }
    }
}

/// Storm turbulence: smooth buffeting of the player's attitude inside storm clouds (and,
/// weaker, under the deck).
fn turbulence(time: Res<Time>, state: Res<WeatherState>, mut q: Query<&mut crate::flight::Aircraft, With<crate::flight::Player>>) {
    if state.weather.sky != Sky::Storm {
        return;
    }
    let strength = if state.whiteout > 0.5 {
        1.0
    } else if state.below_deck {
        0.35
    } else {
        return;
    };
    let t = time.elapsed_secs();
    let dt = time.delta_secs();
    let s = (state.weather.seed % 97) as f32;
    let wave = |a: f32, b: f32, c: f32| (t * a + s).sin() * 0.6 + (t * b + 2.0 * s).sin() * 0.3 + (t * c + 3.0 * s).sin() * 0.1;
    let roll = 0.3 * strength * wave(1.7, 4.3, 11.0) * dt;
    let pitch = 0.12 * strength * wave(2.3, 5.9, 13.0) * dt;
    for mut a in &mut q {
        if a.on_ground || a.ended.is_some() {
            continue;
        }
        let o = a.orientation;
        a.orientation = (o * Quat::from_rotation_z(roll) * Quat::from_rotation_x(pitch)).normalize();
    }
}
