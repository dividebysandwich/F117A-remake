//! Weather: the mission's clouds (stealth_mission::weather) as
//! flat-shaded geometry, the overcast sky, the whiteout inside clouds and storm effects.
//!
//! - Deck (overcast, storm): two huge planes at the deck's base and top following the
//!   viewpoint; past their edges the background sphere draws the same deck (palette.wgsl).
//! - Cumulus and cumulonimbus: volumetric clouds (bevy_fast_clouds, a union of spheres
//!   carved by wavelet noise) streamed around the viewpoint from the shared cloud field, so
//!   what is drawn is what blocks the simulation's lines of sight. They are shaded down the
//!   palette's grey cloud ramp, so they follow the time of day; classic mode dithers them.
//! - The deck is shaded by the palette shader (`FLAG_CLOUD`): the grey ramp by face
//!   orientation, hazed per pixel with the weather's visibility.

use std::collections::HashMap;

use bevy::math::DVec3;
use bevy::mesh::MeshTag;
use bevy::prelude::*;
use bevy_fast_clouds::{
    CloudRenderSettings, CloudShading, CloudSphere, CloudSystems, CloudVolume, FastCloudsPlugin,
    LodPolicy, WaveletVolume,
};
use stealth_campaign::weather::{FADE, Forecast, Now, cloud_fade};
use stealth_mission::Weather;
use stealth_mission::weather::{CELL, Cloud, CloudKind, Layer, STORM_CELL};

use crate::camera::EyeCamera;
use crate::camera::OffscreenCamera;
use crate::mission::ActiveMission;
use crate::palette::Palette;
use crate::palette_material::{
    FLAG_CLOUD, FLAG_CLOUD_COVERAGE, PaletteMaterial, PaletteParams, mesh_tag,
};
use crate::settings::{AppState, SetupSet};
use crate::world::{FloatingOrigin, WU, WorldPos, wu_to_bevy};

pub struct WeatherPlugin;

impl Plugin for WeatherPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            MaterialPlugin::<crate::lightning::BoltMaterial>::default(),
            FastCloudsPlugin,
        ))
        .init_resource::<WeatherState>()
        .insert_resource(lod_policy())
        .add_systems(OnEnter(AppState::Flight), setup_weather.in_set(SetupSet::Spawn))
        .add_systems(OnExit(AppState::Flight), teardown_weather)
        .add_systems(
            Update,
            turbulence
                .after(crate::flight::FlightSystems)
                .before(crate::camera::RigSystems)
                .run_if(in_state(AppState::Flight)),
        )
        .add_systems(
            PostUpdate,
            (update_weather, track_eye, follow_deck, stream_clouds, fade_clouds, lightning, rain)
                .chain()
                .after(bevy::transform::TransformSystems::Propagate)
                .before(crate::lighting::LightingSystems)
                .run_if(in_state(AppState::Flight)),
        )
        .add_systems(
            PostUpdate,
            shade_clouds
                .after(crate::lighting::LightingSystems)
                .before(CloudSystems)
                .run_if(in_state(AppState::Flight)),
        );
    }
}

/// Cumulus are drawn this far around the viewpoint (map units, 64 per km).
const DRAW_RADIUS: i32 = 40 * 64;
/// Size of the deck planes (metres from the viewpoint to their edge).
const DECK_HALF: f32 = 280_000.0;
/// Horizontal period of the cloud texture (metres, `NOISE_PERIOD` in palette.wgsl).
const NOISE_PERIOD: f64 = 81920.0;

#[derive(Resource, Default)]
pub struct WeatherState {
    /// The clouds the lines of sight see (with changing weather, those at least half built up).
    pub weather: Weather,
    /// The dynamic campaign's changing weather.
    forecast: Option<Forecast>,
    /// Its weather now and what comes next.
    pub now: Option<Now>,
    /// Drawn coverage of the cumulus, stratus and cumulonimbus layers (percent, unrounded).
    cover: [f32; 3],
    /// Stratus field threshold 0..1 of the drawn deck.
    stratus_threshold: f32,
    /// Storm cells built up, 0..1.
    storm: f32,
    vis_km: f32,
    /// Viewpoint: map units and feet.
    pub eye_map: (i32, i32),
    pub eye_ft: f32,
    /// In the shade of a cloud (under covered stratus or a storm cell), 0..1, eased.
    pub shade: f32,
    /// How grey and dim the day is under the clouds, 0..1, eased.
    gloom: f32,
    /// Under (or in) a storm cell: rain, turbulence, close lightning.
    pub under_storm: bool,
    /// 0 clear air .. 1 inside a cloud (eased).
    pub whiteout: f32,
    /// Lightning flash brightness.
    pub flash: f32,
    /// `--strike`: seconds until the test strike.
    next_flash: f32,
    /// The running lightning strikes.
    bolts: Vec<Strike>,
    /// The simulation frame lightning last looked at (the host's on a coop client).
    strike_step: Option<u32>,
    /// `--strike ground|cloud` (test automation): a strike of that kind after 2 s, held at
    /// its peak.
    test_strike: Option<bool>,
    /// Seconds until the thunder of each strike.
    thunder: Vec<f32>,
    /// Eye position last frame (Bevy metres), for the rain's relative wind.
    prev_eye: Option<DVec3>,
    rain: Option<Entity>,
    drops: Vec<Vec3>,
    rng: Rng,
    /// Cumulus and cumulonimbus volumes.
    puffs: HashMap<CloudKey, Streamed>,
    /// Where and with which layers the clouds were last streamed.
    streamed: Option<((i32, i32), Layer, Layer)>,
}

