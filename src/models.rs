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
use crate::palette_material::{LEVEL_AIRCRAFT, PaletteMaterial, TAG_WARM, mesh_tag};
use bevy::mesh::MeshTag;

/// Largest tile object counted as warm in the FLIR picture (metres across / high): buildings
/// and structures, not terrain.
const WARM_MAX_SIZE: f32 = 500.0;
const WARM_MAX_HEIGHT: f32 = 250.0;

pub struct ModelsPlugin;

impl Plugin for ModelsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ModelLibrary>()
            .init_resource::<ModelGlobals>()
            .add_systems(
                PostUpdate,
                (update_camera_distance, apply_model_state)
                    .chain()
                    .before(bevy::transform::TransformSystems::Propagate),
            )
            .add_systems(Update, flicker_exhaust);
    }
}

/// Per-instance state table read by conditional geometry and articulated parts
/// (the word array passed to the model renderer). Known slots: 1 = destroyed variant,
/// 10 = night lights on, 12 = aircraft navigation lights on, 16 = radar antenna angle,
/// 17 = rotor angle, 18/19/20 = aircraft anti-collision lights lit (`sim_render::anticollision`).
/// Angles use a 16-bit binary angle (0x10000 = 360 degrees).
#[derive(Component, Debug, Clone)]
pub struct ModelState(pub [i16; 32]);

impl Default for ModelState {
    fn default() -> Self {
        Self([0; 32])
    }
}

/// Global state slot values added to every model's own slots: the continuously turning angles
/// of slots 15/16 (radar antennas) and 17 (rotors), driven in flight.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct ModelGlobals {
    pub angles: [i16; 3],
}

impl ModelGlobals {
    fn angle(&self, slot: usize) -> i16 {
        if (15..=17).contains(&slot) { self.angles[slot - 15] } else { 0 }
    }
}

/// Model attributes (.3DX attr byte: low 3 bits = size class, 0x10 = target).
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

/// Convex hull of a detail level from the model's plane list (Q14 normal, 32-bit d, model
/// units): a point is inside when `n·p + d < 0` for every plane.
#[derive(Component)]
pub struct CollisionHull {
    pub planes: Vec<[f32; 4]>,
    pub radius: f32,
    /// Refined mountain surface (terrain.rs), used instead of the planes: inside when below it.
    pub surface: Option<std::sync::Arc<Vec<[Vec3; 3]>>>,
}

impl CollisionHull {
    pub fn contains(&self, p: Vec3) -> bool {
        if p.length() > self.radius + 1.0 {
            return false;
        }
        if let Some(surface) = &self.surface {
            return p.y >= 0.0 && surface.iter().any(|t| below(t, p));
        }
        self.planes.iter().all(|n| n[0] * p.x + n[1] * p.y + n[2] * p.z + n[3] < 0.0)
    }
}

/// Whether `p` lies under triangle `t` (seen from above).
fn below(t: &[Vec3; 3], p: Vec3) -> bool {
    let [a, b, c] = t.map(|v| v.xz());
    let q = p.xz();
    let area = (b - a).perp_dot(c - a);
    if area.abs() < 1e-6 {
        return false;
    }
    let (u, v) = ((q - a).perp_dot(c - a) / area, (b - a).perp_dot(q - a) / area);
    let w = 1.0 - u - v;
    u >= 0.0 && v >= 0.0 && w >= 0.0 && p.y < t[0].y * w + t[1].y * u + t[2].y * v
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
    /// Axis code: 0 = yaw (around up), 1 = pitch, 2 = roll.
    pub axis: u8,
    pub rest: Quat,
}

/// Jet exhaust (nozzle exits `..._exhaust`, airborne flames `..._s13v1`, missile motors `..._s14v1`): alternates
/// between red and yellow like flickering afterburners.
#[derive(Component)]
pub struct ExhaustGlow;

/// Exhaust colours (light red, yellow) and flicker rate in colour changes per second.
const EXHAUST_COLOURS: [u8; 2] = [13, 17];
const EXHAUST_RATE: f32 = 12.0;

