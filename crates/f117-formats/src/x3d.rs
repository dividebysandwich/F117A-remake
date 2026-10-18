//! F-117A `.3DX` model files (theater XX.3DX, STFLT.3DX, PHOTO.3DX).
//! Format: docs/re/formats-3d.md section 6 (decoded from VGAME far segment 2C03).
//!
//! Axes: x = east, y = up, z = -north (model units; tile objects are in their LOD's units).

use anyhow::{Context, Result, bail, ensure};

/// State-table condition: the element is drawn only if `state[slot] == value`.
/// Slot meanings (observed): 10 = night lights on, 12 = aircraft navigation lights on,
/// 1 = damaged/destroyed variant (0 = intact).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Cond {
    pub slot: u8,
    pub value: i8,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PrimKind {
    /// Filled convex polygon, given as a closed loop of vertex indices.
    Polygon(Vec<u8>),
    /// Independent line segments (vertex index pairs).
    Lines(Vec<(u8, u8)>),
    /// Single-pixel lights: zero-length lines in the data (and the unused points type).
    Dots(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Prim {
    pub kind: PrimKind,
    /// Palette colour byte (before the day/night remap table).
    pub color: u8,
    /// All conditions that must hold (from the plane and the primitive).
    pub conds: Vec<Cond>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Plane {
    /// Q14 unit normal; visible when `n·eye + d >= 0`.
    pub normal: [i16; 3],
    pub d: i32,
    pub part: u8,
    pub cond: Option<Cond>,
    /// Vertices needed when the plane is visible.
    pub vlist: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub state_slot: u8,
    pub axis: u8,
    pub pivot: [i16; 3],
}

#[derive(Debug, Clone, PartialEq)]
pub struct Vertex {
    pub pos: [i16; 3],
    /// Articulated part the vertex belongs to (0 = main body).
    pub part: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Body {
    pub offset: usize,
    pub radius: u16,
    /// Extra articulated parts (part k+1 = parts[k]).
    pub parts: Vec<Part>,
    pub vertices: Vec<Vertex>,
    pub planes: Vec<Plane>,
    /// BSP draw order (root, children) when the body uses BSP ordering.
    pub bsp: Option<(u8, Vec<u8>)>,
    pub edges: Vec<(u8, u8)>,
    /// Primitive lists: one per plane (same index), or a single inline list if there are no planes.
    pub lists: Vec<Vec<Prim>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Lod {
    /// Use this body while the camera depth (high word) is <= max_dist.
    pub max_dist: u16,
    /// Index into `X3d::bodies`.
    pub body: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    pub attr: u8,
    pub lods: Vec<Lod>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct X3d {
    pub models: Vec<Model>,
    /// Bodies, shared between LOD tables of different models.
    pub bodies: Vec<Body>,
}

struct Rd<'a> {
    b: &'a [u8],
    p: usize,
}

impl Rd<'_> {
    fn u8(&mut self) -> Result<u8> {
        let v = *self.b.get(self.p).context("3DX: unexpected end of file")?;
        self.p += 1;
        Ok(v)
    }
    fn peek16(&self) -> Result<u16> {
        let s = self.b.get(self.p..self.p + 2).context("3DX: unexpected end of file")?;
        Ok(u16::from_le_bytes([s[0], s[1]]))
    }
    fn u16(&mut self) -> Result<u16> {
        let v = self.peek16()?;
        self.p += 2;
        Ok(v)
    }
    fn i16(&mut self) -> Result<i16> {
        Ok(self.u16()? as i16)
    }
    fn i32(&mut self) -> Result<i32> {
        let lo = self.u16()? as u32;
        let hi = self.u16()? as u32;
        Ok((lo | hi << 16) as i32)
    }
    fn bytes(&mut self, n: usize) -> Result<Vec<u8>> {
        let s = self.b.get(self.p..self.p + n).context("3DX: unexpected end of file")?;
        self.p += n;
        Ok(s.to_vec())
    }
}

pub fn parse(b: &[u8]) -> Result<X3d> {
    let mut r = Rd { b, p: 0 };
    let n = r.u16()? as usize;
    let offs: Vec<usize> = (0..n).map(|_| r.u16().map(|v| v as usize)).collect::<Result<_>>()?;
    let _pool = r.u16()?;
    let attrs = r.bytes(n)?;

    let mut bodies: Vec<Body> = Vec::new();
    let mut body_at: std::collections::HashMap<usize, usize> = Default::default();
    let mut models = Vec::with_capacity(n);
    for (i, &off) in offs.iter().enumerate() {
        let mut r = Rd { b, p: off };
        let mut lods = Vec::new();
        loop {
            let max_dist = r.u16()?;
            let body_off = r.u16()? as usize;
            let body = match body_at.get(&body_off) {
                Some(&idx) => idx,
                None => {
                    let body = parse_body(b, body_off).with_context(|| format!("model {i} body @{body_off:#x}"))?;
                    bodies.push(body);
                    body_at.insert(body_off, bodies.len() - 1);
                    bodies.len() - 1
                }
            };
            lods.push(Lod { max_dist, body });
            if max_dist >= 0x8000 {
                break;
            }
            ensure!(lods.len() <= 16, "model {i}: runaway LOD table");
        }
        models.push(Model { attr: attrs[i], lods });
    }
    Ok(X3d { models, bodies })
}

fn parse_body(b: &[u8], off: usize) -> Result<Body> {
    let mut r = Rd { b, p: off };
    let radius = r.u16()?;
    let nparts = r.i16()?;
    let extra = (nparts - 1).max(0) as usize;
    let slots: Vec<(u8, u8)> = (0..extra).map(|_| Ok((r.u8()?, r.u8()?))).collect::<Result<_>>()?;
    let mut parts = Vec::with_capacity(extra);
    for &(state_slot, axis) in &slots {
        let pivot = [r.i16()?, r.i16()?, r.i16()?];
        parts.push(Part { state_slot, axis, pivot });
    }
    let nverts = r.i16()?;
    let nplanes = r.i16()?;
    let np = nplanes.unsigned_abs() as usize;

    let mut planes = Vec::with_capacity(np);
    let (mut part, mut cond) = (0u8, None);
    while planes.len() < np {
        let w = r.peek16()?;
        if w >> 8 == 0x80 {
            part = (w & 0xFF) as u8;
            r.p += 2;
            continue;
        }
        if w == 0x8100 {
            r.p += 2;
            cond = Some(Cond { slot: r.u8()? & 0x1F, value: r.u8()? as i8 });
            continue;
        }
        let normal = [r.i16()?, r.i16()?, r.i16()?];
        let d = r.i32()?;
        planes.push(Plane { normal, d, part, cond: cond.take(), vlist: vec![] });
    }
    for pl in &mut planes {
        let n = r.u8()? as usize;
        pl.vlist = r.bytes(n)?;
    }
    let bsp = if nplanes < 0 {
        let root = r.u8()?;
        Some((root, r.bytes(2 * np)?))
    } else {
        None
    };

    let mut vertices = Vec::new();
    if nverts > 0 {
        let mut part = 0u8;
        while vertices.len() < nverts as usize {
            let w = r.peek16()?;
            if (0x8000..0x8010).contains(&w) {
                part = (w & 0xF) as u8;
                r.p += 2;
                continue;
            }
            vertices.push(Vertex { pos: [r.i16()?, r.i16()?, r.i16()?], part });
        }
    } else if nverts < 0 {
        bail!("shared vertex pool references are not supported (unused in shipped files)");
    }

    let nedges = r.u16()? as usize;
    let edges: Vec<(u8, u8)> = (0..nedges).map(|_| Ok((r.u8()?, r.u8()?))).collect::<Result<_>>()?;
    for &(a, c) in &edges {
        ensure!((a as usize) < vertices.len() && (c as usize) < vertices.len(), "edge vertex out of range");
    }

    let mut body = Body { offset: off, radius, parts, vertices, planes, bsp, edges, lists: vec![] };
    if np == 0 {
        body.lists.push(parse_prims(b, r.p, &body, None)?);
    } else {
        let list_offs: Vec<usize> = (0..np).map(|_| r.u16().map(|v| v as usize)).collect::<Result<_>>()?;
        for (i, &lo) in list_offs.iter().enumerate() {
            let plane_cond = body.planes[i].cond;
            let list = parse_prims(b, lo, &body, plane_cond)?;
            body.lists.push(list);
        }
    }
    Ok(body)
}

fn parse_prims(b: &[u8], off: usize, body: &Body, plane_cond: Option<Cond>) -> Result<Vec<Prim>> {
    let mut r = Rd { b, p: off };
    let mut out = Vec::new();
    let mut cond: Option<Cond> = None;
    let edge = |e: u8| -> Result<(u8, u8)> {
        body.edges.get(e as usize).copied().context("primitive edge index out of range")
    };
    let same = |a: u8, c: u8| a == c || body.vertices[a as usize].pos == body.vertices[c as usize].pos;
    loop {
        let h = r.u8()?;
        if h == 0xFF {
            break;
        }
        if h & 0x80 != 0 {
            cond = Some(Cond { slot: h & 0x1F, value: r.u8()? as i8 });
            continue;
        }
        let conds: Vec<Cond> = plane_cond.into_iter().chain(cond.take()).collect();
        let n = (h & 0x1F) as usize;
        if h & 0x20 != 0 {
            let color = r.u8()?;
            let verts = r.bytes(n)?;
            out.push(Prim { kind: PrimKind::Dots(verts), color, conds });
        } else if h & 0x40 != 0 {
            let color = r.u8()?;
            let (mut lines, mut dots) = (Vec::new(), Vec::new());
            for e in r.bytes(n)? {
                let (a, c) = edge(e)?;
                if same(a, c) { dots.push(a) } else { lines.push((a, c)) }
            }
            if !lines.is_empty() {
                out.push(Prim { kind: PrimKind::Lines(lines), color, conds: conds.clone() });
            }
            if !dots.is_empty() {
                out.push(Prim { kind: PrimKind::Dots(dots), color, conds });
            }
        } else {
            let edges: Vec<(u8, u8)> = r.bytes(n)?.into_iter().map(edge).collect::<Result<_>>()?;
            let color = r.u8()?;
            if let Some(loop_) = edge_loop(&edges) {
                out.push(Prim { kind: PrimKind::Polygon(loop_), color, conds });
            }
        }
    }
    Ok(out)
}

/// Orders a polygon's edges into a closed vertex loop. Degenerate polygons (< 3 vertices,
/// e.g. polygons collapsed to a line) yield `None`.
fn edge_loop(edges: &[(u8, u8)]) -> Option<Vec<u8>> {
    let (first, rest) = edges.split_first()?;
    let mut remaining: Vec<(u8, u8)> = rest.to_vec();
    let mut verts = vec![first.0, first.1];
    while !remaining.is_empty() {
        let last = *verts.last().unwrap();
        let i = remaining.iter().position(|&(a, c)| a == last || c == last)?;
        let (a, c) = remaining.swap_remove(i);
        let next = if a == last { c } else { a };
        if next == verts[0] {
            continue; // closing edge
        }
        verts.push(next);
    }
    (verts.len() >= 3).then_some(verts)
}

#[cfg(test)]
mod tests {
    #[test]
    fn edge_loops() {
        assert_eq!(super::edge_loop(&[(0, 1), (2, 3), (1, 2), (3, 0)]), Some(vec![0, 1, 2, 3]));
        assert_eq!(super::edge_loop(&[(1, 0), (1, 2), (2, 0)]), Some(vec![1, 0, 2]));
        assert_eq!(super::edge_loop(&[(0, 1), (1, 0)]), None);
    }
}
