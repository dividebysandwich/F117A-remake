//! Fires and smoke. Everything destroyed on the ground burns for the rest
//! of the flight: a plume of smoke rising into the sky over a fire, small ones for vehicles and
//! aircraft wrecks, big ones for buildings, installations and ships. In a conventional war
//! (ROE 2) towns and roads are burning from the start. Theaters with a front line (the
//! remake's own) burn along it in every mission: the fighting positions on both sides, the
//! ruined towns.
//!
//! - Smoke: a volume ray-marched in its proxy cylinder by the palette shader (`TAG_SMOKE`):
//!   alpha blended in modern mode, a dithered screen door otherwise.
//!   Plumes lean downwind and grow to full height over the first minute or so.
//! - Fire: mesh particles (small flames in the fire palette colours) rising and fading near the
//!   viewpoint, plus a flickering local light (with the local lighting option).

use bevy::math::{DVec2, DVec3};
use bevy::mesh::{MeshTag, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;

use crate::combat::{Combat, CombatSystems};
use crate::lighting::LightEmitter;
use crate::mission::ActiveMission;
use crate::palette::Palette;
use crate::palette_material::{
    FLAG_CLOUD, FLAG_CLOUD_SOFT, FLAG_LINE, PaletteMaterial, PaletteParams, TAG_SMOKE,
    TAG_SMOKE_FIRE, TAG_WARM, mesh_tag,
};
use crate::settings::{AppState, SetupSet};
use crate::world::{Theater, WU, WorldAnchor, WorldPos, map_to_wu, wu_to_bevy};

pub struct FiresPlugin;

impl Plugin for FiresPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Fires>()
            .add_systems(OnEnter(AppState::Flight), setup.in_set(SetupSet::Spawn))
            .add_systems(OnExit(AppState::Flight), teardown)
            .add_systems(
                Update,
                (
                    ignite.run_if(resource_exists::<Combat>),
                    war_fires,
                    stream,
                    (grow_smoke, animate_fires),
                )
                    .chain()
                    .after(CombatSystems)
                    .run_if(in_state(AppState::Flight)),
            );
    }
}

/// Smoke plumes are drawn within this distance (metres), fires within this one.
const SMOKE_RANGE: f64 = 45_000.0;
const FIRE_RANGE: f64 = 5_000.0;
/// Seconds for a new plume to reach its full height.
const GROW_SECS: f32 = 75.0;
/// A kill this close to a burning site (metres) feeds that fire instead of starting one.
const MERGE_M: f64 = 40.0;
/// Front line: metres of line per fire where the fighting is heaviest (intensity 1), and how
/// far either side of the line the fires reach.
const FRONT_SPACING_M: f64 = 1200.0;
const FRONT_DEPTH_M: f64 = 7000.0;

/// Fresh flame colours: yellow-orange, orange, white; they cool to red-orange and dark red.
/// (Palette entries outside the time-of-day fade, 0x50..0x5F and below 0x10.)
const FLAME_HOT: [u8; 3] = [89, 91, 15];
const FLAME_WARM: u8 = 93;
const FLAME_COOL: u8 = 12;
/// Height of the fire light above the ground (metres).
const LIGHT_HEIGHT: f32 = 2.5;

#[derive(Clone, Copy, PartialEq)]
enum Size {
    /// Vehicles, aircraft wrecks.
    Small,
    /// Buildings, installations, ships.
    Big,
}

impl Size {
    /// Plume width (proxy radius) and height in metres.
    fn plume(self) -> (f32, f32) {
        match self {
            Size::Small => (90.0, 480.0),
            Size::Big => (300.0, 1500.0),
        }
    }

    /// Flames: count, spread radius, height (metres), particle size.
    fn flames(self) -> (usize, f32, f32, f32) {
        match self {
            Size::Small => (12, 2.5, 7.0, 1.6),
            Size::Big => (26, 11.0, 18.0, 3.5),
        }
    }

    /// Fire light: colour * intensity, range in metres.
    fn light(self) -> (f32, f32) {
        match self {
            Size::Small => (1.3, 45.0),
            Size::Big => (2.4, 110.0),
        }
    }
}

struct Site {
    /// Ground point (Bevy metres).
    pos: DVec3,
    size: Size,
    /// Plume scale variation.
    scale: f32,
    /// Seconds burning (the plume grows meanwhile).
    age: f32,
    seed: u32,
    /// Downwind direction (radians about +Y from +X).
    yaw: f32,
    smoke: Option<Entity>,
    fire: Option<(Entity, Vec<Flame>)>,
}

