// Upsamples the clouds marched at a reduced resolution (premultiplied colour and coverage)
// over the view.

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

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    // The cloud texture is rounded up: only part of it covers the view.
    let used = view.viewport.zw / globals.params2.x;
    let uv = in.uv * used / vec2<f32>(textureDimensions(clouds));
    let c = textureSampleLevel(clouds, clouds_sampler, uv, 0.0);
    if (c.a <= 0.0) { discard; }
    return c;
}
