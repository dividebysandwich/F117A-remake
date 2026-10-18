// HUD combiner glass: the HUD canvas collimated to infinity. Each fragment looks up the HUD
// pixel in the direction it is seen from the eye (cockpit frame = the glass mesh's local
// frame), so the symbology stays on the outside world wherever the head is and only shows
// through the glass.

#import bevy_pbr::{
    mesh_functions,
    mesh_view_bindings::view,
    view_transformations::position_world_to_clip,
}

// Original projection (cockpit::FOCAL, VSCALE; cockpit3d::geometry::CX, CY).
const FOCAL: f32 = 256.0;
const VSCALE: f32 = 0.75;
const CX: f32 = 159.5;
const CY: f32 = 52.0;

// Glass tint where the HUD is blank (rgb, alpha).
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> tint: vec4<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var hud_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var hud_sampler: sampler;

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    // Eye to vertex in the cockpit frame (linear, so it interpolates exactly).
    @location(0) dir: vec3<f32>,
};

@vertex
fn vertex(v: Vertex) -> VertexOutput {
    let world_from_local = mesh_functions::get_world_from_local(v.instance_index);
    let world = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(v.position, 1.0));
    let eye = (mesh_functions::get_local_from_world(v.instance_index) * vec4<f32>(view.world_position, 1.0)).xyz;
    var out: VertexOutput;
    out.clip_position = position_world_to_clip(world.xyz);
    out.dir = v.position - eye;
    return out;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let z = max(-in.dir.z, 1e-4);
    let px = CX + FOCAL * in.dir.x / z;
    let py = CY - VSCALE * FOCAL * in.dir.y / z;
    let size = vec2<f32>(textureDimensions(hud_texture));
    let c = textureSampleLevel(hud_texture, hud_sampler, vec2<f32>(px, py) / size, 0.0);
    let inside = px >= 0.0 && py >= 0.0 && px < size.x && py < size.y;
    if inside && c.a > 0.5 {
        return vec4<f32>(c.rgb, 1.0);
    }
    if tint.a <= 0.0 {
        discard;
    }
    return tint;
}
