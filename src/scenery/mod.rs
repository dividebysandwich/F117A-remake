//! Extra scenery (not in the original). Every theater's airfields get taxiways, aprons,
//! shelters, support buildings and airfield lighting (`airfield`). The green theaters also get
//! towns with street grids and street lamps, roads between them and along the original road
//! lines, farms, industry, transmission and pole lines, radio masts, forests and lone trees;
//! the desert theaters otherwise keep their original look.
//!
//! The layout is generated once per theater (`network`), clear of water, concrete and the
//! theater's own objects (`ground`). Around the viewpoint, 1 km blocks are built into one mesh
//! per palette colour (`content`), drawn with the palette materials and haze like the theater
//! models; airfields are built whole at the start. Lamps are batched dots, lit when the
//! original's lights come on.

mod airfield;
mod content;
mod geom;
mod ground;
mod network;
mod style;

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::math::DVec3;
use bevy::mesh::{Indices, MeshTag, PrimitiveTopology};
use bevy::prelude::*;

use crate::models::{ModelAssets, ModelLibrary};
use crate::palette::{DetailLevel, Palette, REMAP_DETAIL_HIGH};
use crate::palette_material::{TAG_DOT, mesh_tag};
use crate::settings::AppState;
use crate::world::{ShapeHandles, Theater, WorldAnchor, WorldPos, bevy_to_wu, WU};
use content::{BLOCK, Tier};
use geom::Geom;

pub struct SceneryPlugin;

impl Plugin for SceneryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Blocks>().init_resource::<Airfields>().add_systems(OnExit(AppState::Flight), cleanup).add_systems(
            Update,
            (
                build_scenery.run_if(resource_exists::<Theater>).run_if(not(resource_exists::<Scenery>)),
                (stream_blocks, airfields, lamps_at_night).run_if(resource_exists::<Scenery>),
            )
                .chain()
                .run_if(in_state(AppState::Flight)),
        );
    }
}

