//! Loading converted models (.glb, exported from the Blender master copies) and spawning them
//! with the shared palette materials.
//!
//! Each .glb holds one model per root node (`<model>_lod<N>` naming for detail levels).
//! Triangles take their colour from the material name `pal_NNN`; line and dot primitives
//! from their node name (`..._lines_pal_NNN`, `..._dots_pal_NNN`).

use std::collections::HashMap;

use bevy::gltf::{Gltf, GltfMaterial, GltfMesh, GltfNode};
use bevy::prelude::*;

use crate::palette::{Palette, index_from_name};
use crate::palette_material::mesh_tag;
use bevy::mesh::MeshTag;

pub struct ModelsPlugin;

impl Plugin for ModelsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ModelLibrary>().init_resource::<ModelGlobals>().add_systems(
            PostUpdate,
            (update_camera_distance, apply_model_state)
                .chain()
                .before(bevy::transform::TransformSystems::Propagate),
        );
    }
}

/// Per-instance state table read by conditional geometry and articulated parts
/// (the word array VGAME passes to its model renderer). Known slots: 1 = destroyed variant,
/// 10 = night lights on, 12 = aircraft navigation lights on, 16 = radar antenna angle,
/// 17 = rotor angle. Angles use the original 16-bit binary angle (0x10000 = 360 degrees).
#[derive(Component, Debug, Clone)]
pub struct ModelState(pub [i16; 32]);

impl Default for ModelState {
    fn default() -> Self {
        Self([0; 32])
    }
}

/// Global state slot values added to every model's own slots: the continuously turning angles
/// of slots 15/16 (radar antennas) and 17 (rotors), driven in flight (visuals.md section 5).
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct ModelGlobals {
    pub angles: [i16; 3],
}

impl ModelGlobals {
    fn angle(&self, slot: usize) -> i16 {
        if (15..=17).contains(&slot) { self.angles[slot - 15] } else { 0 }
    }
}

/// Original model attributes (.3DX attr byte: low 3 bits = size class, 0x10 = target).
#[derive(Component, Clone, Copy)]
#[allow(dead_code)] // for tools and future size-class haze
pub struct ModelInfo {
    pub attr: u8,
}

/// Distance from the main camera to the model origin, in model units (set every frame), and
/// the zoom-scaled distance from the tracking camera while it is live.
#[derive(Component, Default)]
pub struct CameraDistance {
    pub main: f32,
    pub track: Option<f32>,
}

/// One detail level of a model; shown while the camera distance is within `max_dist` and
/// beyond the previous level's.
#[derive(Component)]
pub struct LodLevel {
    pub root: Entity,
    pub level: u32,
    pub max_dist: f32,
}

/// Which views currently pick this detail level (set every frame). The level's `Visibility`
/// is on while either does; its meshes get render layers so each view draws only its own pick.
#[derive(Component, Default, Clone, Copy)]
pub struct LodShown {
    pub main: bool,
    pub track: bool,
}

/// A mesh inside a detail level (the `LodLevel` entity), whose render layers follow `LodShown`.
#[derive(Component)]
pub struct LodMesh(pub Entity);

/// Convex hull of a detail level from the original's plane list (Q14 normal, 32-bit d, model
/// units): a point is inside when `n·p + d < 0` for every plane.
#[derive(Component)]
pub struct CollisionHull {
    pub planes: Vec<[f32; 4]>,
    pub radius: f32,
}

impl CollisionHull {
    pub fn contains(&self, p: Vec3) -> bool {
        if p.length() > self.radius + 1.0 {
            return false;
        }
        self.planes.iter().all(|n| n[0] * p.x + n[1] * p.y + n[2] * p.z + n[3] < 0.0)
    }
}

/// Geometry drawn only when all `(slot, value)` conditions hold in the root's `ModelState`.
#[derive(Component)]
pub struct CondNode {
    pub root: Entity,
    pub conds: Vec<(usize, i16)>,
}

/// Sub-object rotated by a state slot angle around its pivot (the node origin).
#[derive(Component)]
pub struct ArticulatedPart {
    pub root: Entity,
    pub slot: usize,
    /// Original axis code: 0 = yaw (around up), 1 = pitch, 2 = roll.
    pub axis: u8,
    pub rest: Quat,
}

