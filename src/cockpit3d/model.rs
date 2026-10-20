//! The cockpit model: `assets/models/cockpit_modern.glb` / `cockpit_classic.glb`, exported from
//! the Blender sources `assets-src/blend/cockpit_*.blend` (`tools/models/export.sh`).
//!
//! Every visible part comes from the model. The game binds behaviour to it by material name
//! and by object custom properties (glTF extras):
//!
//! Materials:
//! * `panel_grey`, `dark`, `black`, `frame`, `seat`, `cushion`, `yellow`, `rubber`, `metal`,
//!   `highlight`: the shared cockpit materials (`CockpitMaterials`).
//! * `swatch`: the panel's solid parts, coloured by the live palette (UVs point at a cell of
//!   the 16x16 palette swatch: index = row * 16 + column).
//! * `atlas`: a lamp, readout, label or bezel button face drawn live into the instrument atlas.
//!   The object's `atlas_face` names the face (`layout`), `atlas_uv` the uv rectangle its UVs
//!   were made for; they are moved to wherever the face is in the atlas.
//! * `screen_left`, `screen_right`, `adi`, `approach`: live displays (UVs 0..1 over the screen).
//! * `trackcam`: the tracking camera picture behind the right screen.
//! * `hud_glass`: the HUD combiner glass (the HUD is drawn collimated through it).
//! * `label_<TEXT>`: a side panel button label showing TEXT.
//! * Anything else is used as modelled (base colour, roughness, metallic).
//!
//! Object properties:
//! * `control` (effect, e.g. `Action(Gear)`, `HudMode(1)`, `Osb(Left, 3)`), `kind` (cap, osb,
//!   side, gear, eject), `half` ([x, y] half size of the face in metres), `height` (face above
//!   the object's origin, along +Z), `travel`: a pushable control; its face points along +Z.
//!   Its `moving` child is what moves when pressed; a `highlight` object under it is shown
//!   while a VR fingertip touches the control.
//! * `stick`, `throttle`: the levers (their origins: the stick pivot, the throttle handle at
//!   idle); `anchor: "throttle_full"`: the handle at full thrust.
//! * `lamp` ([r, g, b], linear) and `range`: a flood lamp (with the local lights option).
//! * `no_shadow`: casts no shadow.

use std::collections::HashMap;

use bevy::camera::visibility::NoFrustumCulling;
use bevy::gltf::{Gltf, GltfMaterial, GltfMesh, GltfNode};
use bevy::light::NotShadowCaster;
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;

use super::controls::{Control, Highlight, Moving};
use super::displays::{self, CanvasRole, LitMaterial, PanelMaterials, TrackCamScreen};
use super::hud_glass::HudGlassMaterial;
use super::layout::PanelLayout;
use super::levers::{CockpitAnchors, Stick, ThrottleLever};
use super::{CockpitLamp, CockpitMaterials};
use crate::font::Fonts;

/// The two cockpit models.
#[derive(Resource)]
pub struct CockpitModels {
    pub modern: Handle<Gltf>,
    pub classic: Handle<Gltf>,
}

pub(super) fn load_models(mut commands: Commands, server: Res<AssetServer>) {
    commands.insert_resource(CockpitModels { modern: server.load("models/cockpit_modern.glb"), classic: server.load("models/cockpit_classic.glb") });
}

impl CockpitMaterials {
    fn by_name(&self, name: &str) -> Option<&Handle<StandardMaterial>> {
        Some(match name {
            "panel_grey" => &self.panel_grey,
            "dark" => &self.dark,
            "black" => &self.black,
            "frame" => &self.frame,
            "seat" => &self.seat,
            "cushion" => &self.cushion,
            "yellow" => &self.yellow,
            "rubber" => &self.rubber,
            "metal" => &self.metal,
            "highlight" => &self.highlight,
            _ => return None,
        })
    }
}

/// Assets read while spawning the model.
#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct ModelAssets<'w> {
    pub gltfs: Res<'w, Assets<Gltf>>,
    pub nodes: Res<'w, Assets<GltfNode>>,
    pub meshes: Res<'w, Assets<GltfMesh>>,
    pub materials: Res<'w, Assets<GltfMaterial>>,
}

