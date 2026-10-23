// Cockpit materials (cockpit3d/shading.rs): Bevy's forward StandardMaterial fragment shader in
// three variants. COCKPIT_UNLIT leaves out the PBR lighting; COCKPIT_SIMPLE_LIT (lit materials
// without local lights) does only what is active then: the directional lights (with their
// shadow maps with COCKPIT_SHADOWS), the ambient light and emissive, with Bevy's own BRDF;
// COCKPIT_FULL is Bevy's full lighting (point lights, shadows).

#import bevy_pbr::{
    mesh_view_bindings::globals,
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, main_pass_post_lighting_processing},
    forward_io::{VertexOutput, FragmentOutput},
}
#ifdef COCKPIT_UNLIT
#import bevy_pbr::pbr_bindings
#endif
#ifdef COCKPIT_SIMPLE_LIT
#import bevy_pbr::{
    pbr_types::PbrInput,
    pbr_functions::{calculate_diffuse_color, calculate_F0, calculate_F0_dielectric},
    lighting,
    lighting::LAYER_BASE,
    ambient,
    mesh_view_bindings as view_bindings,
}
#endif
#ifdef COCKPIT_SHADOWS
#import bevy_pbr::shadows
#import bevy_pbr::mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT
#import bevy_pbr::mesh_types::MESH_FLAGS_SHADOW_RECEIVER_BIT
#endif
#ifdef COCKPIT_FULL
#import bevy_pbr::{
    pbr_functions::apply_pbr_lighting,
    pbr_types::STANDARD_MATERIAL_FLAGS_UNLIT_BIT,
}
#endif

// x: darkness of a night without moonlight 0..1, y: night vision goggles, z: daylight 0..1,
// w: classic shaders (shading.rs `CockpitShading::night`).
@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> night: vec4<f32>;

#ifdef COCKPIT_SHADOWS
// Classic shaders: a hard edged shadow (the nearest shadow map texel), like the world's
// (palette.wgsl `sun_shadow`).
fn hard_shadow(i: u32, world: vec4<f32>, n: vec3<f32>, view_z: f32) -> f32 {
    let light = &view_bindings::lights.directional_lights[i];
    let ci = shadows::get_cascade_index(i, view_z);
    if (ci >= (*light).num_cascades) {
        return 1.0;
    }
    let cascade = &(*light).cascades[ci];
    let p = world.xyz + (*light).shadow_normal_bias * (*cascade).texel_size * n
        + (*light).shadow_depth_bias * (*light).direction_to_light;
    let l = shadows::world_to_directional_light_local(i, ci, vec4<f32>(p, 1.0));
    if (l.w == 0.0) {
        return 1.0;
    }
    let size = vec2<f32>(textureDimensions(view_bindings::directional_shadow_textures));
#ifdef NO_ARRAY_TEXTURES_SUPPORT
    let d = textureLoad(view_bindings::directional_shadow_textures, vec2<i32>(l.xy * size), 0);
#else
    let d = textureLoad(view_bindings::directional_shadow_textures, vec2<i32>(l.xy * size), i32((*light).depth_texture_base_index + ci), 0);
#endif
    return select(0.0, 1.0, l.z >= d);
}
#endif

