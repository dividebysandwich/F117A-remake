// Smoke trails and contrails (trails.rs): ribbons of segments along the trail's centre line,
// each vertex pushed sideways (perpendicular to the trail and to the eye ray) in the vertex
// shader, so the segments face whichever view draws them (main view, XR eyes, tracking camera).

#import bevy_pbr::{
    mesh_functions,
    mesh_view_bindings::view,
    view_transformations::position_world_to_clip,
}

// Leading part of the shared palette globals (palette_material.rs `Globals`): the live palette.
struct Globals {
    palette: array<vec4<f32>, 256>,
};

struct TrailMaterial {
    // x: palette index, y: opacity.
    params: vec4<f32>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> material: TrailMaterial;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var<storage, read> globals: Globals;

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    // Point on the centre line.
    @location(0) position: vec3<f32>,
    // Trail direction at that point.
    @location(1) tangent: vec3<f32>,
    // x: signed half width (metres), y: alpha.
    @location(2) side: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    // x: -1 .. 1 across the ribbon, y: alpha.
    @location(0) side: vec2<f32>,
};

@vertex
fn vertex(v: Vertex) -> VertexOutput {
    let world_from_local = mesh_functions::get_world_from_local(v.instance_index);
    let centre = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(v.position, 1.0)).xyz;
    let t = normalize((world_from_local * vec4<f32>(v.tangent, 0.0)).xyz);
    let to_eye = view.world_position - centre;
    var s = cross(t, to_eye);
    if (dot(s, s) < 1e-8) {
        // Looking straight along the trail: any perpendicular will do.
        s = cross(t, vec3<f32>(0.0, 1.0, 0.0));
    }
    s = normalize(s);
    var out: VertexOutput;
    out.clip_position = position_world_to_clip(centre + s * v.side.x);
    out.side = vec2<f32>(sign(v.side.x), v.side.y);
    return out;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // Soft edges across the ribbon.
    let edge = 1.0 - in.side.x * in.side.x;
    let a = clamp(in.side.y, 0.0, 1.0) * edge * material.params.y;
    if (a <= 0.003) {
        discard;
    }
    return vec4<f32>(globals.palette[u32(material.params.x) & 0xFFu].rgb, a);
}