/// Where the spawned parts get their run-time materials and canvases.
pub(super) struct Spawner<'a> {
    pub mats: &'a CockpitMaterials,
    pub panel: &'a PanelMaterials,
    pub layout: &'a PanelLayout,
    pub fonts: &'a Fonts,
    pub tc_image: Option<&'a crate::trackcam::TrackCamImage>,
    pub classic_hud: bool,
    pub local_lights: bool,
    pub meshes: &'a mut Assets<Mesh>,
    pub materials: &'a mut Assets<StandardMaterial>,
    pub hud_materials: &'a mut Assets<HudGlassMaterial>,
    pub images: &'a mut Assets<Image>,
    /// Modelled materials by glTF material (made once).
    modelled: HashMap<AssetId<GltfMaterial>, Handle<StandardMaterial>>,
    pub anchors: CockpitAnchors,
}

impl<'a> Spawner<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        mats: &'a CockpitMaterials,
        panel: &'a PanelMaterials,
        layout: &'a PanelLayout,
        fonts: &'a Fonts,
        tc_image: Option<&'a crate::trackcam::TrackCamImage>,
        classic_hud: bool,
        local_lights: bool,
        meshes: &'a mut Assets<Mesh>,
        materials: &'a mut Assets<StandardMaterial>,
        hud_materials: &'a mut Assets<HudGlassMaterial>,
        images: &'a mut Assets<Image>,
    ) -> Self {
        Self {
            mats,
            panel,
            layout,
            fonts,
            tc_image,
            classic_hud,
            local_lights,
            meshes,
            materials,
            hud_materials,
            images,
            modelled: HashMap::new(),
            anchors: CockpitAnchors::default(),
        }
    }

    /// Spawns the model's root objects under `root`. Returns false if the file is not loaded.
    pub fn spawn(&mut self, commands: &mut Commands, assets: &ModelAssets, file: &Handle<Gltf>, root: Entity) -> bool {
        let Some(g) = assets.gltfs.get(file) else { return false };
        let names: HashMap<AssetId<GltfMaterial>, String> = g.named_materials.iter().map(|(n, h)| (h.id(), n.to_string())).collect();
        let children: std::collections::HashSet<AssetId<GltfNode>> =
            g.nodes.iter().filter_map(|n| assets.nodes.get(n)).flat_map(|n| n.children.iter().map(|c| c.id())).collect();
        for n in g.nodes.iter().filter(|n| !children.contains(&n.id())) {
            let e = self.node(commands, assets, &names, n, Transform::IDENTITY);
            commands.entity(root).add_child(e);
        }
        true
    }

    fn node(&mut self, commands: &mut Commands, assets: &ModelAssets, names: &HashMap<AssetId<GltfMaterial>, String>, handle: &Handle<GltfNode>, parent: Transform) -> Entity {
        let Some(node) = assets.nodes.get(handle) else { return commands.spawn_empty().id() };
        let extras: serde_json::Value = node.extras.as_ref().and_then(|x| serde_json::from_str(&x.value).ok()).unwrap_or_default();
        let at = parent * node.transform;
        let e = commands.spawn((Name::new(node.name.clone()), node.transform, Visibility::default())).id();
        let mut ec = commands.entity(e);
        if extras.get("control").is_some() {
            match Control::from_model(&extras) {
                Some(c) => {
                    if c.is_eject() {
                        self.anchors.eject = at.translation;
                    }
                    ec.insert(c);
                }
                None => warn!("cockpit model: bad control {} on {}", extras["control"], node.name),
            }
        }
        if extras["moving"].as_bool() == Some(true) {
            ec.insert(Moving);
        }
        if extras["highlight"].as_bool() == Some(true) {
            ec.insert((Highlight, Visibility::Hidden));
        }
        if extras["stick"].as_bool() == Some(true) {
            ec.insert(Stick);
            self.anchors.stick_pivot = at.translation;
        }
        if extras["throttle"].as_bool() == Some(true) {
            ec.insert(ThrottleLever);
            self.anchors.throttle_idle = at.translation;
        }
        if extras["anchor"].as_str() == Some("throttle_full") {
            self.anchors.throttle_full = at.translation;
        }
        if let Some(c) = extras["lamp"].as_array()
            && self.local_lights
        {
            let f = |i: usize| c.get(i).and_then(|v| v.as_f64()).unwrap_or(1.0) as f32;
            ec.insert((
                CockpitLamp,
                PointLight {
                    intensity: 0.0,
                    range: extras["range"].as_f64().unwrap_or(1.6) as f32,
                    color: Color::linear_rgb(f(0), f(1), f(2)),
                    shadow_maps_enabled: false,
                    ..default()
                },
            ));
        }
        let no_shadow = extras["no_shadow"].as_bool() == Some(true);

        if let Some(mesh) = node.mesh.as_ref().and_then(|m| assets.meshes.get(m)) {
            let single = mesh.primitives.len() == 1;
            for prim in &mesh.primitives {
                let target = if single { e } else { commands.spawn((Transform::default(), Visibility::default())).id() };
                if !single {
                    commands.entity(e).add_child(target);
                }
                let name = prim.material.as_ref().and_then(|m| names.get(&m.id())).map_or("", String::as_str);
                self.primitive(commands, target, name, prim, &extras, assets);
                let mut t = commands.entity(target);
                t.insert(NoFrustumCulling);
                if no_shadow {
                    t.insert(NotShadowCaster);
                }
            }
        }
        let kids: Vec<Entity> = node.children.iter().map(|c| self.node(commands, assets, names, c, at)).collect();
        commands.entity(e).add_children(&kids);
        e
    }

    /// A mesh primitive with its run-time material (by material name).
    fn primitive(&mut self, commands: &mut Commands, e: Entity, name: &str, prim: &bevy::gltf::GltfPrimitive, extras: &serde_json::Value, assets: &ModelAssets) {
        let mut mesh = prim.mesh.clone();
        let mut ec = commands.entity(e);
        let material = match name {
            "swatch" => self.panel.solid.clone(),
            "atlas" => {
                if let Some(m) = self.atlas_mesh(&prim.mesh, extras) {
                    mesh = m;
                }
                self.panel.atlas.clone()
            }
            "screen_left" | "screen_right" | "adi" | "approach" => {
                let role = match name {
                    "screen_left" => CanvasRole::Left,
                    "screen_right" => CanvasRole::Right,
                    "adi" => CanvasRole::Adi,
                    _ => CanvasRole::Approach,
                };
                let (tex, m) = displays::screen(role, self.layout, self.materials, self.images);
                ec.insert((role, tex));
                m
            }
            "trackcam" => {
                ec.insert((TrackCamScreen, Visibility::Hidden));
                displays::trackcam_material(self.materials, self.tc_image)
            }
            "hud_glass" => {
                let (tex, m) = displays::hud_material(self.hud_materials, self.images, self.classic_hud);
                ec.insert((CanvasRole::Hud, tex, Mesh3d(mesh), MeshMaterial3d(m), NotShadowCaster));
                return;
            }
            n if n.starts_with("label_") => {
                let (tex, m) = displays::side_label(&n["label_".len()..], self.fonts, self.materials, self.images);
                ec.insert((tex, LitMaterial(m.clone())));
                m
            }
            n => match self.mats.by_name(n) {
                Some(m) => m.clone(),
                None => self.modelled(prim.material.as_ref(), assets),
            },
        };
        commands.entity(e).insert((Mesh3d(mesh), MeshMaterial3d(material)));
    }

    /// The face's mesh with its UVs moved to the face's place in the instrument atlas.
    fn atlas_mesh(&mut self, mesh: &Handle<Mesh>, extras: &serde_json::Value) -> Option<Handle<Mesh>> {
        let key = extras["atlas_face"].as_str()?;
        let Some(&(n0, n1)) = self.panel.faces.get(key) else {
            warn!("cockpit model: no atlas face {key}");
            return None;
        };
        let r: Vec<f32> = extras["atlas_uv"].as_array()?.iter().filter_map(|v| v.as_f64().map(|x| x as f32)).collect();
        let (o0, o1) = (Vec2::new(*r.first()?, *r.get(1)?), Vec2::new(*r.get(2)?, *r.get(3)?));
        let mut m = self.meshes.get(mesh)?.clone();
        if let Some(VertexAttributeValues::Float32x2(uv)) = m.attribute_mut(Mesh::ATTRIBUTE_UV_0) {
            for u in uv.iter_mut() {
                let t = (Vec2::from(*u) - o0) / (o1 - o0).max(Vec2::splat(1e-6));
                *u = (n0 + t * (n1 - n0)).to_array();
            }
        }
        Some(self.meshes.add(m))
    }

    /// A material as modelled in Blender.
    fn modelled(&mut self, material: Option<&Handle<GltfMaterial>>, assets: &ModelAssets) -> Handle<StandardMaterial> {
        let Some(h) = material else { return self.mats.panel_grey.clone() };
        if let Some(m) = self.modelled.get(&h.id()) {
            return m.clone();
        }
        let m = assets.materials.get(h).map_or_else(StandardMaterial::default, |g| StandardMaterial {
            base_color: g.base_color,
            base_color_texture: g.base_color_texture.clone(),
            emissive: g.emissive,
            perceptual_roughness: g.perceptual_roughness,
            metallic: g.metallic,
            fog_enabled: false,
            ..default()
        });
        let m = self.materials.add(m);
        self.modelled.insert(h.id(), m.clone());
        m
    }
}

