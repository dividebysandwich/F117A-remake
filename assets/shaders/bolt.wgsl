// Lightning channels (weather.rs): ribbons along each segment of the channel, pushed sideways
// in the vertex shader so they face whichever view draws them, at least a couple of pixels
// wide however far away. Additive: white at the core of each branch, falling off into blue
// at its edges.

#import bevy_pbr::{
    mesh_functions,
    mesh_view_bindings::view,
    view_transformations::position_world_to_clip,
}

struct BoltMaterial {
    // x: brightness 0..1 (the stroke timeline), y: minimum half width in radians.
    params: vec4<f32>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> material: BoltMaterial;

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    // Point on the channel.
    @location(0) position: vec3<f32>,
    // Segment direction.
    @location(1) tangent: vec3<f32>,
    // x: signed half width (metres), y: branch brightness 0..1.
    @location(2) side: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    // x: -1 .. 1 across the ribbon, y: branch brightness.
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
        s = cross(t, vec3<f32>(0.0, 1.0, 0.0));
    }
    s = normalize(s);
    let w = max(abs(v.side.x), length(to_eye) * material.params.y);
    var out: VertexOutput;
    out.clip_position = position_world_to_clip(centre + s * w * sign(v.side.x));
    out.side = vec2<f32>(sign(v.side.x), v.side.y);
    return out;
}

const CORE: vec3<f32> = vec3<f32>(1.0, 1.0, 1.0);
const EDGE: vec3<f32> = vec3<f32>(0.35, 0.5, 1.0);

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let u = abs(in.side.x);
    let a = (1.0 - smoothstep(0.3, 1.0, u)) * material.params.x * in.side.y;
    if (a <= 0.003) {
        discard;
    }
    // White hot core, blue glow towards the edges.
    let rgb = mix(CORE * 1.6, EDGE, smoothstep(0.12, 0.7, u));
    return vec4<f32>(rgb, a);
}
