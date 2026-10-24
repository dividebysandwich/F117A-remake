//! Terrain upgrade. Once a theater's model file is loaded, its ground
//! models are refined in place, before the tile streaming, the MFD map and the scenery layout
//! read them, so all three see the same geometry:
//!
//! - Coastlines and lake shores: the long straight boundary edges of the water overlays (and
//!   of the map-only land overlays stacked with them) become fractal polylines. Every polygon
//!   edge on such a line gets the same points, so the stacked overlays stay seamless; edges on
//!   a model's outline (tile seams) and the original vertices stay where they were.
//! - Mountains: the large raised terrain models are subdivided two or three times with their
//!   edge midpoints pushed along the surface normal (the ground outline and the open edges
//!   where a range continues in the next model stay straight, so the pieces meet), and the new
//!   faces step a little along their colour ramp by their slope to the sun, like the theater
//!   models' per-face shading. The refined surface replaces their plane hull for collisions.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bevy::gltf::{Gltf, GltfMaterial, GltfMesh, GltfNode};
use bevy::math::DVec2;
use bevy::mesh::{PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;
use stealth_formats::terrain::LEVEL_UNIT;

use crate::models::ModelLibrary;
use crate::palette::{Palette, REMAP_DETAIL_HIGH, VgaPalette, index_from_name};
use crate::world::{Theater, WU};

pub struct TerrainPlugin;

impl Plugin for TerrainPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RefinedTerrain>().add_systems(
            Update,
            refine.run_if(resource_exists::<Theater>).run_if(resource_exists::<Palette>),
        );
    }
}

/// Theater model files already refined (the loaded assets are kept by `ModelLibrary`), and the
/// refined mountain surfaces by detail level node name (node space, model units).
#[derive(Resource, Default)]
pub struct RefinedTerrain {
    files: HashSet<String>,
    pub surfaces: HashMap<String, Arc<Vec<[Vec3; 3]>>>,
}

impl RefinedTerrain {
    /// The theater's models are ready to be spawned or read.
    pub fn ready(&self, stem: &str) -> bool {
        self.files.contains(stem)
    }
}

/// Coastline sample spacing as a fraction of the model's ground extent.
const COAST_SAMPLES: f64 = 40.0;
/// Largest coastline offset in sample spacings.
const COAST_AMPLITUDE: f64 = 2.6;
/// Offsets fade in over this many sample spacings from the original vertices.
const COAST_TAPER: f64 = 3.0;
/// A coastline point moves at most this fraction of the way across its polygon.
const COAST_ROOM: f64 = 0.45;
/// Mountain midpoint offsets per subdivision level, as a fraction of the edge length.
const MOUNTAIN_AMPLITUDE: [f32; 3] = [0.07, 0.045, 0.03];
/// Smallest mountain (ground extent in metres at its finest detail level) and the size from
/// which it gets a third subdivision.
const MOUNTAIN_MIN_M: f64 = 1400.0;
const MOUNTAIN_FINE_M: f64 = 12_000.0;
/// Height in metres of a model unit of lift between stacked flat overlays, at most: the
/// converter lifts coplanar polygons by fractions of a unit (painter's order), which on the
/// coarse levels is metres, and from the lower polygon's side the step to a higher neighbour
/// shows the background through as a thin line.
const OVERLAY_LIFT_M: f64 = 0.5;