/// Parses a trailing number after `tag` (e.g. "x_lod2" with "_lod" -> 2).
fn suffix_number(name: &str, tag: &str) -> Option<u32> {
    let i = name.rfind(tag)?;
    let rest = &name[i + tag.len()..];
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() || rest.len() != digits.len() && !rest[digits.len()..].starts_with('.') {
        return None;
    }
    digits.parse().ok()
}

/// Parses `_s<slot>v<value>` condition suffixes from a node name.
fn parse_conds(name: &str) -> Vec<(usize, i16)> {
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

/// Distances used for the LOD choice. While the tracking camera is live, its own distance is
/// divided by the zoom magnification (focal 256 << z vs. 256 for the main view), so zoomed-in
/// targets get the detail of their on-screen size instead of disappearing past the last level.
fn update_camera_distance(
    cameras: Query<(&Camera, &GlobalTransform), Without<crate::trackcam::TrackCamera>>,
    track: Query<(&Camera, &GlobalTransform), With<crate::trackcam::TrackCamera>>,
    track_info: Option<Res<crate::mfd::TrackCam>>,
    mut models: Query<(&GlobalTransform, &mut CameraDistance)>,
) {
    let Some((_, cam)) = cameras.iter().find(|(c, _)| c.is_active) else { return };
    let cam = cam.translation();
    let zoomed = track.iter().find(|(c, _)| c.is_active).zip(track_info).map(|((_, t), info)| {
        let z = info.zoom.saturating_sub(1).min(6);
        (t.translation(), (1u32 << z) as f32)
    });
    for (tf, mut d) in &mut models {
        let scale = tf.scale().x.max(1e-6);
        let p = tf.translation();
        d.main = p.distance(cam) / scale;
        d.track = zoomed.map(|(t, mag)| p.distance(t) / mag / scale);
    }
}

fn apply_model_state(
    roots: Query<(&ModelState, &CameraDistance)>,
    mut lods: Query<(&LodLevel, &mut LodShown, &mut Visibility), (Without<CondNode>, Without<ArticulatedPart>)>,
    mut conds: Query<(&CondNode, &mut Visibility), Without<LodLevel>>,
    mut parts: Query<(&ArticulatedPart, &mut Transform)>,
    all_lods: Query<&LodLevel>,
    mut lod_meshes: Query<(&LodMesh, &mut bevy::camera::visibility::RenderLayers)>,
    globals: Res<ModelGlobals>,
) {
    for (lod, mut shown, mut vis) in &mut lods {
        let Ok((_, dist)) = roots.get(lod.root) else { continue };
        // The chosen level is the first whose max_dist covers the distance.
        let prev = all_lods
            .iter()
            .filter(|l| l.root == lod.root && l.level < lod.level)
            .map(|l| l.max_dist)
            .fold(0.0, f32::max);
        let picks = |d: f32| d <= lod.max_dist && (lod.level == 0 || d > prev);
        let main = picks(dist.main);
        let track = dist.track.map_or(main, picks);
        *shown = LodShown { main, track };
        vis.set_if_neq(if main || track { Visibility::Inherited } else { Visibility::Hidden });
    }
    for (m, mut layers) in &mut lod_meshes {
        let Ok((_, shown, _)) = lods.get(m.0) else { continue };
        use crate::camera::{MAIN_LOD_LAYER, TRACKCAM_LOD_LAYER};
        let layer = match (shown.main, shown.track) {
            (true, false) => MAIN_LOD_LAYER,
            (false, true) => TRACKCAM_LOD_LAYER,
            _ => 0,
        };
        if !layers.intersects(&bevy::camera::visibility::RenderLayers::layer(layer)) {
            *layers = bevy::camera::visibility::RenderLayers::layer(layer);
        }
    }
    for (c, mut vis) in &mut conds {
        let Ok((state, _)) = roots.get(c.root) else { continue };
        let show = c.conds.iter().all(|&(s, v)| state.0.get(s) == Some(&v));
        vis.set_if_neq(if show { Visibility::Inherited } else { Visibility::Hidden });
    }
    for (p, mut tf) in &mut parts {
        let Ok((state, _)) = roots.get(p.root) else { continue };
        let value = state.0[p.slot].wrapping_add(globals.angle(p.slot));
        let angle = -(value as u16 as f32) / 65536.0 * std::f32::consts::TAU;
        let r = match p.axis {
            0 => Quat::from_rotation_y(angle),
            1 => Quat::from_rotation_x(angle),
            _ => Quat::from_rotation_z(angle),
        };
        tf.rotation = p.rest * r;
    }
}

/// Loaded model files, keyed by file stem (e.g. "stflt", "ce").
#[derive(Resource, Default)]
pub struct ModelLibrary {
    pub files: HashMap<String, Handle<Gltf>>,
}

impl ModelLibrary {
    pub fn load(&mut self, server: &AssetServer, stem: &str) -> Handle<Gltf> {
        self.files
            .entry(stem.to_string())
            .or_insert_with(|| server.load(format!("models/{stem}.glb")))
            .clone()
    }
}

/// Everything needed to instantiate glTF nodes.
#[derive(bevy::ecs::system::SystemParam)]
pub struct ModelAssets<'w> {
    pub gltfs: Res<'w, Assets<Gltf>>,
    pub nodes: Res<'w, Assets<GltfNode>>,
    pub meshes: Res<'w, Assets<GltfMesh>>,
    pub mesh_data: Res<'w, Assets<Mesh>>,
    pub palette: Res<'w, Palette>,
    pub dots: Res<'w, crate::dots::DotAssets>,
}

