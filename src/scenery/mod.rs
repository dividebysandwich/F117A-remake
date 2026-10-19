//! Extra scenery (not in the original). Every theater's airfields get taxiways, aprons,
//! shelters, support buildings and airfield lighting (`airfield`). The green theaters also get
//! towns with street grids and street lamps, roads between them and along the original road
//! lines, farms, industry, transmission and pole lines, radio masts, forests and lone trees;
//! the desert theaters otherwise keep their original look.
//!
//! The steppe theaters' open land is farmland: fields and shelterbelts (`fields`), streamed in
//! larger blocks further out.
//!
//! The layout is generated once per theater (`network`), clear of water, concrete and the
//! theater's own objects (`ground`). Around the viewpoint, 1 km blocks are built into one mesh
//! per palette colour (`content`), drawn with the palette materials and haze like the theater
//! models; airfields are built whole at the start. Lamps are batched dots, lit when the
//! original's lights come on.

mod airfield;

pub use airfield::PapiSite;
pub use network::Rng;
mod content;
mod fields;
mod geom;
mod ground;
mod network;
mod style;

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::math::{DVec2, DVec3};
use bevy::mesh::{MeshTag, PrimitiveTopology};
use bevy::prelude::*;

use crate::dots::{dot_batch, dot_batch_coloured};
use crate::models::{ModelAssets, ModelLibrary};
use crate::palette::{DetailLevel, Palette, REMAP_DETAIL_HIGH};
use crate::palette_material::{TAG_DOT, TAG_WARM, dot_scale, mesh_tag};
use crate::settings::AppState;
use crate::world::{ShapeHandles, Theater, WorldAnchor, WorldPos, bevy_to_wu, WU};
use content::Tier;
use geom::Geom;

pub struct SceneryPlugin;

impl Plugin for SceneryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Blocks>().init_resource::<Airfields>().init_resource::<PapiSites>().add_systems(OnExit(AppState::Flight), cleanup).add_systems(
            Update,
            (
                build_scenery.run_if(resource_exists::<Theater>).run_if(not(resource_exists::<Scenery>)),
                (stream_blocks, airfields, lamps_at_night, papi_lights).run_if(resource_exists::<Scenery>),
            )
                .chain()
                .run_if(in_state(AppState::Flight)),
        );
    }
}

/// Streaming radius per tier (metres). Far trees fill the blocks out to their radius that the
/// detailed trees don't.
fn radius(tier: Tier) -> f64 {
    match tier {
        Tier::Main => 7000.0,
        Tier::Detail => 1000.0,
        Tier::Far => 3500.0,
        Tier::Fields => 24_000.0,
    }
}

/// The other tree tier of a block (shown until this one is built, so trees don't blink).
fn counterpart(tier: Tier) -> Option<Tier> {
    match tier {
        Tier::Main | Tier::Fields => None,
        Tier::Detail => Some(Tier::Far),
        Tier::Far => Some(Tier::Detail),
    }
}

/// Blocks built per frame (nearest first).
const BUILDS_PER_FRAME: usize = 3;

/// The theater's scenery layout (no style and an empty network for the desert theaters).
#[derive(Resource)]
pub struct Scenery {
    style: Option<style::Style>,
    ground: ground::Ground,
    net: network::Network,
    airfields: Vec<airfield::Airfield>,
}

impl Scenery {
    /// The theater has generated towns (the green theaters).
    pub fn has_towns(&self) -> bool {
        !self.net.towns.is_empty()
    }

    /// Open land (not water or concrete) clear of raised objects by `r` metres.
    pub fn is_open(&self, p: DVec2, r: f64) -> bool {
        self.ground.is_free(p, r)
    }

