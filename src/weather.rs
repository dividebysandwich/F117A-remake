//! Weather (not in the original game): the mission's clouds (f117_mission::weather) as
//! flat-shaded geometry, the overcast sky, the whiteout inside clouds and storm effects.
//!
//! - Deck (overcast, storm): two huge planes at the deck's base and top following the
//!   viewpoint; past their edges the background sphere draws the same deck (palette.wgsl).
//! - Cumulus and cumulonimbus: faceted meshes (a few lumpy templates per kind) streamed
//!   around the viewpoint from the shared cloud field, so what is drawn is what blocks the
//!   simulation's lines of sight.
//! - Shading is done by the palette shader (`FLAG_CLOUD`): the grey ramp by face orientation,
//!   hazed per pixel with the weather's visibility.

use std::collections::HashMap;

use bevy::math::DVec3;
use bevy::mesh::{MeshTag, VertexAttributeValues};
use bevy::prelude::*;
use f117_mission::weather::{CELL, Cloud, CloudKind, STORM_CELL};
use f117_mission::{Sky, Weather};

use crate::camera::EyeCamera;
use crate::trackcam::TrackCamera;
use crate::mission::ActiveMission;
use crate::palette::Palette;
use crate::palette_material::{FLAG_CLOUD, FLAG_CLOUD_COVERAGE, FLAG_CLOUD_SOLID, PaletteMaterial, PaletteParams, mesh_tag};
use crate::settings::{AppState, SetupSet};
use crate::world::{FloatingOrigin, WU, WorldPos, wu_to_bevy};

pub struct WeatherPlugin;

impl Plugin for WeatherPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<crate::lightning::BoltMaterial>::default())
            .init_resource::<WeatherState>()
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
/// Distinct cumulus and cumulonimbus shapes.
const TEMPLATES: u32 = 8;
const TOWER_TEMPLATES: u32 = 4;
/// Horizontal period of the cloud texture (metres, `NOISE_PERIOD` in palette.wgsl).
const NOISE_PERIOD: f64 = 81920.0;

#[derive(Resource, Default)]
pub struct WeatherState {
    pub weather: Weather,
    /// Viewpoint: map units and feet.
    pub eye_map: (i32, i32),
    pub eye_ft: f32,
    /// In the shade of a cloud (under covered stratus or a storm cell).
    pub shaded: bool,
    /// Under (or in) a storm cell: rain, turbulence, close lightning.
    pub under_storm: bool,
    /// 0 clear air .. 1 inside a cloud (eased).
    pub whiteout: f32,
    /// Lightning flash brightness.
    pub flash: f32,
    next_flash: f32,
    /// The current lightning strike.
    bolt: Option<Strike>,
    /// `--strike ground|cloud` (test automation): a strike of that kind after 2 s, held at
    /// its peak.
    test_strike: Option<bool>,
    /// Seconds until the thunder of the last flash.
    thunder_in: Option<f32>,
    /// Eye position last frame (Bevy metres), for the rain's relative wind.
    prev_eye: Option<DVec3>,
    rain: Option<Entity>,
    drops: Vec<Vec3>,
    rng: u32,
    /// Cumulus and cumulonimbus (solid).
    puff_material: Handle<PaletteMaterial>,
    cumulus_templates: Vec<Handle<Mesh>>,
    tower_templates: Vec<Handle<Mesh>>,
    puffs: HashMap<CloudKey, Streamed>,
    streamed_at: Option<(i32, i32)>,
}

impl WeatherState {
    /// Haze distance scale for the world under the clouds (reduced visibility in storms).
    pub fn haze_scale(&self) -> f32 {
        if self.weather.has_storms() && (self.under_storm || self.shaded) {
            (20.0 / self.weather.vis_km.max(1) as f32).max(1.0)
        } else {
            1.0
        }
    }

    /// The lightning channel as a line light in render space: (end, intensity), (other end,
    /// reach in metres).
    pub fn bolt_light(&self, origin: DVec3) -> (Vec4, Vec4) {
        match &self.bolt {
            Some(b) if b.light > 0.0 => (
                (b.light_a - origin).as_vec3().extend(b.light),
                (b.light_b - origin).as_vec3().extend(BOLT_REACH),
            ),
            _ => (Vec4::ZERO, Vec4::ZERO),
        }
    }