#[allow(clippy::too_many_arguments)]
fn refine(
    theater: Res<Theater>,
    palette: Res<Palette>,
    server: Res<AssetServer>,
    mut library: ResMut<ModelLibrary>,
    gltfs: Res<Assets<Gltf>>,
    nodes: Res<Assets<GltfNode>>,
    mut gltf_meshes: ResMut<Assets<GltfMesh>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut refined: ResMut<RefinedTerrain>,
) {
    if refined.ready(&theater.id) {
        return;
    }
    let handle = library.load(&server, &theater.id);
    let Some(gltf) = gltfs.get(&handle) else { return };
    let start = bevy::platform::time::Instant::now();
    let colours: HashMap<AssetId<GltfMaterial>, u8> = gltf
        .named_materials
        .iter()
        .filter_map(|(n, h)| Some((h.id(), index_from_name(n)?)))
        .collect();
    let materials: HashMap<u8, Handle<GltfMaterial>> = gltf
        .named_materials
        .iter()
        .filter_map(|(n, h)| Some((index_from_name(n)?, h.clone())))
        .collect();
    let remap = &palette.remaps[REMAP_DETAIL_HIGH].entries;
    let class = |c: u8| {
        classify(remap.get(c as usize).copied().unwrap_or(0xFF00 | c as u16), &palette.theater.day)
    };

    // Finest and coarsest detail level each shape is placed on (its model unit in metres).
    let mut level: HashMap<usize, usize> = HashMap::new();
    let mut coarsest: HashMap<usize, usize> = HashMap::new();
    for (lod, tiles) in theater.tiles.tiles.iter().enumerate().skip(1) {
        for o in tiles.iter().flatten() {
            let l = level.entry(o.shape as usize).or_insert(lod);
            *l = (*l).min(lod);
            let c = coarsest.entry(o.shape as usize).or_insert(lod);
            *c = (*c).max(lod);
        }
    }

    let mut done: HashSet<AssetId<GltfMesh>> = HashSet::new();
    let (mut coasts, mut mountains) = (0, 0);
    for shape in 0..theater.model_count() {
        let Some(&lod) = level.get(&shape) else { continue };
        let Some(root) = gltf.named_nodes.get(format!("{}_{:03}", theater.id, shape).as_str())
        else {
            continue;
        };
        let Some(root) = nodes.get(root) else { continue };
        for lod_node in &root.children {
            let Some(node) = nodes.get(lod_node) else { continue };
            let mut parts = Vec::new();
            collect(&nodes, lod_node, Mat4::IDENTITY, &mut parts);
            // Destructible or animated models (conditional geometry) are buildings, not terrain.
            if parts.iter().any(|p| p.cond)
                || parts.iter().any(|p| p.mesh.as_ref().is_some_and(|m| done.contains(&m.id())))
            {
                continue;
            }
            let mut geo = Geometry::read(&parts, &gltf_meshes, &meshes, &colours);
            let mut changed = false;
            if coast(&mut geo, &class) {
                coasts += 1;
                changed = true;
            }
            let lift = OVERLAY_LIFT_M / (LEVEL_UNIT[coarsest[&shape]] * WU);
            if lift < 1.0 && flatten_lifts(&mut geo, lift as f32) {
                changed = true;
            }
            let unit_m = LEVEL_UNIT[lod] * WU;
            if node.name.ends_with("_lod0")
                && !parts.iter().any(|p| p.dots)
                && let Some(levels) = mountain_levels(&geo, unit_m)
            {
                let surface = mountain(&mut geo, levels, &|c| materials.contains_key(&c));
                let inv = node.transform.to_matrix().inverse();
                let local = surface.iter().map(|t| t.map(|p| inv.transform_point3(p))).collect();
                refined.surfaces.insert(node.name.clone(), Arc::new(local));
                mountains += 1;
                changed = true;
            }
            if changed {
                geo.write(&parts, &mut gltf_meshes, &mut meshes, &materials);
            }
            done.extend(parts.iter().filter_map(|p| p.mesh.as_ref().map(|m| m.id())));
        }
    }
    debug!(
        "terrain: refined {coasts} coastline and {mountains} mountain models in {:.2?}",
        start.elapsed()
    );
    refined.files.insert(theater.id.clone());
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Class {
    /// Blue overlays (sea and lakes).
    Water,
    /// Invisible in the 3D view (colour word 0xFBxx), drawn by the map.
    Overlay,
    /// Anything else (land patches, concrete, roads, ...): kept straight.
    Other,
}

/// Class of a flat polygon's colour word, judged like `scenery::ground` by the colour it shows
/// over the middle of the ground gradient in the day palette.
fn classify(word: u16, day: &VgaPalette) -> Class {
    let lo = (word & 0xFF) as u8;
    let shown = match word >> 8 {
        0xFB => return Class::Overlay,
        0xFD => 0x78 | lo,
        0xFE => 0x78 & lo,
        _ => lo,
    };
    let [r, g, b] = day.colors[shown as usize].map(i32::from);
    if b > r + 8 && b > g + 8 { Class::Water } else { Class::Other }
}

/// A node with a mesh below a detail level node, with its transform to the model's frame.
struct Part {
    mesh: Option<Handle<GltfMesh>>,
    to_model: Mat4,
    cond: bool,
    dots: bool,
}

fn collect(nodes: &Assets<GltfNode>, handle: &Handle<GltfNode>, parent: Mat4, out: &mut Vec<Part>) {
    let Some(node) = nodes.get(handle) else { return };
    let m = parent * node.transform.to_matrix();
    let name = node.name.as_str();
    let cond = name.split("_s").skip(1).any(|s| {
        s.split_once('v')
            .is_some_and(|(slot, _)| !slot.is_empty() && slot.bytes().all(|b| b.is_ascii_digit()))
    });
    out.push(Part { mesh: node.mesh.clone(), to_model: m, cond, dots: name.contains("_dots_") });
    for c in &node.children {
        collect(nodes, c, m, out);
    }
}

/// Triangles and lines of one detail level in the model frame, per primitive.
struct Geometry {
    prims: Vec<Prim>,
}

struct Prim {
    part: usize,
    index: usize,
    colour: u8,
    tris: Vec<[Vec3; 3]>,
    lines: Vec<[Vec3; 2]>,
    lists: bool,
    changed: bool,
}

impl Geometry {
    fn read(
        parts: &[Part],
        gltf_meshes: &Assets<GltfMesh>,
        meshes: &Assets<Mesh>,
        colours: &HashMap<AssetId<GltfMaterial>, u8>,
    ) -> Self {
        let mut prims = Vec::new();
        for (k, part) in parts.iter().enumerate() {
            let Some(gm) = part.mesh.as_ref().and_then(|h| gltf_meshes.get(h)) else { continue };
            for (index, prim) in gm.primitives.iter().enumerate() {
                let Some(mesh) = meshes.get(&prim.mesh) else { continue };
                let Some(VertexAttributeValues::Float32x3(pos)) =
                    mesh.attribute(Mesh::ATTRIBUTE_POSITION)
                else {
                    continue;
                };
                let pos: Vec<Vec3> =
                    pos.iter().map(|&p| part.to_model.transform_point3(Vec3::from(p))).collect();
                let idx: Vec<usize> =
                    mesh.indices().map_or_else(|| (0..pos.len()).collect(), |i| i.iter().collect());
                let colour = prim
                    .material
                    .as_ref()
                    .and_then(|m| colours.get(&m.id()).copied())
                    .unwrap_or(15);
                let mut p = Prim {
                    part: k,
                    index,
                    colour,
                    tris: vec![],
                    lines: vec![],
                    lists: true,
                    changed: false,
                };
                match mesh.primitive_topology() {
                    PrimitiveTopology::TriangleList => {
                        p.tris = idx
                            .as_chunks::<3>()
                            .0
                            .iter()
                            .map(|t| [pos[t[0]], pos[t[1]], pos[t[2]]])
                            .collect()
                    }
                    PrimitiveTopology::LineList => {
                        p.lines =
                            idx.as_chunks::<2>().0.iter().map(|l| [pos[l[0]], pos[l[1]]]).collect()
                    }
                    _ => p.lists = false,
                }
                prims.push(p);
            }
        }
        Self { prims }
    }

    /// Writes the changed primitives back (node space, positions only, unindexed), adding
    /// primitives for new triangle colours and dropping emptied ones.
    fn write(
        &self,
        parts: &[Part],
        gltf_meshes: &mut Assets<GltfMesh>,
        meshes: &mut Assets<Mesh>,
        materials: &HashMap<u8, Handle<GltfMaterial>>,
    ) {
        for (k, part) in parts.iter().enumerate() {
            let Some(handle) = &part.mesh else { continue };
            let prims: Vec<&Prim> = self.prims.iter().filter(|p| p.part == k).collect();
            if !prims.iter().any(|p| p.changed) {
                continue;
            }
            let Some(mut gm) = gltf_meshes.get_mut(handle) else { continue };
            let inv = part.to_model.inverse();
            let template = gm.primitives.first().cloned();
            let mut keep = vec![true; gm.primitives.len()];
            for p in prims.iter().filter(|p| p.changed) {
                let (topology, pos): (_, Vec<Vec3>) = if p.lines.is_empty() {
                    (
                        PrimitiveTopology::TriangleList,
                        p.tris.iter().flatten().map(|&v| inv.transform_point3(v)).collect(),
                    )
                } else {
                    (
                        PrimitiveTopology::LineList,
                        p.lines.iter().flatten().map(|&v| inv.transform_point3(v)).collect(),
                    )
                };
                let mesh = |usage| {
                    Mesh::new(topology, usage)
                        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos.clone())
                };
                if p.index < gm.primitives.len() {
                    if pos.is_empty() {
                        keep[p.index] = false;
                        continue;
                    }
                    let id = gm.primitives[p.index].mesh.id();
                    if let Some(mut m) = meshes.get_mut(id) {
                        let usage = m.asset_usage;
                        *m = mesh(usage);
                    }
                } else if let (Some(t), Some(material)) = (&template, materials.get(&p.colour))
                    && !pos.is_empty()
                {
                    let mut prim = t.clone();
                    prim.index = gm.primitives.len();
                    prim.name = format!("{}_pal_{:03}", t.name, p.colour);
                    prim.material = Some(material.clone());
                    prim.mesh = meshes.add(mesh(bevy::asset::RenderAssetUsages::default()));
                    gm.primitives.push(prim);
                    keep.push(true);
                }
            }
            let mut i = 0;
            gm.primitives.retain(|_| {
                i += 1;
                keep[i - 1]
            });
        }
    }
}