    /// Where a conventional war burns (fires.rs): in and around the towns and along the roads
    /// and the original road lines, on open ground. Positions in metres (east, north) and
    /// whether the fire is a big one.
    pub fn war_sites(&self, seed: u64) -> Vec<(DVec2, bool)> {
        let mut rng = network::Rng::new(seed);
        let mut out = Vec::new();
        let mut place = |p: DVec2, big: bool| {
            if self.ground.is_free(p, 8.0) {
                out.push((p, big));
            }
        };
        for t in &self.net.towns {
            let n = match t.kind {
                network::TownKind::City => 1 + (rng.u64() % 2) as usize,
                network::TownKind::Town => rng.chance(0.2) as usize,
                network::TownKind::Village => rng.chance(0.03) as usize,
            };
            for _ in 0..n {
                let a = rng.range(0.0, std::f64::consts::TAU);
                let p = t.c + DVec2::from_angle(a) * rng.range(0.0, t.r * 1.1);
                let big = t.kind == network::TownKind::City && rng.chance(0.5);
                place(p, big);
            }
        }
        // Burning vehicles along the roads, about one per 150 km.
        let roads = self.net.roads.iter().filter(|r| r.kind != network::RoadKind::Street).map(|r| r.pts.as_slice());
        for pts in roads.chain(self.ground.highways.iter().map(|h| h.as_slice())) {
            for w in pts.windows(2) {
                let len = w[0].distance(w[1]);
                if !rng.chance((len / 150_000.0).min(1.0)) {
                    continue;
                }
                let along = (w[1] - w[0]) / len.max(1e-6);
                let p = w[0] + along * rng.range(0.0, len) + along.perp() * rng.range(-40.0, 40.0);
                let big = rng.chance(0.15);
                place(p, big);
            }
        }
        out
    }
}

#[derive(Resource, Default)]
struct Blocks(HashMap<(Tier, i32, i32), Entity>);

/// Where the ILS camera looks on an approach to each runway end (see `trackcam.rs`).
#[derive(Resource, Default)]
pub struct PapiSites(pub Vec<airfield::PapiSite>);

/// Root entities of the airfields (built whole, shown with the scenery setting).
#[derive(Resource, Default)]
struct Airfields {
    spawned: bool,
    roots: Vec<Entity>,
}

/// A batch of approach flashers lit in its turn of the sequence.
#[derive(Component)]
struct Strobe(usize);

/// A PAPI light unit (a one-dot batch at its transform; data from `geom::PapiLight`).
#[derive(Component)]
struct Papi {
    toward: Vec3,
    angle: f32,
}

/// PAPI lights are seen within this distance (metres) and this cosine of the angle off the
/// approach axis (about ±30°).
const PAPI_RANGE: f32 = 10_000.0;
const PAPI_COS_SPREAD: f32 = 0.87;
/// PAPI lights are drawn this many times the size of other lights, so the colours read on a
/// long final.
const PAPI_DOT_SCALE: u32 = 3;

/// Root of a streamed block.
#[derive(Component)]
struct SceneryBlock;

/// Batched lamp dots of a block (block-local positions), lit at night.
#[derive(Component)]
pub struct LampBatch {
    /// Positions and palette colours, in drawing order.
    pub points: Vec<Vec3>,
    pub colours: Vec<u8>,
    /// Largest distance of a point from the batch origin.
    pub radius: f32,
}