fn flicker_exhaust(
    time: Res<Time>,
    palette: Option<Res<Palette>>,
    mut glows: Query<&mut MeshMaterial3d<PaletteMaterial>, With<ExhaustGlow>>,
) {
    let Some(palette) = palette else { return };
    let c = EXHAUST_COLOURS[(time.elapsed_secs() * EXHAUST_RATE) as usize % 2];
    let m = palette.material(c);
    for mut mat in &mut glows {
        if mat.0 != m {
            mat.0 = m.clone();
        }
    }
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
/// The recon cameras and the campaign's unit view share the tracking camera's levels: the
/// nearest of them counts.
fn update_camera_distance(
    cameras: Query<(&Camera, &GlobalTransform), Without<crate::camera::OffscreenCamera>>,
    track: Query<(&Camera, &GlobalTransform), With<crate::trackcam::TrackCamera>>,
    recon: Query<
        (&Camera, &GlobalTransform),
        Or<(With<crate::recon::ReconCamera>, With<crate::campaign::UnitCamera>)>,
    >,
    track_info: Option<Res<crate::mfd::TrackCam>>,
    mut models: Query<(&GlobalTransform, &mut CameraDistance)>,
) {
    let Some((_, cam)) = cameras.iter().find(|(c, _)| c.is_active) else { return };
    let cam = cam.translation();
    let zoomed = track.iter().find(|(c, _)| c.is_active).zip(track_info).map(|((_, t), info)| {
        let z = info.zoom.saturating_sub(1).min(6);
        (t.translation(), (1u32 << z) as f32)
    });
    let views: Vec<(Vec3, f32)> = zoomed
        .into_iter()
        .chain(recon.iter().filter(|(c, _)| c.is_active).map(|(_, t)| (t.translation(), 1.0)))
        .collect();
    for (tf, mut d) in &mut models {
        let scale = tf.scale().x.max(1e-6);
        let p = tf.translation();
        d.main = p.distance(cam) / scale;
        d.track = views.iter().map(|&(t, mag)| p.distance(t) / mag / scale).reduce(f32::min);
    }
}

/// Helicopters placed as theater tile objects: modelled in aircraft units (not the tile
/// level's), with the rotor on the antenna slot 16 instead of the rotor slot 17.
pub const TILE_HELICOPTERS: [&str; 2] = ["vn_066", "vn_077"];

/// Model scale (metres per model unit) of a tile object at a level of `unit` world units.
pub fn tile_model_scale(name: &str, unit: f64) -> f32 {
    if TILE_HELICOPTERS.contains(&name) {
        crate::player::AIRCRAFT_MODEL_SCALE as f32
    } else {
        (unit * crate::world::WU) as f32
    }
}

/// Range multiplier for a model's last (coarsest) detail level.
const LAST_LOD_RANGE: f32 = 4.0;
/// Range multiplier for the other detail levels: the coarse levels (often just a box or a dot)
/// are meant for a few hundred pixels' worth of distance.
const NEAR_LOD_RANGE: f32 = 4.0;

/// On a model root: its own range multiplier for the last detail level (instead of
/// `LAST_LOD_RANGE`).
#[derive(Component, Clone, Copy)]
pub struct LodRange(pub f32);

/// On a model root: all its detail levels are drawn this many times further (ships, which
/// stand out on the open sea).
#[derive(Component, Clone, Copy)]
pub struct LodBias(pub f32);

fn apply_model_state(
    roots: Query<(&ModelState, &CameraDistance, Option<&LodRange>, Option<&LodBias>)>,
    mut lods: Query<
        (&LodLevel, &mut LodShown, &mut Visibility),
        (Without<CondNode>, Without<ArticulatedPart>),
    >,
    mut conds: Query<(&CondNode, &mut Visibility), Without<LodLevel>>,
    mut parts: Query<(&ArticulatedPart, &mut Transform)>,
    all_lods: Query<&LodLevel>,
    mut lod_meshes: Query<(&LodMesh, &mut bevy::camera::visibility::RenderLayers)>,
    globals: Res<ModelGlobals>,
    detail: Option<Res<crate::palette::DetailLevel>>,
) {
    let reduce = detail.map_or(1.0, |d| d.lod_range());
    for (lod, mut shown, mut vis) in &mut lods {
        let Ok((_, dist, range, bias)) = roots.get(lod.root) else { continue };
        let reduce = reduce * bias.map_or(1.0, |b| b.0);
        // The chosen level is the first whose max_dist covers the distance.
        let prev = all_lods
            .iter()
            .filter(|l| l.root == lod.root && l.level < lod.level)
            .map(|l| l.max_dist * NEAR_LOD_RANGE * reduce)
            .fold(0.0, f32::max);
        // Every level is drawn NEAR_LOD_RANGE times further than its model says, the last one
        // LAST_LOD_RANGE (or the root's LodRange) times, all reduced by the Alt-D detail setting.
        let last = !all_lods.iter().any(|l| l.root == lod.root && l.level > lod.level);
        let factor = if last { range.map_or(LAST_LOD_RANGE, |r| r.0) } else { NEAR_LOD_RANGE };
        let max_dist = lod.max_dist * factor * reduce;
        let picks = |d: f32| d <= max_dist && (lod.level == 0 || d > prev);
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
        let Ok((state, ..)) = roots.get(c.root) else { continue };
        let show = c.conds.iter().all(|&(s, v)| state.0.get(s) == Some(&v));
        vis.set_if_neq(if show { Visibility::Inherited } else { Visibility::Hidden });
    }
    for (p, mut tf) in &mut parts {
        let Ok((state, ..)) = roots.get(p.root) else { continue };
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
    pub terrain: Res<'w, crate::terrain::RefinedTerrain>,
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
    pub fn spawn(
        &self,
        commands: &mut Commands,
        file: &Handle<Gltf>,
        name: &str,
        level: u32,
        no_fog: bool,
    ) -> Option<Entity> {
        self.spawn_parts(commands, file, name, level, no_fog, false)
    }

    /// Spawns only a model's dot lights (with their conditions and detail levels), for an
    /// aircraft whose exterior is drawn another way (the own aircraft from its cockpit).
    pub fn spawn_lights(
        &self,
        commands: &mut Commands,
        file: &Handle<Gltf>,
        name: &str,
        level: u32,
        no_fog: bool,
    ) -> Option<Entity> {
        self.spawn_parts(commands, file, name, level, no_fog, true)
    }

    fn spawn_parts(
        &self,
        commands: &mut Commands,
        file: &Handle<Gltf>,
        name: &str,
        level: u32,
        no_fog: bool,
        lights_only: bool,
    ) -> Option<Entity> {
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
        let root = commands
            .spawn((
                ModelState::default(),
                CameraDistance::default(),
                ModelInfo { attr: attr as u8 },
            ))
            .id();
        self.spawn_node(commands, node, &material_names, root, Some(root), None, tag, lights_only);
        Some(root)
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn_node(
        &self,
        commands: &mut Commands,
        handle: &Handle<GltfNode>,
        material_names: &HashMap<AssetId<GltfMaterial>, &str>,
        root: Entity,
        reuse: Option<Entity>,
        mut lod: Option<Entity>,
        tag: MeshTag,
        lights_only: bool,
    ) -> Entity {
        let node = self.nodes.get(handle).expect("glTF node loaded with its file");
        let entity = match reuse {
            Some(e) => e,
            None => commands.spawn_empty().id(),
        };
        let mut e = commands.entity(entity);
        e.insert((Name::new(node.name.clone()), node.transform, Visibility::default()));
        let extras: serde_json::Value = node
            .extras
            .as_ref()
            .and_then(|x| serde_json::from_str(&x.value).ok())
            .unwrap_or_default();
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
                            let v: Vec<f32> = p
                                .as_array()?
                                .iter()
                                .filter_map(|x| x.as_f64().map(|f| f as f32))
                                .collect();
                            (v.len() == 4).then(|| [v[0], v[1], v[2], v[3]])
                        })
                        .collect()
                })
                .unwrap_or_default();
            if !planes.is_empty() {
                let mut radius = extras["radius"].as_f64().unwrap_or(0.0) as f32;
                let surface = self.terrain.surfaces.get(name).cloned();
                if let Some(s) = &surface {
                    radius = s.iter().flatten().map(|v| v.length()).fold(radius, f32::max);
                }
                e.insert(CollisionHull { planes, radius, surface });
            }
        }
        if suffix_number(name, "_part").is_some() {
            let mut slot = extras["state_slot"].as_u64().unwrap_or(0) as usize;
            if slot == 16 && TILE_HELICOPTERS.iter().any(|h| name.starts_with(h)) {
                slot = 17;
            }
            e.insert(ArticulatedPart {
                root,
                slot,
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
                // Single-pixel lights -> fullbright billboards.
                let color = node_color.unwrap_or(15);
                for prim in &mesh.primitives {
                    for p in self.positions(&prim.mesh) {
                        let child = commands
                            .spawn((
                                crate::dots::DotLight::bundle(p, &self.dots, tag.0),
                                MeshMaterial3d(self.palette.line_material(color)),
                            ))
                            .id();
                        if let Some(l) = lod {
                            commands.entity(child).insert((
                                LodMesh(l),
                                bevy::camera::visibility::RenderLayers::layer(0),
                            ));
                        }
                        commands.entity(entity).add_child(child);
                    }
                }
            } else if !lights_only {
                for prim in &mesh.primitives {
                    let prim_color = prim
                        .material
                        .as_ref()
                        .and_then(|m| material_names.get(&m.id()))
                        .and_then(|n| index_from_name(n));
                    let color = node_color.or(prim_color).unwrap_or(15);
                    let material = if name.contains("_lines_pal_") {
                        self.palette.line_material(color)
                    } else {
                        self.palette.material(color)
                    };
                    let (flat, warm) = self.shape_class(&prim.mesh, tag.0 & 7);
                    let child_tag = if warm { MeshTag(tag.0 | TAG_WARM) } else { tag.clone() };
                    let child = commands
                        .spawn((Mesh3d(prim.mesh.clone()), MeshMaterial3d(material), child_tag))
                        .id();
                    // `_s14v1` is the missile motor flame (STFLT 1/10/15). The F-117 keeps its
                    // original shape, so its nozzle faces stay in the body mesh.
                    if name.ends_with("_exhaust")
                        || name.ends_with("_s13v1")
                        || name.ends_with("_s14v1")
                        || name == "stflt_024_lod0" && prim_color == Some(EXHAUST_COLOURS[0])
                    {
                        commands.entity(child).insert(ExhaustGlow);
                    }
                    // Flat ground overlays are stacked a few centimetres apart (world.rs painter's
                    // order); as sun shadow casters they would only shadow the overlays below.
                    if flat {
                        commands.entity(child).insert(bevy::light::NotShadowCaster);
                    }
                    if let Some(l) = lod {
                        commands
                            .entity(child)
                            .insert((LodMesh(l), bevy::camera::visibility::RenderLayers::layer(0)));
                    }
                    commands.entity(entity).add_child(child);
                }
            }
        }
        for c in &node.children {
            let child = self.spawn_node(
                commands,
                c,
                material_names,
                root,
                None,
                lod,
                tag.clone(),
                lights_only,
            );
            commands.entity(entity).add_child(child);
        }
        entity
    }

    /// The triangles of a model node's own meshes (not its children's), by palette colour
    /// (model units).
    pub fn node_triangles(&self, file: &Handle<Gltf>, name: &str) -> Vec<(u8, Vec<[Vec3; 3]>)> {
        let Some(g) = self.gltfs.get(file) else { return vec![] };
        let Some(node) = g.named_nodes.get(name).and_then(|n| self.nodes.get(n)) else {
            return vec![];
        };
        let Some(mesh) = node.mesh.as_ref().and_then(|m| self.meshes.get(m)) else { return vec![] };
        let names: HashMap<AssetId<GltfMaterial>, &str> =
            g.named_materials.iter().map(|(n, h)| (h.id(), n.as_ref())).collect();
        let mut out = Vec::new();
        for prim in &mesh.primitives {
            let Some(m) = self.mesh_data.get(&prim.mesh) else { continue };
            let colour = prim
                .material
                .as_ref()
                .and_then(|m| names.get(&m.id()))
                .and_then(|n| index_from_name(n))
                .unwrap_or(15);
            let p = self.positions(&prim.mesh);
            let idx: Vec<usize> = match m.indices() {
                Some(i) => i.iter().collect(),
                None => (0..p.len()).collect(),
            };
            let tris = idx
                .as_chunks::<3>()
                .0
                .iter()
                .filter_map(|t| Some([*p.get(t[0])?, *p.get(t[1])?, *p.get(t[2])?]))
                .collect();
            out.push((colour, tris));
        }
        out
    }

    /// A model node's triangles flattened onto its ground plane (y = 0), as one mesh.
    pub fn flattened(&self, file: &Handle<Gltf>, name: &str) -> Option<Mesh> {
        let tris: Vec<[Vec3; 3]> = self
            .node_triangles(file, name)
            .into_iter()
            .flat_map(|(_, t)| t)
            .map(|t| t.map(|v| Vec3::new(v.x, 0.0, v.z)))
            .collect();
        (!tris.is_empty()).then(|| triangle_mesh(&tris))
    }

    fn positions(&self, mesh: &Handle<Mesh>) -> Vec<Vec3> {
        let Some(m) = self.mesh_data.get(mesh) else { return vec![] };
        match m.attribute(Mesh::ATTRIBUTE_POSITION) {
            Some(bevy::mesh::VertexAttributeValues::Float32x3(v)) => {
                v.iter().map(|&p| Vec3::from(p)).collect()
            }
            _ => vec![],
        }
    }

    /// Whether all of a mesh's vertices lie in the model's ground plane (.3DX coordinates
    /// are integers; the converter lifts coplanar polygons by fractions of a unit).
    /// (flat ground overlay, warm in the FLIR picture) for a mesh of a model at a haze level:
    /// aircraft, vehicles and missiles are warm; of the tile objects the raised ones of
    /// building size (not hills and mountains).
    fn shape_class(&self, mesh: &Handle<Mesh>, level: u32) -> (bool, bool) {
        if level == LEVEL_AIRCRAFT {
            return (false, true);
        }
        let p = self.positions(mesh);
        if p.is_empty() {
            return (false, false);
        }
        if p.iter().all(|v| v.y.abs() < 1.0) {
            return (true, false);
        }
        let (lo, hi) = p.iter().fold((Vec3::MAX, Vec3::MIN), |(lo, hi), &v| (lo.min(v), hi.max(v)));
        let metres = (hi - lo)
            * (stealth_formats::terrain::LEVEL_UNIT[(level as usize).min(4)] * crate::world::WU)
                as f32;
        (false, metres.x.max(metres.z) < WARM_MAX_SIZE && metres.y < WARM_MAX_HEIGHT)
    }
}

