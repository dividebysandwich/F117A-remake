//! The theater world: coordinate systems, floating origin and tile-object streaming.
//!
//! Sim coordinates:
//! - world units (wu): x east, y north, 0..2^20; altitude in the same unit. 1 wu = 0.488 m
//!   (the 2^20 wu theater is 512 "km" on the HUD).
//! - map units = wu >> 5, map y grows southward: wy = (0x8000 - map_y) << 5.
//!
//! Bevy: metres, X = east, Y = up, Z = -north. Positions are kept in f64 (`WorldPos`) and
//! rendered relative to a floating origin near the camera to avoid f32 jitter.
//!
//! The ground is made of theater models placed per tile (tile grid + per-tile object lists). Each detail
//! level (LOD 1..4) shows the cells around the camera; finer levels draw over coarser ones.

use std::collections::HashMap;

use bevy::math::DVec3;
use bevy::prelude::*;
use serde::Deserialize;
use stealth_formats::terrain::{GRID_DIM, Grid, LEVEL_UNIT, TileSets};
use stealth_formats::wld::World;

use crate::models::{ModelAssets, ModelLibrary};
use crate::settings::{AppState, MissionSetup, SetupSet};

fn load_theater(
    mut commands: Commands,
    setup: Res<MissionSetup>,
    mission: Option<Res<crate::mission::ActiveMission>>,
    mut streamed: ResMut<StreamedCells>,
    mut overrides: ResMut<TileOverrides>,
) {
    let mission = mission.as_deref().map(|m| &m.0);
    insert_theater(&mut commands, &setup.theater, mission, &mut streamed, &mut overrides);
}

/// Loads a theater for drawing (a mission's special target models in it).
pub(crate) fn insert_theater(
    commands: &mut Commands,
    id: &str,
    mission: Option<&stealth_mission::Mission>,
    streamed: &mut StreamedCells,
    overrides: &mut TileOverrides,
) {
    let theater = Theater::load(id);
    streamed.cells.clear();
    *overrides = TileOverrides::default();
    let handles = ShapeHandles::new(&theater, mission, overrides);
    commands.insert_resource(handles);
    commands.insert_resource(Ships(theater.ships()));
    commands.insert_resource(theater);
}

/// Model file and index for each tile shape (the shape handle table): theater models, then the
/// mission's special target models (photo.glb) appended after them, plus Desert Storm's two
/// fixed PHOTO substitutions.
#[derive(Resource, Clone, Default)]
pub struct ShapeHandles {
    pub models: Vec<(String, u16)>,
}

impl ShapeHandles {
    fn new(
        theater: &Theater,
        mission: Option<&stealth_mission::Mission>,
        overrides: &mut TileOverrides,
    ) -> Self {
        let count = theater.model_count();
        let mut models: Vec<(String, u16)> =
            (0..count as u16).map(|i| (theater.id.clone(), i)).collect();
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
                if let Some((lod, col, row, idx)) =
                    theater.find_tile_object(t.position.x, t.position.y)
                {
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

pub(crate) fn unload_theater(mut commands: Commands, mut streamed: ResMut<StreamedCells>) {
    for (_, (_, entities)) in streamed.cells.drain() {
        for e in entities {
            commands.entity(e).try_despawn();
        }
    }
    commands.remove_resource::<Theater>();
}

/// A ship among the theater's tile objects.
#[derive(Debug, Clone, Copy)]
pub struct ShipSite {
    /// (LOD level, cell column, cell row, index in the cell's list), as `TileOverrides`.
    pub key: (usize, i32, i32, u16),
    pub shape: u8,
    /// World units east, north.
    pub east: f64,
    pub north: f64,
}

impl ShipSite {
    /// Map units (y south).
    pub fn map_xy(&self) -> (i32, i32) {
        ((self.east / 32.0).round() as i32, 0x8000 - (self.north / 32.0).round() as i32)
    }
}

/// The theater's ships (tile objects), for the displays.
#[derive(Resource, Default)]
pub struct Ships(pub Vec<ShipSite>);

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

    /// The tile object is drawn as built again (a campaign repair).
    pub fn clear(&mut self, lod: usize, col: i32, row: i32, index: u16) {
        if self.shapes.remove(&(lod, col, row, index)).is_some() {
            self.dirty.push((lod, col, row));
        }
    }
}

/// Metres per world unit.
pub const WU: f64 = 512_000.0 / 1_048_576.0;

/// Longitude and latitude of a theater's south-west corner, the map laid on the globe at game
/// scale around one real airbase (Tacview, the sun and the moon). The game's maps squeeze the
/// real distances, so places away from that airbase land only roughly where they really are.
pub fn geo_origin(id: &str) -> (f64, f64) {
    match id {
        // Mittenwalde.
        "ce" => (10.84, 50.34),
        // Havana.
        "cu" => (-83.91, 19.15),
        // Pyongyang.
        "ko" => (125.04, 38.26),
        // Baghdad.
        "ku" => (42.85, 30.05),
        // Tripoli.
        "lb" => (12.49, 30.24),
        // Homs.
        "me" => (35.09, 32.62),
        // Monchegorsk.
        "nc" => (25.51, 66.10),
        // Bushehr.
        "pg" => (48.67, 26.48),
        // Dzhankoi.
        "ua" => (32.51, 44.78),
        // Gia Lam (Hanoi).
        "vn" => (101.84, 17.04),
        _ => (10.0, 45.0),
    }
}
/// Height step separating coplanar ground overlays (metres).
const LAYER_STEP_M: f64 = 0.02;
/// Cells streamed around the camera per LOD (index = LOD), as a radius in cells.
const CELL_RADIUS: [i32; 5] = [0, 2, 2, 1, 2];
/// Ships (on the open sea, seen from far away) are streamed out to about this range (metres)
/// from the finer levels' cells, which otherwise end a few kilometres out.
const SHIP_RANGE_M: f64 = 25_000.0;
/// Ships' detail levels are drawn this many times further than other models'.
const SHIP_LOD_BIAS: f32 = 4.0;

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FloatingOrigin>()
            .init_resource::<StreamedCells>()
            .init_resource::<StreamSites>()
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

/// Converts a world position (wu east, wu north, wu altitude) to Bevy metres.
pub fn wu_to_bevy(east: f64, north: f64, alt: f64) -> DVec3 {
    DVec3::new(east * WU, alt * WU, -north * WU)
}

/// Bevy metres -> world units (east, north, alt).
pub fn bevy_to_wu(p: DVec3) -> (f64, f64, f64) {
    (p.x / WU, -p.z / WU, p.y / WU)
}

/// Map units (theater data) -> world units (east, north).
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
    /// Model attribute bytes (`attr`) by shape, for the simulation's wreck test. Only the
    /// remake's theaters carry them (the others' are in the exported sim data).
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
        crate::asset_path::load_ron(&format!("theaters/{id}.ron"))
    }

