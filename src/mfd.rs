//! Multi-function displays (docs/re/cockpit.md section 3).
//!
//! Left MFD (x 74..150, y 110..169), F3 toggles:
//! - MAP: top-down render of the theater tile models through the 16-colour map remap table
//!   (VGAME `1A18:8387` / `119C`), north up, centred ahead of the aircraft; re-rendered on zoom or
//!   mode changes and when the aircraft leaves the display (like the original). CPU-rasterised
//!   straight into palette indices from the theater model meshes.
//! - TAC: heading-up tactical display with a 1024-map-unit grid (`1A18:8C10`).
//!
//! Right MFD (x 169..245, y 110..169): tracking camera (TC), WAY, WPN, ORD pages.
//! Z/X zoom the map (2..9) or the TAC display (0..7).

use std::collections::HashMap;

use bevy::ecs::system::SystemParam;
use bevy::gltf::{Gltf, GltfMaterial, GltfMesh, GltfNode};
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;
use f117_formats::terrain::{GRID_DIM, LEVEL_UNIT};

use crate::canvas::Canvas;
use crate::cockpit::CockpitArt;
use crate::flight::{Aircraft, Player, to_bam};
use crate::font::{Fonts, ink, upper};
use crate::hud::{HudState, Stores, Waypoints, map_units};
use crate::input::{Action, ActionEvent};
use crate::models::ModelLibrary;
use crate::palette::{Palette, index_from_name};
use crate::world::{Theater, WU, WorldPos, bevy_to_wu};

pub struct MfdPlugin;

impl Plugin for MfdPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MfdState>()
            .init_resource::<MissionOrders>()
            .init_resource::<TrackCam>()
            .init_resource::<Contacts>()
            .init_resource::<MissionClock>()
            .init_resource::<MapRaster>()
            .add_systems(OnEnter(crate::settings::AppState::Flight), reset_mfd)
            .add_systems(
                Update,
                (mfd_keys, (build_map_geometry, render_map).chain())
                    .chain()
                    .after(crate::flight::FlightSystems)
                    .run_if(in_state(crate::settings::AppState::Flight)),
            );
    }
}