fn build_scenery(
    mut commands: Commands,
    theater: Res<Theater>,
    handles: Res<ShapeHandles>,
    library: Res<ModelLibrary>,
    assets: ModelAssets,
) {
    let style = style::Style::for_theater(&theater.id, theater.index);
    // Wait until the tile streaming has loaded the theater's models.
    let loaded = assets.terrain.ready(&theater.id) && library.files.get("photo").is_some_and(|h| assets.gltfs.get(h).is_some());
    if !loaded {
        return;
    }
    let start = std::time::Instant::now();
    let remap = &assets.palette.remaps[REMAP_DETAIL_HIGH].entries;
    let mut ground = ground::Ground::build(&theater, &handles, &assets, &library.files, remap, &assets.palette.theater.day);
    let fields: Vec<airfield::Airfield> = ground
        .runways
        .clone()
        .iter()
        .enumerate()
        .map(|(i, rw)| airfield::plan(rw, &ground, network::hash(0xA1F1E1D, theater.index as i64, i as i64, 0)))
        .collect();
    for r in fields.iter().flat_map(airfield::footprint) {
        ground.claim(r);
    }
    let net = style.as_ref().map(|s| network::Network::build(&ground, s, &theater.towns)).unwrap_or_default();
    info!(
        "scenery: {} airfields, {} towns, {} roads, {} buildings, {} power lines in {:.2?}",
        fields.len(),
        net.towns.len(),
        net.roads.len(),
        net.buildings.len(),
        net.lines.len(),
        start.elapsed()
    );
    commands.insert_resource(PapiSites(fields.iter().flat_map(airfield::papi_sites).collect()));
    commands.insert_resource(Scenery { style, ground, net, airfields: fields });
}

fn cleanup(mut commands: Commands, mut blocks: ResMut<Blocks>, mut airfields: ResMut<Airfields>) {
    airfields.spawned = false;
    for e in blocks.0.drain().map(|(_, e)| e).chain(airfields.roots.drain(..)) {
        commands.entity(e).try_despawn();
    }
    commands.remove_resource::<Scenery>();
    commands.insert_resource(PapiSites::default());
}

fn stream_blocks(
    mut commands: Commands,
    scenery: Res<Scenery>,
    detail: Res<DetailLevel>,
    palette: Res<Palette>,
    anchor: Query<&WorldPos, With<WorldAnchor>>,
    mut blocks: ResMut<Blocks>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let Ok(anchor) = anchor.single() else { return };
    let (east, north, _) = bevy_to_wu(anchor.0);
    let eye = bevy::math::DVec2::new(east, north) * WU;
    let mut wanted = Vec::new();
    // Alt-D: detail 2 drops the trees, detail 0 and 1 all extra scenery.
    let fields = scenery.style.as_ref().is_some_and(|s| s.fields);
    let tiers: &[Tier] = match (scenery.style.is_some(), detail.0, fields) {
        (false, _, _) | (_, 0..=1, _) => &[],
        (_, 2, false) => &[Tier::Main],
        (_, 2, true) => &[Tier::Main, Tier::Fields],
        (_, _, false) => &[Tier::Main, Tier::Detail, Tier::Far],
        (_, _, true) => &[Tier::Main, Tier::Detail, Tier::Far, Tier::Fields],
    };
    for &tier in tiers {
        let r = radius(tier);
        let size = tier.block_size();
        let (lo, hi) = (((eye - r) / size).floor().as_ivec2(), ((eye + r) / size).floor().as_ivec2());
        for by in lo.y..=hi.y {
            for bx in lo.x..=hi.x {
                let c = (bevy::math::DVec2::new(bx as f64, by as f64) + 0.5) * size;
                let d = c.distance(eye);
                let near_trees = tier == Tier::Far && d < radius(Tier::Detail) + size * 0.5;
                if d < r + size * 0.5 && !near_trees {
                    wanted.push((d, (tier, bx, by)));
                }
            }
        }
    }
    let built: std::collections::HashSet<(Tier, i32, i32)> = blocks.0.keys().copied().collect();
    blocks.0.retain(|&(tier, bx, by), e| {
        let key = (tier, bx, by);
        // A tree block switching tiers stays until its replacement is built.
        let waiting = counterpart(tier).is_some_and(|other| {
            let k = (other, bx, by);
            !built.contains(&k) && wanted.iter().any(|(_, w)| *w == k)
        });
        let keep = waiting || wanted.iter().any(|(_, k)| *k == key);
        if !keep {
            commands.entity(*e).try_despawn();
        }
        keep
    });
    wanted.retain(|(_, k)| !blocks.0.contains_key(k));
    wanted.sort_by(|a, b| a.0.total_cmp(&b.0));
    for &(_, key @ (tier, bx, by)) in wanted.iter().take(BUILDS_PER_FRAME) {
        let geom = content::build(&scenery, bx, by, tier);
        let c = (bevy::math::DVec2::new(bx as f64, by as f64) + 0.5) * tier.block_size();
        let e = spawn_block(&mut commands, &mut meshes, &palette, geom, DVec3::new(c.x, 0.0, -c.y), tier);
        blocks.0.insert(key, e);
    }
}