/// Scales the lifts of the flat overlays (and flat lines) by `k`, keeping their order; false
/// when the detail level has none.
fn flatten_lifts(geo: &mut Geometry, k: f32) -> bool {
    let mut any = false;
    for prim in &mut geo.prims {
        let flat =
            |vs: &[Vec3]| vs.iter().all(|v| v.y.abs() < 1.0) && vs.iter().any(|v| v.y != 0.0);
        let tris = prim.tris.iter_mut().filter(|t| flat(&t[..])).flatten();
        let lines = prim.lines.iter_mut().filter(|l| flat(&l[..])).flatten();
        for v in tris.chain(lines) {
            v.y *= k;
            prim.changed = true;
            any = true;
        }
    }
    any
}

// ------------------------------------------------------------------------------------------
// Coastlines

fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Hash of integers to -1..1.
fn signed(seed: u64, k: i64) -> f64 {
    (mix(seed ^ mix(k as u64)) >> 11) as f64 / (1u64 << 52) as f64 - 1.0
}

/// 1D fractal value noise, about -1..1 (`x` in sample spacings), wavelengths 8 down to 1.
fn fbm(seed: u64, x: f64) -> f64 {
    let (mut sum, mut norm, mut amp, mut wave) = (0.0, 0.0, 1.0, 8.0);
    for octave in 0..4u64 {
        let u = x / wave;
        let (k, f) = (u.floor(), u - u.floor());
        let s = f * f * (3.0 - 2.0 * f);
        let seed = seed.wrapping_add(octave * 0x9E37_79B9);
        sum += amp * (signed(seed, k as i64) * (1.0 - s) + signed(seed, k as i64 + 1) * s);
        norm += amp * amp;
        amp *= 0.6;
        wave *= 0.5;
    }
    (sum / norm.sqrt()).clamp(-1.0, 1.0)
}

struct Line {
    d: DVec2,
    n: DVec2,
    off: f64,
    /// (triangle, side of the line it lies on).
    edges: Vec<(usize, f64)>,
    /// Positions along the line of every vertex on it.
    breaks: Vec<f64>,
    active: bool,
    scale: f64,
    /// Points inserted along the line: position and offset.
    samples: Vec<(f64, f64)>,
}

