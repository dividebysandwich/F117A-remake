//! The palette material: every model polygon, line and dot is drawn with one of these
//! (shader: assets/shaders/palette.wgsl). Shared state (live palette, sky/ground band tables)
//! lives in one storage buffer referenced by all materials.

use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError};
use bevy::render::storage::ShaderBuffer;
use bevy::shader::ShaderRef;

const SHADER: &str = "shaders/palette.wgsl";
const PREPASS_SHADER: &str = "shaders/palette_prepass.wgsl";

pub const FLAG_BACKGROUND: u32 = 1;
pub const FLAG_LINE: u32 = 2;
/// Background drawn as the FLIR camera sees it: flat sky 0x88, flat ground 0x7C.
pub const FLAG_FLIR: u32 = 4;

#[derive(Clone, Copy, Debug, Default, ShaderType)]
pub struct PaletteParams {
    pub word: u32,
    pub flags: u32,
    pub _pad0: u32,
    pub _pad1: u32,
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct PaletteMaterial {
    #[uniform(0)]
    pub params: PaletteParams,
    #[storage(1, read_only)]
    pub globals: Handle<ShaderBuffer>,
}

impl Material for PaletteMaterial {
    fn vertex_shader() -> ShaderRef {
        SHADER.into()
    }

    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }

    // The prepass pipeline is only used for the shadow maps (no camera has a depth prepass):
    // world geometry casts sun shadows when the shadows option is on.
    fn enable_prepass() -> bool {
        true
    }

    fn enable_shadows() -> bool {
        true
    }

    fn prepass_vertex_shader() -> ShaderRef {
        PREPASS_SHADER.into()
    }

    fn prepass_fragment_shader() -> ShaderRef {
        ShaderRef::Default
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // Only positions are used (models have no normals or UVs).
        let vertex_layout = layout.0.get_layout(&[Mesh::ATTRIBUTE_POSITION.at_shader_location(0)])?;
        descriptor.vertex.buffers = vec![vertex_layout];
        // Back faces are culled like the original's plane visibility test (the converter
        // winds polygons towards their plane's visible side; dot billboards face the camera).
        descriptor.primitive.cull_mode = Some(bevy::render::render_resource::Face::Back);
        Ok(())
    }
}

/// Shared shader data (must match `Globals` in palette.wgsl).
#[derive(Clone, ShaderType)]
pub struct Globals {
    pub palette: [Vec4; 256],
    pub sky_ring: [Vec4; 4],
    pub ground_g: [Vec4; 4],
    /// x: haze enabled, y: metres per world unit, z: FLIR, w: modern mode
    pub params: Vec4,
    /// Direction to the sun (xyz) and its strength (w) for the lighting options.
    pub sun: Vec4,
    /// Ambient light (rgb); w: lighting flags (`LIGHT_SHADOWS` | `LIGHT_LOCAL`).
    pub ambient: Vec4,
    /// x: number of local lights, y: daylight 0 (night) .. 1 (day).
    pub light_info: Vec4,
    /// x: XR dot angular size in radians (0 = flat screen; see dots.rs).
    pub dots: Vec4,
    /// Local lights, two entries each: (position, range), (linear colour * intensity, unused).
    pub local_lights: [Vec4; 2 * MAX_LOCAL_LIGHTS],
}

/// Lighting flags in `Globals.ambient.w`.
pub const LIGHT_SHADOWS: u32 = 1;
pub const LIGHT_LOCAL: u32 = 2;
/// Local lights handed to the world shader (must match palette.wgsl).
pub const MAX_LOCAL_LIGHTS: usize = 32;

impl Default for Globals {
    fn default() -> Self {
        Self {
            palette: [Vec4::ONE; 256],
            sky_ring: [Vec4::ZERO; 4],
            ground_g: [Vec4::ZERO; 4],
            params: Vec4::ZERO,
            sun: Vec4::ZERO,
            ambient: Vec4::ZERO,
            light_info: Vec4::ZERO,
            dots: Vec4::ZERO,
            local_lights: [Vec4::ZERO; 2 * MAX_LOCAL_LIGHTS],
        }
    }
}

/// Packs 16 values into 4 vec4s.
pub fn pack16(v: [f32; 16]) -> [Vec4; 4] {
    [
        Vec4::new(v[0], v[1], v[2], v[3]),
        Vec4::new(v[4], v[5], v[6], v[7]),
        Vec4::new(v[8], v[9], v[10], v[11]),
        Vec4::new(v[12], v[13], v[14], v[15]),
    ]
}

/// Mesh tag layout for per-object haze: bits 0..2 level code (0..4 = LOD0..4, 5 = aircraft),
/// bits 3..5 size class (.3DX attr & 7), bit 6 = no haze, bit 7 = dot billboard (`TAG_DOT`).
pub fn mesh_tag(level: u32, size_class: u32, no_fog: bool) -> u32 {
    (level & 7) | ((size_class & 7) << 3) | ((no_fog as u32) << 6)
}

pub const LEVEL_AIRCRAFT: u32 = 5;
/// Mesh tag bit of dot lights: the shader expands the unit quad around the object origin to a
/// constant pixel size in each view.
pub const TAG_DOT: u32 = 1 << 7;