struct Flame {
    entity: Entity,
    /// Start offset on the ground, phase and lifetime (seconds).
    offset: Vec2,
    phase: f32,
    life: f32,
    colour: u8,
}

#[derive(Resource, Default)]
struct Fires {
    sites: Vec<Site>,
    /// The war's fires have been placed (or the mission isn't a war).
    war_done: bool,
    /// The front line's fires have been placed.
    front_done: bool,
    /// Wind direction for the plumes (radians), from the mission's weather seed.
    wind: f32,
    rng: u32,
    /// Entities of changed sites, despawned by `stream`.
    dead: Vec<Entity>,
    smoke_material: Handle<PaletteMaterial>,
    proxy: Handle<Mesh>,
    flame: Handle<Mesh>,
    /// Solid materials of the flame colours (by palette index).
    flame_materials: Vec<(u8, Handle<PaletteMaterial>)>,
}

impl Fires {
    fn flame_material(&self, c: u8) -> Handle<PaletteMaterial> {
        self.flame_materials
            .iter()
            .find(|(k, _)| *k == c)
            .map(|(_, m)| m.clone())
            .unwrap_or_default()
    }

    fn random(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng >> 8) as f32 / (1u32 << 24) as f32
    }

    fn add(&mut self, pos: DVec3, size: Size, age: f32) {
        if let Some(s) = self.sites.iter_mut().find(|s| s.pos.distance(pos) < MERGE_M) {
            // A big kill next to a small fire turns it into a big one.
            if size == Size::Big && s.size == Size::Small {
                s.size = Size::Big;
                s.age = s.age.min(age);
                self.dead.extend(s.smoke.take());
                self.dead.extend(s.fire.take().map(|(e, _)| e));
            }
            return;
        }
        let seed = (self.random() * 65535.0) as u32;
        let yaw = self.wind + (self.random() - 0.5) * 0.35;
        let scale = 0.8 + 0.4 * self.random();
        self.sites.push(Site { pos, size, scale, age, seed, yaw, smoke: None, fire: None });
    }
}

/// Proxy of a plume: a closed unit cylinder (y 0..1) with both windings, so the volume also
/// shows from inside (back faces are culled; the shader picks the side to march from:
/// palette.wgsl `proxy_fragment`).
fn plume_proxy() -> Mesh {
    // Circumscribes the shader's unit cylinder.
    let mut mesh =
        Cylinder::new(1.0 / (std::f32::consts::PI / 16.0).cos(), 1.0).mesh().resolution(16).build();
    mesh.remove_attribute(Mesh::ATTRIBUTE_NORMAL);
    mesh.remove_attribute(Mesh::ATTRIBUTE_UV_0);
    if let Some(VertexAttributeValues::Float32x3(pos)) =
        mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION)
    {
        for p in pos.iter_mut() {
            p[1] += 0.5;
        }
    }
    let tris: Vec<u32> = mesh.indices().expect("indexed mesh").iter().map(|i| i as u32).collect();
    let back = tris.chunks(3).flat_map(|t| [t[0], t[2], t[1]]);
    let all: Vec<u32> = tris.iter().copied().chain(back).collect();
    mesh.insert_indices(bevy::mesh::Indices::U32(all));
    mesh
}