#[cfg(test)]
mod tests {
    use super::super::controls::parse_effect;
    use super::super::displays::atlas_faces;
    use super::super::layout::PanelLayout;

    /// The JSON part of a .glb file.
    fn gltf_json(name: &str) -> serde_json::Value {
        let path = format!("{}/assets/models/{name}.glb", env!("CARGO_MANIFEST_DIR"));
        let d = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let len = u32::from_le_bytes(d[12..16].try_into().unwrap()) as usize;
        serde_json::from_slice(&d[20..20 + len]).unwrap()
    }

    /// Both cockpit models have everything the game binds to, and nothing it can't read.
    #[test]
    fn cockpit_models() {
        for (name, classic) in [("cockpit_modern", false), ("cockpit_classic", true)] {
            let j = gltf_json(name);
            let nodes = j["nodes"].as_array().unwrap();
            let extras: Vec<&serde_json::Value> = nodes.iter().map(|n| &n["extras"]).collect();
            let materials: Vec<&str> = j["materials"].as_array().unwrap().iter().filter_map(|m| m["name"].as_str()).collect();
            for m in ["swatch", "atlas", "screen_left", "screen_right", "adi", "approach", "trackcam", "hud_glass", "label_MENU"] {
                assert!(materials.contains(&m), "{name}: no material {m}");
            }
            let has = |k: &str| extras.iter().filter(|x| x.get(k).is_some()).count();
            assert_eq!(has("stick"), 1, "{name}: stick");
            assert_eq!(has("throttle"), 1, "{name}: throttle");
            assert!(extras.iter().any(|x| x["anchor"] == "throttle_full"), "{name}: throttle_full anchor");
            assert!(extras.iter().any(|x| x["kind"] == "eject"), "{name}: ejection handle");
            assert!(extras.iter().any(|x| x["control"] == "Action(Menu)"), "{name}: MENU button");
            let keys: Vec<String> = atlas_faces(&PanelLayout::for_classic(classic)).iter().map(|(f, _)| format!("{f:?}")).collect();
            for x in &extras {
                if let Some(c) = x["control"].as_str() {
                    assert!(parse_effect(c).is_some(), "{name}: control {c}");
                    assert!(super::super::controls::Control::from_model(x).is_some(), "{name}: control {x}");
                }
                if let Some(f) = x["atlas_face"].as_str() {
                    assert!(keys.iter().any(|k| k == f), "{name}: atlas face {f} not in the layout");
                }
            }
        }
    }
}