impl Line {
    fn at(&self, t: f64) -> DVec2 {
        self.d * t + self.n * self.off
    }
}

/// A flat triangle of the model with its line per edge.
struct Flat {
    prim: usize,
    tri: usize,
    p: [Vec3; 3],
    class: Class,
    lines: [Option<usize>; 3],
}

fn q(p: Vec3) -> DVec2 {
    DVec2::new(p.x as f64, p.z as f64)
}

/// Refines the shorelines of a detail level; false when it has none.
fn coast(geo: &mut Geometry, class: &dyn Fn(u8) -> Class) -> bool {
    let mut flats: Vec<Flat> = Vec::new();
    for (pi, prim) in geo.prims.iter().enumerate() {
        let c = class(prim.colour);
        for (ti, t) in prim.tris.iter().enumerate() {
            if t.iter().all(|v| v.y.abs() < 1.0)
                && (q(t[1]) - q(t[0])).perp_dot(q(t[2]) - q(t[0])).abs() > 1e-6
            {
                flats.push(Flat { prim: pi, tri: ti, p: *t, class: c, lines: [None; 3] });
            }
        }
    }
    if !flats.iter().any(|f| f.class == Class::Water) {
        return false;
    }
    let (mut lo, mut hi) = (DVec2::splat(f64::MAX), DVec2::splat(f64::MIN));
    for v in flats.iter().flat_map(|f| f.p) {
        lo = lo.min(q(v));
        hi = hi.max(q(v));
    }
    let spacing = ((hi - lo).max_element() / COAST_SAMPLES).max(2.0);

    // Lines through the edges (collinear edges share one).
    let mut lines: Vec<Line> = Vec::new();
    for (fi, f) in flats.iter_mut().enumerate() {
        for e in 0..3 {
            let (a, b, c) = (q(f.p[e]), q(f.p[(e + 1) % 3]), q(f.p[(e + 2) % 3]));
            let mut d = (b - a).normalize();
            if d.x < -1e-9 || (d.x.abs() <= 1e-9 && d.y < 0.0) {
                d = -d;
            }
            let n = d.perp();
            let off = n.dot(a);
            let id = match lines
                .iter()
                .position(|l| l.d.perp_dot(d).abs() < 1e-5 && (l.off - off).abs() < 0.05)
            {
                Some(i) => i,
                None => {
                    lines.push(Line {
                        d,
                        n,
                        off,
                        edges: vec![],
                        breaks: vec![],
                        active: false,
                        scale: 1.0,
                        samples: vec![],
                    });
                    lines.len() - 1
                }
            };
            let side = (n.dot(c) - off).signum();
            lines[id].edges.push((fi, side));
            f.lines[e] = Some(id);
        }
    }
    let near = |v: f64, w: f64| (v - w).abs() < 0.5;
    for (li, line) in lines.iter_mut().enumerate() {
        for f in &flats {
            for v in f.p {
                if (line.n.dot(q(v)) - line.off).abs() < 0.05 {
                    line.breaks.push(line.d.dot(q(v)));
                }
            }
        }
        line.breaks.sort_by(f64::total_cmp);
        line.breaks.dedup_by(|a, b| (*a - *b).abs() < 1e-3);
        // Boundaries of a class: its polygons lie on one side only.
        let one_sided = |c: Class| {
            let sides: Vec<f64> =
                line.edges.iter().filter(|(fi, _)| flats[*fi].class == c).map(|e| e.1).collect();
            !sides.is_empty() && sides.iter().all(|&s| s == sides[0])
        };
        let outline = line.edges.iter().any(|&(fi, _)| {
            let f = &flats[fi];
            let e = f.lines.iter().position(|l| *l == Some(li)).unwrap_or(0);
            let (a, b) = (q(f.p[e]), q(f.p[(e + 1) % 3]));
            (near(a.x, lo.x) && near(b.x, lo.x))
                || (near(a.x, hi.x) && near(b.x, hi.x))
                || (near(a.y, lo.y) && near(b.y, lo.y))
                || (near(a.y, hi.y) && near(b.y, hi.y))
        });
        let other = line.edges.iter().any(|&(fi, _)| flats[fi].class == Class::Other);
        line.active = !outline && !other && (one_sided(Class::Water) || one_sided(Class::Overlay));
    }
    if !lines.iter().any(|l| l.active) {
        return false;
    }

    // Offsets along the active lines.
    for line in lines.iter_mut().filter(|l| l.active) {
        let seed = mix((line.off * 4.0).round() as i64 as u64
            ^ mix((line.d.y.atan2(line.d.x) * 1e4).round() as i64 as u64));
        let ts = || {
            line.edges
                .iter()
                .flat_map(|&(fi, _)| flats[fi].p)
                .filter(|v| (line.n.dot(q(*v)) - line.off).abs() < 0.05)
                .map(|v| line.d.dot(q(v)))
        };
        let (t0, t1) = (ts().fold(f64::MAX, f64::min), ts().fold(f64::MIN, f64::max));
        let mut points: Vec<f64> =
            line.breaks.iter().copied().filter(|&t| t > t0 - 1e-3 && t < t1 + 1e-3).collect();
        let mut k = (t0 / spacing).ceil();
        while k * spacing < t1 {
            let t = k * spacing;
            if line.breaks.iter().all(|b| (b - t).abs() > spacing * 0.25) {
                points.push(t);
            }
            k += 1.0;
        }
        points.sort_by(f64::total_cmp);
        line.samples = points
            .into_iter()
            .map(|t| {
                let gap = line.breaks.iter().map(|b| (b - t).abs()).fold(f64::MAX, f64::min);
                if gap < 1e-3 {
                    return (t, 0.0);
                }
                let taper = (gap / (spacing * COAST_TAPER)).min(1.0);
                let taper = taper * taper * (3.0 - 2.0 * taper);
                let mut off = fbm(seed, t / spacing) * COAST_AMPLITUDE * spacing * taper;
                // Room on the side it moves to: across the polygons on that side and within
                // the model's outline.
                let p = line.at(t);
                let dir = line.n * off.signum();
                let mut room = [
                    (lo.x - p.x) / dir.x,
                    (hi.x - p.x) / dir.x,
                    (lo.y - p.y) / dir.y,
                    (hi.y - p.y) / dir.y,
                ]
                .into_iter()
                .filter(|r| r.is_finite() && *r > 0.0)
                .fold(f64::MAX, f64::min);
                for &(fi, side) in &line.edges {
                    if side != off.signum() {
                        continue;
                    }
                    let f = &flats[fi];
                    let span =
                        f.p.iter()
                            .filter(|v| (line.n.dot(q(**v)) - line.off).abs() < 0.05)
                            .map(|v| line.d.dot(q(*v)));
                    let (a, b) = span.fold((f64::MAX, f64::MIN), |(a, b), t| (a.min(t), b.max(t)));
                    if t <= a || t >= b {
                        continue;
                    }
                    for e in 0..3 {
                        if let Some(r) = ray_segment(p, dir, q(f.p[e]), q(f.p[(e + 1) % 3])) {
                            room = room.min(r);
                        }
                    }
                }
                off = off.signum() * off.abs().min(room * COAST_ROOM);
                (t, off)
            })
            .collect();
    }

    // Rebuild the triangles on active lines; halve a line's offsets where a polygon would fold.
    let mut out: Vec<Option<Vec<[Vec3; 3]>>> = Vec::new();
    for round in 0..8 {
        out.clear();
        let mut bad: HashSet<usize> = HashSet::new();
        for f in &flats {
            if !f.lines.iter().flatten().any(|&l| lines[l].active) {
                out.push(None);
                continue;
            }
            let poly = outline_points(f, &lines);
            match triangulate(&poly, f.p) {
                Some(t) => out.push(Some(t)),
                None => {
                    bad.extend(
                        f.lines
                            .iter()
                            .flatten()
                            .copied()
                            .filter(|&l| lines[l].active && lines[l].scale > 0.0),
                    );
                    out.push(None);
                }
            }
        }
        if bad.is_empty() {
            break;
        }
        for l in bad {
            lines[l].scale = if round >= 6 { 0.0 } else { lines[l].scale * 0.5 };
        }
    }

    let mut replaced: HashMap<(usize, usize), Vec<[Vec3; 3]>> = HashMap::new();
    for (f, t) in flats.iter().zip(out) {
        if let Some(t) = t {
            replaced.insert((f.prim, f.tri), t);
        }
    }
    for (pi, prim) in geo.prims.iter_mut().enumerate() {
        if !(0..prim.tris.len()).any(|ti| replaced.contains_key(&(pi, ti))) {
            continue;
        }
        let tris = std::mem::take(&mut prim.tris);
        for (ti, t) in tris.into_iter().enumerate() {
            match replaced.remove(&(pi, ti)) {
                Some(new) => prim.tris.extend(new),
                None => prim.tris.push(t),
            }
        }
        prim.changed = true;
    }
    true
}

