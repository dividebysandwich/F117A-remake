// Upsamples the marched clouds (premultiplied colour and coverage) over the view, and writes
// the depth at which they turn opaque into the view's depth buffer (`depth`).

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput
#import bevy_render::view::View

struct CloudGlobals {
    sun: vec4<f32>,
    params0: vec4<f32>,
    params1: vec4<f32>,
    // Resolution scale (view pixels per cloud pixel).
    params2: vec4<f32>,
}

@group(0) @binding(0) var clouds: texture_2d<f32>;
@group(0) @binding(1) var clouds_sampler: sampler;
@group(0) @binding(2) var<uniform> view: View;
@group(0) @binding(3) var<uniform> globals: CloudGlobals;
@group(0) @binding(4) var distance: texture_2d<f32>;

const NO_CLOUD: f32 = 65504.0;

struct DepthOut {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
}

// Reverse infinite z: depth = near / view z. Colour is not written.
@fragment
fn depth(in: FullscreenVertexOutput) -> DepthOut {
    let texel = vec2<i32>((in.position.xy - view.viewport.xy) / globals.params2.x);
    let z = textureLoad(distance, texel, 0).r;
    if (z >= NO_CLOUD) { discard; }
    let near = view.clip_from_view[3][2];
    var out: DepthOut;
    out.color = vec4<f32>(0.0);
    out.depth = near / max(z, near);
    return out;
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    // The cloud texture is rounded up: only part of it covers the view.
    let used = view.viewport.zw / globals.params2.x;
    let uv = in.uv * used / vec2<f32>(textureDimensions(clouds));
    let c = textureSampleLevel(clouds, clouds_sampler, uv, 0.0);
    if (c.a <= 0.0) { discard; }
    return c;
}
