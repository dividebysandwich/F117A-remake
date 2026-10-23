//! The HUD combiner glass material (shader: assets/shaders/hud_glass.wgsl): shows the HUD
//! canvas collimated to infinity through the glass, like a real HUD.

use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;

const SHADER: &str = "shaders/hud_glass.wgsl";

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct HudGlassMaterial {
    /// Glass tint where the HUD is blank (linear rgb, alpha).
    #[uniform(0)]
    pub tint: Vec4,
    #[texture(1)]
    #[sampler(2)]
    pub image: Handle<Image>,
    /// Canvas pixel of the boresight (xy) and canvas pixels per unit of the tangent plane
    /// (zw, horizontal and vertical).
    #[uniform(3)]
    pub proj: Vec4,
    /// x: 1 = modern HUD (every set texel drawn in HUD green), y: brightness.
    #[uniform(4)]
    pub style: Vec4,
}

impl HudGlassMaterial {
    /// Canvas size, projection and style of the classic or the modern HUD (`scale`: the modern
    /// HUD's canvas pixels per unit; the classic HUD is shrunk alike, `hud::classic_zoom`).
    pub fn layout(classic: bool, scale: f32) -> ((usize, usize), Vec4, Vec4) {
        use crate::hud::modern;
        if classic {
            let f = crate::cockpit::FOCAL * crate::hud::classic_zoom(scale);
            (
                (320, 200),
                Vec4::new(159.5, 52.0, f, f * crate::cockpit::VSCALE),
                Vec4::new(0.0, 1.0, 0.0, 0.0),
            )
        } else {
            (
                (modern::WIDTH, modern::HEIGHT),
                Vec4::new(modern::CX, modern::CY, scale, scale),
                Vec4::new(1.0, 1.0, 0.0, 0.0),
            )
        }
    }
}

impl Material for HudGlassMaterial {
    fn vertex_shader() -> ShaderRef {
        SHADER.into()
    }

    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let vertex_layout =
            layout.0.get_layout(&[Mesh::ATTRIBUTE_POSITION.at_shader_location(0)])?;
        descriptor.vertex.buffers = vec![vertex_layout];
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

/// The helmet-mounted display's visor (`hmd.rs`): the HUD glass's collimated canvas on a quad
/// carried by the head, drawn over everything (no depth test: the cockpit never hides it).
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct HmdMaterial {
    #[uniform(0)]
    pub tint: Vec4,
    #[texture(1)]
    #[sampler(2)]
    pub image: Handle<Image>,
    #[uniform(3)]
    pub proj: Vec4,
    #[uniform(4)]
    pub style: Vec4,
}

impl Material for HmdMaterial {
    fn vertex_shader() -> ShaderRef {
        SHADER.into()
    }

    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let vertex_layout =
            layout.0.get_layout(&[Mesh::ATTRIBUTE_POSITION.at_shader_location(0)])?;
        descriptor.vertex.buffers = vec![vertex_layout];
        descriptor.primitive.cull_mode = None;
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.depth_compare = Some(bevy::render::render_resource::CompareFunction::Always);
            depth.depth_write_enabled = Some(false);
        }
        Ok(())
    }
}