/// Distance along a ray to a segment, if it hits.
fn ray_segment(p: DVec2, dir: DVec2, a: DVec2, b: DVec2) -> Option<f64> {
    let e = b - a;
    let den = dir.perp_dot(e);
    if den.abs() < 1e-12 {
        return None;
    }
    let w = a - p;
    let t = w.perp_dot(e) / den;
    let s = w.perp_dot(dir) / den;
    (t > 1e-6 && (-1e-9..=1.0 + 1e-9).contains(&s)).then_some(t)
}

/// A triangle's outline with the points of its active lines (y along each edge).
fn outline_points(f: &Flat, lines: &[Line]) -> Vec<(DVec2, f32)> {
    let mut pts = Vec::new();
    for e in 0..3 {
        let (a, b) = (f.p[e], f.p[(e + 1) % 3]);
        pts.push((q(a), a.y));
        let Some(l) = f.lines[e].map(|l| &lines[l]).filter(|l| l.active) else { continue };
        let (ta, tb) = (l.d.dot(q(a)), l.d.dot(q(b)));
        let inside =
            |t: f64| (t - ta) * (t - tb) < 0.0 && (t - ta).abs() > 1e-3 && (t - tb).abs() > 1e-3;
        let mut add: Vec<(f64, f64)> =
            l.samples.iter().copied().filter(|&(t, _)| inside(t)).collect();
        if tb < ta {
            add.reverse();
        }
        for (t, off) in add {
            let y = a.y + (b.y - a.y) * ((t - ta) / (tb - ta)) as f32;
            pts.push((l.at(t) + l.n * off * l.scale, y));
        }
    }
    pts
}

