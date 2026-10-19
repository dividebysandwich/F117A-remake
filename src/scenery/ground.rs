//! Where scenery may go, from the theater's own tile objects: the flat ground overlays (water,
//! fields, concrete) decide land and water like the original's painter's order, the footprints
//! of raised geometry (mountains, buildings, airbases, targets) are kept clear, and the flat
//! road lines of the coarse levels are picked up as highways.

use std::collections::HashMap;

use bevy::gltf::{Gltf, GltfMaterial, GltfNode};
use bevy::math::DVec2;
use bevy::mesh::{PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;
use f117_formats::terrain::{GRID_DIM, LEVEL_UNIT};

use crate::models::ModelAssets;
use crate::palette::{VgaPalette, index_from_name};
use crate::world::{ShapeHandles, Theater, WU};

/// Bucket size of the spatial indexes (metres).
pub const CELL: f64 = 500.0;
/// Clearance around raised tile geometry and concrete (metres).
const RAISED_MARGIN: f32 = 25.0;
const SOLID_MARGIN: f32 = 45.0;

/// Ids of items overlapping each `CELL` square (ground positions in metres, x east, y north).
#[derive(Default)]
pub struct Index(HashMap<(i32, i32), Vec<u32>>);

impl Index {
    fn cell(p: DVec2) -> (i32, i32) {
        ((p.x / CELL).floor() as i32, (p.y / CELL).floor() as i32)
    }

    pub fn insert(&mut self, min: DVec2, max: DVec2, id: u32) {
        let (a, b) = (Self::cell(min), Self::cell(max));
        for y in a.1..=b.1 {
            for x in a.0..=b.0 {
                self.0.entry((x, y)).or_default().push(id);
            }
        }
    }

    pub fn at(&self, p: DVec2) -> &[u32] {
        self.0.get(&Self::cell(p)).map_or(&[], Vec::as_slice)
    }

    /// Ids in the cells overlapping a rectangle (an id may repeat).
    pub fn around(&self, min: DVec2, max: DVec2) -> impl Iterator<Item = u32> + '_ {
        let (a, b) = (Self::cell(min), Self::cell(max));
        (a.1..=b.1)
            .flat_map(move |y| (a.0..=b.0).map(move |x| (x, y)))
            .flat_map(|c| self.0.get(&c).into_iter().flatten().copied())
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Surface {
    /// Blue overlays: the sea (OR with the sky bands, byte 49) and lakes (AND with the ground
    /// gradient into the blue 0x60 ramp, byte 57).
    Water,
    /// Other overlays mixed with the ground gradient.
    Land,
    /// Solid colour (runways, concrete, ...).
    Solid,
}

/// The surface a colour word gives a flat polygon (None: invisible), judged by the colour it
/// shows over the middle of the ground gradient in the day palette.
fn surface(word: u16, day: &VgaPalette) -> Option<Surface> {
    let lo = (word & 0xFF) as u8;
    let (shown, mixed) = match word >> 8 {
        0xFB => return None,
        0xFD => (0x78 | lo, true),
        0xFE => (0x78 & lo, true),
        _ => (lo, false),
    };
    let [r, g, b] = day.colors[shown as usize].map(i32::from);
    Some(if b > r + 8 && b > g + 8 {
        Surface::Water
    } else if mixed {
        Surface::Land
    } else {
        Surface::Solid
    })
}

struct FlatTri {
    p: [Vec2; 3],
    /// Painter's order: coarse levels first, then the cell's list order.
    order: u32,
    surface: Surface,
}

impl FlatTri {
    fn contains(&self, q: Vec2) -> bool {
        let [a, b, c] = self.p;
        let s = |p: Vec2, q0: Vec2, q1: Vec2| (q0 - p).perp_dot(q1 - p);
        let (d0, d1, d2) = (s(q, a, b), s(q, b, c), s(q, c, a));
        !((d0 < 0.0 || d1 < 0.0 || d2 < 0.0) && (d0 > 0.0 || d1 > 0.0 || d2 > 0.0))
    }
}

/// Geometry of one tile shape that matters here, in model units (x east, y north).
#[derive(Default)]
struct ShapeGeom {
    flat: Vec<([Vec2; 3], Surface)>,
    /// Footprint of the raised geometry.
    raised: Option<Rect>,
    /// Flat lines in the road greys.
    lines: Vec<(Vec2, Vec2)>,
    /// Runway strips: long, narrow concrete rectangles.
    runways: Vec<Rect>,
}

/// A runway of the theater (metres, axis-aligned like all the original runways).
#[derive(Clone, Copy, Debug)]
pub struct Runway {
    pub c: DVec2,
    /// Unit direction along the runway.
    pub axis: DVec2,
    pub half_len: f64,
    pub half_width: f64,
}

pub struct Ground {
    tris: Vec<FlatTri>,
    tri_index: Index,
    /// Areas kept clear (metres); `true` for raised objects, `false` for concrete margins.
    boxes: Vec<(Rect, bool)>,
    box_index: Index,
    /// Road lines of the original terrain (polylines, metres).
    pub highways: Vec<Vec<DVec2>>,
    pub runways: Vec<Runway>,
}

impl Ground {
    fn top(&self, p: DVec2) -> Option<Surface> {
        let q = p.as_vec2();
        self.tri_index
            .at(p)
            .iter()
            .map(|&i| &self.tris[i as usize])
            .filter(|t| t.contains(q))
            .max_by_key(|t| t.order)
            .map(|t| t.surface)
    }

    /// Land, not concrete, and at least `r` metres from raised tile geometry.
    pub fn is_free(&self, p: DVec2, r: f64) -> bool {
        if matches!(self.top(p), Some(Surface::Water | Surface::Solid)) {
            return false;
        }
        self.boxes_clear(p, r, false)
    }

    /// Not water or concrete at the point, and at least `r` metres from raised geometry
    /// (concrete nearby is fine).
    pub fn is_clear(&self, p: DVec2, r: f64) -> bool {
        !matches!(self.top(p), Some(Surface::Water | Surface::Solid)) && self.boxes_clear(p, r, true)
    }

    /// Not water at the point and at least `r` metres from raised geometry.
    pub fn is_dry_clear(&self, p: DVec2, r: f64) -> bool {
        self.top(p) != Some(Surface::Water) && self.boxes_clear(p, r, true)
    }

    fn boxes_clear(&self, p: DVec2, r: f64, raised_only: bool) -> bool {
        let q = p.as_vec2();
        let r = r as f32;
        let d = DVec2::splat(r as f64);
        !self.box_index.around(p - d, p + d).any(|i| {
            let (b, raised) = self.boxes[i as usize];
            (raised || !raised_only) && q.x > b.min.x - r && q.x < b.max.x + r && q.y > b.min.y - r && q.y < b.max.y + r
        })
    }

    /// Keeps an area clear of later scenery (counts as a raised object).
    pub fn claim(&mut self, r: Rect) {
        self.add_box(r, true);
    }

    pub fn build(theater: &Theater, handles: &ShapeHandles, assets: &ModelAssets, files: &HashMap<String, Handle<Gltf>>, remap: &[u16], day: &VgaPalette) -> Ground {
        let mut shapes: HashMap<u8, ShapeGeom> = HashMap::new();
        let mut g = Ground { tris: vec![], tri_index: Index::default(), boxes: vec![], box_index: Index::default(), highways: vec![], runways: vec![] };
        let mut road_segments = Vec::new();
        for lod in 1..=4usize {
            let unit = LEVEL_UNIT[lod];
            let scale = (unit * WU) as f32;
            let lo = if lod == 4 { -2 } else { 0 };
            for row in lo..GRID_DIM[lod] + lo {
                for col in lo..GRID_DIM[lod] + lo {
                    let tile = theater.grid.tile(lod, col, row) as usize;
                    let Some(objects) = theater.tiles.tiles.get(lod).and_then(|t| t.get(tile)) else { continue };
                    let centre = |c: i32| (c as f64 * 4096.0 + 2048.0) * unit;
                    for (k, o) in objects.iter().enumerate() {
                        let geom = shapes.entry(o.shape).or_insert_with(|| {
                            handles
                                .model(o.shape as usize)
                                .and_then(|(file, name)| Some(extract(assets, assets.gltfs.get(files.get(file)?)?, &name, remap, day)))
                                .unwrap_or_default()
                        });
                        let origin = Vec2::new(
                            ((centre(col) + o.x as f64 * unit) * WU) as f32,
                            ((centre(row) + o.y as f64 * unit) * WU) as f32,
                        );
                        let place = |p: Vec2| origin + p * scale;
                        for (j, (tri, s)) in geom.flat.iter().enumerate() {
                            let p = tri.map(place);
                            let id = g.tris.len() as u32;
                            let (min, max) = (p[0].min(p[1]).min(p[2]), p[0].max(p[1]).max(p[2]));
                            g.tri_index.insert(min.as_dvec2(), max.as_dvec2(), id);
                            let order = ((4 - lod as u32) << 28) | ((k as u32 & 0xFFF) << 16) | (j as u32 & 0xFFFF);
                            g.tris.push(FlatTri { p, order, surface: *s });
                            if *s == Surface::Solid {
                                g.add_box(Rect::from_corners(min, max).inflate(SOLID_MARGIN), false);
                            }
                        }
                        for r in &geom.runways {
                            let r = Rect::from_corners(place(r.min), place(r.max));
                            let (c, size) = (r.center().as_dvec2(), r.size().as_dvec2());
                            let along_north = size.y > size.x;
                            let (len, width) = if along_north { (size.y, size.x) } else { (size.x, size.y) };
                            if len >= 1200.0 && len >= 6.0 * width && width <= 250.0 && !g.runways.iter().any(|w| w.c.distance(c) < 50.0) {
                                let axis = if along_north { DVec2::Y } else { DVec2::X };
                                g.runways.push(Runway { c, axis, half_len: len * 0.5, half_width: width * 0.5 });
                            }
                        }
                        if let Some(r) = geom.raised {
                            g.add_box(Rect::from_corners(place(r.min), place(r.max)).inflate(RAISED_MARGIN), true);
                        } else if lod >= 2 {
                            road_segments.extend(geom.lines.iter().map(|&(a, b)| (place(a).as_dvec2(), place(b).as_dvec2())));
                        }
                    }
                }
            }
        }
        g.highways = chain(road_segments);
        // Parallel taxiway strips of some bases look like runways too: keep the longest strip
        // of each airfield.
        g.runways.sort_by(|a, b| b.half_len.total_cmp(&a.half_len));
        let mut kept: Vec<Runway> = Vec::new();
        for r in std::mem::take(&mut g.runways) {
            let near = kept.iter().any(|k| {
                let d = r.c - k.c;
                d.dot(k.axis).abs() < k.half_len + 200.0 && d.dot(k.axis.perp()).abs() < k.half_width + 400.0
            });
            if !near {
                kept.push(r);
            }
        }
        g.runways = kept;
        g
    }

    fn add_box(&mut self, r: Rect, raised: bool) {
        let id = self.boxes.len() as u32;
        self.box_index.insert(r.min.as_dvec2(), r.max.as_dvec2(), id);
        self.boxes.push((r, raised));
    }
}

/// Joins line segments sharing end points into polylines (duplicates dropped).
fn chain(segments: Vec<(DVec2, DVec2)>) -> Vec<Vec<DVec2>> {
    let key = |p: DVec2| ((p.x * 0.5).round() as i64, (p.y * 0.5).round() as i64);
    let mut seen = std::collections::HashSet::new();
    let mut at: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    let segs: Vec<_> = segments
        .into_iter()
        .filter(|&(a, b)| a.distance(b) > 1.0 && seen.insert((key(a).min(key(b)), key(a).max(key(b)))))
        .collect();
    for (i, &(a, b)) in segs.iter().enumerate() {
        at.entry(key(a)).or_default().push(i);
        at.entry(key(b)).or_default().push(i);
    }
    let mut used = vec![false; segs.len()];
    let mut out = Vec::new();
    for start in 0..segs.len() {
        if used[start] {
            continue;
        }
        used[start] = true;
        let mut line = vec![segs[start].0, segs[start].1];
        for forward in [true, false] {
            loop {
                let end = if forward { *line.last().unwrap() } else { line[0] };
                let Some(&next) = at[&key(end)].iter().find(|&&i| !used[i]) else { break };
                used[next] = true;
                let (a, b) = segs[next];
                let p = if key(a) == key(end) { b } else { a };
                if forward { line.push(p) } else { line.insert(0, p) }
            }
        }
        out.push(line);
    }
    out
}

/// Collects the finest detail level of a model: flat polygons, raised footprint, road lines.
fn extract(assets: &ModelAssets, gltf: &Gltf, name: &str, remap: &[u16], day: &VgaPalette) -> ShapeGeom {
    let mut geom = ShapeGeom::default();
    let Some(node) = gltf.named_nodes.get(name) else { return geom };
    let materials: HashMap<AssetId<GltfMaterial>, &str> = gltf.named_materials.iter().map(|(n, h)| (h.id(), n.as_ref())).collect();
    walk(assets, node, Transform::IDENTITY, false, &materials, remap, day, &mut geom);
    geom
}

#[allow(clippy::too_many_arguments)]
fn walk(
    assets: &ModelAssets,
    handle: &Handle<GltfNode>,
    parent: Transform,
    in_lod0: bool,
    materials: &HashMap<AssetId<GltfMaterial>, &str>,
    remap: &[u16],
    day: &VgaPalette,
    geom: &mut ShapeGeom,
) {
    let Some(node) = assets.nodes.get(handle) else { return };
    let name = node.name.as_str();
    let lod0 = in_lod0 || name.contains("_lod0");
    // Other detail levels, the wreck state and the light dots don't count.
    if (name.contains("_lod") && !lod0) || name.contains("_s1v1") || name.contains("_dots_") {
        return;
    }
    let tf = parent.mul_transform(node.transform);
    if lod0 && let Some(mesh) = node.mesh.as_ref().and_then(|m| assets.meshes.get(m)) {
        for prim in &mesh.primitives {
            let Some(m) = assets.mesh_data.get(&prim.mesh) else { continue };
            let Some(VertexAttributeValues::Float32x3(pos)) = m.attribute(Mesh::ATTRIBUTE_POSITION) else { continue };
            let pos: Vec<Vec3> = pos.iter().map(|&p| tf.transform_point(Vec3::from(p))).collect();
            let idx: Vec<usize> = m.indices().map_or_else(|| (0..pos.len()).collect(), |i| i.iter().collect());
            let ground = |p: Vec3| Vec2::new(p.x, -p.z);
            let colour = index_from_name(name)
                .or_else(|| prim.material.as_ref().and_then(|h| materials.get(&h.id())).and_then(|n| index_from_name(n)))
                .unwrap_or(15);
            let word = remap.get(colour as usize).copied().unwrap_or(0xFF00 | colour as u16);
            if m.primitive_topology() == PrimitiveTopology::LineList {
                for l in idx.as_chunks::<2>().0 {
                    let (a, b) = (pos[l[0]], pos[l[1]]);
                    if a.y.abs() < 1.0 && b.y.abs() < 1.0 && (0xE0..=0xEF).contains(&(word & 0xFF)) {
                        geom.lines.push((ground(a), ground(b)));
                    }
                }
                continue;
            }
            if m.primitive_topology() != PrimitiveTopology::TriangleList {
                continue;
            }
            for t in idx.as_chunks::<3>().0 {
                let p = [pos[t[0]], pos[t[1]], pos[t[2]]];
                if p.iter().all(|v| v.y.abs() < 1.0) {
                    if let Some(s) = surface(word, day) {
                        let q = p.map(ground);
                        if s == Surface::Solid {
                            let r = Rect::from_corners(q[0].min(q[1]).min(q[2]), q[0].max(q[1]).max(q[2]));
                            let size = r.size();
                            if size.max_element() >= 6.0 * size.min_element() && !geom.runways.contains(&r) {
                                geom.runways.push(r);
                            }
                        }
                        geom.flat.push((q, s));
                    }
                } else {
                    for v in p {
                        let q = ground(v);
                        geom.raised = Some(geom.raised.map_or(Rect::from_corners(q, q), |r| r.union_point(q)));
                    }
                }
            }
        }
    }
    for c in &node.children {
        walk(assets, c, tf, lod0, materials, remap, day, geom);
    }
}
