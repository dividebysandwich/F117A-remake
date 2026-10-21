//! Lightning channel rendering (the strikes themselves are in weather.rs): an additive
//! material drawing camera-facing ribbons along the channel (shader: assets/shaders/bolt.wgsl).

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, BlendComponent, BlendFactor, BlendOperation, BlendState, RenderPipelineDescriptor,
    SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;

use crate::trails::{ATTRIBUTE_SIDE, ATTRIBUTE_TANGENT};

const SHADER: &str = "shaders/bolt.wgsl";

/// Minimum half width of a ribbon in radians of view (about 2 pixels at 1280 wide), so
/// distant channels keep their white core and blue glow.
pub const MIN_HALF_ANGLE: f32 = 0.0025;
/// Each ribbon is this much wider than the channel's core, for the blue glow around it.
const GLOW: f32 = 3.0;

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct BoltMaterial {
    /// x: brightness 0..1, y: minimum half width (radians).
    #[uniform(0)]
    pub params: Vec4,
}

impl Material for BoltMaterial {
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
        let vertex_layout = layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            ATTRIBUTE_TANGENT.at_shader_location(1),
            ATTRIBUTE_SIDE.at_shader_location(2),
        ])?;
        descriptor.vertex.buffers = vec![vertex_layout];
        descriptor.primitive.cull_mode = None;
        // Additive: light adds up where branches cross and over the clouds behind.
        if let Some(fragment) = descriptor.fragment.as_mut() {
            for target in fragment.targets.iter_mut().flatten() {
                target.blend = Some(BlendState {
                    color: BlendComponent {
                        src_factor: BlendFactor::SrcAlpha,
                        dst_factor: BlendFactor::One,
                        operation: BlendOperation::Add,
                    },
                    alpha: BlendComponent::OVER,
                });
            }
        }
        Ok(())
    }
}

/// Ribbon mesh of a channel: one quad per segment (relative to `origin`), with the
/// segment's core half width and its branch brightness.
pub fn bolt_mesh(segments: &[(Vec3, Vec3, f32)], origin: Vec3, main_width: f32) -> Mesh {
    let mut pos: Vec<[f32; 3]> = Vec::with_capacity(segments.len() * 4);
    let mut tangent: Vec<[f32; 3]> = Vec::with_capacity(segments.len() * 4);
    let mut side: Vec<[f32; 2]> = Vec::with_capacity(segments.len() * 4);
    let mut idx: Vec<u32> = Vec::with_capacity(segments.len() * 6);
    for &(p, q, w) in segments {
        let t = (q - p).normalize_or_zero().to_array();
        // Thinner branches are also dimmer.
        let bright = (w / main_width).sqrt().clamp(0.3, 1.0);
        let v = pos.len() as u32;
        for (c, s) in [(p, -1.0), (p, 1.0), (q, 1.0), (q, -1.0)] {
            pos.push((c - origin).to_array());
            tangent.push(t);
            side.push([s * w * GLOW, bright]);
        }
        idx.extend([v, v + 1, v + 2, v, v + 2, v + 3]);
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(ATTRIBUTE_TANGENT, tangent)
        .with_inserted_attribute(ATTRIBUTE_SIDE, side)
        .with_inserted_indices(Indices::U32(idx))
}
