//! Cockpit materials: `StandardMaterial` with our fragment shader (assets/shaders/cockpit.wgsl),
//! so an unlit material (the panel picture, screens and labels without the lighting options)
//! gets a shader without the PBR lighting. `unlit` is only a flag in Bevy's own shader, which
//! compiles every lighting path in: on the Steam Frame's GPU that shader is about 2700
//! instructions and needs so many registers that half as many fragments run at once.

use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, RenderPipelineDescriptor, SpecializedMeshPipelineError};
use bevy::shader::ShaderRef;

const SHADER: &str = "shaders/cockpit.wgsl";

pub type CockpitMaterial = ExtendedMaterial<StandardMaterial, CockpitShading>;

/// A cockpit material (keep `extension.unlit` equal to `base.unlit`).
pub fn cockpit_material(base: StandardMaterial) -> CockpitMaterial {
    let unlit = base.unlit;
    ExtendedMaterial { base, extension: CockpitShading { unlit } }
}

/// Picks the shader variant: unlit (no PBR lighting compiled in) or lit.
#[derive(Asset, AsBindGroup, TypePath, Clone, Debug, Default)]
#[bind_group_data(CockpitShadingKey)]
pub struct CockpitShading {
    pub unlit: bool,
}

#[repr(C)]
#[derive(Clone, Copy, Hash, PartialEq, Eq)]
pub struct CockpitShadingKey {
    unlit: bool,
}

impl From<&CockpitShading> for CockpitShadingKey {
    fn from(s: &CockpitShading) -> Self {
        Self { unlit: s.unlit }
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
        if key.bind_group_data.unlit
            && let Some(fragment) = descriptor.fragment.as_mut()
        {
            fragment.shader_defs.push("COCKPIT_UNLIT".into());
        }
        Ok(())
    }
}
