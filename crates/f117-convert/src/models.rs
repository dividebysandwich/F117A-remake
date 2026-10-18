//! .3DX models -> raw .glb (one file per .3DX) for the Blender import step.
//!
//! Node layout per model (see docs/re/README.md "Asset pipeline"):
//! ```text
//! <file>_<iii>                       extras: attr, lod_dists
//!   <file>_<iii>_lod<k>              extras: radius, max_dist   (geometry of part 0, unconditional)
//!     <file>_<iii>_lod<k>_s<S>v<V>   geometry drawn only if state[S] == V (e.g. s10v1 = night lights)
//!     <file>_<iii>_lod<k>_part<p>    articulated part, origin at its pivot; extras: state_slot, axis
//!       ..._part<p>_s<S>v<V>
//! ```
//! Geometry is in model units, axes as in the original (x east, y up, z = -north), which is
//! already glTF's right-handed Y-up frame. Material / node colour names use the model colour byte
//! (`pal_NNN`); the game maps it through the day/night remap tables to a palette index.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use f117_formats::GameDir;
use f117_formats::pal::Palette;
use f117_formats::x3d::{self, Body, Cond, PrimKind, X3d};
use serde_json::json;

use crate::glb::{Face, Glb, Node, Prim};

/// Painter's-order decals: the original draws a body's primitives in list order, later ones over
/// earlier ones. Each polygon is pushed this far (model units) along its normal per earlier
/// polygon in the same geometric plane, so coplanar markings stay on top without z-fighting.
/// Lines and dots (lights) all sit one step above the plane's topmost polygon, so long runs of
/// runway lights are not stacked on top of each other.
const DECAL_STEP: f32 = 0.03;

pub fn convert(game: &GameDir, palette: &Palette, src: &Path) -> Result<()> {
    let raw = src.join("raw");
    std::fs::create_dir_all(&raw)?;
    let mut files: Vec<String> = vec!["STFLT".into(), "PHOTO".into()];
    files.extend(f117_formats::THEATERS.iter().map(|t| t.to_string()));
    let mut total = 0;
    for f in files {
        let x = x3d::parse(&game.read(&format!("{f}.3DX"))?).with_context(|| format!("{f}.3DX"))?;
        let stem = f.to_ascii_lowercase();
        let mut glb = Glb::new(palette);
        let roots: Vec<usize> = (0..x.models.len()).map(|i| glb.add_node(&model_node(&x, i, &stem))).collect();
        glb.write(&roots, &raw.join(format!("{stem}.glb")))?;
        total += x.models.len();
    }
    println!("models: {total} models written to {}", raw.display());
    Ok(())
}

pub fn model_name(stem: &str, i: usize) -> String {
    format!("{stem}_{i:03}")
}

fn model_node(x: &X3d, i: usize, stem: &str) -> Node {
    let m = &x.models[i];
    let name = model_name(stem, i);
    let children = m
        .lods
        .iter()
        .enumerate()
        .map(|(k, lod)| {
            let mut n = body_node(&x.bodies[lod.body], &format!("{name}_lod{k}"));
            let body = &x.bodies[lod.body];
            // Collision hull: the original tests "inside an object" with the same planes it
            // uses for drawing (all planes invisible from the point). Main-part planes only.
            let planes: Vec<[i32; 4]> = body
                .planes
                .iter()
                .filter(|p| p.part == 0 && p.cond.is_none())
                .map(|p| [p.normal[0] as i32, p.normal[1] as i32, p.normal[2] as i32, p.d])
                .collect();
            n.extras = Some(json!({"radius": body.radius, "max_dist": lod.max_dist, "planes": planes}));
            n
        })
        .collect();
    Node {
        name,
        children,
        extras: Some(json!({"attr": m.attr, "lod_dists": m.lods.iter().map(|l| l.max_dist).collect::<Vec<_>>()})),
        ..Default::default()
    }
}

fn cond_suffix(conds: &[Cond]) -> String {
    conds.iter().map(|c| format!("_s{}v{}", c.slot, c.value)).collect()
}

