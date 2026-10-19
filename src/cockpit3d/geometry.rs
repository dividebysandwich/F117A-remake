//! Cockpit geometry in the cockpit frame: origin at the design eye point (`camera::EYE_OFFSET`),
//! -Z forward, +Y up, +X right, metres.
//!
//! The instrument panel is a tilted plane laid out in the original 320x200 screen's pixels:
//! each panel pixel keeps the direction it had from the original eye (`sx = cx + f*x/z`,
//! `sy = cy - 0.75*f*y/z`), so from the design eye the classic panel sits where the original
//! picture was and the HUD lines up. Textured patches are grids so that mapping stays
//! projective; solid parts are flat polygons.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::cockpit::{FOCAL, VSCALE};

/// Original 3D window centre (cockpit view) in screen pixels.
pub const CX: f32 = 159.5;
pub const CY: f32 = 52.0;

/// Panel plane: through the point seen at screen row 150 at 0.72 m, tilted back by 0.3 rad.
const PANEL_DEPTH: f32 = 0.72;
const PANEL_ROW: f32 = 150.0;
const PANEL_TILT: f32 = 0.30;

/// Direction from the eye through an original screen pixel (z = -1).
pub fn pixel_dir(px: f32, py: f32) -> Vec3 {
    Vec3::new((px - CX) / FOCAL, -(py - CY) / (VSCALE * FOCAL), -1.0)
}

/// Panel normal (towards the pilot).
pub fn panel_normal() -> Vec3 {
    Vec3::new(0.0, PANEL_TILT.sin(), PANEL_TILT.cos())
}

/// Rotation taking +Z to the panel normal (for things mounted on the panel).
pub fn panel_rotation() -> Quat {
    Quat::from_rotation_x(-PANEL_TILT)
}

/// Point on the panel plane (moved `offset` metres along the normal) seen at pixel (px, py).
pub fn panel_point(px: f32, py: f32, offset: f32) -> Vec3 {
    let n = panel_normal();
    let p0 = pixel_dir(CX, PANEL_ROW) * PANEL_DEPTH + n * offset;
    let d = pixel_dir(px, py);
    d * (n.dot(p0) / n.dot(d))
}

/// Grid patch of the panel plane covering pixels [x0, x1) x [y0, y1) with custom UVs (e.g.
/// 0..1 over the patch for a screen).
pub fn panel_patch_uv(x0: f32, y0: f32, x1: f32, y1: f32, offset: f32, uv: impl Fn(f32, f32) -> Vec2) -> Mesh {
    let nx = ((x1 - x0) / 8.0).ceil().max(1.0) as u32;
    let ny = ((y1 - y0) / 8.0).ceil().max(1.0) as u32;
    let n = panel_normal();
    let mut pos = Vec::new();
    let mut uvs = Vec::new();
    for j in 0..=ny {
        for i in 0..=nx {
            let px = x0 + (x1 - x0) * i as f32 / nx as f32;
            let py = y0 + (y1 - y0) * j as f32 / ny as f32;
            pos.push(panel_point(px, py, offset).to_array());
            uvs.push(uv(px, py).to_array());
        }
    }
    let mut idx = Vec::new();
    for j in 0..ny {
        for i in 0..nx {
            let a = j * (nx + 1) + i;
            let (b, c, d) = (a + 1, a + nx + 1, a + nx + 2);
            idx.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }
    let count = pos.len();
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![n.to_array(); count])
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(idx))
}

/// Side walls of a recess behind the panel rectangle (depth metres behind the plane).
pub fn recess_walls(x0: f32, y0: f32, x1: f32, y1: f32, depth: f32) -> Mesh {
    let c = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)];
    let mut pos = Vec::new();
    let mut normals = Vec::new();
    let mut idx = Vec::new();
    let centre = panel_point((x0 + x1) / 2.0, (y0 + y1) / 2.0, -depth);
    for k in 0..4 {
        let (a, b) = (c[k], c[(k + 1) % 4]);
        let quad = [
            panel_point(a.0, a.1, 0.0),
            panel_point(b.0, b.1, 0.0),
            panel_point(b.0, b.1, -depth),
            panel_point(a.0, a.1, -depth),
        ];
        let mut nrm = (quad[1] - quad[0]).cross(quad[3] - quad[0]).normalize();
        // Walls face the inside of the recess.
        if nrm.dot(centre - quad[0]) < 0.0 {
            nrm = -nrm;
        }
        let base = pos.len() as u32;
        for p in quad {
            pos.push(p.to_array());
            normals.push(nrm.to_array());
        }
        idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_indices(Indices::U32(idx))
}

