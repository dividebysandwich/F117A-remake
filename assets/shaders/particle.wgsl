// Explosion particles (explosions.rs): camera-facing quads batched into one mesh, each vertex
// pushed out along the view's right / up axes in the vertex shader so the quads face whichever
// view draws them (main view, XR eyes, tracking camera). Soft round puffs with a little noise:
// fireballs glow, smoke and dust take the daylight.

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
    // w: modern shading.
    params: vec4<f32>,
    sun: vec4<f32>,
    ambient: vec4<f32>,
    // y: daylight 0 (night) .. 1 (day), z: night vision goggles.
    light_info: vec4<f32>,
    // zw: tracking camera picture size (FLIR view).
    dots: vec4<f32>,
};

// Brightness of unlit smoke at night.
const NIGHT: f32 = 0.18;

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> unused: vec4<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var<storage, read> globals: Globals;

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    // Particle centre.
    @location(0) position: vec3<f32>,
    // xy: corner (-1 .. 1), z: radius (metres), w: rotation (radians).
    @location(1) corner: vec4<f32>,
    // Linear colour and alpha.
    @location(2) colour: vec4<f32>,
    // x: glow 0 (lit by daylight) .. 1 (emissive), y: noise seed.
    @location(3) look: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) colour: vec4<f32>,
    @location(2) look: vec2<f32>,
};

@vertex
fn vertex(v: Vertex) -> VertexOutput {
    let world_from_local = mesh_functions::get_world_from_local(v.instance_index);
    let centre = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(v.position, 1.0)).xyz;
    let right = view.world_from_view[0].xyz;
    let up = view.world_from_view[1].xyz;
    let c = cos(v.corner.w);
    let s = sin(v.corner.w);
    let k = vec2<f32>(v.corner.x * c - v.corner.y * s, v.corner.x * s + v.corner.y * c) * v.corner.z;
    var out: VertexOutput;
    out.clip_position = position_world_to_clip(centre + right * k.x + up * k.y);
    out.uv = v.corner.xy;
    out.colour = v.colour;
    out.look = v.look;
    return out;
}

fn hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash(i), hash(i + vec2<f32>(1.0, 0.0)), u.x), mix(hash(i + vec2<f32>(0.0, 1.0)), hash(i + vec2<f32>(1.0, 1.0)), u.x), u.y);
}

// Night vision goggles (light_info.z), as in trail.wgsl.
fn nvg(rgb: vec3<f32>) -> vec3<f32> {
    if (globals.light_info.z == 0.0) { return rgb; }
    let lum = dot(rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    let gain = mix(14.0, 2.5, clamp(globals.light_info.y, 0.0, 1.0));
    let v = smoothstep(0.12, 0.95, sqrt(1.0 - exp(-lum * gain)));
    let c = mix(vec3<f32>(0.0, 0.04, 0.01), vec3<f32>(0.3, 0.85, 0.2), v) + vec3<f32>(0.5, 0.15, 0.4) * v * v * v;
    return pow(min(c, vec3<f32>(1.0)), vec3<f32>(2.2));
}

// 4x4 ordered dither threshold (classic mode), as in palette.wgsl.
fn bayer4(p: vec2<u32>) -> f32 {
    var m = array<f32, 16>(0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0);
    return (m[(p.y & 3u) * 4u + (p.x & 3u)] + 0.5) / 16.0;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let r = length(in.uv);
    if (r >= 1.0) {
        discard;
    }
    // Lumpy edge: the radius at which the puff fades varies with the angle and the seed.
    let n = noise(in.uv * 2.6 + vec2<f32>(in.look.y * 17.0, in.look.y * 31.0));
    let edge = 0.55 + 0.45 * n;
    var a = in.colour.a * (1.0 - smoothstep(edge * 0.55, edge, r));
    if (a <= 0.004) {
        discard;
    }
    let glow = in.look.x;
    // Fireballs are hotter (whiter) in the middle.
    var rgb = in.colour.rgb * (1.0 + glow * (1.0 - r) * 0.9);
    // Lit puffs: darker underside / noisy shading, dimmed at night.
    let light = mix(NIGHT, 1.0, clamp(globals.light_info.y, 0.0, 1.0));
    rgb = mix(rgb * light * (0.8 + 0.35 * n + 0.15 * in.uv.y), rgb, glow);
    // FLIR (tracking camera): hot fire white, smoke a cool grey.
    if (globals.params.z != 0.0 && all(abs(view.viewport.zw - globals.dots.zw) < vec2<f32>(0.5))) {
        rgb = mix(vec3<f32>(0.12), vec3<f32>(1.0), glow);
    } else {
        rgb = nvg(rgb);
    }
    // Classic shading: no blending, an ordered dither of the coverage like the plumes.
    if (globals.params.w == 0.0) {
        if (a < bayer4(vec2<u32>(in.clip_position.xy) + vec2<u32>(1u, 3u))) {
            discard;
        }
        a = 1.0;
    }
    return vec4<f32>(rgb, a);
}
