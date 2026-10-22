//! Cockpit materials: `StandardMaterial` with our fragment shader (assets/shaders/cockpit.wgsl)
//! in three variants: unlit materials (the panel picture, screens and labels without the
//! lighting options) without the PBR lighting, lit ones without the lighting options with only
//! the sun and the ambient light, and Bevy's full lighting with the options (point lights,
//! shadows). Bevy's own shader compiles every lighting path in (`unlit` is only a flag): on the
//! Steam Frame's GPU it is about 2700 instructions and needs so many registers that half as many
//! fragments run at once.

use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{
    ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline,
};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;

const SHADER: &str = "shaders/cockpit.wgsl";

pub type CockpitMaterial = ExtendedMaterial<StandardMaterial, CockpitShading>;

/// A cockpit material (the shader variant follows `base.unlit` and the lighting options, see
/// `super::sync_materials`).
pub fn cockpit_material(base: StandardMaterial) -> CockpitMaterial {
    let unlit = base.unlit;
    ExtendedMaterial { base, extension: CockpitShading { unlit, ..default() } }
}

/// A cockpit material the night never darkens (a picture or an overlay, not a surface).
pub fn cockpit_material_bright(base: StandardMaterial) -> CockpitMaterial {
    let mut m = cockpit_material(base);
    m.extension.bright = true;
    m
}

/// Picks the shader variant.
#[derive(Asset, AsBindGroup, TypePath, Clone, Debug, Default)]
#[bind_group_data(CockpitShadingKey)]
pub struct CockpitShading {
    /// `base.unlit`: no PBR lighting compiled in.
    pub unlit: bool,
    /// The lighting options are on: Bevy's full lighting (point lights, shadows); otherwise
    /// only the sun and the ambient light.
    pub full: bool,
    /// x: darkness of a night without moonlight 0..1 (`crate::sky::Sky::dark`): everything
    /// but the emissive parts and the cockpit lamps' light goes dark.
    #[uniform(100)]
    pub night: Vec4,
    /// Never darkened.
    pub bright: bool,
}

#[repr(C)]
#[derive(Clone, Copy, Hash, PartialEq, Eq)]
pub struct CockpitShadingKey {
    unlit: bool,
    full: bool,
}

impl From<&CockpitShading> for CockpitShadingKey {
    fn from(s: &CockpitShading) -> Self {
        Self { unlit: s.unlit, full: s.full }
    }
}

impl MaterialExtension for CockpitShading {
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }

    fn specialize(
        _pipeline: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        key: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        if let Some(fragment) = descriptor.fragment.as_mut() {
            let k = key.bind_group_data;
            if k.unlit {
                fragment.shader_defs.push("COCKPIT_UNLIT".into());
            } else if k.full {
                fragment.shader_defs.push("COCKPIT_FULL".into());
            } else {
                fragment.shader_defs.push("COCKPIT_SIMPLE_LIT".into());
            }
        }
        Ok(())
    }
}