    /// Tile data for the simulation (`findNearestTile`), for theaters not in the exported sim data.
    pub fn sim_tiles(&self) -> stealth_sim::TheaterTiles {
        let level = |l: usize| -> Vec<Vec<(i16, i16, u8)>> {
            self.tiles
                .tiles
                .get(l)
                .map(|v| {
                    v.iter().map(|t| t.iter().map(|o| (o.x, o.y, o.shape)).collect()).collect()
                })
                .unwrap_or_default()
        };
        stealth_sim::TheaterTiles {
            grid: self.grid.clone(),
            objects: [level(1), level(2)],
            model_attr: self.model_attr.clone(),
        }
    }

    /// The ships among the tile objects of the finer levels (where the theaters place them).
    pub fn ships(&self) -> Vec<ShipSite> {
        let mut out = Vec::new();
        for lod in 1..=2usize {
            let unit = LEVEL_UNIT[lod];
            for row in 0..GRID_DIM[lod] {
                for col in 0..GRID_DIM[lod] {
                    let t = self.grid.tile(lod, col, row) as usize;
                    let Some(objs) = self.tiles.tiles.get(lod).and_then(|l| l.get(t)) else {
                        continue;
                    };
                    for (k, o) in
                        objs.iter().enumerate().filter(|(_, o)| self.is_ship_shape(o.shape))
                    {
                        let centre = |c: i32| (c as f64 * 4096.0 + 2048.0) * unit;
                        out.push(ShipSite {
                            key: (lod, col, row, k as u16),
                            shape: o.shape,
                            east: centre(col) + o.x as f64 * unit,
                            north: centre(row) + o.y as f64 * unit,
                        });
                    }
                }
            }
        }
        out
    }

    /// A ship's model (the world's object category 12 or 13, as the simulation's ship test).
    pub fn is_ship_shape(&self, shape: u8) -> bool {
        self.world
            .shape_class
            .get((shape & 0x7F) as usize)
            .is_some_and(|&b| matches!(b & 0xF, 12 | 13))
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
                    let Some(objs) = self.tiles.tiles.get(lod).and_then(|l| l.get(t)) else {
                        continue;
                    };
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
        let o = self
            .world
            .objects
            .iter()
            .find(|o| self.world.name(o.name).eq_ignore_ascii_case(name))?;
        let (e, n) = map_to_wu(o.x, o.y);
        Some(wu_to_bevy(e, n, 0.0))
    }
}

#[derive(Resource, Default)]
pub(crate) struct StreamedCells {
    /// (lod, col, row) -> (only the ships spawned, the entities).
    cells: HashMap<(usize, i32, i32), (bool, Vec<Entity>)>,
    pending: bool,
}