impl WeatherState {
    /// Haze distance scale for the world under the clouds (reduced visibility in storms).
    pub fn haze_scale(&self) -> f32 {
        let under = if self.under_storm { 1.0 } else { self.shade };
        1.0 + ((20.0 / self.vis_km.max(1.0)).max(1.0) - 1.0) * self.storm * under
    }

    /// The weather of a moment of the forecast.
    fn set(&mut self, now: Now) {
        self.weather = now.weather;
        self.cover = now.cover;
        self.stratus_threshold = now.stratus_threshold;
        self.storm = now.storm;
        self.vis_km = now.vis_km;
    }

    /// Drawn opacity of a cumulus or cumulonimbus (fixed weather: all of them).
    fn fade(&self, kind: CloudKind, rank: u8) -> f32 {
        match (&self.forecast, kind) {
            (None, _) => 1.0,
            (_, CloudKind::Cumulonimbus) => cloud_fade(self.cover[2], rank),
            _ => cloud_fade(self.cover[0], rank),
        }
    }

    /// The lightning channel as a line light in render space: (end, intensity), (other end,
    /// reach in metres).
    pub fn bolt_light(&self, origin: DVec3) -> (Vec4, Vec4) {
        let brightest = self.bolts.iter().max_by(|a, b| a.light.total_cmp(&b.light));
        match brightest {
            Some(b) if b.light > 0.0 => (
                (b.light_a - origin).as_vec3().extend(b.light),
                (b.light_b - origin).as_vec3().extend(BOLT_REACH),
            ),
            _ => (Vec4::ZERO, Vec4::ZERO),
        }
    }

    /// Shader globals: (stratus base m, stratus top m, stratus present, whiteout),
    /// (haze scale, flash, cloud visibility m, gloom), the cloud texture's origin and seed, and
    /// the stratus coverage field (threshold, seed, floating origin x / z).
    pub fn globals(&self, origin: DVec3) -> (Vec4, Vec4, Vec4, Vec4) {
        let w = &self.weather;
        let l = w.stratus;
        let clouds = Vec4::new(
            l.base_ft as f32 * WU as f32,
            l.top_ft as f32 * WU as f32,
            (self.cover[1] > 0.0) as u32 as f32,
            self.whiteout,
        );
        let stratus = Vec4::new(
            self.stratus_threshold,
            (w.layer_seed(CloudKind::Stratus) & 0xFFFF) as f32,
            origin.x as f32,
            origin.z as f32,
        );
        let vis_m = (self.vis_km * 1000.0).max(4000.0);
        let noise = Vec4::new(
            origin.x.rem_euclid(NOISE_PERIOD) as f32,
            origin.z.rem_euclid(NOISE_PERIOD) as f32,
            (w.layer_seed(CloudKind::Stratus) & 0xFFFF) as f32,
            self.cover[1] / 100.0,
        );
        (clouds, Vec4::new(self.haze_scale(), self.flash, vis_m, self.gloom), noise, stratus)
    }
}

/// Xorshift random numbers.
#[derive(Default)]
struct Rng(u32);

impl Rng {
    /// 0..1.
    fn random(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 >> 8) as f32 / (1u32 << 24) as f32
    }
}

#[derive(Component)]
struct CloudEntity;

/// A cloud of the field: its kind, layer seed and grid square.
type CloudKey = (CloudKind, u32, i32, i32);
/// A streamed cloud: its entity, centre (map units) and rank.
type Streamed = (Entity, (i32, i32), u8);

