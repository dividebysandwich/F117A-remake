//! Minimal binary glTF (GLB) writer for flat-shaded, palette-coloured geometry.
//!
//! Conventions (see docs/re/README.md "Asset pipeline"):
//! - every face has its own vertices (flat shading, one colour per face);
//! - materials are named `pal_NNN` after the original palette index, unlit (KHR_materials_unlit),
//!   base colour from the default palette so the file also looks right in Blender;
//! - polygons are triangulated as fans; line primitives use mode LINES, dots use mode POINTS;
//! - one glTF node per object (e.g. `<model>_lod<N>`), empties as mesh-less nodes;
//! - Y-up, metres.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;
use f117_formats::pal::Palette;
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Prim {
    Triangles,
    Lines,
    Points,
}

/// A polygon / line / dot with a palette colour index. Vertices are positions in metres (Y-up).
#[derive(Debug, Clone)]
pub struct Face {
    pub prim: Prim,
    pub color: u8,
    pub verts: Vec<[f32; 3]>,
}

#[derive(Debug, Clone, Default)]
pub struct Node {
    pub name: String,
    pub translation: [f32; 3],
    pub faces: Vec<Face>,
    pub children: Vec<Node>,
    /// Free-form properties stored in glTF `extras` (kept by Blender as custom properties).
    pub extras: Option<Value>,
}

pub struct Glb<'a> {
    palette: &'a Palette,
    bin: Vec<u8>,
    accessors: Vec<Value>,
    buffer_views: Vec<Value>,
    meshes: Vec<Value>,
    nodes: Vec<Value>,
    materials: Vec<Value>,
    material_ids: BTreeMap<u8, usize>,
}

impl<'a> Glb<'a> {
    pub fn new(palette: &'a Palette) -> Self {
        Self {
            palette,
            bin: Vec::new(),
            accessors: Vec::new(),
            buffer_views: Vec::new(),
            meshes: Vec::new(),
            nodes: Vec::new(),
            materials: Vec::new(),
            material_ids: BTreeMap::new(),
        }
    }

    fn material(&mut self, color: u8) -> usize {
        if let Some(&id) = self.material_ids.get(&color) {
            return id;
        }
        let [r, g, b] = self.palette.rgb8(color);
        // glTF colour factors are linear.
        let lin = |c: u8| {
            let c = c as f64 / 255.0;
            if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
        };
        let id = self.materials.len();
        self.materials.push(json!({
            "name": format!("pal_{color:03}"),
            "pbrMetallicRoughness": {
                "baseColorFactor": [lin(r), lin(g), lin(b), 1.0],
                "metallicFactor": 0.0, "roughnessFactor": 1.0
            },
            "doubleSided": true,
            "extensions": { "KHR_materials_unlit": {} }
        }));
        self.material_ids.insert(color, id);
        id
    }

    fn push_positions(&mut self, pos: &[[f32; 3]]) -> usize {
        while self.bin.len() % 4 != 0 {
            self.bin.push(0);
        }
        let offset = self.bin.len();
        let mut min = [f32::MAX; 3];
        let mut max = [f32::MIN; 3];
        for p in pos {
            for i in 0..3 {
                self.bin.extend_from_slice(&p[i].to_le_bytes());
                min[i] = min[i].min(p[i]);
                max[i] = max[i].max(p[i]);
            }
        }
        let view = self.buffer_views.len();
        self.buffer_views.push(json!({"buffer": 0, "byteOffset": offset, "byteLength": pos.len() * 12}));
        let acc = self.accessors.len();
        self.accessors.push(json!({
            "bufferView": view, "componentType": 5126, "count": pos.len(), "type": "VEC3",
            "min": min, "max": max
        }));
        acc
    }

    /// Adds a node tree; returns the root node index.
    ///
    /// Lines and dots go into child nodes named `<name>_lines_pal_NNN` / `<name>_dots_pal_NNN`,
    /// because Blender drops materials on loose edges/vertices; the game reads their colour
    /// from the node name.
    pub fn add_node(&mut self, node: &Node) -> usize {
        let (tris, other): (Vec<Face>, Vec<Face>) =
            node.faces.iter().cloned().partition(|f| f.prim == Prim::Triangles);
        let mesh = if tris.is_empty() { None } else { Some(self.add_mesh(&node.name, &tris)) };
        let mut loose: BTreeMap<(Prim, u8), Vec<Face>> = BTreeMap::new();
        for f in other {
            loose.entry((f.prim, f.color)).or_default().push(f);
        }
        let mut children: Vec<usize> = Vec::new();
        for ((prim, color), faces) in loose {
            let kind = if prim == Prim::Lines { "lines" } else { "dots" };
            let name = format!("{}_{kind}_pal_{color:03}", node.name);
            let m = self.add_mesh(&name, &faces);
            self.nodes.push(json!({"name": name, "mesh": m}));
            children.push(self.nodes.len() - 1);
        }
        children.extend(node.children.iter().map(|c| self.add_node(c)));
        let mut n = json!({"name": node.name});
        if node.translation != [0.0; 3] {
            n["translation"] = json!(node.translation);
        }
        if let Some(m) = mesh {
            n["mesh"] = json!(m);
        }
        if !children.is_empty() {
            n["children"] = json!(children);
        }
        if let Some(e) = &node.extras {
            n["extras"] = e.clone();
        }
        self.nodes.push(n);
        self.nodes.len() - 1
    }