/// Ear-clips a simple polygon wound like the triangle `orig`; None when the polygon folds.
fn triangulate(poly: &[(DVec2, f32)], orig: [Vec3; 3]) -> Option<Vec<[Vec3; 3]>> {
    let sign = (q(orig[1]) - q(orig[0])).perp_dot(q(orig[2]) - q(orig[0])).signum();
    let n = poly.len();
    let area: f64 = (0..n).map(|i| poly[i].0.perp_dot(poly[(i + 1) % n].0)).sum::<f64>() * 0.5;
    if area * sign <= 0.0 {
        return None;
    }
    // Simple: no two non-adjacent edges cross or touch.
    for i in 0..n {
        for j in i + 2..n {
            if i == 0 && j == n - 1 {
                continue;
            }
            if segments_touch(poly[i].0, poly[(i + 1) % n].0, poly[j].0, poly[(j + 1) % n].0) {
                return None;
            }
        }
    }
    let cross = |a: DVec2, b: DVec2, c: DVec2| (b - a).perp_dot(c - a) * sign;
    let eps = area.abs() * 1e-9;
    let mut idx: Vec<usize> = (0..n).collect();
    let mut out = Vec::with_capacity(n);
    let mut sum = 0.0;
    let v3 = |i: usize| Vec3::new(poly[i].0.x as f32, poly[i].1, poly[i].0.y as f32);
    while idx.len() > 3 {
        let m = idx.len();
        let ear = (0..m).find_map(|k| {
            let (a, b, c) = (idx[(k + m - 1) % m], idx[k], idx[(k + 1) % m]);
            let x = cross(poly[a].0, poly[b].0, poly[c].0);
            if x < -eps {
                return None;
            }
            if x <= eps {
                return Some((k, None)); // collinear: drop the vertex
            }
            let blocked = idx.iter().any(|&o| {
                o != a && o != b && o != c && {
                    let p = poly[o].0;
                    cross(poly[a].0, poly[b].0, p) > 0.0
                        && cross(poly[b].0, poly[c].0, p) > 0.0
                        && cross(poly[c].0, poly[a].0, p) > 0.0
                }
            });
            (!blocked).then_some((k, Some([a, b, c])))
        })?;
        if let (_, Some([a, b, c])) = ear {
            sum += cross(poly[a].0, poly[b].0, poly[c].0) * 0.5;
            out.push(orient([v3(a), v3(b), v3(c)], orig));
        }
        idx.remove(ear.0);
    }
    let (a, b, c) = (idx[0], idx[1], idx[2]);
    let x = cross(poly[a].0, poly[b].0, poly[c].0);
    if x < -eps {
        return None;
    }
    if x > eps {
        sum += x * 0.5;
        out.push(orient([v3(a), v3(b), v3(c)], orig));
    }
    ((sum - area.abs()).abs() <= area.abs() * 1e-6).then_some(out)
}

/// Winds a triangle like `orig` (front face up or down as the original polygon).
fn orient(t: [Vec3; 3], orig: [Vec3; 3]) -> [Vec3; 3] {
    let s = |t: &[Vec3; 3]| (q(t[1]) - q(t[0])).perp_dot(q(t[2]) - q(t[0]));
    if s(&t) * s(&orig) < 0.0 { [t[0], t[2], t[1]] } else { t }
}

fn segments_touch(a: DVec2, b: DVec2, c: DVec2, d: DVec2) -> bool {
    let o = |p: DVec2, q0: DVec2, q1: DVec2| (q0 - p).perp_dot(q1 - p);
    let (d1, d2, d3, d4) = (o(a, b, c), o(a, b, d), o(c, d, a), o(c, d, b));
    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
    {
        return true;
    }
    let on = |p: DVec2, q0: DVec2, q1: DVec2, o: f64| {
        o.abs() < 1e-9
            && p.x >= q0.x.min(q1.x)
            && p.x <= q0.x.max(q1.x)
            && p.y >= q0.y.min(q1.y)
            && p.y <= q0.y.max(q1.y)
    };
    on(c, a, b, d1) || on(d, a, b, d2) || on(a, c, d, d3) || on(b, c, d, d4)
}

// ------------------------------------------------------------------------------------------
// Mountains

/// Subdivision levels for a large raised terrain model, None for anything else.
fn mountain_levels(geo: &Geometry, unit_m: f64) -> Option<u32> {
    let tris = || geo.prims.iter().flat_map(|p| p.tris.iter());
    let raised = tris().any(|t| t.iter().any(|v| v.y > 1.0));
    let height = tris().flatten().map(|v| v.y).fold(0.0, f32::max);
    if !raised || height < 32.0 || geo.prims.iter().any(|p| !p.lists) {
        return None;
    }
    let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
    for v in tris().flatten() {
        lo = lo.min(v.xz());
        hi = hi.max(v.xz());
    }
    let extent_m = (hi - lo).max_element() as f64 * unit_m;
    (extent_m >= MOUNTAIN_MIN_M).then_some(if extent_m >= MOUNTAIN_FINE_M { 3 } else { 2 })
}

type EdgeKey = [i64; 6];

