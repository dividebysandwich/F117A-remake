//! The theater world: coordinate systems, floating origin and tile-object streaming.
//!
//! Original coordinates:
//! - world units (wu): x east, y north, 0..2^20; altitude in the same unit. 1 wu = 0.488 m
//!   (the 2^20 wu theater is 512 "km" on the HUD).
//! - map units = wu >> 5, map y grows southward: wy = (0x8000 - map_y) << 5.
//!
//! Bevy: metres, X = east, Y = up, Z = -north. Positions are kept in f64 (`WorldPos`) and
//! rendered relative to a floating origin near the camera to avoid f32 jitter.
//!
//! The ground is made of theater models placed per tile (.3DG grid + .3DT lists). Like the
//! original, each detail level (LOD 1..4) shows the cells around the camera; finer levels
//! draw over coarser ones.

use std::collections::HashMap;

use bevy::math::DVec3;
use bevy::prelude::*;
use f117_formats::terrain::{GRID_DIM, Grid, LEVEL_UNIT, TileSets};
use f117_formats::wld::World;
use serde::Deserialize;

use crate::models::{ModelAssets, ModelLibrary};
use crate::settings::{AppState, MissionSetup, SetupSet};

fn load_theater(
    mut commands: Commands,
    setup: Res<MissionSetup>,
    mission: Option<Res<crate::mission::ActiveMission>>,
    mut streamed: ResMut<StreamedCells>,
    mut overrides: ResMut<TileOverrides>,
) {
    let theater = Theater::load(&setup.theater);
    streamed.cells.clear();
    *overrides = TileOverrides::default();
    let handles = ShapeHandles::new(&theater, mission.as_deref().map(|m| &m.0), &mut overrides);
    commands.insert_resource(handles);
    commands.insert_resource(theater);
}

/// Model file and index for each tile shape (the shape handle table): theater models, then the
/// mission's special target models from PHOTO.3DX appended after them, plus Desert Storm's two
/// fixed PHOTO substitutions.
#[derive(Resource, Clone, Default)]
pub struct ShapeHandles {
    pub models: Vec<(String, u16)>,
}

impl ShapeHandles {
    fn new(theater: &Theater, mission: Option<&f117_mission::Mission>, overrides: &mut TileOverrides) -> Self {
        let count = theater.model_count();
        let mut models: Vec<(String, u16)> = (0..count as u16).map(|i| (theater.id.clone(), i)).collect();
        if theater.index == 6 {
            // Desert Storm draws shapes 0x59 and 5 with PHOTO models 13 and 10.
            for (shape, photo) in [(0x59usize, 13u16), (5, 10)] {
                if shape < models.len() {
                    models[shape] = ("photo".into(), photo);
                }
            }
        }
        if let Some(m) = mission {
            for t in [&m.primary, &m.secondary] {
                let Some(special) = t.special_model.filter(|&s| s > 0) else { continue };
                let shape = models.len();
                models.push(("photo".into(), special as u16));
                // The target's tile object is replaced by the special model.
                if let Some((lod, col, row, idx)) = theater.find_tile_object(t.position.x, t.position.y) {
                    overrides.set(lod, col, row, idx, shape as i32);
                }
            }
        }
        Self { models }
    }

    pub fn model(&self, shape: usize) -> Option<(&str, String)> {
        let (file, idx) = self.models.get(shape)?;
        Some((file.as_str(), format!("{file}_{idx:03}")))
    }
}

fn unload_theater(mut commands: Commands, mut streamed: ResMut<StreamedCells>) {
    for (_, entities) in streamed.cells.drain() {
        for e in entities {
            commands.entity(e).try_despawn();
        }
    }
    commands.remove_resource::<Theater>();
}