impl ModelAssets<'_> {
    /// Names of the root nodes (models) in a file, in file order.
    pub fn model_names(&self, file: &Handle<Gltf>) -> Vec<String> {
        let Some(g) = self.gltfs.get(file) else { return vec![] };
        let child: std::collections::HashSet<AssetId<GltfNode>> = g
            .nodes
            .iter()
            .filter_map(|n| self.nodes.get(n))
            .flat_map(|n| n.children.iter().map(|c| c.id()))
            .collect();
        g.nodes
            .iter()
            .filter(|n| !child.contains(&n.id()))
            .filter_map(|n| self.nodes.get(n).map(|n| n.name.clone()))
            .collect()
    }

    /// Spawns a model (a root node with its LODs, parts and conditional geometry) from a loaded
    /// file. `level` is the unit level for distance haze (`palette_material::mesh_tag`).
    /// Returns `None` if the file is not loaded yet or the model does not exist.
    pub fn spawn(&self, commands: &mut Commands, file: &Handle<Gltf>, name: &str, level: u32, no_fog: bool) -> Option<Entity> {
        let g = self.gltfs.get(file)?;
        let node = g.named_nodes.get(name)?;
        let material_names: HashMap<AssetId<GltfMaterial>, &str> =
            g.named_materials.iter().map(|(n, h)| (h.id(), n.as_ref())).collect();
        let attr = self
            .nodes
            .get(node)
            .and_then(|n| n.extras.as_ref())
            .and_then(|x| serde_json::from_str::<serde_json::Value>(&x.value).ok())
            .and_then(|v| v["attr"].as_u64())
            .unwrap_or(0) as u32;
        let tag = MeshTag(mesh_tag(level, attr & 7, no_fog));
        let root = commands.spawn((ModelState::default(), CameraDistance::default(), ModelInfo { attr: attr as u8 })).id();
        self.spawn_node(commands, node, &material_names, root, Some(root), None, tag);
        Some(root)
    }

    fn spawn_node(
        &self,
        commands: &mut Commands,
        handle: &Handle<GltfNode>,
        material_names: &HashMap<AssetId<GltfMaterial>, &str>,
        root: Entity,
        reuse: Option<Entity>,
        mut lod: Option<Entity>,
        tag: MeshTag,
    ) -> Entity {
        let node = self.nodes.get(handle).expect("glTF node loaded with its file");
        let entity = match reuse {
            Some(e) => e,
            None => commands.spawn_empty().id(),
        };
        let mut e = commands.entity(entity);
        e.insert((Name::new(node.name.clone()), node.transform, Visibility::default()));
        let extras: serde_json::Value =
            node.extras.as_ref().and_then(|x| serde_json::from_str(&x.value).ok()).unwrap_or_default();
        let name = node.name.as_str();
        if let Some(k) = suffix_number(name, "_lod") {
            let max_dist = extras["max_dist"].as_u64().unwrap_or(0x8001) as f32;
            e.insert((LodLevel { root, level: k, max_dist }, LodShown::default()));
            lod = Some(entity);
            let planes: Vec<[f32; 4]> = extras["planes"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|p| {
                            let v: Vec<f32> = p.as_array()?.iter().filter_map(|x| x.as_f64().map(|f| f as f32)).collect();
                            (v.len() == 4).then(|| [v[0], v[1], v[2], v[3]])
                        })
                        .collect()
                })
                .unwrap_or_default();
            if !planes.is_empty() {
                let radius = extras["radius"].as_f64().unwrap_or(0.0) as f32;
                e.insert(CollisionHull { planes, radius });
            }
        }
        if suffix_number(name, "_part").is_some() {
            e.insert(ArticulatedPart {
                root,
                slot: extras["state_slot"].as_u64().unwrap_or(0) as usize,
                axis: extras["axis"].as_u64().unwrap_or(0) as u8,
                rest: node.transform.rotation,
            });
        }
        let conds = parse_conds(name);
        if !conds.is_empty() && !name.contains("_pal_") {
            e.insert(CondNode { root, conds });
        }

        if let Some(mesh) = node.mesh.as_ref().and_then(|m| self.meshes.get(m)) {
            let node_color = index_from_name(name);
            if name.contains("_dots_pal_") {
                // Original single-pixel lights -> fullbright billboards.
                let color = node_color.unwrap_or(15);
                for prim in &mesh.primitives {
                    for p in self.positions(&prim.mesh) {
                        let child = commands
                            .spawn((crate::dots::DotLight::bundle(p, &self.dots, tag.0), MeshMaterial3d(self.palette.line_material(color))))
                            .id();
                        if let Some(l) = lod {
                            commands.entity(child).insert((LodMesh(l), bevy::camera::visibility::RenderLayers::layer(0)));
                        }
                        commands.entity(entity).add_child(child);
                    }
                }
            } else {
                for prim in &mesh.primitives {
                    let color = node_color
                        .or_else(|| {
                            prim.material.as_ref().and_then(|m| material_names.get(&m.id())).and_then(|n| index_from_name(n))
                        })
                        .unwrap_or(15);
                    let material = if name.contains("_lines_pal_") {
                        self.palette.line_material(color)
                    } else {
                        self.palette.material(color)
                    };
                    let child = commands.spawn((Mesh3d(prim.mesh.clone()), MeshMaterial3d(material), tag.clone())).id();
                    if let Some(l) = lod {
                        commands.entity(child).insert((LodMesh(l), bevy::camera::visibility::RenderLayers::layer(0)));
                    }
                    commands.entity(entity).add_child(child);
                }
            }
        }
        for c in &node.children {
            let child = self.spawn_node(commands, c, material_names, root, None, lod, tag.clone());
            commands.entity(entity).add_child(child);
        }
        entity
    }

    fn positions(&self, mesh: &Handle<Mesh>) -> Vec<Vec3> {
        let Some(m) = self.mesh_data.get(mesh) else { return vec![] };
        match m.attribute(Mesh::ATTRIBUTE_POSITION) {
            Some(bevy::mesh::VertexAttributeValues::Float32x3(v)) => v.iter().map(|&p| Vec3::from(p)).collect(),
            _ => vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CollisionHull;

    /// Plane hulls from the converted CE models: inside a mountain collides, above it doesn't,
    /// and flat airbase surfaces never contain points above the ground.
    #[test]
    fn hulls_from_original_planes() {
        let Ok(data) = std::fs::read(crate::asset_path::asset("models/ce.glb")) else { return };
        let n = u32::from_le_bytes(data[12..16].try_into().unwrap()) as usize;
        let j: serde_json::Value = serde_json::from_slice(&data[20..20 + n]).unwrap();
        let hull = |name: &str| {
            let node = j["nodes"].as_array().unwrap().iter().find(|x| x["name"] == name).unwrap();
            let planes = node["extras"]["planes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| {
                    let v: Vec<f32> = p.as_array().unwrap().iter().map(|x| x.as_f64().unwrap() as f32).collect();
                    [v[0], v[1], v[2], v[3]]
                })
                .collect();
            CollisionHull { planes, radius: node["extras"]["radius"].as_f64().unwrap() as f32 }
        };
        let mountain = hull("ce_003_lod0");
        assert!(mountain.contains(bevy::math::Vec3::new(0.0, 10.0, 0.0)));
        assert!(!mountain.contains(bevy::math::Vec3::new(0.0, 400.0, 0.0)));
        let airbase = hull("ce_021_lod0");
        assert!(!airbase.contains(bevy::math::Vec3::new(0.0, 2.0, 0.0)));
    }
}