fn edge_key(a: Vec3, b: Vec3) -> EdgeKey {
    let k = |v: Vec3| {
        [(v.x * 64.0).round() as i64, (v.y * 64.0).round() as i64, (v.z * 64.0).round() as i64]
    };
    let (ka, kb) = (k(a), k(b));
    let (x, y) = if ka <= kb { (ka, kb) } else { (kb, ka) };
    [x[0], x[1], x[2], y[0], y[1], y[2]]
}

fn normal(t: &[Vec3; 3]) -> Vec3 {
    (t[1] - t[0]).cross(t[2] - t[0]).normalize_or_zero()
}

/// Subdivides the raised triangles, recolours the new faces and moves the lines lying on
/// subdivided edges onto them. Returns the refined surface (model frame).
fn mountain(geo: &mut Geometry, levels: u32, available: &dyn Fn(u8) -> bool) -> Vec<[Vec3; 3]> {
    let mut faces: Vec<([Vec3; 3], u8)> = Vec::new();
    // New colours go to the mesh holding the raised faces.
    let mut part = 0;
    for p in geo.prims.iter_mut().filter(|p| !p.tris.is_empty()) {
        let (raised, flat): (Vec<[Vec3; 3]>, Vec<[Vec3; 3]>) =
            p.tris.drain(..).partition(|t| t.iter().any(|v| v.y > 1.0));
        if !raised.is_empty() {
            part = p.part;
        }
        p.tris = flat;
        p.changed = true;
        faces.extend(raised.into_iter().map(|t| (t, p.colour)));
    }
    // Overlays: polygons drawn onto a larger coplanar face (snow caps, rock bands), lifted a
    // little off it. They are cut along the refined face instead of being subdivided.
    let area = |t: &[Vec3; 3]| (t[1] - t[0]).cross(t[2] - t[0]).length();
    let mut base_of: Vec<Option<usize>> = vec![None; faces.len()];
    for (i, (t, _)) in faces.iter().enumerate() {
        let (n, c) = (normal(t), (t[0] + t[1] + t[2]) / 3.0);
        base_of[i] = faces
            .iter()
            .enumerate()
            .filter(|&(j, (f, _))| {
                j != i
                    && normal(f).dot(n) > 0.999
                    && normal(f).dot(c - f[0]).abs() < 0.5
                    && area(f) > area(t)
                    && {
                        let w = barycentric(c, f);
                        w.min_element() > -1e-3
                    }
            })
            .max_by(|a, b| area(&a.1.0).total_cmp(&area(&b.1.0)))
            .map(|(j, _)| j);
    }
    for i in 0..faces.len() {
        let mut b = base_of[i];
        while let Some(j) = b.and_then(|j| base_of[j]) {
            b = Some(j);
        }
        base_of[i] = b;
    }
    let mut subs: Vec<Sub> = (0..faces.len())
        .filter(|&i| base_of[i].is_none())
        .map(|i| Sub { t: faces[i].0, rest: faces[i].0, base: i })
        .collect();
    let mut mids: Vec<HashMap<EdgeKey, Vec3>> = Vec::new();
    for amplitude in MOUNTAIN_AMPLITUDE.iter().take(levels as usize) {
        // Summed face normals and face count by edge.
        let mut adjacent: HashMap<EdgeKey, (Vec3, u32)> = HashMap::new();
        for s in &subs {
            let n = normal(&s.t);
            for e in 0..3 {
                let a = adjacent.entry(edge_key(s.t[e], s.t[(e + 1) % 3])).or_default();
                a.0 += n;
                a.1 += 1;
            }
        }
        let mut level_mids: HashMap<EdgeKey, Vec3> = HashMap::new();
        let mut mid = |a: Vec3, b: Vec3| -> Vec3 {
            let key = edge_key(a, b);
            *level_mids.entry(key).or_insert_with(|| {
                let m = (a + b) * 0.5;
                if a.y < 1.0 && b.y < 1.0 {
                    return m; // the ground outline stays
                }
                // An open edge (one face): the range goes on in the neighbouring model, which
                // cannot know this one's offsets, so it stays straight for both to meet.
                let Some(&(n, 2..)) = adjacent.get(&key) else { return m };
                let n = n.normalize_or_zero();
                let h = (mix(key.iter().fold(0x5EED_u64, |h, &k| mix(h ^ k as u64))) >> 11) as f32
                    / (1u64 << 53) as f32;
                let off = (h * 1.35 - 0.35) * amplitude * a.distance(b);
                let lift = if a.y < 1.0 || b.y < 1.0 { 0.5 } else { 1.0 };
                let mut m = m + n * off * lift;
                m.y = m.y.max(0.0);
                m
            })
        };
        let mut next = Vec::with_capacity(subs.len() * 4);
        for s in &subs {
            let [a, b, c] = s.t;
            let (ab, bc, ca) = (mid(a, b), mid(b, c), mid(c, a));
            let [ra, rb, rc] = s.rest;
            let (rab, rbc, rca) = ((ra + rb) * 0.5, (rb + rc) * 0.5, (rc + ra) * 0.5);
            for (t, rest) in [
                ([a, ab, ca], [ra, rab, rca]),
                ([ab, b, bc], [rab, rb, rbc]),
                ([ca, bc, c], [rca, rbc, rc]),
                ([ab, bc, ca], [rab, rbc, rca]),
            ] {
                next.push(Sub { t, rest, base: s.base });
            }
        }
        subs = next;
        mids.push(level_mids);
    }
    // Faces turned further from the sun than their original face get darker ramp steps.
    let sun = crate::lighting::sun_direction();
    let shade = |c: u8, step: i32| {
        let (lo, hi) = ((c & 0xF0) as i32, (c | 0x0F) as i32);
        let mut colour = (c as i32 + step).clamp(lo, hi) as u8;
        while colour != c && !available(colour) {
            colour = if colour > c { colour - 1 } else { colour + 1 };
        }
        colour
    };
    let step = |s: &Sub| {
        ((normal(&faces[s.base].0).dot(sun) - normal(&s.t).dot(sun)) * 5.0).round().clamp(-2.0, 2.0)
            as i32
    };
    let mut by_colour: HashMap<u8, Vec<[Vec3; 3]>> = HashMap::new();
    for s in &subs {
        by_colour.entry(shade(faces[s.base].1, step(s))).or_default().push(s.t);
    }
    for (o, (t, colour)) in faces.iter().enumerate() {
        let Some(b) = base_of[o] else { continue };
        let f = faces[b].0;
        let lift = normal(&f).dot((t[0] + t[1] + t[2]) / 3.0 - f[0]).max(0.02);
        for s in subs.iter().filter(|s| s.base == b) {
            let n = normal(&s.t);
            let piece = clip(t, &s.rest);
            if piece.len() < 3 {
                continue;
            }
            let pts: Vec<Vec3> = piece
                .iter()
                .map(|&p| {
                    let w = barycentric(p, &s.rest);
                    s.t[0] * w.x + s.t[1] * w.y + s.t[2] * w.z + n * lift
                })
                .collect();
            let out = by_colour.entry(shade(*colour, step(s))).or_default();
            for i in 1..pts.len() - 1 {
                let tri = [pts[0], pts[i], pts[i + 1]];
                out.push(if normal(&tri).dot(n) < 0.0 { [tri[0], tri[2], tri[1]] } else { tri });
            }
        }
    }
    let surface: Vec<[Vec3; 3]> = subs.iter().map(|s| s.t).collect();
    for (colour, t) in by_colour {
        match geo
            .prims
            .iter_mut()
            .find(|p| p.colour == colour && p.part == part && p.lines.is_empty())
        {
            Some(p) => p.tris.extend(t),
            // Appended to the mesh by `Geometry::write`.
            None => geo.prims.push(Prim {
                part,
                index: usize::MAX,
                colour,
                tris: t,
                lines: vec![],
                lists: true,
                changed: true,
            }),
        }
    }
    // Lines along subdivided edges follow the new points.
    fn path(
        a: Vec3,
        b: Vec3,
        level: usize,
        mids: &[HashMap<EdgeKey, Vec3>],
        out: &mut Vec<[Vec3; 2]>,
    ) {
        match mids.get(level).and_then(|m| m.get(&edge_key(a, b))) {
            Some(&m) => {
                path(a, m, level + 1, mids, out);
                path(m, b, level + 1, mids, out);
            }
            None => out.push([a, b]),
        }
    }
    for p in geo.prims.iter_mut().filter(|p| !p.lines.is_empty()) {
        let mut out = Vec::new();
        for &[a, b] in &p.lines {
            path(a, b, 0, &mids, &mut out);
        }
        if out.len() != p.lines.len() {
            p.lines = out;
            p.changed = true;
        }
    }
    surface
}

