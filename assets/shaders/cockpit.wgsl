// Cockpit materials (cockpit3d/shading.rs): Bevy's forward StandardMaterial fragment shader in
// three variants. COCKPIT_UNLIT leaves out the PBR lighting; COCKPIT_SIMPLE_LIT (lit materials
// without the lighting options) does only what is active then: the directional lights without
// shadows, the ambient light and emissive, with Bevy's own BRDF; COCKPIT_FULL is Bevy's full
// lighting (point lights, shadows).

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, main_pass_post_lighting_processing},
    forward_io::{VertexOutput, FragmentOutput},
}
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
#ifdef COCKPIT_FULL
#import bevy_pbr::{
    pbr_functions::apply_pbr_lighting,
    pbr_types::STANDARD_MATERIAL_FLAGS_UNLIT_BIT,
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
        direct += lighting::directional_light(i, &input, true);
    }
    let indirect = ambient::ambient_light(in.world_position, in.N, in.V, NdotV, diffuse_color, F0, perceptual_roughness, in.diffuse_occlusion);
    let emissive = in.material.emissive;
    let emissive_light = emissive.rgb * base.a * mix(1.0, view_bindings::view.exposure, emissive.a);
    return vec4<f32>(view_bindings::view.exposure * (direct + indirect) + emissive_light, base.a);
}
#endif

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);
    var out: FragmentOutput;
#ifdef COCKPIT_UNLIT
    out.color = pbr_input.material.base_color;
#endif
#ifdef COCKPIT_SIMPLE_LIT
    out.color = simple_lighting(pbr_input);
#endif
#ifdef COCKPIT_FULL
    if (pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_UNLIT_BIT) == 0u {
        out.color = apply_pbr_lighting(pbr_input);
    } else {
        out.color = pbr_input.material.base_color;
    }
#endif
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