/// Tile objects whose shape was changed by the combat simulation (destroyed buildings, rubble),
/// keyed by (LOD level, cell column, cell row, index in the cell's object list). The value is
/// the shape to draw: the tile's own shape | 0x100 = its wreck state, else a rubble shape.
/// Applied when a cell is streamed in; changed cells are re-streamed.
#[derive(Resource, Default)]
pub struct TileOverrides {
    pub shapes: HashMap<(usize, i32, i32, u16), i32>,
    dirty: Vec<(usize, i32, i32)>,
}

impl TileOverrides {
    pub fn set(&mut self, lod: usize, col: i32, row: i32, index: u16, shape: i32) {
        if self.shapes.insert((lod, col, row, index), shape) != Some(shape) {
            self.dirty.push((lod, col, row));
        }
    }
}

/// Metres per original world unit.
pub const WU: f64 = 512_000.0 / 1_048_576.0;
/// Height step separating coplanar ground overlays (metres).
const LAYER_STEP_M: f64 = 0.02;
/// Cells streamed around the camera per LOD (index = LOD), as a radius in cells.
const CELL_RADIUS: [i32; 5] = [0, 2, 2, 1, 2];

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FloatingOrigin>()
            .init_resource::<StreamedCells>()
            .init_resource::<TileOverrides>()
            .add_systems(OnEnter(AppState::Flight), load_theater.in_set(SetupSet::World))
            .add_systems(OnExit(AppState::Flight), unload_theater)
            .add_systems(Update, stream_tiles.run_if(resource_exists::<Theater>))
            .add_systems(
                PostUpdate,
                (recenter_origin, apply_world_positions)
                    .chain()
                    .before(bevy::transform::TransformSystems::Propagate),
            );
    }
}

/// Converts an original world position (wu east, wu north, wu altitude) to Bevy metres.
pub fn wu_to_bevy(east: f64, north: f64, alt: f64) -> DVec3 {
    DVec3::new(east * WU, alt * WU, -north * WU)
}

/// Bevy metres -> original world units (east, north, alt).
pub fn bevy_to_wu(p: DVec3) -> (f64, f64, f64) {
    (p.x / WU, -p.z / WU, p.y / WU)
}

/// Map units (from .WLD) -> world units (east, north).
pub fn map_to_wu(x: u16, y: u16) -> (f64, f64) {
    ((x as f64) * 32.0, (0x8000 as f64 - y as f64) * 32.0)
}

/// Absolute position in Bevy metres (f64). Transform.translation is derived from it.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct WorldPos(pub DVec3);

/// Entity whose position decides the floating origin and tile streaming (the viewpoint).
#[derive(Component)]
pub struct WorldAnchor;

#[derive(Resource, Default)]
pub struct FloatingOrigin {
    pub origin: DVec3,
}

/// Loaded theater data (assets/theaters/<id>.ron).
#[derive(Resource, Deserialize)]
pub struct Theater {
    pub id: String,
    #[allow(dead_code)] // part of the asset format
    pub index: usize,
    pub grid: Grid,
    pub tiles: TileSets,
    pub world: World,
    pub model_count: usize,
    /// Model attribute bytes (.3DX attr) by shape, for the simulation's wreck test. Only the
    /// remake's theaters carry them (the originals' are in the exported sim data).
    #[serde(default)]
    pub model_attr: Vec<u8>,
    /// Remake theaters: the front line, where the fighting goes on: points (metres east,
    /// north) with the intensity of the fighting on the stretch after them (0..1).
    #[serde(default)]
    pub front: Vec<(f64, f64, f64)>,
    /// Remake theaters: the real towns for the scenery layout.
    #[serde(default)]
    pub towns: Vec<TownSite>,
}

/// A town of a remake theater: centre (metres east, north), radius (metres), size class
/// (0 village, 1 town, 2 city), and whether it lies in ruins (on or behind the front line).
#[derive(Deserialize, Clone, Debug)]
pub struct TownSite {
    pub x: f64,
    pub y: f64,
    pub r: f64,
    pub kind: u8,
    #[serde(default)]
    pub ruined: f64,
}