fn mesh(topology: PrimitiveTopology, pos: Vec<Vec3>) -> Mesh {
    Mesh::new(topology, RenderAssetUsages::default()).with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
}

fn spawn_block(commands: &mut Commands, meshes: &mut Assets<Mesh>, palette: &Palette, geom: Geom, origin: DVec3, tier: Tier) -> Entity {
    // Haze like the original's LOD1 objects, starting a little further out (whole blocks share
    // their centre's haze level); farmland is ground and hazes like the coarse terrain levels.
    let tag = MeshTag(match tier {
        Tier::Fields => mesh_tag(3, 4, false),
        Tier::Main => mesh_tag(1, 6, false),
        _ => mesh_tag(1, 5, false),
    });
    let root = commands.spawn((SceneryBlock, WorldPos(origin), Transform::default(), Visibility::default())).id();
    let mut children = Vec::new();
    // Buildings and structures (main tier) are warm in the FLIR picture, trees are not.
    let solid_tag = if tier == Tier::Main { MeshTag(tag.0 | TAG_WARM) } else { tag.clone() };
    for (c, tris) in geom.solid {
        let e = commands.spawn((Mesh3d(meshes.add(mesh(PrimitiveTopology::TriangleList, tris))), MeshMaterial3d(palette.material(c)), solid_tag.clone())).id();
        // Distant trees stay out of the shadow maps.
        if tier == Tier::Far {
            commands.entity(e).insert(bevy::light::NotShadowCaster);
        }
        children.push(e);
    }
    for (c, tris) in geom.flat {
        children.push(
            commands
                .spawn((Mesh3d(meshes.add(mesh(PrimitiveTopology::TriangleList, tris))), MeshMaterial3d(palette.material(c)), tag.clone(), bevy::light::NotShadowCaster))
                .id(),
        );
    }
    for (c, lines) in geom.lines {
        children.push(
            commands
                .spawn((Mesh3d(meshes.add(mesh(PrimitiveTopology::LineList, lines))), MeshMaterial3d(palette.line_material(c)), tag.clone(), bevy::light::NotShadowCaster))
                .id(),
        );
    }
    let dots = (MeshTag(tag.0 | TAG_DOT), bevy::light::NotShadowCaster, Visibility::Hidden);
    // All lamps of the block in one mesh with per-vertex colours, so they keep their drawing
    // order (dots don't write depth: the later of two overlapping lamps is on top, e.g. a mast's
    // red beacon over its flood lamp).
    if let Some(&(first, _)) = geom.lamps.first() {
        let (colours, points): (Vec<u8>, Vec<Vec3>) = geom.lamps.into_iter().unzip();
        let m = meshes.add(dot_batch_coloured(&points, &colours));
        let radius = points.iter().map(|p| p.length()).fold(0.0, f32::max);
        children.push(commands.spawn((Mesh3d(m), MeshMaterial3d(palette.line_material(first)), dots.clone(), LampBatch { points, colours, radius })).id());
    }
    for (phase, points) in geom.strobes {
        let m = meshes.add(dot_batch(&points));
        children.push(commands.spawn((Mesh3d(m), MeshMaterial3d(palette.line_material(airfield::STROBE)), dots.clone(), Strobe(phase))).id());
    }
    commands.entity(root).add_children(&children);
    root
}

