//! The palette material: every model polygon, line and dot is drawn with one of these
//! (shader: assets/shaders/palette.wgsl). Shared state (live palette, sky/ground band tables)
//! lives in one storage buffer referenced by all materials.

use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::render::storage::ShaderBuffer;
use bevy::shader::ShaderRef;

const SHADER: &str = "shaders/palette.wgsl";
const PREPASS_SHADER: &str = "shaders/palette_prepass.wgsl";

pub const FLAG_BACKGROUND: u32 = 1;
pub const FLAG_LINE: u32 = 2;
/// Background drawn as the FLIR camera sees it: flat sky 0x88, flat ground 0x7C.
pub const FLAG_FLIR: u32 = 4;
/// Cloud surface: grey ramp by face orientation, hazed per pixel (weather.rs).
pub const FLAG_CLOUD: u32 = 8;
/// Smoke plumes: alpha blended (soft edges in modern mode; classic mode dithers).
pub const FLAG_CLOUD_SOFT: u32 = 16;
/// Stratus sheet with MSAA on: alpha to coverage (soft edges with MSAA, depth written).
pub const FLAG_CLOUD_COVERAGE: u32 = 32;
/// Dot lights: drawn in the transparent pass (after all opaque geometry, depth tested but not
/// written; see dots.rs).
pub const FLAG_DOT: u32 = 128;
/// Line in a scenery colour (rivers, roads, shores), not a light: a moonless night darkens it
/// like the ground (`Palette::line_flags`).
pub const FLAG_NIGHT_DIM: u32 = 64;
/// Mesh tag bit of warm objects (aircraft, vehicles, buildings, fires): bright in the FLIR
/// picture, everything else cold.
pub const TAG_WARM: u32 = 1 << 11;
/// Mesh tag bit of dim dot lights (runway edge, threshold and taxiway lamps): they fade out at
/// a shorter range than the others (the shader's `DOT_NEAR`).
pub const TAG_DOT_NEAR: u32 = 1 << 12;
/// Mesh tag bit of smoke plume volumes: the shader ray-marches the plume inside its proxy
/// mesh; bits 16.. carry the plume's noise offset.
pub const TAG_SMOKE: u32 = 1 << 13;
/// Smoke plume over a fire: its base glows.
pub const TAG_SMOKE_FIRE: u32 = 1 << 14;
/// Mesh tag bit of an aircraft's spotting dot: drawn only while the aircraft is too small on
/// screen to make out (the shader's `SPOT_PIXELS`), a little in front of it. Bits 16.. carry
/// the aircraft's size in metres (`spot_size`).
pub const TAG_DOT_SPOT: u32 = 1 << 15;
/// Mesh tag bits of a spotting dot on an aircraft of this size (metres, 1..255).
pub fn spot_size(metres: f32) -> u32 {
    TAG_DOT_SPOT | ((metres.round().clamp(1.0, 255.0) as u32) << 16)
}