/// MFD screen rectangles (inclusive).
pub const LEFT: [i32; 4] = [74, 110, 150, 169];
pub const RIGHT: [i32; 4] = [169, 110, 245, 169];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LeftMfd {
    #[default]
    Map,
    Tac,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RightMfd {
    /// Tracking camera (TC, `DS:DFDC` = 0x13).
    Camera,
    Way,
    Wpn,
    #[default]
    Ord,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CamDir {
    #[default]
    Ahead,
    Right,
    Rear,
    Left,
}

#[derive(Resource, Debug, Clone)]
pub struct MfdState {
    pub left: LeftMfd,
    pub right: RightMfd,
    /// Map zoom 2 (whole theater) ..= 9 (`DS:4080`).
    pub map_zoom: i32,
    /// TAC zoom 0..=7 (`DS:4082`).
    pub tac_zoom: i32,
    pub cam_dir: CamDir,
    /// F6: FLIR on the camera page.
    pub flir: bool,
}

impl Default for MfdState {
    fn default() -> Self {
        Self { left: LeftMfd::Map, right: RightMfd::Ord, map_zoom: 2, tac_zoom: 1, cam_dir: CamDir::Ahead, flir: false }
    }
}

/// Mission orders shown on the ORD page: (colour, text) lines, upper-cased when drawn.
/// Original colours: primary target 17, secondary 41, return-to-base 10. Set from the mission.
#[derive(Resource, Debug, Clone)]
pub struct MissionOrders {
    pub lines: Vec<(u8, String)>,
}

impl Default for MissionOrders {
    fn default() -> Self {
        Self { lines: vec![(10, "RETURN TO BASE".into())] }
    }
}

/// Tracking camera information (right MFD TC page), filled from the combat simulation's
/// target selection. The picture itself is rendered by a second camera (`trackcam.rs`) into a
/// texture shown behind the MFD hole; `live` = that picture is up, so the page background is
/// left see-through.
#[derive(Resource, Debug, Clone, Default)]
pub struct TrackCam {
    pub live: bool,
    /// Target name, two centred lines.
    pub name: [String; 2],
    pub range_km: Option<f32>,
    pub bearing: Option<i32>,
    /// Zoom factor shown as "3X" (0 = hidden).
    pub zoom: u8,
}

/// Other objects for the map and TAC displays (filled by the combat simulation).
#[derive(Resource, Debug, Clone, Default)]
pub struct Contacts {
    pub list: Vec<Contact>,
    /// Radar coverage arc of the sweep being displayed (MAP display).
    pub radar_arc: Option<MapArc>,
}

/// A radar's coverage on the MAP display: radius `range * (EMV + score) / 100`, a full circle
/// while the site is searching, +-45 degrees towards the player once alerted.
#[derive(Debug, Clone, Copy)]
pub struct MapArc {
    /// Centre in map units.
    pub x: f64,
    pub y: f64,
    /// Radius in map units.
    pub radius: f64,
    /// Start/end bearing (BAM high byte, clockwise from north); None = full circle.
    pub arc: Option<(u8, u8)>,
    pub colour: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContactKind {
    Aircraft,
    Missile,
    GroundRadar,
    Target,
}

#[derive(Debug, Clone, Copy)]
pub struct Contact {
    pub kind: ContactKind,
    /// World units east / north / altitude.
    pub east: f64,
    pub north: f64,
    pub alt: f32,
    /// Binary angle.
    pub heading: u16,
}

/// Mission clock for the WAY page ("TIME hh:mm:ss"): seconds of the day at take-off (from the
/// mission) and the elapsed mission time (the simulation's mission ticks; None = flight time).
#[derive(Resource, Debug, Clone, Copy)]
pub struct MissionClock {
    pub start_seconds: f32,
    pub elapsed: Option<f32>,
}

impl Default for MissionClock {
    fn default() -> Self {
        Self { start_seconds: 18.0 * 3600.0, elapsed: None }
    }
}

fn reset_mfd(mut state: ResMut<MfdState>, mut raster: ResMut<MapRaster>) {
    *state = MfdState::default();
    raster.valid = false;
    raster.geometry_theater.clear();
}

fn mfd_keys(mut actions: MessageReader<ActionEvent>, mut state: ResMut<MfdState>, mut raster: ResMut<MapRaster>) {
    for ActionEvent(a) in actions.read() {
        match a {
            Action::Maps => {
                state.left = if state.left == LeftMfd::Map { LeftMfd::Tac } else { LeftMfd::Map };
                raster.valid = false;
            }
            Action::Zoom => match state.left {
                LeftMfd::Map if state.map_zoom < 9 => {
                    state.map_zoom += 1;
                    raster.valid = false;
                }
                LeftMfd::Tac if state.tac_zoom < 7 => state.tac_zoom += 1,
                _ => {}
            },
            Action::Unzoom => match state.left {
                LeftMfd::Map if state.map_zoom > 2 => {
                    state.map_zoom -= 1;
                    raster.valid = false;
                }
                LeftMfd::Tac if state.tac_zoom > 0 => state.tac_zoom -= 1,
                _ => {}
            },
            Action::Ordnance => state.right = RightMfd::Wpn,
            Action::MissionOrders => state.right = RightMfd::Ord,
            Action::SelectWaypoint | Action::ChangeWaypoint | Action::LastWaypoint | Action::NextWaypoint => {
                state.right = RightMfd::Way
            }
            Action::Flir => {
                state.flir = !state.flir;
                state.right = RightMfd::Camera;
            }
            Action::CamAhead | Action::CamRear | Action::CamRight | Action::CamLeft => {
                state.right = RightMfd::Camera;
                state.cam_dir = match a {
                    Action::CamAhead => CamDir::Ahead,
                    Action::CamRear => CamDir::Rear,
                    Action::CamRight => CamDir::Right,
                    _ => CamDir::Left,
                };
            }
            _ => {}
        }
    }
}

// ------------------------------------------------------------------------------------------
// Map: geometry and rasteriser

/// Flat geometry of one theater model for the map, in model units (x east, y north).
#[derive(Default, Clone)]
struct MapModel {
    attr: u8,
    /// Triangles in painter's order (by height).
    tris: Vec<([Vec2; 3], u8)>,
    lines: Vec<(Vec2, Vec2, u8)>,
}

/// The rendered map (the MFD interior, 77x60) and the centre it was rendered for.
#[derive(Resource)]
pub struct MapRaster {
    pub canvas: Canvas,
    /// Map centre in map units (`DS:4084/4086`).
    pub centre: (i32, i32),
    pub zoom: i32,
    pub valid: bool,
    models: HashMap<u8, MapModel>,
    geometry_theater: String,
}

impl Default for MapRaster {
    fn default() -> Self {
        Self {
            canvas: Canvas::new(77, 60),
            centre: (0, 0),
            zoom: 2,
            valid: false,
            models: HashMap::new(),
            geometry_theater: String::new(),
        }
    }
}

/// Parses `_s<slot>v<value>` conditions of a node name.
fn conditions(name: &str) -> Vec<(usize, i16)> {
    let mut out = Vec::new();
    for part in name.split("_s").skip(1) {
        let Some((slot, rest)) = part.split_once('v') else { continue };
        let value: String = rest.chars().take_while(|c| *c == '-' || c.is_ascii_digit()).collect();
        if let (Ok(s), Ok(v)) = (slot.parse::<usize>(), value.parse::<i16>()) {
            out.push((s, v));
        }
    }
    out
}

struct GeometrySource<'a> {
    nodes: &'a Assets<GltfNode>,
    meshes: &'a Assets<GltfMesh>,
    mesh_data: &'a Assets<Mesh>,
    material_names: HashMap<AssetId<GltfMaterial>, String>,
}

impl GeometrySource<'_> {
    /// Collects the most detailed level of a model (intact, lights off).
    fn collect(&self, handle: &Handle<GltfNode>, parent: Mat4, out: &mut Vec<(Vec<Vec3>, u8)>, lines: &mut Vec<(Vec3, Vec3, u8)>) {
        let Some(node) = self.nodes.get(handle) else { return };
        let name = node.name.as_str();
        if name.contains("_dots_") || conditions(name).iter().any(|&(_, v)| v != 0) {
            return;
        }
        // Only level 0 below the model root.
        if let Some(i) = name.rfind("_lod") {
            let digits: String = name[i + 4..].chars().take_while(char::is_ascii_digit).collect();
            if digits.parse::<u32>().is_ok_and(|k| k != 0) {
                return;
            }
        }
        let t = node.transform;
        let m = parent * Mat4::from_scale_rotation_translation(t.scale, t.rotation, t.translation);
        if let Some(mesh) = node.mesh.as_ref().and_then(|h| self.meshes.get(h)) {
            let node_colour = index_from_name(name);
            for prim in &mesh.primitives {
                let colour = node_colour
                    .or_else(|| {
                        prim.material.as_ref().and_then(|h| self.material_names.get(&h.id())).and_then(|n| index_from_name(n))
                    })
                    .unwrap_or(15);
                let Some(data) = self.mesh_data.get(&prim.mesh) else { continue };
                let Some(VertexAttributeValues::Float32x3(pos)) = data.attribute(Mesh::ATTRIBUTE_POSITION) else {
                    continue;
                };
                let pos: Vec<Vec3> = pos.iter().map(|&p| m.transform_point3(Vec3::from(p))).collect();
                let idx: Vec<usize> = match data.indices() {
                    Some(Indices::U16(v)) => v.iter().map(|&i| i as usize).collect(),
                    Some(Indices::U32(v)) => v.iter().map(|&i| i as usize).collect(),
                    None => (0..pos.len()).collect(),
                };
                match data.primitive_topology() {
                    PrimitiveTopology::TriangleList => {
                        for t in idx.chunks_exact(3) {
                            out.push((vec![pos[t[0]], pos[t[1]], pos[t[2]]], colour));
                        }
                    }
                    PrimitiveTopology::LineList => {
                        for l in idx.chunks_exact(2) {
                            lines.push((pos[l[0]], pos[l[1]], colour));
                        }
                    }
                    _ => {}
                }
            }
        }
        for c in &node.children {
            self.collect(c, m, out, lines);
        }
    }
}

/// Extracts the map geometry of the theater models once they are loaded.
#[allow(clippy::too_many_arguments)]
fn build_map_geometry(
    theater: Option<Res<Theater>>,
    library: Res<ModelLibrary>,
    gltfs: Res<Assets<Gltf>>,
    nodes: Res<Assets<GltfNode>>,
    meshes: Res<Assets<GltfMesh>>,
    mesh_data: Res<Assets<Mesh>>,
    mut raster: ResMut<MapRaster>,
) {
    let Some(theater) = theater else { return };
    if raster.geometry_theater == theater.id {
        return;
    }
    let Some(g) = library.files.get(&theater.id).and_then(|h| gltfs.get(h)) else { return };
    let src = GeometrySource {
        nodes: &nodes,
        meshes: &meshes,
        mesh_data: &mesh_data,
        material_names: g.named_materials.iter().map(|(n, h)| (h.id(), n.to_string())).collect(),
    };
    let mut models = HashMap::new();
    for shape in 0..=255u8 {
        let name = format!("{}_{:03}", theater.id, shape);
        let Some(handle) = g.named_nodes.get(name.as_str()) else { continue };
        let attr = nodes
            .get(handle)
            .and_then(|n| n.extras.as_ref())
            .and_then(|x| serde_json::from_str::<serde_json::Value>(&x.value).ok())
            .and_then(|v| v["attr"].as_u64())
            .unwrap_or(0) as u8;
        let (mut tris, mut lines) = (Vec::new(), Vec::new());
        src.collect(handle, Mat4::IDENTITY, &mut tris, &mut lines);
        // Painter's order: coplanar primitives were lifted along their normal by list position.
        tris.sort_by(|a, b| {
            let h = |t: &Vec<Vec3>| t.iter().map(|p| p.y).sum::<f32>();
            h(&a.0).total_cmp(&h(&b.0))
        });
        let flat = |p: Vec3| Vec2::new(p.x, -p.z);
        models.insert(
            shape,
            MapModel {
                attr,
                tris: tris.into_iter().map(|(p, c)| ([flat(p[0]), flat(p[1]), flat(p[2])], c)).collect(),
                lines: lines.into_iter().map(|(a, b, c)| (flat(a), flat(b), c)).collect(),
            },
        );
    }
    if models.is_empty() {
        return;
    }
    raster.models = models;
    raster.geometry_theater = theater.id.clone();
    raster.valid = false;
}

/// Map projection: map units -> MFD screen pixels (VGAME `1A18:8514` / `852B`).
pub fn map_to_screen(centre: (i32, i32), zoom: i32, x: f64, y: f64) -> Vec2 {
    let s = (1i32 << (10 - zoom)) as f64;
    Vec2::new((112.0 + (x - centre.0 as f64) / s) as f32, (139.0 + (y - centre.1 as f64) * 0.75 / s) as f32)
}

/// Map centre for an aircraft position: ahead along the heading, clamped to the theater.
fn map_centre(x: i32, y: i32, heading: u16, zoom: i32) -> (i32, i32) {
    let r = (0x8000u32 >> zoom) as f32;
    let a = heading as f32 / 65536.0 * std::f32::consts::TAU;
    let lo_x = 0x4D << (9 - zoom);
    let lo_y = (60 << (9 - zoom)) / 3 * 4;
    let cx = (x + (a.sin() * r) as i32).clamp(lo_x, 0x7FFF - lo_x);
    let cy = (y - (a.cos() * r) as i32).clamp(lo_y, 0x7FFF - lo_y);
    (cx, cy)
}

fn render_map(
    q: Query<(&Aircraft, &WorldPos), With<Player>>,
    state: Res<MfdState>,
    theater: Option<Res<Theater>>,
    palette: Option<Res<Palette>>,
    mut raster: ResMut<MapRaster>,
) {
    let (Some(theater), Some(palette)) = (theater, palette) else { return };
    let Ok((a, pos)) = q.single() else { return };
    if state.left != LeftMfd::Map || raster.models.is_empty() {
        return;
    }
    let (x, y) = map_units(pos);
    if raster.valid {
        // Re-render when the aircraft leaves the display (VGAME 3BE4).
        let p = map_to_screen(raster.centre, raster.zoom, x as f64, y as f64);
        if p.x >= LEFT[0] as f32 && p.x <= LEFT[2] as f32 && p.y >= LEFT[1] as f32 && p.y <= LEFT[3] as f32 {
            return;
        }
    }
    let (h, _, _) = a.euler();
    let zoom = state.map_zoom;
    let centre = map_centre(x, y, to_bam(h) as i32 as u16, zoom);
    let map16 = palette.remaps.iter().find(|t| t.name == "map16").map(|t| t.entries.clone()).unwrap_or_default();
    let remap = |c: u8| map16.get(c as usize).map_or(c, |w| (*w & 0xFF) as u8);
    let raster = raster.as_mut();
    raster.centre = centre;
    raster.zoom = zoom;
    raster.valid = true;
    rasterise_map(&mut raster.canvas, &raster.models, &theater, centre, zoom, &remap);
}

/// Renders the theater top-down (VGAME `1A18:11F2`): LOD4, LOD3, LOD2 tiles (a level only if
/// its tiles are larger than 16 units on screen), then LOD4 and LOD3 again for the models
/// flagged 0x80 (drawn on top). Only ground-level objects (z = 0) are drawn.
fn rasterise_map(c: &mut Canvas, models: &HashMap<u8, MapModel>, theater: &Theater, centre: (i32, i32), zoom: i32, remap: &dyn Fn(u8) -> u8) {
    c.clear(6);
    let to_px = |east: f64, north: f64| {
        let p = map_to_screen(centre, zoom, east / 32.0, 0x8000 as f64 - north / 32.0);
        Vec2::new(p.x - LEFT[0] as f32, p.y - LEFT[1] as f32)
    };
    // Visible world rectangle (with margin).
    let s = (1i32 << (10 - zoom)) as f64 * 32.0;
    let half_w = 40.0 * s;
    let half_h = 32.0 * s / 0.75;
    let (cx, cn) = (centre.0 as f64 * 32.0, (0x8000 - centre.1) as f64 * 32.0);
    for (level, flagged) in [(4usize, false), (3, false), (2, false), (4, true), (3, true)] {
        if (4 - level as i32) * 2 + (9 - zoom) >= 8 {
            continue;
        }
        let unit = LEVEL_UNIT[level];
        let tile = 4096.0 * unit;
        let lo = if level == 4 { -2 } else { 0 };
        let range = |a: f64, b: f64| ((a / tile).floor() as i32 - 1, (b / tile).floor() as i32 + 1);
        let (c0, c1) = range(cx - half_w, cx + half_w);
        let (r0, r1) = range(cn - half_h, cn + half_h);
        for row in r0.max(lo)..=r1.min(GRID_DIM[level] + lo - 1) {
            for col in c0.max(lo)..=c1.min(GRID_DIM[level] + lo - 1) {
                let tile_type = theater.grid.tile(level, col, row) as usize;
                let Some(objects) = theater.tiles.tiles.get(level).and_then(|t| t.get(tile_type)) else { continue };
                let (tx, ty) = ((col as f64 * 4096.0 + 2048.0) * unit, (row as f64 * 4096.0 + 2048.0) * unit);
                for o in objects.iter().filter(|o| o.z == 0) {
                    let Some(m) = models.get(&o.shape) else { continue };
                    if level >= 3 && (m.attr & 0x80 != 0) != flagged {
                        continue;
                    }
                    let place = |p: Vec2| to_px(tx + (o.x as f64 + p.x as f64) * unit, ty + (o.y as f64 + p.y as f64) * unit);
                    for (t, colour) in &m.tris {
                        fill_triangle(c, [place(t[0]), place(t[1]), place(t[2])], remap(*colour));
                    }
                    for (a, b, colour) in &m.lines {
                        let (a, b) = (place(*a), place(*b));
                        c.line(a.x.floor() as i32, a.y.floor() as i32, b.x.floor() as i32, b.y.floor() as i32, remap(*colour));
                    }
                }
            }
        }
    }
}

/// Fills a triangle (pixel centres inside, top-left fill rule).
fn fill_triangle(c: &mut Canvas, v: [Vec2; 3], colour: u8) {
    let area = (v[1] - v[0]).perp_dot(v[2] - v[0]);
    if area.abs() < 1e-6 {
        return;
    }
    let v = if area < 0.0 { [v[0], v[2], v[1]] } else { v };
    let min = v[0].min(v[1]).min(v[2]);
    let max = v[0].max(v[1]).max(v[2]);
    let (x0, x1) = ((min.x.floor() as i32).max(0), (max.x.ceil() as i32).min(c.w as i32 - 1));
    let (y0, y1) = ((min.y.floor() as i32).max(0), (max.y.ceil() as i32).min(c.h as i32 - 1));
    for y in y0..=y1 {
        for x in x0..=x1 {
            let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
            let inside = (0..3).all(|i| {
                let (a, b) = (v[i], v[(i + 1) % 3]);
                let e = (b - a).perp_dot(p - a);
                let top_left = (a.y == b.y && b.x < a.x) || b.y < a.y;
                e > 0.0 || (e == 0.0 && top_left)
            });
            if inside {
                c.set(x, y, colour);
            }
        }
    }
}

// ------------------------------------------------------------------------------------------
// Drawing

#[derive(SystemParam)]
pub struct MfdParams<'w, 's> {
    pub aircraft: Query<'w, 's, (&'static Aircraft, &'static WorldPos), With<Player>>,
    pub state: Res<'w, MfdState>,
    pub raster: Res<'w, MapRaster>,
    pub orders: Res<'w, MissionOrders>,
    pub cam: Res<'w, TrackCam>,
    pub contacts: Res<'w, Contacts>,
    pub clock: Res<'w, MissionClock>,
    pub stores: Res<'w, Stores>,
    pub waypoints: Res<'w, Waypoints>,
    pub hud: Res<'w, HudState>,
    pub theater: Option<Res<'w, Theater>>,
    pub time: Res<'w, Time>,
}

/// MFD text helper (`1A18:89FE`): x = left + 4*col, y = 110 + 6*row.
fn mfd_text(c: &mut Canvas, fonts: &Fonts, left: i32, col: i32, row: i32, s: &str, colour: u8) {
    fonts.text(c, left + 4 * col, 110 + 6 * row, &upper(s), colour);
}

fn hms(seconds: f32) -> (u32, u32, u32) {
    let s = seconds.max(0.0) as u32 % 86400;
    (s / 3600, s / 60 % 60, s % 60)
}

impl MfdParams<'_, '_> {
    pub fn draw(&self, c: &mut Canvas, art: &CockpitArt, fonts: &Fonts) {
        let ac = self.aircraft.single().ok();
        c.set_clip(LEFT[0], LEFT[1], LEFT[2], LEFT[3]);
        match self.state.left {
            LeftMfd::Map => self.draw_map(c, ac),
            LeftMfd::Tac => self.draw_tac(c, art, ac),
        }
        c.set_clip(RIGHT[0], RIGHT[1], RIGHT[2], RIGHT[3]);
        match self.state.right {
            RightMfd::Camera => self.draw_camera_page(c, fonts),
            RightMfd::Way => self.draw_way(c, fonts, ac),
            RightMfd::Wpn => self.draw_wpn(c, art, fonts),
            RightMfd::Ord => self.draw_ord(c, fonts),
        }
        c.reset_clip();
    }

    fn draw_map(&self, c: &mut Canvas, ac: Option<(&Aircraft, &WorldPos)>) {
        if !self.raster.valid {
            c.fill_rect(LEFT[0], LEFT[1], 77, 60, 6);
            return;
        }
        c.copy_rect(&self.raster.canvas, 0, 0, 77, 60, LEFT[0], LEFT[1]);
        let (centre, zoom) = (self.raster.centre, self.raster.zoom);
        let blink = crate::hud::blink(&self.time);
        for k in &self.contacts.list {
            let p = map_to_screen(centre, zoom, k.east / 32.0, 0x8000 as f64 - k.north / 32.0);
            let (x, y) = (p.x as i32, p.y as i32);
            match k.kind {
                ContactKind::Aircraft => c.fill_rect(x, y, 2, 2, 41),
                ContactKind::Missile => c.set(x, y, 17),
                ContactKind::GroundRadar => c.fill_rect(x, y, 2, 2, 0),
                ContactKind::Target if blink => c.fill_rect(x - 1, y - 1, 3, 3, 15),
                ContactKind::Target => {}
            }
        }
        // Radar coverage arc of the current sweep.
        if let Some(arc) = &self.contacts.radar_arc {
            let o = map_to_screen(centre, zoom, arc.x, arc.y);
            let r = arc.radius / (1i32 << (10 - zoom)) as f64;
            let (a0, a1) = arc.arc.map_or((0.0, 256.0), |(a, b)| {
                let (a, b) = (a as f64, b as f64);
                (a, if b < a { b + 256.0 } else { b })
            });
            c.set_clip(LEFT[0], LEFT[1], LEFT[0] + 76, LEFT[1] + 59);
            let steps = ((a1 - a0) * r / 16.0).clamp(16.0, 512.0) as i32;
            let mut prev: Option<(i32, i32)> = None;
            for i in 0..=steps {
                let t = (a0 + (a1 - a0) * i as f64 / steps as f64) / 256.0 * std::f64::consts::TAU;
                let p = (o.x as f64 + r * t.sin(), o.y as f64 - r * 0.75 * t.cos());
                let p = (p.0 as i32, p.1 as i32);
                if let Some(q) = prev {
                    c.line(q.0, q.1, p.0, p.1, arc.colour);
                }
                prev = Some(p);
            }
            c.reset_clip();
        }
        // Own aircraft: white chevron.
        if let Some((_, pos)) = ac {
            let (x, y) = map_units(pos);
            let p = map_to_screen(centre, zoom, x as f64, y as f64);
            let (x, y) = (p.x as i32 - 1, p.y as i32);
            c.set(x, y - 2, 15);
            c.set(x, y - 1, 15);
            c.set(x - 1, y, 15);
            c.set(x + 1, y, 15);
        }
    }

    /// TAC display (`1A18:8C10`).
    fn draw_tac(&self, c: &mut Canvas, art: &CockpitArt, ac: Option<(&Aircraft, &WorldPos)>) {
        c.fill_rect(LEFT[0], LEFT[1], 77, 60, 0);
        let Some((a, pos)) = ac else { return };
        let (ax, ay) = map_units(pos);
        let (h, _, _) = a.euler();
        let (sh, ch) = h.sin_cos();
        let zoom = self.state.tac_zoom;
        let shift = 7 - zoom;
        // Heading-up projection (`1A18:90B9`); also returns whether an icon is inside the display.
        let proj = |x: i32, y: i32| -> (i32, i32, bool) {
            let dx = ((x - ax) >> shift) as f32;
            let dy = ((ay - y) >> shift) as f32;
            let sx = (ch * dx - sh * dy) as i32 + 112;
            let sy = 152 - (sh * dx + ch * dy) as i32;
            (sx, sy, (78..=147).contains(&sx) && (113..=166).contains(&sy))
        };
        // Grid every 1024 map units, colour 8.
        let n = if zoom < 2 { (1 << (2 - zoom)) + 1 } else { 1 };
        let (bx, by) = (ax & !0x3FF, ay & !0x3FF);
        for k in (1 - n)..=n {
            let x = bx + (k << 10);
            let (p, q) = (proj(x, by + 0x1400), proj(x, by - 0x1000));
            c.line(p.0, p.1, q.0, q.1, 8);
            let y = by + (k << 10);
            let (p, q) = (proj(bx + 0x1400, y), proj(bx - 0x1000, y));
            c.line(p.0, p.1, q.0, q.1, 8);
        }
        let icon = |c: &mut Canvas, kind: i32, row: i32, x: i32, y: i32| {
            c.blit(&art.pit, kind * 8 + 1, row * 8 + 41, 7, 7, x - 3, y - 3, Some(0), None);
        };
        // Airbases (crossed circle).
        if let Some(t) = &self.theater {
            for o in t.world.objects.iter().filter(|o| o.target_flags & 0x100 != 0) {
                let (x, y, vis) = proj(o.x as i32, o.y as i32);
                if vis {
                    icon(c, 5, 3, x, y);
                }
            }
        }
        // Other aircraft by relative heading and altitude band; missiles as short lines.
        for k in &self.contacts.list {
            let mx = (k.east / 32.0) as i32;
            let my = 0x8000 - (k.north / 32.0) as i32;
            let (x, y, vis) = proj(mx, my);
            if !vis {
                continue;
            }
            match k.kind {
                ContactKind::Aircraft => {
                    let rel = k.heading.wrapping_sub(to_bam(h) as i32 as u16).wrapping_add(0x800);
                    let dalt = k.alt - a.altitude;
                    let band = if dalt < -1000.0 { 1 } else if dalt > 1000.0 { 2 } else { 0 };
                    icon(c, (rel >> 12) as i32 & 15, band, x, y);
                }
                ContactKind::Missile => {
                    let rel = (k.heading as f32 / 65536.0) * std::f32::consts::TAU - h;
                    let len = (zoom + 1) as f32;
                    c.line(x, y, x + (rel.sin() * len) as i32, y - (rel.cos() * len) as i32, 41);
                }
                ContactKind::GroundRadar => icon(c, 1, 3, x, y),
                ContactKind::Target => icon(c, 5, 3, x, y),
            }
        }
        // Own aircraft.
        icon(c, 0, 3, 112, 152);
    }

    /// Tracking camera page (`1A18:BF71`). With the camera picture live the background stays
    /// see-through (index 0) and the 3D picture shows behind the MFD; otherwise a placeholder
    /// (the level "ahead" view's sky bands and ground).
    fn draw_camera_page(&self, c: &mut Canvas, fonts: &Fonts) {
        const SKY: [(i32, u8); 11] =
            [(3, 138), (6, 137), (6, 136), (3, 135), (3, 134), (3, 133), (1, 132), (2, 131), (1, 129), (1, 128), (31, 238)];
        let flir = self.state.flir;
        if self.cam.live {
            c.fill_rect(RIGHT[0], RIGHT[1], 77, 60, 0);
        } else {
            let mut y = RIGHT[1];
            for (rows, colour) in SKY {
                // FLIR: the green ramp 152..159.
                let colour = if flir { 152 + (colour as u32 * 7 / 255) as u8 } else { colour };
                c.fill_rect(RIGHT[0], y, 77, rows, colour);
                y += rows;
            }
        }
        // Black text with FLIR (colour 5: black in every flight palette, 0 is see-through).
        let text = if flir { 5 } else { self.hud.colours().0 };
        for (k, line) in self.cam.name.iter().enumerate() {
            let s = upper(line);
            fonts.text(c, 208 - 2 * s.len() as i32, 112 + 6 * k as i32, &s, text);
        }
        if let Some(r) = self.cam.range_km {
            fonts.text(c, 181, 155, format!("RANGE {r:.1} KM").as_bytes(), text);
        }
        if let Some(b) = self.cam.bearing {
            fonts.text(c, 185, 161, format!("BRG {b}").as_bytes(), text);
        }
        if self.cam.zoom > 0 {
            fonts.text(c, 236, 163, format!("{}X", self.cam.zoom).as_bytes(), text);
        }
    }

    /// WAY page (`1A18:9208`).
    fn draw_way(&self, c: &mut Canvas, fonts: &Fonts, ac: Option<(&Aircraft, &WorldPos)>) {
        c.fill_rect(RIGHT[0], RIGHT[1], 77, 60, 1);
        let t = self.clock.start_seconds + self.clock.elapsed.unwrap_or_else(|| ac.map_or(0.0, |(a, _)| a.time));
        let (hh, mm, ss) = hms(t);
        mfd_text(c, fonts, RIGHT[0], 2, 0, &format!("TIME  {hh:02}:{mm:02}:{ss:02}"), 15);
        if let Some((a, pos)) = ac {
            let (mut e, mut n, _) = bevy_to_wu(pos.0);
            // Leg times at the current speed (at least 360 kt), ETA accumulated.
            let kmh = (a.knots().max(360.0) * 1.852) as f64;
            let mut eta = t as f64;
            for (i, wp) in self.waypoints.points.iter().enumerate().take(4) {
                let km = ((wp.east - e).hypot(wp.north - n) * WU / 1000.0).round();
                let minutes = (km / kmh * 60.0).round();
                eta += minutes * 60.0;
                let (eh, em, _) = hms(eta as f32);
                let colour = if i == self.waypoints.current { 15 } else if i % 2 == 1 { 3 } else { 11 };
                mfd_text(c, fonts, RIGHT[0], 1, 1 + 2 * i as i32, &format!("WAYPOINT {}-{} KM", i + 1, km as i64), colour);
                mfd_text(c, fonts, RIGHT[0], 1, 2 + 2 * i as i32, &format!("{} MIN. ETA {eh:02}:{em:02}", minutes as i64), colour);
                (e, n) = (wp.east, wp.north);
            }
            // Fuel bar (y 164..167, x 170..244): remaining fuel in green.
            let full = 10000.0;
            let w = ((a.fuel / full).clamp(0.0, 1.0) * 75.0).round() as i32;
            c.fill_rect(170, 164, 75, 4, 0);
            c.fill_rect(170, 164, w, 4, 47);
        }
    }

    /// WPN page (`1A18:9678`): backdrop (frame and ORDNANCE.PIC side views) with counts and names.
    fn draw_wpn(&self, c: &mut Canvas, art: &CockpitArt, fonts: &Fonts) {
        c.fill_rect(RIGHT[0], RIGHT[1], 77, 60, 0);
        for (i, bay) in self.stores.bays.iter().enumerate().take(4) {
            let y = RIGHT[1] + 15 * i as i32;
            if let Some(&(sx, sy)) = ORDNANCE_PICTURES.get(bay.picture) {
                c.copy_rect(&art.ordnance, sx, sy, 77, 15, RIGHT[0], y);
            }
        }
        frame(c, RIGHT, 47);
        for y in [124, 139, 154] {
            c.hline(RIGHT[0], RIGHT[2], y, 47);
        }
        for (i, bay) in self.stores.bays.iter().enumerate().take(4) {
            let y = 112 + 15 * i as i32;
            fonts.text(c, 172, y + 6, format!("0{}", bay.count).as_bytes(), 47);
            fonts.text(c, 172, y, &upper(&bay.name), 47);
        }
    }

    /// ORD page (`1A18:97A4`).
    fn draw_ord(&self, c: &mut Canvas, fonts: &Fonts) {
        c.fill_rect(RIGHT[0], RIGHT[1], 77, 60, 0);
        for (row, (colour, line)) in self.orders.lines.iter().enumerate().take(10) {
            // Colour-change byte + text, as the original order strings.
            let mut s = vec![ink(*colour)];
            s.extend(upper(line));
            fonts.text(c, RIGHT[0] + 4, 110 + 6 * row as i32, &s, 15);
        }
    }
}

/// Weapon side views in ORDNANCE.PIC (`DS:00A2`), 77x15 each.
const ORDNANCE_PICTURES: [(i32, i32); 19] = [
    (9, 37),
    (9, 64),
    (9, 89),
    (9, 117),
    (9, 142),
    (9, 172),
    (104, 37),
    (104, 64),
    (105, 89),
    (106, 117),
    (106, 143),
    (106, 172),
    (198, 37),
    (198, 64),
    (197, 90),
    (198, 117),
    (200, 144),
    (200, 173),
    (198, 15),
];

fn frame(c: &mut Canvas, r: [i32; 4], colour: u8) {
    c.rect(r[0], r[1], r[2] - r[0] + 1, r[3] - r[1] + 1, colour);
}