fn recenter_origin(
    mut origin: ResMut<FloatingOrigin>,
    anchor: Query<&WorldPos, With<WorldAnchor>>,
) {
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

/// Places streamed like the anchor while something looks at them from afar (the recon photos
/// of the targets): their own cells of each level, without the ships' range.
#[derive(Resource, Default)]
pub struct StreamSites {
    pub points: Vec<DVec3>,
    /// A unit watched from close by (the campaign's unit view), streamed while watched.
    pub follow: Option<DVec3>,
    /// The ground point the tracking camera (targeting pod) looks at, streamed while it does.
    pub pod: Option<DVec3>,
    /// The tile objects around `points` are spawned.
    pub streamed: bool,
}

/// Spawns/despawns tile objects around the anchor and the stream sites, per LOD level.
#[allow(clippy::too_many_arguments)]
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
    mut sites: ResMut<StreamSites>,
) {
    let Ok(anchor) = anchor.single() else { return };
    let file = library.load(&server, &theater.id);
    let photo = library.load(&server, "photo");
    if assets.gltfs.get(&photo).is_none() || !assets.terrain.ready(&theater.id) {
        return; // models still loading or being refined (terrain.rs)
    }
    // Cells wanted: (lod, col, row) -> ships only (beyond the level's own radius).
    let mut wanted: HashMap<(usize, i32, i32), bool> = HashMap::new();
    let eyes = std::iter::once((anchor.0, true))
        .chain(sites.points.iter().chain(&sites.follow).chain(&sites.pod).map(|&p| (p, false)));
    for (eye, ships) in eyes {
        let (east, north, _) = bevy_to_wu(eye);
        for lod in 1..=4usize {
            let cell = 4096.0 * LEVEL_UNIT[lod];
            let (cx, cy) = ((east / cell).floor() as i32, (north / cell).floor() as i32);
            let lo = if lod == 4 { -2 } else { 0 };
            let r = CELL_RADIUS[lod];
            let far = if ships && lod <= 2 {
                ((SHIP_RANGE_M / (cell * WU)).ceil() as i32).max(r)
            } else {
                r
            };
            for row in cy - far..=cy + far {
                for col in cx - far..=cx + far {
                    if col >= lo
                        && row >= lo
                        && col < GRID_DIM[lod] + lo
                        && row < GRID_DIM[lod] + lo
                    {
                        let ships_only = (col - cx).abs() > r || (row - cy).abs() > r;
                        *wanted.entry((lod, col, row)).or_insert(ships_only) &= ships_only;
                    }
                }
            }
        }
    }
    let dirty = std::mem::take(&mut overrides.dirty);
    streamed.cells.retain(|key, (ships_only, entities)| {
        let keep = wanted.get(key) == Some(ships_only) && !dirty.contains(key);
        if !keep {
            for &e in entities.iter() {
                commands.entity(e).despawn();
            }
        }
        keep
    });
    for (key @ (lod, col, row), ships_only) in wanted {
        if streamed.cells.contains_key(&key) {
            continue;
        }
        let tile_type = theater.grid.tile(lod, col, row) as usize;
        let objects = theater
            .tiles
            .tiles
            .get(lod)
            .and_then(|t| t.get(tile_type))
            .cloned()
            .unwrap_or_default();
        let unit = LEVEL_UNIT[lod];
        let centre = |c: i32| (c as f64 * 4096.0 + 2048.0) * unit;
        let mut entities = Vec::new();
        for (k, o) in objects.iter().enumerate() {
            let ship = theater.is_ship_shape(o.shape);
            if ships_only && !ship {
                continue;
            }
            // Destroyed tile objects: the same model in its wreck state, or a rubble model.
            let (shape, wrecked) = match overrides.shapes.get(&(lod, col, row, k as u16)) {
                Some(&s) if (s & 0xFF) as u8 == o.shape && s & 0x100 != 0 => (o.shape, true),
                Some(&s) => ((s & 0xFF) as u8, false),
                None => (o.shape, false),
            };
            let Some((model_file, name)) = handles.model(shape as usize) else { continue };
            let gltf = if model_file == "photo" { &photo } else { &file };
            let Some(e) = assets.spawn(&mut commands, gltf, &name, lod as u32, false) else {
                continue;
            };
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
            // entries go on top; lift them by a few centimetres.
            if o.z == 0 {
                pos.y += (4 - lod) as f64 * LAYER_STEP_M * 8.0 + k as f64 * LAYER_STEP_M;
            }
            commands.entity(e).insert((
                WorldPos(pos),
                Transform::from_scale(Vec3::splat(crate::models::tile_model_scale(&name, unit))),
                TileObject {
                    lod: lod as u8,
                    order: k as u16,
                    destroyed: shape != o.shape || wrecked,
                },
            ));
            if ship {
                commands.entity(e).insert(crate::models::LodBias(SHIP_LOD_BIAS));
            }
            entities.push(e);
        }
        streamed.cells.insert(key, (ships_only, entities));
        streamed.pending = true;
    }
    if !sites.streamed {
        sites.streamed = true;
    }
}

/// A streamed tile object (ground patch, building, ...).
#[derive(Component)]
#[allow(dead_code)] // lod / order: for debugging and tools
pub struct TileObject {
    pub lod: u8,
    /// Position in the tile's object list (draw order for flat ground patches).
    pub order: u16,
    /// Drawn as wreck or rubble (lights off).
    pub destroyed: bool,
}
