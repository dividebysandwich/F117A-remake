// Smoke trails and contrails (trails.rs): ribbons of segments along the trail's centre line,
// each vertex pushed sideways (perpendicular to the trail and to the eye ray) in the vertex
// shader, so the segments face whichever view draws them (main view, XR eyes, tracking camera).

#import bevy_pbr::{
    mesh_functions,
    mesh_view_bindings::view,
    view_transformations::position_world_to_clip,
}

// Leading part of the shared palette globals (palette_material.rs `Globals`).
struct Globals {
    palette: array<vec4<f32>, 256>,
    sky_ring: array<vec4<f32>, 4>,
    ground_g: array<vec4<f32>, 4>,
    params: vec4<f32>,
    sun: vec4<f32>,
    ambient: vec4<f32>,
    // y: daylight 0 (night) .. 1 (day), z: night vision goggles.
    light_info: vec4<f32>,
    // zw: tracking camera picture size (FLIR view).
    dots: vec4<f32>,
};

// Brightness of the trails at night (unlit smoke reads as grey against the dark sky).
const NIGHT: f32 = 0.22;

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

// Night vision goggles (light_info.z): the image intensified to a green phosphor picture,
// with more gain the darker it is, a contrast curve and a little grain.
fn nvg(rgb: vec3<f32>, clip: vec4<f32>) -> vec3<f32> {
    if (globals.light_info.z == 0.0) { return rgb; }
    let lum = dot(rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    let gain = mix(14.0, 2.5, clamp(globals.light_info.y, 0.0, 1.0));
    var v = sqrt(1.0 - exp(-lum * gain));
    v = smoothstep(0.12, 0.95, v);
    let p = vec2<u32>(clip.xy);
    var h = p.x * 0x27d4eb2du ^ p.y * 0x165667b1u ^ u32(globals.dots.y * 60.0) * 0x9e3779b9u;
    h = (h ^ (h >> 15u)) * 0x2c1b3c6du;
    v = clamp(v + (f32(h >> 8u) / 16777216.0 - 0.5) * 0.06, 0.0, 1.0);
    // Phosphor green, near white in the highlights; sRGB -> linear.
    let c = mix(vec3<f32>(0.0, 0.04, 0.01), vec3<f32>(0.3, 0.85, 0.2), v) + vec3<f32>(0.5, 0.15, 0.4) * v * v * v;
    return pow(min(c, vec3<f32>(1.0)), vec3<f32>(2.2));
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // Soft edges across the ribbon.
    let edge = 1.0 - in.side.x * in.side.x;
    let a = clamp(in.side.y, 0.0, 1.0) * edge * material.params.y;
    if (a <= 0.003) {
        discard;
    }
    // FLIR (tracking camera): cool grey smoke.
    if (globals.params.z != 0.0 && all(abs(view.viewport.zw - globals.dots.zw) < vec2<f32>(0.5))) {
        return vec4<f32>(vec3<f32>(0.12), a);
    }
    let light = mix(NIGHT, 1.0, clamp(globals.light_info.y, 0.0, 1.0));
    return vec4<f32>(nvg(globals.palette[u32(material.params.x) & 0xFFu].rgb * light, in.clip_position), a);
}