#[derive(Clone, Copy, Debug, Default, ShaderType)]
pub struct PaletteParams {
    pub word: u32,
    pub flags: u32,
    /// The lighting options that are on (`LIGHT_SHADOWS` | `LIGHT_LOCAL`): the shader variant
    /// with sun shadows and/or local lights (kept in sync by `sync_lighting`).
    pub lit: u32,
    pub _pad1: u32,
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
#[bind_group_data(PaletteKey)]
pub struct PaletteMaterial {
    #[uniform(0)]
    pub params: PaletteParams,
    #[storage(1, read_only)]
    pub globals: Handle<ShaderBuffer>,
}

/// The shader variant of a material: objects (polygons, lines, dots), the background sphere,
/// or clouds and smoke.
#[repr(C)]
#[derive(Clone, Copy, Hash, PartialEq, Eq)]
pub struct PaletteKey {
    kind: u8,
    lit: u8,
}

const KIND_OBJECT: u8 = 0;
const KIND_BACKGROUND: u8 = 1;
const KIND_CLOUD: u8 = 2;

impl From<&PaletteMaterial> for PaletteKey {
    fn from(m: &PaletteMaterial) -> Self {
        let f = m.params.flags;
        let kind = if f & FLAG_CLOUD != 0 {
            KIND_CLOUD
        } else if f & FLAG_BACKGROUND != 0 {
            KIND_BACKGROUND
        } else {
            KIND_OBJECT
        };
        Self { kind, lit: m.params.lit as u8 }
    }
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
    fn alpha_mode(&self) -> AlphaMode {
        if self.params.flags & (FLAG_CLOUD_SOFT | FLAG_DOT) != 0 {
            AlphaMode::Blend
        } else if self.params.flags & FLAG_CLOUD_COVERAGE != 0 {
            AlphaMode::AlphaToCoverage
        } else {
            AlphaMode::Opaque
        }
    }

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
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        if let Some(fragment) = descriptor.fragment.as_mut() {
            match key.bind_group_data.kind {
                KIND_BACKGROUND => fragment.shader_defs.push("PALETTE_BACKGROUND".into()),
                KIND_CLOUD => fragment.shader_defs.push("PALETTE_CLOUD".into()),
                _ => {}
            }
            // Each option compiled in only when it is on (the local light loop's registers slow
            // down every pixel on the Steam Frame).
            let lit = key.bind_group_data.lit as u32;
            if lit != 0 {
                fragment.shader_defs.push("PALETTE_LIT".into());
            }
            if lit & LIGHT_SHADOWS != 0 {
                fragment.shader_defs.push("PALETTE_SHADOWS".into());
            }
            if lit & LIGHT_LOCAL != 0 {
                fragment.shader_defs.push("PALETTE_LOCAL".into());
            }
        }
        // Only positions are used (models have no normals or UVs). Batched dot lights (scenery
        // lamps) also carry each vertex's quad corner as UV.
        let mut attributes = vec![Mesh::ATTRIBUTE_POSITION.at_shader_location(0)];
        if layout.0.contains(Mesh::ATTRIBUTE_UV_0) {
            attributes.push(Mesh::ATTRIBUTE_UV_0.at_shader_location(1));
            descriptor.vertex.shader_defs.push("DOT_BATCH".into());
        }
        if layout.0.contains(crate::dots::ATTRIBUTE_DOT_COLOUR) {
            attributes.push(crate::dots::ATTRIBUTE_DOT_COLOUR.at_shader_location(2));
            descriptor.vertex.shader_defs.push("DOT_COLOUR".into());
            if let Some(fragment) = descriptor.fragment.as_mut() {
                fragment.shader_defs.push("DOT_COLOUR".into());
            }
        }
        let vertex_layout = layout.0.get_layout(&attributes)?;
        descriptor.vertex.buffers = vec![vertex_layout];
        // Back faces are culled like the classic plane visibility test (the converter
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
    /// x: haze enabled, y: metres per world unit, z: FLIR (tracking camera), w: modern mode
    pub params: Vec4,
    /// Direction to the sun (xyz) and its strength (w) for the lighting options.
    pub sun: Vec4,
    /// Ambient light (rgb); w: lighting flags (`LIGHT_SHADOWS` | `LIGHT_LOCAL`).
    pub ambient: Vec4,
    /// x: number of local lights, y: daylight 0 (night) .. 1 (day), z: night vision goggles.
    pub light_info: Vec4,
    /// x: XR dot angular size in radians (0 = flat screen; see dots.rs), y: animation clock
    /// in seconds (wraps every hour), zw: the tracking camera's picture size in pixels (the
    /// view drawn in FLIR when `params.z` is set).
    pub dots: Vec4,
    /// Local lights, two entries each: (position, range), (linear colour * intensity, unused).
    pub local_lights: [Vec4; 2 * MAX_LOCAL_LIGHTS],
    /// Deck base / top (metres), deck present (z), whiteout 0..1 (w).
    pub clouds: Vec4,
    /// x: haze distance scale, y: lightning flash 0..1, z: visibility under clouds (metres),
    /// w: gloom 0..1.
    pub weather: Vec4,
    /// Cloud texture: floating origin (x, z) modulo the noise period, seed (z); stratus
    /// layer coverage 0..1 (w).
    pub noise: Vec4,
    /// Stratus coverage: threshold 0..1, noise seed, floating origin x / z.
    pub stratus: Vec4,
    /// Lightning channel as a line light: end (xyz) and intensity (w), other end (xyz) and
    /// reach in metres (w).
    pub bolt_a: Vec4,
    pub bolt_b: Vec4,
    /// Continuous time of day: direction to the sun (xyz), on (w = 1).
    pub sky_sun: Vec4,
    /// Direction to the moon (xyz), star visibility 0..1 (w).
    pub sky_moon: Vec4,
    /// x: darkness of a night without moonlight 0..1 (0 under night vision goggles), y: the
    /// strength of its red horizon glow, zw: the recon cameras' picture size (`recon.rs`).
    pub sky_night: Vec4,
    /// The recon cameras' palette (linear RGB): the day's, as in the near infrared.
    pub recon_palette: [Vec4; 256],
    /// Direction to the moon (xyz) and its light's strength (w), for modern shading.
    pub moon: Vec4,
    /// The key light's shadow maps: its index among Bevy's directional lights (x) and number
    /// of cascades (y, 0 without shadows).
    pub shadow: Vec4,
    /// The cascades' far bounds (metres from the eye; unused ones huge).
    pub shadow_bounds: Vec4,
}

/// Lighting flags in `Globals.ambient.w`.
pub const LIGHT_SHADOWS: u32 = 1;
pub const LIGHT_LOCAL: u32 = 2;
/// The shadow maps are the moon's (it is brighter than the sun).
pub const LIGHT_MOON_KEY: u32 = 4;
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
            clouds: Vec4::ZERO,
            weather: Vec4::new(1.0, 0.0, 20_000.0, 0.0),
            noise: Vec4::ZERO,
            stratus: Vec4::ZERO,
            bolt_a: Vec4::ZERO,
            bolt_b: Vec4::ZERO,
            sky_sun: Vec4::ZERO,
            sky_moon: Vec4::ZERO,
            sky_night: Vec4::ZERO,
            recon_palette: [Vec4::ONE; 256],
            moon: Vec4::ZERO,
            shadow: Vec4::ZERO,
            shadow_bounds: Vec4::ZERO,
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
/// bits 3..5 size class (model attr & 7), bit 6 = no haze, bit 7 = dot billboard (`TAG_DOT`),
/// bits 8..10 dot size multiplier (`dot_scale`), bit 12 short-range dot (`TAG_DOT_NEAR`).
pub fn mesh_tag(level: u32, size_class: u32, no_fog: bool) -> u32 {
    (level & 7) | ((size_class & 7) << 3) | ((no_fog as u32) << 6)
}

pub const LEVEL_AIRCRAFT: u32 = 5;
/// Mesh tag bit of dot lights: the shader expands the unit quad around the object origin to a
/// constant pixel size in each view.
pub const TAG_DOT: u32 = 1 << 7;
/// Dot size multiplier minus one (0..7) in mesh tag bits 8..10.
pub fn dot_scale(scale: u32) -> u32 {
    (scale.clamp(1, 8) - 1) << 8
}

/// Dot size in tenths of the plain dot's (1..127) in mesh tag bits 24..30, for sizes the
/// multiplier of [`dot_scale`] can't give; overrides it.
pub fn dot_size_tenths(tenths: u32) -> u32 {
    tenths.clamp(1, 127) << 24
}

/// Mesh tag bit of a dot only drawn far away (the shader's `DOT_FAR_MIN`): gun rounds, whose
/// tracer lines show them up close.
pub const TAG_DOT_FAR: u32 = 1 << 31;

/// Switches every palette material to the shader variant with or without the lighting options
/// (also materials created later).
pub fn sync_lighting(
    settings: Res<crate::settings::Settings>,
    scene: Option<Res<crate::menu_scene::MenuScene>>,
    mut materials: ResMut<Assets<PaletteMaterial>>,
) {
    // The scene behind the menu always has shadows.
    let shadows = settings.shadows || scene.is_some();
    let lit = (shadows as u32 * LIGHT_SHADOWS) | (settings.local_lights as u32 * LIGHT_LOCAL);
    let stale: Vec<_> =
        materials.iter().filter(|(_, m)| m.params.lit != lit).map(|(id, _)| id).collect();
    for id in stale {
        if let Some(mut m) = materials.get_mut(id) {
            m.params.lit = lit;
        }
    }
}