/// A piece of a subdivided mountain face: refined corners, the same corners on the original
/// face, and the original face.
struct Sub {
    t: [Vec3; 3],
    rest: [Vec3; 3],
    base: usize,
}

/// Barycentric weights of `p` (on the triangle's plane) in triangle `t`.
fn barycentric(p: Vec3, t: &[Vec3; 3]) -> Vec3 {
    let (e0, e1, d) = (t[1] - t[0], t[2] - t[0], p - t[0]);
    let (d00, d01, d11, d20, d21) = (e0.dot(e0), e0.dot(e1), e1.dot(e1), d.dot(e0), d.dot(e1));
    let den = d00 * d11 - d01 * d01;
    if den.abs() < 1e-12 {
        return Vec3::new(1.0, 0.0, 0.0);
    }
    let v = (d11 * d20 - d01 * d21) / den;
    let w = (d00 * d21 - d01 * d20) / den;
    Vec3::new(1.0 - v - w, v, w)
}

/// The part of triangle `t` inside triangle `by` (both on the same plane; `t` is projected).
fn clip(t: &[Vec3; 3], by: &[Vec3; 3]) -> Vec<Vec3> {
    let n = (by[1] - by[0]).cross(by[2] - by[0]);
    let mut poly: Vec<Vec3> =
        t.iter().map(|&p| p - n * n.dot(p - by[0]) / n.length_squared()).collect();
    for e in 0..3 {
        let (a, b) = (by[e], by[(e + 1) % 3]);
        // Inside: on the same side of edge a-b as the triangle (its normal side).
        let side = |p: Vec3| (b - a).cross(p - a).dot(n);
        let mut out = Vec::with_capacity(poly.len() + 1);
        for i in 0..poly.len() {
            let (p, q) = (poly[i], poly[(i + 1) % poly.len()]);
            let (sp, sq) = (side(p), side(q));
            if sp >= 0.0 {
                out.push(p);
            }
            if (sp >= 0.0) != (sq >= 0.0) {
                out.push(p + (q - p) * (sp / (sp - sq)));
            }
        }
        poly = out;
        if poly.len() < 3 {
            return vec![];
        }
    }
    poly
}