/// Stratus plane at the layer's base or top: offset from the viewpoint is kept at zero
/// horizontally.
#[derive(Component)]
struct DeckPlane {
    top: bool,
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

/// The spheres of a cumulus filling the simulation's cylinder (radius `r`, height `h`,
/// metres, base centre at the origin): a ring of puffs around a few inner ones and a crown.
/// The volume is cut flat at the base.
fn cumulus_spheres(r: f32, h: f32, seed: u64) -> Vec<CloudSphere> {
    let s = (0.6 * h).clamp(0.3 * r, 0.55 * r);
    let mut out = Vec::new();
    let mut k = 0u64;
    let mut jitter = || {
        k += 1;
        unit(hash(seed ^ k.wrapping_mul(0x9E37_79B9)))
    };
    // (count, distance from the axis, height of the centre, radius), all over s or r.
    for (n, dist, y, rad) in [(6, 0.68, 0.5, 0.85), (4, 0.3, 0.75, 1.0), (2, 0.12, 1.0, 1.05)] {
        let turn = jitter() * std::f32::consts::TAU;
        for i in 0..n {
            let a = turn + (i as f32 + 0.35 * (jitter() - 0.5)) / n as f32 * std::f32::consts::TAU;
            let d = dist * r * (0.85 + 0.3 * jitter());
            let top = (h - s * rad).max(0.0);
            let cy = if y >= 1.0 { top.max(0.6 * s) } else { y * s + 0.25 * s * jitter() };
            out.push(CloudSphere {
                center: Vec3::new(a.cos() * d, cy, a.sin() * d),
                radius: s * rad * (0.85 + 0.3 * jitter()),
            });
        }
    }
    out
}

/// The spheres of a cumulonimbus (radius `r`, height `h`): a massive column of big overlapping
/// puffs, staggered around the axis, under an anvil of big puffs spreading past the column.
fn tower_spheres(r: f32, h: f32, seed: u64) -> Vec<CloudSphere> {
    let jitter = |i: u64| unit(hash(seed ^ i.wrapping_mul(0x9E37_79B9_7F4A_7C15)));
    let col = 0.95 * r;
    let anvil = 0.6 * r;
    let anvil_y = (h - 0.7 * anvil).max(col);
    // Puffs every half radius up the column (up to 5), narrowing a little with height.
    let stack = (((anvil_y - 0.4 * col) / (0.5 * col)).ceil() as usize).clamp(1, 5);
    let mut out = Vec::new();
    let turn = jitter(1) * std::f32::consts::TAU;
    for i in 0..stack {
        let f = i as f32 / (stack - 1).max(1) as f32;
        let rad = col * (1.0 - 0.2 * f) * (0.9 + 0.2 * jitter(10 + i as u64));
        let a = turn + i as f32 * 2.4;
        let off = 0.3 * r * (0.5 + 0.5 * jitter(20 + i as u64));
        let y = 0.6 * col + f * (anvil_y - 0.6 * col - 0.3 * anvil);
        out.push(CloudSphere { center: Vec3::new(a.cos() * off, y, a.sin() * off), radius: rad });
    }
    // Anvil: one over the column and a ring spreading out to about 1.5 r.
    out.push(CloudSphere { center: Vec3::new(0.0, anvil_y, 0.0), radius: anvil * 1.25 });
    let ring = 12 - out.len();
    for i in 0..ring {
        let a = turn
            + (i as f32 + 0.3 * (jitter(30 + i as u64) - 0.5)) / ring as f32
                * std::f32::consts::TAU;
        let d = 0.85 * r * (0.85 + 0.3 * jitter(40 + i as u64));
        out.push(CloudSphere {
            center: Vec3::new(a.cos() * d, anvil_y + 0.15 * anvil, a.sin() * d),
            radius: anvil * (0.85 + 0.3 * jitter(50 + i as u64)),
        });
    }
    out
}

/// How much darker storm cells are drawn (fraction of their brightness taken away).
const STORM_DARKEN: f32 = 0.5;

/// LOD by the size clouds appear on screen (`SM89_CLOUD_LOD_PX`: pixels a cloud must cover
/// for full detail).
fn lod_policy() -> LodPolicy {
    let px = std::env::var("SM89_CLOUD_LOD_PX").ok().and_then(|v| v.parse().ok());
    LodPolicy { use_projected_size: true, pixel_ref: px.unwrap_or(500.0), ..default() }
}

#[allow(clippy::too_many_arguments)]
fn setup_weather(
    mut commands: Commands,
    mission: Option<Res<ActiveMission>>,
    campaign: Option<Res<crate::campaign::ActiveCampaign>>,
    palette: Res<Palette>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<PaletteMaterial>>,
    settings: Res<crate::settings::Settings>,
    mut state: ResMut<WeatherState>,
) {
    let weather = mission.map(|m| m.0.weather).unwrap_or_default();
    // Stratus sheets are cut out by their coverage field: alpha to coverage with MSAA, a
    // dithered screen door otherwise.
    let coverage = if settings.msaa { FLAG_CLOUD_COVERAGE } else { 0 };
    let material = materials.add(PaletteMaterial {
        params: PaletteParams { word: 0xFFE0, flags: FLAG_CLOUD | coverage, ..default() },
        globals: palette.globals.clone(),
    });
    let args: Vec<String> = std::env::args().collect();
    let test_strike = args
        .iter()
        .position(|a| a == "--strike")
        .and_then(|i| args.get(i + 1))
        .map(|k| k == "ground");
    let l = weather.stratus;
    *state = WeatherState {
        weather,
        forecast: campaign.and_then(|c| Forecast::new(&c.state().setup)),
        cover: [weather.cumulus, l, weather.cumulonimbus].map(|l| l.cover as f32),
        stratus_threshold: weather.stratus_threshold as f32 / 65535.0,
        storm: weather.has_storms() as u32 as f32,
        vis_km: weather.vis_km as f32,
        test_strike,
        rng: Rng(weather.seed | 1),
        next_flash: if test_strike.is_some() { 2.0 } else { 3.0 },
        ..default()
    };
    if !l.present() && state.forecast.is_none() {
        info!("weather: {}", weather.describe());
        return;
    }
    // The base seen from below, the top seen from above.
    for (normal, top) in [(Vec3::NEG_Y, false), (Vec3::Y, true)] {
        let alt_ft = if top { l.top_ft } else { l.base_ft };
        commands.spawn((
            CloudEntity,
            DeckPlane { top },
            Mesh3d(
                meshes.add(positions_only(
                    Plane3d::new(normal, Vec2::splat(DECK_HALF)).mesh().build(),
                )),
            ),
            MeshMaterial3d(material.clone()),
            MeshTag(mesh_tag(1, 0, true)),
            WorldPos(DVec3::new(0.0, alt_ft as f64 * WU, 0.0)),
            Transform::default(),
            bevy::camera::visibility::NoFrustumCulling,
            bevy::light::NotShadowCaster,
        ));
    }
}

fn teardown_weather(
    mut commands: Commands,
    q: Query<Entity, With<CloudEntity>>,
    mut state: ResMut<WeatherState>,
) {
    for e in &q {
        commands.entity(e).try_despawn();
    }
    *state = WeatherState::default();
}

/// The main viewpoint: the eye camera (XR root in VR), else the free camera.
type Eyes<'w, 's> = Query<
    'w,
    's,
    (&'static GlobalTransform, Has<EyeCamera>),
    (Or<(With<EyeCamera>, With<Camera3d>)>, Without<OffscreenCamera>),
>;

fn eye_position(eyes: &Eyes, origin: &FloatingOrigin) -> Option<DVec3> {
    let (tf, _) = eyes.iter().max_by_key(|(_, eye)| *eye)?;
    Some(tf.translation().as_dvec3() + origin.origin)
}

/// The dynamic campaign's weather now (from the campaign clock, the same on every computer
/// of a session); a coop client's mirror simulation gets it too.
fn update_weather(
    campaign: Option<Res<crate::campaign::ActiveCampaign>>,
    combat: Option<ResMut<crate::combat::Combat>>,
    mut state: ResMut<WeatherState>,
) {
    let Some(c) = campaign else { return };
    let before = state.weather.sky;
    let Some(f) = state.forecast.as_mut() else { return };
    let now = f.at(c.state().clock);
    state.set(now);
    state.now = Some(now);
    if now.weather.sky != before {
        info!("weather: {}", now.weather.describe());
    }
    if c.role == crate::campaign::Role::Client
        && let Some(mut combat) = combat
    {
        combat.sim.set_weather(now.weather);
    }
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Viewpoint position, whiteout, shade and deck state.
fn track_eye(
    time: Res<Time>,
    origin: Res<FloatingOrigin>,
    eyes: Eyes,
    mut state: ResMut<WeatherState>,
) {
    let Some(p) = eye_position(&eyes, &origin) else { return };
    let (east, north, alt) = crate::world::bevy_to_wu(p);
    let map = ((east / 32.0) as i32, (0x8000 as f64 - north / 32.0) as i32);
    let ft = alt as f32;
    let w = state.weather;
    let inside = w.in_cloud(map.0, map.1, ft as i32);
    let dt = time.delta_secs();
    let k = (dt * 3.0).min(1.0);
    state.whiteout += ((inside as u32 as f32) - state.whiteout) * k;
    state.eye_map = map;
    state.eye_ft = ft;
    state.under_storm = w.under_storm(map.0, map.1, ft as i32);
    // Under the deck where the drawn sheet covers the sun (palette.wgsl `stratus_cover`).
    let below = state.cover[1] > 0.0 && ft < w.stratus.base_ft as f32;
    let deck = if below {
        let thr = state.stratus_threshold;
        smoothstep(thr, thr + 0.035, w.stratus_field(map.0 as f32, map.1 as f32))
    } else {
        0.0
    };
    let shade = if state.under_storm { 1.0 } else { deck };
    state.shade += (shade - state.shade) * (dt * 0.5).min(1.0);
    // Grey days: under a deck as far as it covers the sky, and in storms.
    let overcast = if ft < w.stratus.top_ft as f32 { state.cover[1] / 100.0 } else { 0.0 };
    let gloom = (0.8 * overcast + 0.6 * state.storm).min(1.0);
    state.gloom += (gloom - state.gloom) * (dt * 0.3).min(1.0);
}

fn follow_deck(
    origin: Res<FloatingOrigin>,
    eyes: Eyes,
    state: Res<WeatherState>,
    mut decks: Query<(&DeckPlane, &mut WorldPos)>,
) {
    let Some(p) = eye_position(&eyes, &origin) else { return };
    let l = state.weather.stratus;
    for (deck, mut pos) in &mut decks {
        let alt_ft = if deck.top { l.top_ft } else { l.base_ft };
        let want = DVec3::new(p.x.round(), alt_ft as f64 * WU, p.z.round());
        if pos.0 != want {
            pos.0 = want;
        }
    }
}

/// Spawns the cumulus and cumulonimbus near the viewpoint and drops the ones left behind.
fn stream_clouds(
    mut commands: Commands,
    origin: Res<FloatingOrigin>,
    mut volumes: ResMut<Assets<WaveletVolume>>,
    mut state: ResMut<WeatherState>,
) {
    if !clouds_enabled() {
        return;
    }
    let (x, y) = state.eye_map;
    // Changing weather: the clouds still or already faintly there are drawn too.
    let mut w = state.weather;
    if state.forecast.is_some() {
        for (l, c) in [(&mut w.cumulus, state.cover[0]), (&mut w.cumulonimbus, state.cover[2])] {
            l.cover = (c * (100.0 + FADE) / 100.0).ceil().clamp(0.0, 100.0) as u8;
        }
    }
    if let Some(((sx, sy), cu, cb)) = state.streamed
        && (sx - x).abs() < CELL / 2
        && (sy - y).abs() < CELL / 2
        && cu == w.cumulus
        && cb == w.cumulonimbus
    {
        return;
    }
    state.streamed = Some(((x, y), w.cumulus, w.cumulonimbus));
    let keep = (DRAW_RADIUS + STORM_CELL) as i64;
    state.puffs.retain(|&(kind, seed, gx, gy), (e, (cx, cy), _)| {
        let (dx, dy) = ((*cx - x) as i64, (*cy - y) as i64);
        let near = dx * dx + dy * dy <= keep * keep;
        let still = w.layer_seed(kind) == seed && w.cloud(kind, gx, gy).is_some();
        if !(near && still) {
            commands.entity(*e).try_despawn();
        }
        near && still
    });
    let new: Vec<Cloud> = w
        .clouds_near(x, y, DRAW_RADIUS)
        .filter(|c| !state.puffs.contains_key(&(c.kind, w.layer_seed(c.kind), c.gx, c.gy)))
        .collect();
    for c in new {
        let h = hash(
            ((w.layer_seed(c.kind) as u64) << 32)
                ^ ((c.gx as u16 as u64) << 16)
                ^ c.gy as u16 as u64
                ^ 0xC10D,
        );
        let r_m = c.r as f32 * 32.0 * WU as f32;
        let pos = wu_to_bevy(c.x as f64 * 32.0, (0x8000 - c.y) as f64 * 32.0, c.base_ft as f64);
        let h_m = (c.top_ft - c.base_ft) as f32 * WU as f32;
        let spheres = match c.kind {
            CloudKind::Cumulonimbus => tower_spheres(r_m, h_m, h),
            _ => cumulus_spheres(r_m, h_m, h),
        };
        let volume = WaveletVolume::from_spheres(spheres, h as u32, 3, 4, 0.72).with_floor(0.0);
        let tf = Transform::from_translation((pos - origin.origin).as_vec3());
        let e = commands
            .spawn((
                CloudEntity,
                CloudVolume::new(volumes.add(volume))
                    .with_core(1.0)
                    .with_fill(state.fade(c.kind, c.rank))
                    .with_darken(darken(&state, c.kind)),
                WorldPos(pos),
                tf,
                // Drawn this frame: transforms were already propagated.
                GlobalTransform::from(tf),
                Visibility::default(),
            ))
            .id();
        state.puffs.insert((c.kind, w.layer_seed(c.kind), c.gx, c.gy), (e, (c.x, c.y), c.rank));
    }
}

/// Storm cells are dark, and so is everything under a storm sky.
fn darken(state: &WeatherState, kind: CloudKind) -> f32 {
    match kind {
        CloudKind::Cumulonimbus => STORM_DARKEN,
        _ => STORM_DARKEN / 2.0 * state.storm,
    }
}

/// Clouds fade in and out as their layer's coverage changes.
fn fade_clouds(state: Res<WeatherState>, mut volumes: Query<&mut CloudVolume>) {
    if state.forecast.is_none() {
        return;
    }
    for (&(kind, ..), &(e, _, rank)) in &state.puffs {
        let Ok(mut v) = volumes.get_mut(e) else { continue };
        let (fill, dark) = (state.fade(kind, rank), darken(&state, kind));
        if v.fill != fill || v.darken != dark {
            v.fill = fill;
            v.darken = dark;
        }
    }
}

/// `SM89_CLOUDS=0`: no cumulus or cumulonimbus drawn (measuring their cost).
fn clouds_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("SM89_CLOUDS").map_or(true, |v| v != "0"))
}

/// Lightning colour on the clouds (`BOLT_COLOR` in palette.wgsl).
const BOLT_COLOR: Vec3 = Vec3::new(0.55, 0.58, 0.75);
/// The palette's grey cloud ramp (`CLOUD_RAMP` in palette.wgsl).
const CLOUD_RAMP: u8 = 0xE0;
/// Upper sky band (`band` in palette.wgsl): what night clouds are lit like.
const NIGHT_SKY: u8 = 0x8B;

/// The volumetric clouds' light and colours, from the same globals as the palette shader:
/// the live palette's cloud ramp (it changes with the time of day), the sun, the haze, the
/// lightning; in classic mode whole ramp colours, dithered. For measuring: `SM89_CLOUD_STEPS`
/// (march steps), `SM89_CLOUD_JCAP` (finest wavelet scale 0..3), `SM89_CLOUD_SHADOW=0|1`,
/// `SM89_CLOUD_SCALE` (view pixels per marched pixel).
#[allow(clippy::too_many_arguments)]
fn shade_clouds(
    palette: Res<Palette>,
    lighting: Res<crate::lighting::LightingState>,
    settings: Res<crate::settings::Settings>,
    hud: Option<Res<crate::hud::HudState>>,
    origin: Res<FloatingOrigin>,
    state: Res<WeatherState>,
    mut shading: ResMut<CloudShading>,
    mut render: ResMut<CloudRenderSettings>,
    sky: Res<crate::sky::Sky>,
) {
    let nvg = hud.is_some_and(|h| h.nvg);
    // A night without moonlight blacks the clouds out too (but for the goggles).
    let lit = if nvg || !sky.on { 1.0 } else { 1.0 - sky.dark };
    let tint = |c: Vec3| if nvg { nvg_color(c, lighting.daylight) } else { c };
    let flash_scale = 1.0 + (0.3 - 1.0) * lighting.daylight.clamp(0.0, 1.0);
    let (line_a, line_b) = state.bolt_light(origin.origin);
    let linear = |i: u8| {
        let c = palette.color(i).to_linear();
        Vec3::new(c.red, c.green, c.blue)
    };
    // The night palette's cloud ramp is all but black: by night the clouds are lit by the
    // moon and the sky instead, tops brighter than the sky behind them, bases darker.
    let night = 1.0 - lighting.daylight.clamp(0.0, 1.0);
    let sky = linear(NIGHT_SKY);
    // Moonlight is greyer than the sky.
    let sky = sky.lerp(Vec3::splat(sky.dot(Vec3::new(0.2126, 0.7152, 0.0722))), 0.5);
    *shading = CloudShading {
        sun_dir: lighting.sun.truncate(),
        ramp: std::array::from_fn(|i| {
            let k = i as f32 / 15.0;
            let moonlit = (sky * (1.8 - 1.3 * k) + Vec3::splat(0.004 * (1.0 - k))) * night;
            // Grey days (`whiteout` in palette.wgsl).
            let c = linear(CLOUD_RAMP + i as u8).max(moonlit) * (1.0 - 0.3 * state.gloom) * lit;
            tint(c).extend(1.0)
        }),
        haze_distance: (state.vis_km * 1000.0).max(4000.0),
        flash: BOLT_COLOR * state.flash * flash_scale,
        line_a,
        line_b,
        line_color: BOLT_COLOR * flash_scale,
        // Inside a cloud everything fades into it (`whiteout` in palette.wgsl).
        fog: tint(linear(CLOUD_RAMP + 2) * lit).extend(state.whiteout),
    };
    // VR (two large eye views on a mobile GPU): marched at a third of the resolution, coarse
    // scales only, fewer steps, no second occupancy sample for the self shadow.
    let vr = settings.vr;
    let env = |name: &str| std::env::var(name).ok().and_then(|v| v.parse::<u32>().ok());
    let want = CloudRenderSettings {
        j_cap: env("SM89_CLOUD_JCAP").unwrap_or(if vr { 1 } else { 3 }),
        h_cap: if vr { 2 } else { 4 },
        step_count: env("SM89_CLOUD_STEPS").unwrap_or(if vr { 6 } else { 16 }),
        max_march_span: 30_000.0,
        interior_span: 3_000.0,
        sigma_t: 0.02,
        dither: !settings.modern_shading,
        sun_shadow: env("SM89_CLOUD_SHADOW").map_or(!vr, |v| v != 0),
        resolution_scale: env("SM89_CLOUD_SCALE").unwrap_or(if vr { 3 } else { 1 }),
        ..default()
    };
    if *render != want {
        *render = want;
    }
}

/// `nvg` in palette.wgsl without the grain: a colour as the night vision goggles show it.
fn nvg_color(rgb: Vec3, daylight: f32) -> Vec3 {
    let lum = rgb.dot(Vec3::new(0.2126, 0.7152, 0.0722));
    let gain = 14.0 + (2.5 - 14.0) * daylight.clamp(0.0, 1.0);
    let v = (1.0 - (-lum * gain).exp()).sqrt();
    let t = ((v - 0.12) / (0.95 - 0.12)).clamp(0.0, 1.0);
    let v = t * t * (3.0 - 2.0 * t);
    let c = Vec3::new(0.0, 0.04, 0.01).lerp(Vec3::new(0.3, 0.85, 0.2), v)
        + Vec3::new(0.5, 0.15, 0.4) * v * v * v;
    c.min(Vec3::ONE).powf(2.2)
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
fn bolt_path(
    rng: &mut Rng,
    a: Vec3,
    b: Vec3,
    ground: bool,
    out: &mut Vec<(Vec3, Vec3, f32)>,
    width: f32,
    depth: u32,
) {
    let mut pts = vec![a, b];
    let mut rough = 0.22;
    for _ in 0..7 {
        let mut next = Vec::with_capacity(pts.len() * 2);
        for w in pts.windows(2) {
            let (p, q) = (w[0], w[1]);
            let len = p.distance(q);
            let off = Vec3::new(
                rng.random() - 0.5,
                (rng.random() - 0.5) * if ground { 0.3 } else { 0.6 },
                rng.random() - 0.5,
            ) * 2.0
                * len
                * rough;
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
    let n =
        if ground { 3 + (rng.random() * 4.0) as usize } else { 4 + (rng.random() * 5.0) as usize };
    let span = a.distance(b);
    for _ in 0..n {
        let i = (rng.random() * (pts.len() - 1) as f32 * if ground { 0.8 } else { 1.0 }) as usize;
        let start = pts[i];
        let main = (b - a).normalize_or_zero();
        let side = Vec3::new(rng.random() - 0.5, 0.0, rng.random() - 0.5).normalize_or_zero();
        let dir = if ground {
            (main * 0.7 + side * 0.6).normalize_or_zero()
        } else {
            (side + Vec3::Y * (rng.random() - 0.5) * 0.3).normalize_or_zero()
        };
        let len = span * (0.12 + 0.25 * rng.random());
        bolt_path(rng, start, start + dir * len, ground, out, width * 0.5, depth - 1);
    }
}

/// Odds against a strike in a storm cell at each simulation frame (about one a minute).
const STRIKE_ODDS: u64 = 900;

/// Storms: lightning strikes in the storm cells around the viewpoint, cloud-to-ground or
/// in-cloud, each a generated channel mesh with a few return strokes. The channel lights
/// the ground and the clouds around it, a strike's flash reaches far (weaker with distance),
/// and its thunder arrives at the speed of sound. When and how a cell strikes follows from
/// the cell and the simulation frame alone, so every player of a session sees the same
/// strikes (a coop client counts the host's frames).
#[allow(clippy::too_many_arguments)]
fn lightning(
    mut commands: Commands,
    time: Res<Time>,
    origin: Res<FloatingOrigin>,
    eyes: Eyes,
    combat: Option<Res<crate::combat::Combat>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<crate::lightning::BoltMaterial>>,
    bolt_materials: Query<&MeshMaterial3d<crate::lightning::BoltMaterial>>,
    mut sounds: MessageWriter<crate::sound::PlaySound>,
    mut state: ResMut<WeatherState>,
) {
    let dt = time.delta_secs();
    let len = state.thunder.len();
    state.thunder.retain_mut(|t| {
        *t -= dt;
        *t > 0.0
    });
    if state.thunder.len() < len {
        // No thunder among the game's sounds: the explosion rumble stands in.
        sounds.write(crate::sound::PlaySound { id: 5, priority: 0 });
    }
    // The running strikes.
    let test = state.test_strike;
    let mut flash: f32 = 0.0;
    let mut bolts = std::mem::take(&mut state.bolts);
    bolts.retain_mut(|b| {
        b.age = if test.is_some() { 0.01 } else { b.age + dt };
        b.light = b.intensity();
        if let Ok(m) = bolt_materials.get(b.entity)
            && let Some(mut m) = materials.get_mut(&m.0)
        {
            m.params.x = b.light;
        }
        // Far-reaching: the sky and the whole scene brighten, less for distant strikes.
        flash = flash.max(b.light * (0.05 + 0.3 / (1.0 + (b.distance / 6000.0).powi(2))));
        let done = b.age > b.end();
        if done {
            commands.entity(b.entity).try_despawn();
        }
        !done
    });
    state.bolts = bolts;
    state.flash = flash;
    let step = combat.map(|c| c.sim.step_count());
    let last = std::mem::replace(&mut state.strike_step, step);
    let w = state.weather;
    if !w.has_storms() {
        return;
    }
    let Some(eye) = eye_position(&eyes, &origin) else { return };
    let (ex, ey) = state.eye_map;
    let cells: Vec<Cloud> = w
        .clouds_near(ex, ey, STRIKE_RADIUS)
        .filter(|c| c.kind == CloudKind::Cumulonimbus)
        .collect();
    if cells.is_empty() {
        return;
    }
    let mut spawn = |state: &mut WeatherState, c: &Cloud, rng: &mut Rng, ground, centre, age| {
        let s = strike(
            &mut commands,
            &origin,
            &mut meshes,
            &mut materials,
            eye,
            c,
            rng,
            ground,
            centre,
            age,
        );
        // Thunder at the speed of sound (inaudible beyond ~25 km).
        if s.distance < 25_000.0 {
            state.thunder.push(s.distance / 343.0 - age);
        }
        state.bolts.push(s);
    };
    if let Some(ground) = test {
        state.next_flash -= dt;
        if state.next_flash > 0.0 || !state.bolts.is_empty() {
            return;
        }
        let tf = eyes.iter().max_by_key(|(_, e)| *e).map(|(tf, _)| *tf);
        let Some(tf) = tf else { return };
        let f = tf.forward();
        let c = if ground {
            cells[0]
        } else {
            // Test in-cloud strikes: the cell most straight ahead of the view.
            let ahead = |c: &Cloud| {
                let p =
                    wu_to_bevy(c.x as f64 * 32.0, (0x8000 - c.y) as f64 * 32.0, eye.y / WU) - eye;
                (p.as_vec3().normalize_or_zero().dot(*f) * 1000.0) as i32
            };
            *cells.iter().max_by_key(|c| ahead(c)).unwrap()
        };
        // Test strikes to the ground: 8 km ahead of the view.
        let centre = ground.then(|| {
            let f = f.as_vec3().with_y(0.0).normalize_or_zero().as_dvec3();
            DVec3::new(eye.x, 0.0, eye.z) + f * 8000.0
        });
        let mut rng = Rng(w.seed | 1);
        spawn(&mut state, &c, &mut rng, ground, centre, 0.0);
        return;
    }
    // The frames since the last look (none after a jump, as on joining).
    let (Some(step), Some(last)) = (step, last) else { return };
    if step <= last || step - last > 2 * crate::combat::FS as u32 {
        return;
    }
    let seed = w.layer_seed(CloudKind::Cumulonimbus) as u64;
    for s in last + 1..=step {
        for c in &cells {
            let h = hash(
                (seed << 32 ^ (c.gx as u16 as u64) << 16 ^ c.gy as u16 as u64)
                    ^ (s as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15),
            );
            if !h.is_multiple_of(STRIKE_ODDS) {
                continue;
            }
            debug!("lightning: frame {s}, storm cell {} {}", c.gx, c.gy);
            let mut rng = Rng((h >> 32) as u32 | 1);
            let ground = rng.random() < 0.4;
            let age = (step - s) as f32 / crate::combat::FS as f32;
            spawn(&mut state, c, &mut rng, ground, None, age);
        }
    }
}

/// A strike in storm cell `c` (`centre`: on the ground there instead), its channel from
/// `rng`.
#[allow(clippy::too_many_arguments)]
fn strike(
    commands: &mut Commands,
    origin: &FloatingOrigin,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<crate::lightning::BoltMaterial>,
    eye: DVec3,
    c: &Cloud,
    rng: &mut Rng,
    ground: bool,
    centre: Option<DVec3>,
    age: f32,
) -> Strike {
    let ft_m = WU as f32;
    let centre =
        centre.unwrap_or_else(|| wu_to_bevy(c.x as f64 * 32.0, (0x8000 - c.y) as f64 * 32.0, 0.0));
    let r_m = c.r as f32 * 32.0 * ft_m;
    let at = |k: f32, alt_ft: f32, rng: &mut Rng| {
        let a = rng.random() * std::f32::consts::TAU;
        let d = r_m * k * rng.random().sqrt();
        Vec3::new(centre.x as f32 + a.cos() * d, alt_ft * ft_m, centre.z as f32 + a.sin() * d)
    };
    let base = c.base_ft as f32;
    let (a, b) = if ground {
        // From the cloud base down to the ground below it.
        let top = at(0.6, base + 1500.0 + 3000.0 * rng.random(), rng);
        let foot =
            top + Vec3::new((rng.random() - 0.5) * 3000.0, 0.0, (rng.random() - 0.5) * 3000.0);
        (top, Vec3::new(foot.x, 0.0, foot.z))
    } else {
        // Inside the cell: a long, mostly horizontal discharge.
        let h = base + (c.top_ft as f32 - base) * (0.15 + 0.4 * rng.random());
        let p = at(0.7, h, rng);
        (p, at(0.7, h + (rng.random() - 0.5) * 6000.0, rng))
    };
    let mut segs = Vec::new();
    bolt_path(rng, a, b, ground, &mut segs, if ground { 3.0 } else { 2.0 }, 2);
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
    let n =
        if ground { 1 + (rng.random() * 4.0) as usize } else { 1 + (rng.random() * 2.0) as usize };
    let mut t = 0.0;
    let mut strokes = Vec::new();
    for _ in 0..n {
        let d = 0.04 + 0.04 * rng.random();
        strokes.push((t, d));
        t += d + 0.05 + 0.12 * rng.random();
    }
    Strike {
        entity,
        age,
        strokes,
        ground,
        // In-cloud: the channel itself; cloud-to-ground: from the ground up to the base.
        light_a: a.as_dvec3(),
        light_b: b.as_dvec3(),
        light: 0.0,
        distance: ((a + b) * 0.5).distance(eye.as_vec3()),
    }
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
    let raining = state.under_storm || (w.has_storms() && state.storm > 0.5 && state.shade > 0.5);
    let Ok((cam, cam_tf)) = eye_cams.single() else { return };
    if !raining {
        if let Some(e) = state.rain.take() {
            commands.entity(e).try_despawn();
        }
        return;
    }
    if state.drops.is_empty() {
        let drops: Vec<Vec3> = (0..DROPS)
            .map(|_| {
                Vec3::new(state.rng.random(), state.rng.random(), state.rng.random())
                    * 2.0
                    * RAIN_BOX
                    - Vec3::splat(RAIN_BOX)
            })
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
        *d = (*d + Vec3::splat(RAIN_BOX)).rem_euclid(Vec3::splat(2.0 * RAIN_BOX))
            - Vec3::splat(RAIN_BOX);
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
                params: PaletteParams {
                    word: 0xFFE4,
                    flags: crate::palette_material::FLAG_LINE,
                    ..default()
                },
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
                    bevy::camera::visibility::RenderLayers::layer(
                        crate::camera::MAIN_BACKGROUND_LAYER,
                    ),
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
fn turbulence(
    time: Res<Time>,
    state: Res<WeatherState>,
    mut q: Query<&mut crate::flight::Aircraft, With<crate::flight::Player>>,
) {
    if !state.under_storm {
        return;
    }
    // Inside the cell's cloud, or (weaker) under it.
    let strength = if state.whiteout > 0.5 { 1.0 } else { 0.35 };
    let t = time.elapsed_secs();
    let dt = time.delta_secs();
    let s = (state.weather.seed % 97) as f32;
    let wave = |a: f32, b: f32, c: f32| {
        (t * a + s).sin() * 0.6 + (t * b + 2.0 * s).sin() * 0.3 + (t * c + 3.0 * s).sin() * 0.1
    };
    let roll = 0.3 * strength * wave(1.7, 4.3, 11.0) * dt;
    let pitch = 0.12 * strength * wave(2.3, 5.9, 13.0) * dt;
    for mut a in &mut q {
        if a.on_ground || a.ended.is_some() {
            continue;
        }
        let o = a.orientation;
        a.orientation =
            (o * Quat::from_rotation_z(roll) * Quat::from_rotation_x(pitch)).normalize();
    }
}