/// Lamps come on with the original's lights (sky dark enough); the approach flashers run
/// their sequence twice a second.
fn lamps_at_night(
    time: Res<Time>,
    palette: Res<Palette>,
    mut lamps: Query<&mut Visibility, (With<LampBatch>, Without<Strobe>)>,
    mut strobes: Query<(&Strobe, &mut Visibility), Without<LampBatch>>,
) {
    let on = palette.lights_on();
    let v = if on { Visibility::Inherited } else { Visibility::Hidden };
    for mut vis in &mut lamps {
        vis.set_if_neq(v);
    }
    let step = (time.elapsed_secs() * 2.0 * airfield::STROBE_PHASES as f32) as usize % airfield::STROBE_PHASES;
    for (s, mut vis) in &mut strobes {
        vis.set_if_neq(if on && s.0 == step { Visibility::Inherited } else { Visibility::Hidden });
    }
}

/// Builds the airfields once, then makes them follow the scenery setting (and Alt-D detail 0
/// hides them).
#[allow(clippy::too_many_arguments)]
fn airfields(
    mut commands: Commands,
    scenery: Res<Scenery>,
    detail: Res<DetailLevel>,
    palette: Res<Palette>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut airfields: ResMut<Airfields>,
    mut vis: Query<&mut Visibility, With<SceneryBlock>>,
) {
    if !airfields.spawned {
        airfields.spawned = true;
        for a in &scenery.airfields {
            let c = a.runway.c;
            let mut geom = airfield::build(a);
            let papi = std::mem::take(&mut geom.papi);
            let e = spawn_block(&mut commands, &mut meshes, &palette, geom, DVec3::new(c.x, 0.0, -c.y), Tier::Main);
            for l in papi {
                let light = commands
                    .spawn((
                        Mesh3d(meshes.add(dot_batch(&[Vec3::ZERO]))),
                        MeshMaterial3d(palette.line_material(airfield::PAPI_LOW)),
                        MeshTag(mesh_tag(1, 6, false) | TAG_DOT | dot_scale(PAPI_DOT_SCALE)),
                        Transform::from_translation(l.pos),
                        bevy::light::NotShadowCaster,
                        Papi { toward: l.toward, angle: l.angle },
                    ))
                    .id();
                commands.entity(e).add_child(light);
            }
            airfields.roots.push(e);
        }
    }
    let v = if detail.0 > 0 { Visibility::Inherited } else { Visibility::Hidden };
    for &e in &airfields.roots {
        if let Ok(mut vis) = vis.get_mut(e) {
            vis.set_if_neq(v);
        }
    }
}

/// PAPI: each unit shows white when the main view is above its angle, red below; seen only
/// from the approach side. Day and night, like the real lights.
#[allow(clippy::type_complexity)]
fn papi_lights(
    palette: Res<Palette>,
    cameras: Query<(&Camera, &GlobalTransform), (With<Camera3d>, Without<crate::trackcam::TrackCamera>)>,
    mut lights: Query<(&Papi, &GlobalTransform, &mut MeshMaterial3d<crate::palette_material::PaletteMaterial>, &mut Visibility)>,
) {
    // The main view (cockpit, external or free camera).
    let Some((_, eye)) = cameras.iter().find(|(c, _)| c.is_active) else { return };
    let eye = eye.translation();
    let (low, high) = (palette.line_material(airfield::PAPI_LOW), palette.line_material(airfield::PAPI_HIGH));
    for (papi, tf, mut material, mut vis) in &mut lights {
        let d = eye - tf.translation();
        let flat = Vec3::new(d.x, 0.0, d.z);
        let dist = flat.length();
        let seen = dist > 1.0 && dist < PAPI_RANGE && flat.dot(papi.toward) > dist * PAPI_COS_SPREAD;
        vis.set_if_neq(if seen { Visibility::Inherited } else { Visibility::Hidden });
        if seen {
            let want = if d.y.atan2(dist) > papi.angle { &high } else { &low };
            if material.0 != *want {
                material.0 = want.clone();
            }
        }
    }
}