/// A mesh of separate triangles (positions only, like the model files).
pub fn triangle_mesh(tris: &[[Vec3; 3]]) -> Mesh {
    let pos: Vec<[f32; 3]> = tris.iter().flatten().map(|v| v.to_array()).collect();
    let mut m = Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::RENDER_WORLD,
    );
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    m
}

#[cfg(test)]
mod tests {
    use super::CollisionHull;

    /// Plane hulls from the converted CE models: inside a mountain collides, above it doesn't,
    /// and flat airbase surfaces never contain points above the ground.
    #[test]
    fn hulls_from_model_planes() {
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
                    let v: Vec<f32> =
                        p.as_array().unwrap().iter().map(|x| x.as_f64().unwrap() as f32).collect();
                    [v[0], v[1], v[2], v[3]]
                })
                .collect();
            CollisionHull {
                planes,
                radius: node["extras"]["radius"].as_f64().unwrap() as f32,
                surface: None,
            }
        };
        let mountain = hull("ce_003_lod0");
        assert!(mountain.contains(bevy::math::Vec3::new(0.0, 10.0, 0.0)));
        assert!(!mountain.contains(bevy::math::Vec3::new(0.0, 400.0, 0.0)));
        let airbase = hull("ce_021_lod0");
        assert!(!airbase.contains(bevy::math::Vec3::new(0.0, 2.0, 0.0)));
    }
}