impl Theater {
    pub fn load(id: &str) -> Theater {
        let path = crate::asset_path::asset(&format!("theaters/{id}.ron"));
        let p = path.display();
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{p}: {e}"));
        ron::from_str(&text).unwrap_or_else(|e| panic!("{p}: {e}"))
    }

    /// Tile data for the simulation (`findNearestTile`), for theaters not in the exported sim data.
    pub fn sim_tiles(&self) -> f117_sim::TheaterTiles {
        let level = |l: usize| -> Vec<Vec<(i16, i16, u8)>> {
            self.tiles.tiles.get(l).map(|v| v.iter().map(|t| t.iter().map(|o| (o.x, o.y, o.shape)).collect()).collect()).unwrap_or_default()
        };
        f117_sim::TheaterTiles { grid: self.grid.clone(), objects: [level(1), level(2)], model_attr: self.model_attr.clone() }
    }

    /// Number of theater models (shapes beyond this index are appended specials).
    pub fn model_count(&self) -> usize {
        self.model_count
    }

    /// The tile object (LOD 1..3) exactly at a map position, as the mission generator places
    /// targets on tile objects: (lod, col, row, index in the cell's list).
    pub fn find_tile_object(&self, mx: i32, my: i32) -> Option<(usize, i32, i32, u16)> {
        for lod in 1..=3usize {
            let unit = LEVEL_UNIT[lod] as i64;
            for row in 0..GRID_DIM[lod] {
                for col in 0..GRID_DIM[lod] {
                    let t = self.grid.tile(lod, col, row) as usize;
                    let Some(objs) = self.tiles.tiles.get(lod).and_then(|l| l.get(t)) else { continue };
                    for (k, o) in objs.iter().enumerate() {
                        let east = (col as i64 * 4096 + 2048 + o.x as i64) * unit;
                        let north = (row as i64 * 4096 + 2048 + o.y as i64) * unit;
                        if (east >> 5) as i32 == mx && 0x8000 - (north >> 5) as i32 == my {
                            return Some((lod, col, row, k as u16));
                        }
                    }
                }
            }
        }
        None
    }

    /// Position of a named WLD object in Bevy metres (ground level).
    pub fn object_position(&self, name: &str) -> Option<DVec3> {
        let o = self.world.objects.iter().find(|o| self.world.name(o.name).eq_ignore_ascii_case(name))?;
        let (e, n) = map_to_wu(o.x, o.y);
        Some(wu_to_bevy(e, n, 0.0))
    }
}

#[derive(Resource, Default)]
struct StreamedCells {
    cells: HashMap<(usize, i32, i32), Vec<Entity>>,
    pending: bool,
}

fn recenter_origin(mut origin: ResMut<FloatingOrigin>, anchor: Query<&WorldPos, With<WorldAnchor>>) {
    let Ok(p) = anchor.single() else { return };
    // Re-centre when the anchor is more than 2 km away from the origin.
    if (p.0 - origin.origin).length() > 2000.0 {
        origin.origin = DVec3::new(p.0.x.round(), 0.0, p.0.z.round());
    }
}

fn apply_world_positions(
    origin: Res<FloatingOrigin>,
    mut q: Query<(Ref<WorldPos>, &mut Transform)>,
) {
    let all = origin.is_changed();
    for (p, mut tf) in &mut q {
        if all || p.is_changed() {
            tf.translation = (p.0 - origin.origin).as_vec3();
        }
    }
}