/// Box on the panel over the pixel rectangle (x, y, w, h) from `base` to `base + height` metres
/// along the normal, its corners where the pixels' corners are (so a box's edges follow the
/// projected pixel rows and columns and adjacent boxes meet).
pub fn panel_box(x: f32, y: f32, w: f32, h: f32, base: f32, height: f32) -> Mesh {
    let c = [(x, y), (x + w, y), (x + w, y + h), (x, y + h)];
    let lo = c.map(|(px, py)| panel_point(px, py, base));
    let hi = c.map(|(px, py)| panel_point(px, py, base + height));
    let mut faces: Vec<[Vec3; 4]> = vec![hi, lo];
    faces.extend((0..4).map(|k| [lo[k], lo[(k + 1) % 4], hi[(k + 1) % 4], hi[k]]));
    let centre = (lo.iter().chain(&hi).copied().sum::<Vec3>()) / 8.0;
    let (mut pos, mut normals, mut idx) = (Vec::new(), Vec::new(), Vec::new());
    for q in faces {
        let mut n = (q[1] - q[0]).cross(q[3] - q[0]).normalize();
        let mut q = q;
        // Outward faces, wound counter-clockwise seen from outside.
        if n.dot(q[0] - centre) < 0.0 {
            n = -n;
            q.reverse();
        }
        let b = pos.len() as u32;
        pos.extend(q.map(|p| p.to_array()));
        normals.extend([n.to_array(); 4]);
        idx.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_indices(Indices::U32(idx))
}

/// Transform of a unit cuboid stretched between two points with the given cross-section.
pub fn beam(a: Vec3, b: Vec3, thickness: Vec2) -> Transform {
    let d = b - a;
    let len = d.length().max(1e-4);
    let up = if d.normalize().dot(Vec3::Y).abs() > 0.95 { Vec3::Z } else { Vec3::Y };
    Transform::from_translation((a + b) / 2.0)
        .looking_to(d / len, up)
        .with_scale(Vec3::new(thickness.x, thickness.y, len))
}

/// Centre, size (metres, along panel x/y) of a panel pixel rectangle (x, y, w, h).
pub fn panel_rect(x: f32, y: f32, w: f32, h: f32, offset: f32) -> (Vec3, Vec2) {
    let (x0, y0, x1, y1) = (x, y, x + w, y + h);
    let c = panel_point((x0 + x1) / 2.0, (y0 + y1) / 2.0, offset);
    let width = panel_point(x0, (y0 + y1) / 2.0, offset).distance(panel_point(x1, (y0 + y1) / 2.0, offset));
    let height = panel_point((x0 + x1) / 2.0, y0, offset).distance(panel_point((x0 + x1) / 2.0, y1, offset));
    (c, Vec2::new(width, height))
}

/// A flat quad facing +Z with UVs covering `uv` (min, max) of a texture.
pub fn uv_quad(size: Vec2, uv_min: Vec2, uv_max: Vec2) -> Mesh {
    let (hx, hy) = (size.x / 2.0, size.y / 2.0);
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(
            Mesh::ATTRIBUTE_POSITION,
            vec![[-hx, -hy, 0.0], [hx, -hy, 0.0], [hx, hy, 0.0], [-hx, hy, 0.0]],
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; 4])
        .with_inserted_attribute(
            Mesh::ATTRIBUTE_UV_0,
            vec![[uv_min.x, uv_max.y], [uv_max.x, uv_max.y], [uv_max.x, uv_min.y], [uv_min.x, uv_min.y]],
        )
        .with_inserted_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]))
}

/// Texture coordinate of palette index `c` in the 16x16 palette swatch texture.
pub fn swatch_uv(c: u8) -> [f32; 2] {
    [((c % 16) as f32 + 0.5) / 16.0, ((c / 16) as f32 + 0.5) / 16.0]
}

/// Sets every texture coordinate of a mesh to one palette swatch colour.
pub fn with_swatch(mut mesh: Mesh, c: u8) -> Mesh {
    let n = mesh.count_vertices();
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![swatch_uv(c); n]);
    mesh
}

/// Flat convex polygon on the panel plane (panel pixels, `offset` metres along the normal).
pub fn panel_polygon(points: &[(f32, f32)], offset: f32) -> Mesh {
    let n = panel_normal();
    let pos: Vec<[f32; 3]> = points.iter().map(|&(x, y)| panel_point(x, y, offset).to_array()).collect();
    let count = pos.len() as u32;
    // Fan; the winding faces the pilot.
    let mut idx: Vec<u32> = (1..count - 1).flat_map(|k| [0, k, k + 1]).collect();
    let a = Vec3::from(pos[0]);
    let face = (Vec3::from(pos[1]) - a).cross(Vec3::from(pos[2]) - a);
    if face.dot(n) < 0.0 {
        for t in idx.as_chunks_mut::<3>().0 {
            t.swap(1, 2);
        }
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![n.to_array(); count as usize])
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 0.0]; count as usize])
        .with_inserted_indices(Indices::U32(idx))
}

/// Splits the rectangle (x0, y0)-(x1, y1) into rectangles that cover it except the holes.
pub fn rect_minus_holes(x0: f32, y0: f32, x1: f32, y1: f32, holes: &[[f32; 4]]) -> Vec<[f32; 4]> {
    let mut xs = vec![x0, x1];
    let mut ys = vec![y0, y1];
    for h in holes {
        xs.extend([h[0], h[2]].into_iter().filter(|v| *v > x0 && *v < x1));
        ys.extend([h[1], h[3]].into_iter().filter(|v| *v > y0 && *v < y1));
    }
    xs.sort_by(f32::total_cmp);
    xs.dedup();
    ys.sort_by(f32::total_cmp);
    ys.dedup();
    let mut out: Vec<[f32; 4]> = Vec::new();
    for yw in ys.windows(2) {
        let mut row: Vec<[f32; 4]> = Vec::new();
        for xw in xs.windows(2) {
            let (cx, cy) = ((xw[0] + xw[1]) / 2.0, (yw[0] + yw[1]) / 2.0);
            if holes.iter().any(|h| cx > h[0] && cx < h[2] && cy > h[1] && cy < h[3]) {
                continue;
            }
            // Merge with the cell to the left.
            match row.last_mut() {
                Some(r) if r[2] == xw[0] => r[2] = xw[1],
                _ => row.push([xw[0], yw[0], xw[1], yw[1]]),
            }
        }
        out.extend(row);
    }
    out
}