/// Streaming radius per tier (metres).
fn radius(tier: Tier) -> f64 {
    match tier {
        Tier::Main => 7000.0,
        Tier::Detail => 3500.0,
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

#[derive(Resource, Default)]
struct Blocks(HashMap<(Tier, i32, i32), Entity>);

/// Root entities of the airfields (built whole, shown with the scenery setting).
#[derive(Resource, Default)]
struct Airfields {
    spawned: bool,
    roots: Vec<Entity>,
}

/// A batch of approach flashers lit in its turn of the sequence.
#[derive(Component)]
struct Strobe(usize);

/// Root of a streamed block.
#[derive(Component)]
struct SceneryBlock;

/// Batched lamp dots of a block (block-local positions), lit at night.
#[derive(Component)]
pub struct LampBatch {
    pub colour: u8,
    pub points: Vec<Vec3>,
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
    let loaded = [theater.id.as_str(), "photo"].iter().all(|f| library.files.get(*f).is_some_and(|h| assets.gltfs.get(h).is_some()));
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
    let net = style.as_ref().map(|s| network::Network::build(&ground, s)).unwrap_or_default();
    info!(
        "scenery: {} airfields, {} towns, {} roads, {} buildings, {} power lines in {:.2?}",
        fields.len(),
        net.towns.len(),
        net.roads.len(),
        net.buildings.len(),
        net.lines.len(),
        start.elapsed()
    );
    commands.insert_resource(Scenery { style, ground, net, airfields: fields });
}

fn cleanup(mut commands: Commands, mut blocks: ResMut<Blocks>, mut airfields: ResMut<Airfields>) {
    airfields.spawned = false;
    for e in blocks.0.drain().map(|(_, e)| e).chain(airfields.roots.drain(..)) {
        commands.entity(e).try_despawn();
    }
    commands.remove_resource::<Scenery>();
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
    let tiers: &[Tier] = match (scenery.style.is_some(), detail.0) {
        (false, _) | (_, 0..=1) => &[],
        (_, 2) => &[Tier::Main],
        _ => &[Tier::Main, Tier::Detail],
    };
    for &tier in tiers {
        let r = radius(tier);
        let (lo, hi) = (((eye - r) / BLOCK).floor().as_ivec2(), ((eye + r) / BLOCK).floor().as_ivec2());
        for by in lo.y..=hi.y {
            for bx in lo.x..=hi.x {
                let c = (bevy::math::DVec2::new(bx as f64, by as f64) + 0.5) * BLOCK;
                let d = c.distance(eye);
                if d < r + BLOCK * 0.5 {
                    wanted.push((d, (tier, bx, by)));
                }
            }
        }
    }
    blocks.0.retain(|key, e| {
        let keep = wanted.iter().any(|(_, k)| k == key);
        if !keep {
            commands.entity(*e).try_despawn();
        }
        keep
    });
    wanted.retain(|(_, k)| !blocks.0.contains_key(k));
    wanted.sort_by(|a, b| a.0.total_cmp(&b.0));
    for &(_, key @ (tier, bx, by)) in wanted.iter().take(BUILDS_PER_FRAME) {
        let geom = content::build(&scenery, bx, by, tier);
        let c = (bevy::math::DVec2::new(bx as f64, by as f64) + 0.5) * BLOCK;
        let e = spawn_block(&mut commands, &mut meshes, &palette, geom, DVec3::new(c.x, 0.0, -c.y), tier);
        blocks.0.insert(key, e);
    }
}

fn mesh(topology: PrimitiveTopology, pos: Vec<Vec3>) -> Mesh {
    Mesh::new(topology, RenderAssetUsages::default()).with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
}

fn spawn_block(commands: &mut Commands, meshes: &mut Assets<Mesh>, palette: &Palette, geom: Geom, origin: DVec3, tier: Tier) -> Entity {
    // Haze like the original's LOD1 objects, starting a little further out (whole blocks share
    // their centre's haze level).
    let tag = MeshTag(mesh_tag(1, if tier == Tier::Detail { 5 } else { 6 }, false));
    let root = commands.spawn((SceneryBlock, WorldPos(origin), Transform::default(), Visibility::default())).id();
    let mut children = Vec::new();
    for (c, tris) in geom.solid {
        children.push(commands.spawn((Mesh3d(meshes.add(mesh(PrimitiveTopology::TriangleList, tris))), MeshMaterial3d(palette.material(c)), tag.clone())).id());
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
    for (c, points) in geom.lamps {
        let m = meshes.add(dot_batch(&points));
        let radius = points.iter().map(|p| p.length()).fold(0.0, f32::max);
        children.push(commands.spawn((Mesh3d(m), MeshMaterial3d(palette.line_material(c)), dots.clone(), LampBatch { colour: c, points, radius })).id());
    }
    for (phase, points) in geom.strobes {
        let m = meshes.add(dot_batch(&points));
        children.push(commands.spawn((Mesh3d(m), MeshMaterial3d(palette.line_material(airfield::STROBE)), dots.clone(), Strobe(phase))).id());
    }
    commands.entity(root).add_children(&children);
    root
}

/// Four vertices per dot at its centre; the shader spreads them by the quad corner.
fn dot_batch(points: &[Vec3]) -> Mesh {
    let pos: Vec<Vec3> = points.iter().flat_map(|&p| [p; 4]).collect();
    let corners: Vec<[f32; 2]> = points.iter().flat_map(|_| [[-0.5, -0.5], [0.5, -0.5], [-0.5, 0.5], [0.5, 0.5]]).collect();
    let idx: Vec<u32> = (0..points.len() as u32).flat_map(|i| [0, 1, 3, 0, 3, 2].map(|k| 4 * i + k)).collect();
    mesh(PrimitiveTopology::TriangleList, pos).with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, corners).with_inserted_indices(Indices::U32(idx))
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
            let e = spawn_block(&mut commands, &mut meshes, &palette, airfield::build(a), DVec3::new(c.x, 0.0, -c.y), Tier::Main);
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