    /// Shader globals: (stratus base m, stratus top m, stratus present, whiteout),
    /// (haze scale, flash, cloud visibility m, 0), the cloud texture's origin and seed, and
    /// the stratus coverage field (threshold, seed, floating origin x / z).
    pub fn globals(&self, origin: DVec3) -> (Vec4, Vec4, Vec4, Vec4) {
        let w = &self.weather;
        let l = w.stratus;
        let clouds = Vec4::new(l.base_ft as f32 * WU as f32, l.top_ft as f32 * WU as f32, l.present() as u32 as f32, self.whiteout);
        let stratus = Vec4::new(w.stratus_threshold as f32 / 65535.0, (w.seed & 0xFFFF) as f32, origin.x as f32, origin.z as f32);
        let vis_m = (w.vis_km as f32 * 1000.0).max(4000.0);
        let noise = Vec4::new(
            origin.x.rem_euclid(NOISE_PERIOD) as f32,
            origin.z.rem_euclid(NOISE_PERIOD) as f32,
            (w.seed & 0xFFFF) as f32,
            l.cover as f32 / 100.0,
        );
        (clouds, Vec4::new(self.haze_scale(), self.flash, vis_m, 0.0), noise, stratus)
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

/// A cloud of the field: its kind and grid square.
type CloudKey = (CloudKind, i32, i32);
/// A streamed cloud: its entity and centre (map units).
type Streamed = (Entity, (i32, i32));

/// Stratus plane: offset from the viewpoint is kept at zero horizontally.
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

/// Unit 0..1 from a hash.
fn unit(h: u64) -> f32 {
    (h & 0xFFFF) as f32 / 65535.0
}

/// A cumulus shape: a lumpy faceted dome with a flat base, x/z in about -1..1, y in 0..1.
fn cumulus_mesh(seed: u32) -> Mesh {
    let mut mesh = positions_only(Sphere::new(1.0).mesh().ico(1).expect("icosphere"));
    if let Some(VertexAttributeValues::Float32x3(pos)) = mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION) {
        for (i, p) in pos.iter_mut().enumerate() {
            let bump = 0.8 + 0.35 * unit(hash(((seed as u64) << 32) | i as u64));
            let v = Vec3::from(*p) * bump;
            // Flat, slightly bulging base; puffy top.
            let y = if v.y < 0.0 { v.y * 0.12 } else { v.y };
            *p = [v.x, (y + 0.12) / 1.27, v.z];
        }
    }
    mesh
}

/// Radius of a cumulonimbus at height y (0..1): a column of about 0.9 with an anvil of 1.35
/// near the top.
fn tower_radius(y: f32) -> f32 {
    let s = |a: f32, b: f32, x: f32| {
        let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    (0.9 + s(0.7, 0.86, y) * 0.45) * (1.0 - s(0.93, 1.0, y) * 0.45)
}

/// A cumulonimbus shape: a lumpy faceted column with an anvil, closed at both ends, x/z in
/// about -1.4..1.4, y in 0..1.
fn tower_mesh(seed: u32) -> Mesh {
    const SIDES: usize = 12;
    const RINGS: [f32; 11] = [0.0, 0.14, 0.28, 0.42, 0.56, 0.68, 0.78, 0.86, 0.92, 0.97, 1.0];
    let mut ring = Vec::new();
    for (r, &y) in RINGS.iter().enumerate() {
        let row: Vec<Vec3> = (0..SIDES)
            .map(|j| {
                let h = hash(((seed as u64) << 32) | (r * SIDES + j) as u64);
                let a = (j as f32 + 0.3 * (unit(h >> 32) - 0.5)) / SIDES as f32 * std::f32::consts::TAU;
                let rad = tower_radius(y) * (0.82 + 0.3 * unit(h));
                // Base and top stay level.
                let dy = if r == 0 || r == RINGS.len() - 1 { 0.0 } else { 0.025 * (unit(h >> 16) - 0.5) };
                Vec3::new(a.cos() * rad, y + dy, a.sin() * rad)
            })
            .collect();
        ring.push(row);
    }
    let top = Vec3::new(0.0, 1.0 + 0.02 * unit(hash(seed as u64)), 0.0);
    let mut tris: Vec<[Vec3; 3]> = Vec::new();
    // Sides, wound outwards from the axis.
    for r in 0..RINGS.len() - 1 {
        for j in 0..SIDES {
            let k = (j + 1) % SIDES;
            let (a, b, c, d) = (ring[r][j], ring[r][k], ring[r + 1][k], ring[r + 1][j]);
            for mut t in [[a, b, c], [a, c, d]] {
                let centre = (t[0] + t[1] + t[2]) / 3.0;
                let n = (t[1] - t[0]).cross(t[2] - t[0]);
                if n.dot(Vec3::new(centre.x, 0.0, centre.z)) < 0.0 {
                    t.swap(1, 2);
                }
                tris.push(t);
            }
        }
    }
    // Base and top caps.
    let last = RINGS.len() - 1;
    for j in 0..SIDES {
        let k = (j + 1) % SIDES;
        for (centre, a, b, up) in [(Vec3::ZERO, ring[0][j], ring[0][k], -1.0), (top, ring[last][j], ring[last][k], 1.0)] {
            let mut t = [centre, a, b];
            if (t[1] - t[0]).cross(t[2] - t[0]).y * up < 0.0 {
                t.swap(1, 2);
            }
            tris.push(t);
        }
    }
    let pos: Vec<Vec3> = tris.into_iter().flatten().collect();
    Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, bevy::asset::RenderAssetUsages::default()).with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
}

fn setup_weather(
    mut commands: Commands,
    mission: Option<Res<ActiveMission>>,
    palette: Res<Palette>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<PaletteMaterial>>,
    settings: Res<crate::settings::Settings>,
    mut state: ResMut<WeatherState>,
) {
    let weather = mission.map(|m| m.0.weather).unwrap_or_default();
    // Stratus sheets are cut out by their coverage field: alpha to coverage with modern
    // mode's MSAA, a dithered screen door otherwise.
    let coverage = if settings.modern_shading { FLAG_CLOUD_COVERAGE } else { 0 };
    let material = materials.add(PaletteMaterial {
        params: PaletteParams { word: 0xFFE0, flags: FLAG_CLOUD | coverage, ..default() },
        globals: palette.globals.clone(),
    });
    let args: Vec<String> = std::env::args().collect();
    let test_strike = args.iter().position(|a| a == "--strike").and_then(|i| args.get(i + 1)).map(|k| k == "ground");
    *state = WeatherState {
        weather,
        test_strike,
        rng: weather.seed | 1,
        next_flash: if test_strike.is_some() { 2.0 } else { 3.0 },
        puff_material: materials.add(PaletteMaterial {
            params: PaletteParams { word: 0xFFE0, flags: FLAG_CLOUD | FLAG_CLOUD_SOLID, ..default() },
            globals: palette.globals.clone(),
        }),
        cumulus_templates: (0..TEMPLATES).map(|i| meshes.add(cumulus_mesh(weather.seed ^ i))).collect(),
        tower_templates: (0..TOWER_TEMPLATES).map(|i| meshes.add(tower_mesh(weather.seed ^ (i + 0x100)))).collect(),
        ..default()
    };
    info!("weather: {}", weather.describe());
    let l = weather.stratus;
    if !l.present() {
        return;
    }
    // The base seen from below, the top seen from above.
    for (normal, alt_ft) in [(Vec3::NEG_Y, l.base_ft), (Vec3::Y, l.top_ft)] {
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
    state.shaded = w.under_cloud(map.0, map.1, ft as i32);
    state.under_storm = w.under_storm(map.0, map.1, ft as i32);
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

/// Spawns the cumulus and cumulonimbus near the viewpoint and drops the ones left behind.
fn stream_clouds(mut commands: Commands, origin: Res<FloatingOrigin>, mut state: ResMut<WeatherState>) {
    let (x, y) = state.eye_map;
    let w = state.weather;
    if let Some((sx, sy)) = state.streamed_at
        && (sx - x).abs() < CELL / 2
        && (sy - y).abs() < CELL / 2
    {
        return;
    }
    state.streamed_at = Some((x, y));
    let keep = (DRAW_RADIUS + STORM_CELL) as i64;
    state.puffs.retain(|_, (e, (cx, cy))| {
        let (dx, dy) = ((*cx - x) as i64, (*cy - y) as i64);
        let near = dx * dx + dy * dy <= keep * keep;
        if !near {
            commands.entity(*e).try_despawn();
        }
        near
    });
    let new: Vec<Cloud> = w.clouds_near(x, y, DRAW_RADIUS).filter(|c| !state.puffs.contains_key(&(c.kind, c.gx, c.gy))).collect();
    for c in new {
        let h = hash(((w.seed as u64) << 32) ^ ((c.gx as u16 as u64) << 16) ^ c.gy as u16 as u64 ^ 0xC10D);
        let templates = match c.kind {
            CloudKind::Cumulonimbus => &state.tower_templates,
            _ => &state.cumulus_templates,
        };
        let template = templates[(h % templates.len() as u64) as usize].clone();
        let r_m = c.r as f32 * 32.0 * WU as f32;
        let pos = wu_to_bevy(c.x as f64 * 32.0, (0x8000 - c.y) as f64 * 32.0, c.base_ft as f64);
        let h_m = (c.top_ft - c.base_ft) as f32 * WU as f32;
        let e = commands
            .spawn((
                CloudEntity,
                Mesh3d(template),
                MeshMaterial3d(state.puff_material.clone()),
                MeshTag(mesh_tag(1, 0, true)),
                WorldPos(pos),
                Transform::from_translation((pos - origin.origin).as_vec3())
                    .with_rotation(Quat::from_rotation_y((h >> 16 & 0xFFFF) as f32 / 65535.0 * std::f32::consts::TAU))
                    .with_scale(Vec3::new(r_m, h_m, r_m)),
                bevy::light::NotShadowCaster,
            ))
            .id();
        state.puffs.insert((c.kind, c.gx, c.gy), (e, (c.x, c.y)));
    }
}

/// Reach of a strike's light (metres from the channel).
const BOLT_REACH: f32 = 10000.0;
/// Strikes happen in storm cells this far around the viewpoint (map units).
const STRIKE_RADIUS: i32 = 45 * 64;

/// One lightning strike: its mesh and stroke timeline.
struct Strike {
    entity: Entity,
    /// Seconds since the strike began.
    age: f32,
    /// Return strokes: start time and duration (s).
    strokes: Vec<(f32, f32)>,
    /// Cloud-to-ground (with a continuing current between strokes) or in-cloud.
    ground: bool,
    /// Light of the channel (world metres) and its current intensity.
    light_a: DVec3,
    light_b: DVec3,
    light: f32,
    /// Distance to the viewpoint (m), for the far-reaching flash and the thunder.
    distance: f32,
}

impl Strike {
    fn end(&self) -> f32 {
        self.strokes.last().map_or(0.0, |&(t, d)| t + d + 0.15)
    }

    /// Brightness 0..1 at the strike's age: each return stroke flares and decays; a
    /// cloud-to-ground channel glows faintly between them.
    fn intensity(&self) -> f32 {
        let mut v: f32 = 0.0;
        for &(t, d) in &self.strokes {
            let x = self.age - t;
            if (0.0..d + 0.12).contains(&x) {
                v = v.max(if x < d { 1.0 - 0.4 * x / d } else { 0.6 * (1.0 - (x - d) / 0.12) });
            }
        }
        if self.ground && self.age < self.end() - 0.15 {
            v = v.max(0.12);
        }
        v
    }
}

/// A lightning channel from `a` to `b`: recursive midpoint displacement (the jagged
/// tortuous path) with forked side branches. Returns line segments with their widths.
fn bolt_path(state: &mut WeatherState, a: Vec3, b: Vec3, ground: bool, out: &mut Vec<(Vec3, Vec3, f32)>, width: f32, depth: u32) {
    let mut pts = vec![a, b];
    let mut rough = 0.22;
    for _ in 0..7 {
        let mut next = Vec::with_capacity(pts.len() * 2);
        for w in pts.windows(2) {
            let (p, q) = (w[0], w[1]);
            let len = p.distance(q);
            let off = Vec3::new(state.random() - 0.5, (state.random() - 0.5) * if ground { 0.3 } else { 0.6 }, state.random() - 0.5) * 2.0 * len * rough;
            next.push(p);
            next.push((p + q) * 0.5 + off);
        }
        next.push(*pts.last().unwrap());
        pts = next;
        rough *= 0.62;
    }
    for w in pts.windows(2) {
        out.push((w[0], w[1], width));
    }
    if depth == 0 {
        return;
    }
    // Branches fork off downwards (cloud-to-ground) or sideways (in-cloud, "spider").
    let n = if ground { 3 + (state.random() * 4.0) as usize } else { 4 + (state.random() * 5.0) as usize };
    let span = a.distance(b);
    for _ in 0..n {
        let i = (state.random() * (pts.len() - 1) as f32 * if ground { 0.8 } else { 1.0 }) as usize;
        let start = pts[i];
        let main = (b - a).normalize_or_zero();
        let side = Vec3::new(state.random() - 0.5, 0.0, state.random() - 0.5).normalize_or_zero();
        let dir = if ground { (main * 0.7 + side * 0.6).normalize_or_zero() } else { (side + Vec3::Y * (state.random() - 0.5) * 0.3).normalize_or_zero() };
        let len = span * (0.12 + 0.25 * state.random());
        bolt_path(state, start, start + dir * len, ground, out, width * 0.5, depth - 1);
    }
}

/// Storms: lightning strikes in the storm cells around the viewpoint, cloud-to-ground or
/// in-cloud, each a generated channel mesh with a few return strokes. The channel lights
/// the ground and the clouds around it, a strike's flash reaches far (weaker with distance),
/// and its thunder arrives at the speed of sound.
#[allow(clippy::too_many_arguments)]
fn lightning(
    mut commands: Commands,
    time: Res<Time>,
    origin: Res<FloatingOrigin>,
    eyes: Eyes,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<crate::lightning::BoltMaterial>>,
    mut bolts: Query<&MeshMaterial3d<crate::lightning::BoltMaterial>>,
    mut sounds: MessageWriter<crate::sound::PlaySound>,
    mut state: ResMut<WeatherState>,
) {
    if !state.weather.has_storms() {
        state.flash = 0.0;
        return;
    }
    let dt = time.delta_secs();
    let Some(eye) = eye_position(&eyes, &origin) else { return };
    if let Some(t) = state.thunder_in.as_mut() {
        *t -= dt;
        if *t <= 0.0 {
            state.thunder_in = None;
            // No thunder among the original sounds: the explosion rumble stands in.
            sounds.write(crate::sound::PlaySound { id: 5, priority: 0 });
        }
    }
    // The running strike.
    if let Some(mut b) = state.bolt.take() {
        b.age = if state.test_strike.is_some() { 0.01 } else { b.age + dt };
        b.light = b.intensity();
        if let Ok(m) = bolts.get_mut(b.entity)
            && let Some(mut m) = materials.get_mut(&m.0)
        {
            m.params.x = b.light;
        }
        // Far-reaching: the sky and the whole scene brighten, less for distant strikes.
        state.flash = b.light * (0.05 + 0.3 / (1.0 + (b.distance / 6000.0).powi(2)));
        if b.age > b.end() {
            commands.entity(b.entity).try_despawn();
        } else {
            state.bolt = Some(b);
        }
        return;
    }
    state.flash = 0.0;
    state.next_flash -= dt;
    if state.next_flash > 0.0 {
        return;
    }
    state.next_flash = if state.under_storm { 2.5 + 7.0 * state.random() } else { 5.0 + 15.0 * state.random() };
    // A storm cell near the viewpoint.
    let w = state.weather;
    let (ex, ey) = state.eye_map;
    let cells: Vec<Cloud> = w.clouds_near(ex, ey, STRIKE_RADIUS).filter(|c| c.kind == CloudKind::Cumulonimbus).collect();
    if cells.is_empty() {
        return;
    }
    let ground = state.test_strike.unwrap_or_else(|| state.random() < 0.4);
    let c = if state.test_strike == Some(false)
        && let Some((tf, _)) = eyes.iter().max_by_key(|(_, e)| *e)
    {
        // Test in-cloud strikes: the cell most straight ahead of the view.
        let f = tf.forward();
        let ahead = |c: &Cloud| {
            let p = wu_to_bevy(c.x as f64 * 32.0, (0x8000 - c.y) as f64 * 32.0, eye.y / WU) - eye;
            (p.as_vec3().normalize_or_zero().dot(*f) * 1000.0) as i32
        };
        *cells.iter().max_by_key(|c| ahead(c)).unwrap()
    } else {
        cells[(state.random() * cells.len() as f32) as usize % cells.len()]
    };
    let ft_m = WU as f32;
    let mut centre = wu_to_bevy(c.x as f64 * 32.0, (0x8000 - c.y) as f64 * 32.0, 0.0);
    if state.test_strike == Some(true)
        && let Some((tf, _)) = eyes.iter().max_by_key(|(_, e)| *e)
    {
        // Test strikes: 8 km ahead of the view.
        let f = tf.forward().as_vec3().with_y(0.0).normalize_or_zero().as_dvec3();
        centre = DVec3::new(eye.x, 0.0, eye.z) + f * 8000.0;
    }
    let r_m = c.r as f32 * 32.0 * ft_m;
    let at = |k: f32, alt_ft: f32, st: &mut WeatherState| {
        let a = st.random() * std::f32::consts::TAU;
        let d = r_m * k * st.random().sqrt();
        Vec3::new(centre.x as f32 + a.cos() * d, alt_ft * ft_m, centre.z as f32 + a.sin() * d)
    };
    let base = c.base_ft as f32;
    let (a, b) = if ground {
        // From the cloud base down to the ground below it.
        let top = at(0.6, base + 1500.0 + 3000.0 * state.random(), &mut state);
        let foot = top + Vec3::new((state.random() - 0.5) * 3000.0, 0.0, (state.random() - 0.5) * 3000.0);
        (top, Vec3::new(foot.x, 0.0, foot.z))
    } else {
        // Inside the cell: a long, mostly horizontal discharge.
        let h = base + (c.top_ft as f32 - base) * (0.15 + 0.4 * state.random());
        (at(0.7, h, &mut state), at(0.7, h + (state.random() - 0.5) * 6000.0, &mut state))
    };
    let mut segs = Vec::new();
    bolt_path(&mut state, a, b, ground, &mut segs, if ground { 3.0 } else { 2.0 }, 2);
    let eye_f = eye.as_vec3();
    let main_width = if ground { 9.0 } else { 6.0 };
    let mesh = meshes.add(crate::lightning::bolt_mesh(&segs, a, main_width));
    let material = materials.add(crate::lightning::BoltMaterial {
        params: Vec4::new(0.0, crate::lightning::MIN_HALF_ANGLE, 0.0, 0.0),
    });
    let world_a = a.as_dvec3();
    let entity = commands
        .spawn((
            CloudEntity,
            Mesh3d(mesh),
            MeshMaterial3d(material),
            WorldPos(world_a),
            Transform::from_translation((world_a - origin.origin).as_vec3()),
            bevy::camera::visibility::NoFrustumCulling,
            bevy::light::NotShadowCaster,
        ))
        .id();
    // 1-4 return strokes (cloud-to-ground), in-cloud discharges flicker once or twice.
    let n = if ground { 1 + (state.random() * 4.0) as usize } else { 1 + (state.random() * 2.0) as usize };
    let mut t = 0.0;
    let mut strokes = Vec::new();
    for _ in 0..n {
        let d = 0.04 + 0.04 * state.random();
        strokes.push((t, d));
        t += d + 0.05 + 0.12 * state.random();
    }
    let mid = (a + b) * 0.5;
    let distance = mid.distance(eye_f);
    // Thunder at the speed of sound (inaudible beyond ~25 km).
    if distance < 25_000.0 {
        state.thunder_in = Some(distance / 343.0);
    }
    state.bolt = Some(Strike {
        entity,
        age: 0.0,
        strokes,
        ground,
        // In-cloud: the channel itself; cloud-to-ground: from the ground up to the base.
        light_a: a.as_dvec3(),
        light_b: b.as_dvec3(),
        light: 0.0,
        distance,
    });
}

/// Rain streaks around the eye (under storm cells, and under the storm preset's stratus).
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
    let w = state.weather;
    let raining = state.under_storm || (w.sky == Sky::Storm && state.shaded);
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

/// Storm turbulence: smooth buffeting of the player's attitude inside storm cells (and,
/// weaker, under them).
fn turbulence(time: Res<Time>, state: Res<WeatherState>, mut q: Query<&mut crate::flight::Aircraft, With<crate::flight::Player>>) {
    if !state.under_storm {
        return;
    }
    // Inside the cell's cloud, or (weaker) under it.
    let strength = if state.whiteout > 0.5 { 1.0 } else { 0.35 };
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