fn body_node(body: &Body, name: &str) -> Node {
    // Group faces by (part, conditions).
    let mut groups: BTreeMap<(u8, Vec<Cond>), Vec<Face>> = BTreeMap::new();
    let vpos = |i: u8| {
        let v = &body.vertices[i as usize];
        let pivot = if v.part == 0 { [0; 3] } else { body.parts[v.part as usize - 1].pivot };
        [(v.pos[0] - pivot[0]) as f32, (v.pos[1] - pivot[1]) as f32, (v.pos[2] - pivot[2]) as f32]
    };
    // Draw-order rank per geometric plane (rounded normal and offset).
    let mut ranks: BTreeMap<(i32, i32, i32, i32), u32> = BTreeMap::new();
    // (kind, colour, vertices, normal, plane key, part, conditions, body without planes)
    #[allow(clippy::type_complexity)]
    let mut items: Vec<(Prim, u8, Vec<[f32; 3]>, [f32; 3], (i32, i32, i32, i32), u8, Vec<Cond>, bool)> = Vec::new();
    for (li, list) in body.lists.iter().enumerate() {
        let plane_normal = body.planes.get(li).map(|p| p.normal.map(|n| n as f32 / 16384.0));
        for prim in list.iter() {
            let (kind, idx): (Prim, Vec<u8>) = match &prim.kind {
                PrimKind::Polygon(v) => (Prim::Triangles, v.clone()),
                PrimKind::Lines(l) => (Prim::Lines, l.iter().flat_map(|&(a, b)| [a, b]).collect()),
                PrimKind::Dots(d) => (Prim::Points, d.clone()),
            };
            let part = body.vertices[idx[0] as usize].part;
            let mut verts: Vec<[f32; 3]> = idx.iter().map(|&i| vpos(i)).collect();
            // Back-face culling like the original's plane visibility: wind each polygon
            // counter-clockwise as seen from its plane's visible side (glTF front face).
            if kind == Prim::Triangles
                && let (Some(n), Some(pn)) = (newell(&verts), plane_normal)
                && n[0] * pn[0] + n[1] * pn[1] + n[2] * pn[2] < 0.0
            {
                verts.reverse();
            }
            let normal = match kind {
                Prim::Triangles => newell(&verts).or(plane_normal),
                _ => plane_normal,
            }
            .unwrap_or([0.0, 1.0, 0.0]);
            let d = normal[0] * verts[0][0] + normal[1] * verts[0][1] + normal[2] * verts[0][2];
            let key = (
                (normal[0] * 100.0).round() as i32,
                (normal[1] * 100.0).round() as i32,
                (normal[2] * 100.0).round() as i32,
                (d * 2.0).round() as i32,
            );
            let rank = ranks.entry(key).or_insert(0);
            if kind == Prim::Triangles {
                let off = DECAL_STEP * *rank as f32;
                *rank += 1;
                for v in &mut verts {
                    for a in 0..3 {
                        v[a] += normal[a] * off;
                    }
                }
            }
            items.push((kind, prim.color, verts, normal, key, part, prim.conds.clone(), plane_normal.is_none()));
        }
    }
    for (kind, color, mut verts, normal, key, part, conds, no_plane) in items {
        if kind != Prim::Triangles {
            let off = DECAL_STEP * ranks.get(&key).copied().unwrap_or(0) as f32;
            for v in &mut verts {
                for a in 0..3 {
                    v[a] += normal[a] * off;
                }
            }
        }
        {
            let faces = groups.entry((part, conds)).or_default();
            match kind {
                // Line segments are stored as pairs; split so each becomes its own 2-vertex face.
                Prim::Lines => {
                    for seg in verts.chunks(2) {
                        faces.push(Face { prim: Prim::Lines, color, verts: seg.to_vec() });
                    }
                }
                // Bodies without planes are never culled in the original: add the back side.
                Prim::Triangles if no_plane => {
                    let mut back = verts.clone();
                    back.reverse();
                    faces.push(Face { prim: kind, color, verts });
                    faces.push(Face { prim: kind, color, verts: back });
                }
                _ => faces.push(Face { prim: kind, color, verts }),
            }
        }
    }

    let mut root = Node { name: name.to_string(), ..Default::default() };
    let mut part_nodes: BTreeMap<u8, Node> = BTreeMap::new();
    for ((part, conds), faces) in groups {
        let parent: &mut Node = if part == 0 {
            &mut root
        } else {
            part_nodes.entry(part).or_insert_with(|| {
                let p = &body.parts[part as usize - 1];
                Node {
                    name: format!("{name}_part{part}"),
                    translation: p.pivot.map(|v| v as f32),
                    extras: Some(json!({"state_slot": p.state_slot, "axis": p.axis})),
                    ..Default::default()
                }
            })
        };
        if conds.is_empty() {
            parent.faces.extend(faces);
        } else {
            let cname = format!("{}{}", parent.name, cond_suffix(&conds));
            parent.children.push(Node {
                name: cname,
                faces,
                extras: Some(json!({"conds": conds.iter().map(|c| [c.slot as i32, c.value as i32]).collect::<Vec<_>>()})),
                ..Default::default()
            });
        }
    }
    root.children.extend(part_nodes.into_values());
    root
}

/// Polygon normal (Newell's method), normalised; None for degenerate polygons.
fn newell(v: &[[f32; 3]]) -> Option<[f32; 3]> {
    let mut n = [0f32; 3];
    for i in 0..v.len() {
        let (a, b) = (v[i], v[(i + 1) % v.len()]);
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    (len > 1e-6).then(|| [n[0] / len, n[1] / len, n[2] / len])
}