/// Spawns/despawns tile objects around the anchor, per LOD level.
fn stream_tiles(
    mut commands: Commands,
    theater: Res<Theater>,
    mut library: ResMut<ModelLibrary>,
    server: Res<AssetServer>,
    assets: ModelAssets,
    anchor: Query<&WorldPos, With<WorldAnchor>>,
    mut streamed: ResMut<StreamedCells>,
    mut overrides: ResMut<TileOverrides>,
    handles: Res<ShapeHandles>,
) {
    let Ok(anchor) = anchor.single() else { return };
    let file = library.load(&server, &theater.id);
    let photo = library.load(&server, "photo");
    if assets.gltfs.get(&photo).is_none() || !assets.terrain.ready(&theater.id) {
        return; // models still loading or being refined (terrain.rs)
    }
    let (east, north, _) = bevy_to_wu(anchor.0);
    let mut wanted = Vec::new();
    for lod in 1..=4usize {
        let cell = 4096.0 * LEVEL_UNIT[lod];
        let (cx, cy) = ((east / cell).floor() as i32, (north / cell).floor() as i32);
        let lo = if lod == 4 { -2 } else { 0 };
        let r = CELL_RADIUS[lod];
        for row in cy - r..=cy + r {
            for col in cx - r..=cx + r {
                if col >= lo && row >= lo && col < GRID_DIM[lod] + lo && row < GRID_DIM[lod] + lo {
                    wanted.push((lod, col, row));
                }
            }
        }
    }
    let dirty = std::mem::take(&mut overrides.dirty);
    streamed.cells.retain(|key, entities| {
        let keep = wanted.contains(key) && !dirty.contains(key);
        if !keep {
            for &e in entities.iter() {
                commands.entity(e).despawn();
            }
        }
        keep
    });
    for key @ (lod, col, row) in wanted {
        if streamed.cells.contains_key(&key) {
            continue;
        }
        let tile_type = theater.grid.tile(lod, col, row) as usize;
        let objects = theater.tiles.tiles.get(lod).and_then(|t| t.get(tile_type)).cloned().unwrap_or_default();
        let unit = LEVEL_UNIT[lod];
        let centre = |c: i32| (c as f64 * 4096.0 + 2048.0) * unit;
        let mut entities = Vec::with_capacity(objects.len());
        for (k, o) in objects.iter().enumerate() {
            // Destroyed tile objects: the same model in its wreck state, or a rubble model.
            let (shape, wrecked) = match overrides.shapes.get(&(lod, col, row, k as u16)) {
                Some(&s) if (s & 0xFF) as u8 == o.shape && s & 0x100 != 0 => (o.shape, true),
                Some(&s) => ((s & 0xFF) as u8, false),
                None => (o.shape, false),
            };
            let Some((model_file, name)) = handles.model(shape as usize) else { continue };
            let gltf = if model_file == "photo" { &photo } else { &file };
            let Some(e) = assets.spawn(&mut commands, gltf, &name, lod as u32, false) else { continue };
            if wrecked {
                let mut state = crate::models::ModelState::default();
                state.0[1] = 1;
                commands.entity(e).insert(state);
            }
            let mut pos = wu_to_bevy(
                centre(col) + o.x as f64 * unit,
                centre(row) + o.y as f64 * unit,
                o.z as f64 * unit,
            );
            // Painter's order for coplanar ground overlays: finer levels and later list
            // entries were drawn on top in the original; lift them by a few centimetres.
            if o.z == 0 {
                pos.y += (4 - lod) as f64 * LAYER_STEP_M * 8.0 + k as f64 * LAYER_STEP_M;
            }
            commands.entity(e).insert((
                WorldPos(pos),
                Transform::from_scale(Vec3::splat((unit * WU) as f32)),
                TileObject { lod: lod as u8, order: k as u16, destroyed: shape != o.shape || wrecked },
            ));
            entities.push(e);
        }
        streamed.cells.insert(key, entities);
        streamed.pending = true;
    }
}

/// A streamed tile object (ground patch, building, ...).
#[derive(Component)]
#[allow(dead_code)] // lod / order: for debugging and tools
pub struct TileObject {
    pub lod: u8,
    /// Position in the tile's object list (original draw order for flat ground patches).
    pub order: u16,
    /// Drawn as wreck or rubble (lights off).
    pub destroyed: bool,
}