/// A flame particle: a stretched octahedron (unit width, pointed top), outward wound.
fn flame_mesh() -> Mesh {
    let top = Vec3::new(0.0, 1.5, 0.0);
    let bottom = Vec3::new(0.0, -0.4, 0.0);
    let ring: Vec<Vec3> = (0..4)
        .map(|i| {
            let a = i as f32 * std::f32::consts::FRAC_PI_2;
            Vec3::new(a.cos() * 0.5, 0.0, a.sin() * 0.5)
        })
        .collect();
    let mut pos = Vec::new();
    for i in 0..4 {
        let (a, b) = (ring[i], ring[(i + 1) % 4]);
        for tip in [top, bottom] {
            let mut t = [tip, a, b];
            let n = (t[1] - t[0]).cross(t[2] - t[0]);
            if n.dot((t[0] + t[1] + t[2]) / 3.0 - Vec3::new(0.0, 0.3, 0.0)) < 0.0 {
                t.swap(1, 2);
            }
            pos.extend(t);
        }
    }
    Mesh::new(PrimitiveTopology::TriangleList, bevy::asset::RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
}

fn setup(
    mut fires: ResMut<Fires>,
    palette: Res<Palette>,
    mission: Option<Res<ActiveMission>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<PaletteMaterial>>,
) {
    let seed = mission.as_ref().map_or(1, |m| m.0.weather.seed);
    let rng = seed.wrapping_mul(0x9E37_79B9) | 1;
    *fires = Fires {
        // Only a conventional war burns from the start.
        war_done: mission.as_ref().is_none_or(|m| m.0.settings.roe != 2),
        wind: ((seed >> 7) & 0xFFFF) as f32 / 65535.0 * std::f32::consts::TAU,
        rng,
        smoke_material: materials.add(PaletteMaterial {
            params: PaletteParams {
                word: 0xFFE0,
                flags: FLAG_CLOUD | FLAG_CLOUD_SOFT,
                ..default()
            },
            globals: palette.globals.clone(),
        }),
        proxy: meshes.add(plume_proxy()),
        flame: meshes.add(flame_mesh()),
        flame_materials: FLAME_HOT
            .iter()
            .chain(&[FLAME_WARM, FLAME_COOL])
            .map(|&c| {
                let m = materials.add(PaletteMaterial {
                    params: PaletteParams {
                        word: 0xFF00 | c as u32,
                        flags: FLAG_LINE,
                        ..default()
                    },
                    globals: palette.globals.clone(),
                });
                (c, m)
            })
            .collect(),
        ..default()
    };
}

fn teardown(mut commands: Commands, mut fires: ResMut<Fires>) {
    let fires = &mut *fires;
    for s in &mut fires.sites {
        fires.dead.extend(s.smoke.take());
        fires.dead.extend(s.fire.take().map(|(e, _)| e));
    }
    for e in fires.dead.drain(..) {
        commands.entity(e).try_despawn();
    }
    *fires = Fires::default();
}

/// New fires where the simulation destroyed something.
fn ignite(combat: Res<Combat>, mut fires: ResMut<Fires>) {
    for k in &combat.kills {
        let pos = crate::sim_render::map_point(k.x as f64 + 0.5, k.y as f64 + 0.5, 0.0);
        fires.add(pos, if k.vehicle { Size::Small } else { Size::Big }, 0.0);
    }
}

/// Theater object names that are not towns (for the theaters without generated towns).
fn is_town_name(name: &str) -> bool {
    const NOT: [&str; 46] = [
        "air",
        "hangar",
        "tower",
        "sam",
        "missile",
        "radar",
        "depot",
        "hq",
        "headquarters",
        "bridge",
        "camp",
        "boat",
        "ship",
        "destroyer",
        "uss ",
        "cv ",
        "tank",
        "dump",
        "plant",
        "rig",
        "refinery",
        "pump",
        "special",
        "runway",
        "oil",
        "sub pen",
        "typhoon",
        "krivak",
        "cruiser",
        "palace",
        "mosque",
        "command",
        "guards",
        "island",
        "manifold",
        "trail",
        "buildings",
        "windmill",
        "trawler",
        "cargo",
        "merchant",
        "scud",
        "well",
        "lpar",
        "h3",
        "aegis",
    ];
    let n = name.to_ascii_lowercase();
    !n.is_empty() && !NOT.iter().any(|k| n.contains(k))
}

/// Fires along the front line and in the ruined towns: (ground point in metres, big).
fn front_sites(
    theater: &Theater,
    scenery: &crate::scenery::Scenery,
    seed: u64,
) -> Vec<(DVec2, bool)> {
    let mut rng = crate::scenery::Rng::new(seed ^ 0xF407);
    let mut out = Vec::new();
    for w in theater.front.windows(2) {
        let ((ax, ay, k), (bx, by, _)) = (w[0], w[1]);
        let (a, b) = (DVec2::new(ax, ay), DVec2::new(bx, by));
        let len = a.distance(b);
        let along = (b - a) / len.max(1.0);
        let expected = len * k / FRONT_SPACING_M;
        let n = expected.floor() as usize + rng.chance(expected.fract()) as usize;
        for _ in 0..n {
            // Most fires close to the line, some deep in the rear.
            let off = rng.range(-1.0, 1.0);
            let p = a
                + along * rng.range(0.0, len)
                + along.perp() * off * off * off.signum() * FRONT_DEPTH_M;
            if scenery.is_open(p, 8.0) {
                out.push((p, rng.chance(0.3 * k)));
            }
        }
    }
    for t in theater.towns.iter().filter(|t| t.ruined >= 0.3) {
        let c = DVec2::new(t.x, t.y);
        for _ in 0..1 + (t.ruined * 2.5) as usize {
            let p = c + DVec2::from_angle(rng.range(0.0, std::f64::consts::TAU))
                * rng.range(0.0, t.r * 0.9);
            out.push((p, rng.chance(0.35)));
        }
    }
    out
}

/// The front line's fires and the war's, once the scenery layout exists.
fn war_fires(
    mut fires: ResMut<Fires>,
    scenery: Option<Res<crate::scenery::Scenery>>,
    theater: Option<Res<Theater>>,
) {
    if fires.war_done && fires.front_done {
        return;
    }
    let (Some(scenery), Some(theater)) = (scenery, theater) else { return };
    if !fires.front_done {
        fires.front_done = true;
        let sites = front_sites(&theater, &scenery, fires.rng as u64);
        if !sites.is_empty() {
            debug!("fires: {} burning along the front", sites.len());
        }
        for (p, big) in sites {
            fires.add(
                wu_to_bevy(p.x / WU, p.y / WU, 0.0),
                if big { Size::Big } else { Size::Small },
                GROW_SECS,
            );
        }
    }
    if fires.war_done {
        return;
    }
    fires.war_done = true;
    let seed = fires.rng as u64 ^ 0xF1AE;
    let mut sites = scenery.war_sites(seed);
    // Without generated towns, the theater's own towns burn.
    if !scenery.has_towns() {
        let mut rng = crate::scenery::Rng::new(seed ^ 0x70);
        for o in &theater.world.objects {
            if o.target_flags & 0x100 != 0 || !is_town_name(theater.world.name(o.name)) {
                continue;
            }
            let (e, n) = map_to_wu(o.x, o.y);
            let c = DVec2::new(e, n) * WU;
            for _ in 0..1 + rng.u64() % 3 {
                let p = c + DVec2::from_angle(rng.range(0.0, std::f64::consts::TAU))
                    * rng.range(100.0, 1500.0);
                if scenery.is_open(p, 8.0) {
                    sites.push((p, rng.chance(0.4)));
                }
            }
        }
    }
    debug!("fires: {} burning in the war", sites.len());
    for (p, big) in sites {
        // Long burning already: full grown plumes.
        fires.add(
            wu_to_bevy(p.x / WU, p.y / WU, 0.0),
            if big { Size::Big } else { Size::Small },
            GROW_SECS,
        );
    }
}

/// Spawns the plumes and fires near the viewpoint and drops the far ones.
fn stream(
    mut commands: Commands,
    mut fires: ResMut<Fires>,
    anchor: Query<&WorldPos, With<WorldAnchor>>,
) {
    let Ok(eye) = anchor.single() else { return };
    let eye = eye.0;
    let fires = &mut *fires;
    for e in fires.dead.drain(..) {
        commands.entity(e).try_despawn();
    }
    let mut rng = fires.rng;
    let mut random = move || {
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        (rng >> 8) as f32 / (1u32 << 24) as f32
    };
    let materials: Vec<(u8, Handle<PaletteMaterial>)> = fires.flame_materials.clone();
    let flame_material =
        |c: u8| materials.iter().find(|(k, _)| *k == c).map(|(_, m)| m.clone()).unwrap_or_default();
    for s in &mut fires.sites {
        let d = DVec2::new(s.pos.x - eye.x, s.pos.z - eye.z).length();
        match (d < SMOKE_RANGE, s.smoke) {
            (true, None) => {
                let tag =
                    mesh_tag(1, 0, true) | TAG_SMOKE | TAG_SMOKE_FIRE | (s.seed & 0xFFFF) << 16;
                s.smoke = Some(
                    commands
                        .spawn((
                            Mesh3d(fires.proxy.clone()),
                            MeshMaterial3d(fires.smoke_material.clone()),
                            MeshTag(tag),
                            WorldPos(s.pos),
                            plume_transform(s),
                            bevy::light::NotShadowCaster,
                        ))
                        .id(),
                );
            }
            (false, Some(e)) => {
                commands.entity(e).try_despawn();
                s.smoke = None;
            }
            _ => {}
        }
        match (d < FIRE_RANGE, s.fire.is_some()) {
            (true, false) => {
                let (n, spread, _, _) = s.size.flames();
                // The root carries the fire light (used with the local lighting option) a little
                // above the ground.
                let root = commands
                    .spawn((
                        WorldPos(s.pos + DVec3::Y * LIGHT_HEIGHT as f64),
                        Transform::default(),
                        Visibility::default(),
                        LightEmitter { color: Vec3::ZERO, range: s.size.light().1 },
                    ))
                    .id();
                let mut flames = Vec::with_capacity(n);
                for i in 0..n {
                    let colour = if random() < 0.2 { FLAME_HOT[2] } else { FLAME_HOT[i % 2] };
                    let r = spread * random().sqrt();
                    let a = random() * std::f32::consts::TAU;
                    let entity = commands
                        .spawn((
                            Mesh3d(fires.flame.clone()),
                            MeshMaterial3d(flame_material(colour)),
                            MeshTag(mesh_tag(1, 0, true) | TAG_WARM),
                            Transform::from_scale(Vec3::ZERO),
                            bevy::light::NotShadowCaster,
                        ))
                        .id();
                    commands.entity(root).add_child(entity);
                    flames.push(Flame {
                        entity,
                        offset: Vec2::from_angle(a) * r,
                        phase: random(),
                        life: 0.7 + 0.7 * random(),
                        colour,
                    });
                }
                s.fire = Some((root, flames));
            }
            (false, true) => {
                if let Some((e, _)) = s.fire.take() {
                    commands.entity(e).try_despawn();
                }
            }
            _ => {}
        }
    }
}

/// Plume scale for its age: rises to full height, widening a little.
fn plume_transform(s: &Site) -> Transform {
    let (w, h) = s.size.plume();
    let g = (s.age / GROW_SECS).clamp(0.0, 1.0);
    let g = g * (2.0 - g);
    let (w, h) = (w * s.scale * (0.35 + 0.65 * g), h * s.scale * g.max(0.02));
    Transform::from_rotation(Quat::from_rotation_y(s.yaw)).with_scale(Vec3::new(w, h, w))
}

fn grow_smoke(time: Res<Time>, mut fires: ResMut<Fires>, mut q: Query<&mut Transform>) {
    let dt = time.delta_secs();
    for s in &mut fires.sites {
        if s.age >= GROW_SECS {
            continue;
        }
        s.age += dt;
        if let Some(e) = s.smoke
            && let Ok(mut tf) = q.get_mut(e)
        {
            let want = plume_transform(s);
            tf.rotation = want.rotation;
            tf.scale = want.scale;
        }
    }
}

/// Flames rise, drift downwind and shrink, cooling from yellow to dark red; the fire light
/// flickers.
fn animate_fires(
    time: Res<Time>,
    fires: Res<Fires>,
    mut tfs: Query<&mut Transform>,
    mut mats: Query<&mut MeshMaterial3d<PaletteMaterial>>,
    mut lights: Query<&mut LightEmitter>,
) {
    let t = time.elapsed_secs_wrapped();
    let wind = Vec2::from_angle(fires.wind);
    for s in &fires.sites {
        let Some((root, flames)) = &s.fire else { continue };
        let (_, _, height, size) = s.size.flames();
        // Grows in with the plume (a fresh fire flares up first).
        let strength = (s.age / 3.0).clamp(0.0, 1.0);
        for f in flames {
            let k = ((t / f.life + f.phase) % 1.0).abs();
            let rise = k * height * (0.7 + 0.3 * f.phase);
            let drift = wind * k * k * height * 0.4;
            let wobble = (t * 7.0 + f.phase * 20.0).sin() * 0.3 * size;
            if let Ok(mut tf) = tfs.get_mut(f.entity) {
                tf.translation = Vec3::new(
                    f.offset.x + drift.x + wobble,
                    rise - LIGHT_HEIGHT,
                    -(f.offset.y + drift.y),
                );
                let shrink = (1.0 - k) * (0.4 + 2.4 * k).min(1.0);
                tf.scale = Vec3::new(size, size * (1.2 + k), size) * shrink * strength;
                tf.rotation = Quat::from_rotation_y(f.phase * 6.0 + t * 1.5);
            }
            // Cooling: the base colour, then orange-brown, then dark red.
            let colour = if k < 0.45 {
                f.colour
            } else if k < 0.75 {
                FLAME_WARM
            } else {
                FLAME_COOL
            };
            if let Ok(mut m) = mats.get_mut(f.entity) {
                let want = fires.flame_material(colour);
                if m.0 != want {
                    m.0 = want;
                }
            }
        }
        if let Ok(mut light) = lights.get_mut(*root) {
            let p = s.seed as f32 * 0.37;
            let flicker = 0.72
                + 0.14 * (t * 11.0 + p).sin()
                + 0.09 * (t * 23.7 + p * 2.0).sin()
                + 0.05 * (t * 41.3).sin();
            let (k, _) = s.size.light();
            light.color = Vec3::new(1.0, 0.5, 0.16) * k * flicker * strength;
        }
    }
}