    fn add_mesh(&mut self, name: &str, faces: &[Face]) -> usize {
        // One primitive per (kind, colour).
        let mut groups: BTreeMap<(Prim, u8), Vec<[f32; 3]>> = BTreeMap::new();
        for f in faces {
            let out = groups.entry((f.prim, f.color)).or_default();
            match f.prim {
                Prim::Triangles => {
                    for i in 1..f.verts.len().saturating_sub(1) {
                        out.extend([f.verts[0], f.verts[i], f.verts[i + 1]]);
                    }
                }
                Prim::Lines => {
                    for w in f.verts.windows(2) {
                        out.extend([w[0], w[1]]);
                    }
                }
                Prim::Points => out.extend(f.verts.iter().copied()),
            }
        }
        let mut prims = Vec::new();
        for ((prim, color), pos) in groups {
            if pos.is_empty() {
                continue;
            }
            let acc = self.push_positions(&pos);
            let mat = self.material(color);
            let mode = match prim {
                Prim::Triangles => 4,
                Prim::Lines => 1,
                Prim::Points => 0,
            };
            prims.push(json!({"attributes": {"POSITION": acc}, "material": mat, "mode": mode}));
        }
        self.meshes.push(json!({"name": name, "primitives": prims}));
        self.meshes.len() - 1
    }

    pub fn write(mut self, roots: &[usize], path: &Path) -> Result<()> {
        while self.bin.len() % 4 != 0 {
            self.bin.push(0);
        }
        let mut doc = json!({
            "asset": {"version": "2.0", "generator": "f117-convert"},
            "extensionsUsed": ["KHR_materials_unlit"],
            "scene": 0,
            "scenes": [{"nodes": roots}],
            "nodes": self.nodes,
            "meshes": self.meshes,
            "materials": self.materials,
            "accessors": self.accessors,
            "bufferViews": self.buffer_views,
        });
        if !self.bin.is_empty() {
            doc["buffers"] = json!([{"byteLength": self.bin.len()}]);
        }
        let mut js = serde_json::to_vec(&doc)?;
        while js.len() % 4 != 0 {
            js.push(b' ');
        }
        let total = 12 + 8 + js.len() + if self.bin.is_empty() { 0 } else { 8 + self.bin.len() };
        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(b"glTF");
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&(total as u32).to_le_bytes());
        out.extend_from_slice(&(js.len() as u32).to_le_bytes());
        out.extend_from_slice(b"JSON");
        out.extend_from_slice(&js);
        if !self.bin.is_empty() {
            out.extend_from_slice(&(self.bin.len() as u32).to_le_bytes());
            out.extend_from_slice(b"BIN\0");
            out.extend_from_slice(&self.bin);
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, out)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_test_glb() {
        let pal = Palette { colors: (0..256).map(|i| [(i % 64) as u8, (i / 4 % 64) as u8, 20]).collect() };
        let mut g = Glb::new(&pal);
        let node = Node {
            name: "test_lod0".into(),
            faces: vec![
                Face { prim: Prim::Triangles, color: 17, verts: vec![[0., 0., 0.], [1., 0., 0.], [1., 1., 0.], [0., 1., 0.]] },
                Face { prim: Prim::Lines, color: 40, verts: vec![[0., 0., 0.], [0., 0., 2.]] },
                Face { prim: Prim::Points, color: 200, verts: vec![[0., 3., 0.]] },
            ],
            children: vec![Node { name: "eye".into(), translation: [0., 1., 0.], ..Default::default() }],
            ..Default::default()
        };
        let root = g.add_node(&node);
        let out = std::env::temp_dir().join("f117_glb_test.glb");
        g.write(&[root], &out).unwrap();
        assert!(std::fs::metadata(&out).unwrap().len() > 100);
    }
}