#ifdef COCKPIT_SIMPLE_LIT
// apply_pbr_lighting (bevy_pbr/src/render/pbr_functions.wgsl) for directional lights without
// shadows and the ambient light.
fn simple_lighting(in: PbrInput) -> vec4<f32> {
    let base = in.material.base_color;
    let metallic = in.material.metallic;
    let perceptual_roughness = in.material.perceptual_roughness;
    let NdotV = max(dot(in.N, in.V), 0.0001);
    let diffuse_color = calculate_diffuse_color(base.rgb, metallic, in.material.specular_transmission, in.material.diffuse_transmission);
    let F0 = calculate_F0(base.rgb, metallic, in.material.reflectance);

    var input: lighting::LightingInput;
    input.layers[LAYER_BASE].NdotV = NdotV;
    input.layers[LAYER_BASE].N = in.N;
    input.layers[LAYER_BASE].R = reflect(-in.V, in.N);
    input.layers[LAYER_BASE].perceptual_roughness = perceptual_roughness;
    input.layers[LAYER_BASE].roughness = lighting::perceptualRoughnessToRoughness(perceptual_roughness);
    input.P = in.world_position.xyz;
    input.V = in.V;
    input.diffuse_color = diffuse_color;
    input.metallic = metallic;
    input.F0_dielectric = calculate_F0_dielectric(in.material.reflectance);
    input.F0_metallic = base.rgb;
    input.F_ab = lighting::F_AB(perceptual_roughness, NdotV);

    var direct = vec3<f32>(0.0);
    for (var i = 0u; i < view_bindings::lights.n_directional_lights; i++) {
        var shadow = 1.0;
#ifdef COCKPIT_SHADOWS
        if ((in.flags & MESH_FLAGS_SHADOW_RECEIVER_BIT) != 0u
                && (view_bindings::lights.directional_lights[i].flags & DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u
                && dot(in.N, view_bindings::lights.directional_lights[i].direction_to_light) > 0.0) {
            let view_z = (view_bindings::view.view_from_world * in.world_position).z;
            if (night.w != 0.0) {
                shadow = hard_shadow(i, in.world_position, in.world_normal, view_z);
            } else {
                shadow = shadows::fetch_directional_shadow(i, in.world_position, in.world_normal, view_z, in.frag_coord.xy);
            }
        }
#endif
        direct += lighting::directional_light(i, &input, true) * shadow;
    }
    let indirect = ambient::ambient_light(in.world_position, in.N, in.V, NdotV, diffuse_color, F0, perceptual_roughness, in.diffuse_occlusion);
    let emissive = in.material.emissive;
    let emissive_light = emissive.rgb * base.a * mix(1.0, view_bindings::view.exposure, emissive.a);
    return vec4<f32>(view_bindings::view.exposure * (direct + indirect) * (1.0 - night.x) + emissive_light, base.a);
}
#endif

// Night vision goggles: `nvg` in palette.wgsl, the cockpit as the goggles show it.
fn nvg(rgb: vec3<f32>, frag: vec4<f32>) -> vec3<f32> {
    if (night.y == 0.0) { return rgb; }
    let lum = dot(rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    let gain = mix(14.0, 2.5, clamp(night.z, 0.0, 1.0));
    var v = sqrt(1.0 - exp(-lum * gain));
    v = smoothstep(0.12, 0.95, v);
    let p = vec2<u32>(frag.xy);
    var h = p.x * 0x27d4eb2du ^ p.y * 0x165667b1u ^ u32(globals.time * 60.0) * 0x9e3779b9u;
    h = (h ^ (h >> 15u)) * 0x2c1b3c6du;
    v = clamp(v + (f32(h >> 8u) / 16777216.0 - 0.5) * 0.06, 0.0, 1.0);
    let c = mix(vec3<f32>(0.0, 0.04, 0.01), vec3<f32>(0.3, 0.85, 0.2), v) + vec3<f32>(0.5, 0.15, 0.4) * v * v * v;
    return pow(min(c, vec3<f32>(1.0)), vec3<f32>(2.2));
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);
    var out: FragmentOutput;
    // A dark night: unlit surfaces go dark but where they glow (lamps, screens, labels); lit
    // ones lose the sun and the ambient light but keep the cockpit lamps' light.
    let dark = night.x;
#ifdef COCKPIT_UNLIT
    out.color = pbr_input.material.base_color;
    // Bevy leaves the emissive texture out of unlit materials.
    var e = pbr_bindings::material.emissive.rgb;
#ifdef VERTEX_UVS_A
    e *= textureSample(pbr_bindings::emissive_texture, pbr_bindings::emissive_sampler, in.uv).rgb;
#endif
    let glow = clamp(max(max(e.r, e.g), e.b) * 8.0, 0.0, 1.0);
    out.color = vec4<f32>(out.color.rgb * mix(1.0 - dark, 1.0, glow), out.color.a);
#endif
#ifdef COCKPIT_SIMPLE_LIT
    out.color = simple_lighting(pbr_input);
#endif
#ifdef COCKPIT_FULL
    if (pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_UNLIT_BIT) == 0u {
        pbr_input.diffuse_occlusion *= 1.0 - dark;
        pbr_input.specular_occlusion *= 1.0 - dark;
        out.color = apply_pbr_lighting(pbr_input);
    } else {
        out.color = pbr_input.material.base_color;
    }
#endif
    out.color = vec4<f32>(nvg(out.color.rgb, in.position), out.color.a);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
