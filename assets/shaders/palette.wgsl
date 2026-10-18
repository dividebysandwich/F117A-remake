// Palette-indexed rendering that reproduces VGAME's MCGA driver (docs/re/visuals.md):
// colour words (high byte = fill mode), per-object distance haze down 16-colour ramps,
// and the sky/ground band gradient (background sphere and AND/OR terrain overlays).

#import bevy_pbr::{
    mesh_functions,
    mesh_view_bindings::{view, lights},
    view_transformations::position_world_to_clip,
    shadows::fetch_directional_shadow,
}

const MAX_LOCAL_LIGHTS: u32 = 32u;
const LIGHT_SHADOWS: u32 = 1u;
const LIGHT_LOCAL: u32 = 2u;

struct Globals {
    // Live palette (linear RGB).
    palette: array<vec4<f32>, 256>,
    // Sky band boundaries (16, packed 4 per vec4), in original screen units at f = 256.
    sky_ring: array<vec4<f32>, 4>,
    // Ground band boundaries below the horizon (16), same units; depend on altitude.
    ground_g: array<vec4<f32>, 4>,
    // x: haze enabled, y: metres per world unit, z: FLIR, w: modern mode
    params: vec4<f32>,
    // Direction to the sun (xyz), strength (w).
    sun: vec4<f32>,
    // Ambient (rgb), lighting flags (w).
    ambient: vec4<f32>,
    // x: local light count.
    light_info: vec4<f32>,
    // x: XR dot angular size in radians (0 = flat screen).
    dots: vec4<f32>,
    // Two entries per light: (position, range), (colour * intensity, unused).
    local_lights: array<vec4<f32>, 64>,
};

struct PaletteMaterial {
    // Colour word: low byte = palette index, high byte = fill mode (FF solid, FE AND, FD OR,
    // FC screen door, FB invisible, < FB two-colour checkerboard).
    word: u32,
    // bit 0: background sphere (sky/ground gradient), bit 1: line/dot (always solid low byte)
    flags: u32,
    _pad0: u32,
    _pad1: u32,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> material: PaletteMaterial;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var<storage, read> globals: Globals;

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) @interpolate(flat) fog: u32,
    // Unrounded fog level (modern mode: smooth haze between ramp entries).
    @location(2) @interpolate(flat) fog_f: f32,
};

// Dot lights: size relative to one original pixel (320x200 screen) and lower bound in pixels.
const DOT_SIZE: f32 = 0.6;
const DOT_MIN_PIXELS: f32 = 2.0;

// World units per model unit, by level code in the mesh tag (bits 0..2):
// 0: LOD0 (0.5), 1: LOD1 (1), 2: LOD2 (4), 3: LOD3 (16), 4: LOD4 (64), 5: aircraft (1/8)
fn level_unit(code: u32) -> f32 {
    switch code {
        case 0u: { return 0.5; }
        case 1u: { return 1.0; }
        case 2u: { return 4.0; }
        case 3u: { return 16.0; }
        case 4u: { return 64.0; }
        default: { return 0.125; }
    }
}

@vertex
fn vertex(v: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(v.instance_index);
    let world = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(v.position, 1.0));
    out.world_position = world.xyz;
    out.clip_position = position_world_to_clip(world.xyz);
    let tag = mesh_functions::get_tag(v.instance_index);

    // Dot lights: the unit quad expanded around the origin to a constant size in this view's
    // pixels (so the tracking camera's zoom doesn't magnify them); in XR at most the original
    // pixel's angular size.
    if (((tag >> 7u) & 1u) == 1u) {
        let center = world_from_local[3].xyz;
        let clip = position_world_to_clip(center);
        let vp = view.viewport.zw;
        var px = max(vp.y / 200.0 * DOT_SIZE, DOT_MIN_PIXELS);
        if (globals.dots.x > 0.0) {
            px = min(px, max(globals.dots.x * 0.5 * vp.y * view.clip_from_view[1][1], DOT_MIN_PIXELS));
        }
        out.world_position = center;
        out.clip_position = clip + vec4<f32>(v.position.xy * px * 2.0 / vp * clip.w, 0.0, 0.0);
    }

    // Per-object haze (2AA0:0120): fog = clamp((depth/512 in object units) - sizeClass, 0, 31) >> 1
    let size_class = (tag >> 3u) & 7u;
    let no_fog = (tag >> 6u) & 1u;
    let origin = world_from_local[3].xyz;
    let forward = -view.world_from_view[2].xyz;
    let depth_m = max(dot(origin - view.world_position, forward), 0.0);
    let depth_units = depth_m / (globals.params.y * level_unit(tag & 7u));
    let d = i32(depth_units / 512.0) - i32(size_class);
    var fog = u32(clamp(d, 0, 31)) >> 1u;
    var fog_f = clamp(depth_units / 512.0 - f32(size_class), 0.0, 31.0) / 2.0;
    if (no_fog == 1u || globals.params.x == 0.0) {
        fog = 0u;
        fog_f = 0.0;
    }
    out.fog = fog;
    out.fog_f = fog_f;
    return out;
}

fn ring(arr: array<vec4<f32>, 4>, i: u32) -> f32 {
    let v = arr[i >> 2u];
    switch i & 3u {
        case 0u: { return v.x; }
        case 1u: { return v.y; }
        case 2u: { return v.z; }
        default: { return v.w; }
    }
}

// Band index (0..15) of the sky (elevation >= 0) or ground (below the horizon) for a view
// direction, using the original's screen-unit band boundaries: R = 256 * tan(angle).
fn band(dir: vec3<f32>) -> vec2<u32> {
    let horiz = max(length(dir.xz), 1e-6);
    let r = 256.0 * abs(dir.y) / horiz;
    var k = 0u;
    if (dir.y >= 0.0) {
        let sky = globals.sky_ring;
        for (var i = 1u; i < 16u; i++) {
            if (r >= ring(sky, i)) { k = i; }
        }
        return vec2<u32>(0x80u + k, 1u);
    }
    let g = globals.ground_g;
    for (var i = 1u; i < 16u; i++) {
        if (r >= ring(g, i)) { k = i; }
    }
    return vec2<u32>(0x70u + k, 0u);
}

// Position within the bands: (band colour base, band k, fraction towards band k + 1).
fn band_f(dir: vec3<f32>) -> vec3<f32> {
    let horiz = max(length(dir.xz), 1e-6);
    let r = 256.0 * abs(dir.y) / horiz;
    var arr = globals.ground_g;
    var base = 0x70u;
    if (dir.y >= 0.0) {
        arr = globals.sky_ring;
        base = 0x80u;
    }
    var k = 0u;
    for (var i = 1u; i < 16u; i++) {
        if (r >= ring(arr, i)) { k = i; }
    }
    var f = 0.0;
    if (k < 15u) {
        let a = ring(arr, k);
        let b = ring(arr, k + 1u);
        if (b > a) { f = clamp((r - a) / (b - a), 0.0, 1.0); }
    }
    return vec3<f32>(f32(base), f32(k), f);
}

fn pal(i: u32) -> vec3<f32> {
    return globals.palette[i & 0xFFu].rgb;
}

fn haze(idx: u32, fog: u32) -> u32 {
    if (idx < 0xA0u || fog == 0u) {
        return idx;
    }
    return max(idx & 0xF0u, idx - min(fog, idx));
}

// Modern mode: haze between ramp entries.
fn haze_smooth(idx: u32, fog: f32) -> vec3<f32> {
    if (idx < 0xA0u || fog <= 0.0) {
        return pal(idx);
    }
    let lo = haze(idx, u32(floor(fog)));
    let hi = haze(idx, u32(floor(fog)) + 1u);
    return mix(pal(lo), pal(hi), fract(fog));
}

// Modern mode: subtle flat shading from the original's (unused) light vector (-36, 36, 36).
fn shade(world: vec3<f32>, dir: vec3<f32>) -> f32 {
    var n = normalize(cross(dpdx(world), dpdy(world)));
    if (dot(n, dir) > 0.0) { n = -n; }
    let l = normalize(vec3<f32>(-36.0, 36.0, 36.0));
    return 0.86 + 0.14 * dot(n, l);
}

// Lighting options: the palette colour lit by the sun (with shadow maps) and local lights,
// using the flat face normal. Unlit parts keep roughly the original brightness.
fn lit(rgb: vec3<f32>, world: vec3<f32>, dir: vec3<f32>, clip: vec4<f32>, flags: u32) -> vec3<f32> {
    var n = normalize(cross(dpdx(world), dpdy(world)));
    if (dot(n, dir) > 0.0) { n = -n; }
    var light = globals.ambient.rgb;
    if ((flags & LIGHT_SHADOWS) != 0u) {
        let ndl = max(dot(n, globals.sun.xyz), 0.0);
        var shadow = 1.0;
        if (lights.n_directional_lights > 0u && ndl > 0.0) {
            let view_z = (view.view_from_world * vec4<f32>(world, 1.0)).z;
            shadow = fetch_directional_shadow(0u, vec4<f32>(world, 1.0), n, view_z, clip.xy);
        }
        light += vec3<f32>(globals.sun.w * ndl * shadow);
    } else {
        light = vec3<f32>(1.0);
    }
    if ((flags & LIGHT_LOCAL) != 0u) {
        let count = min(u32(globals.light_info.x), MAX_LOCAL_LIGHTS);
        for (var i = 0u; i < count; i++) {
            let p = globals.local_lights[2u * i];
            let c = globals.local_lights[2u * i + 1u];
            let to_light = p.xyz - world;
            let d = length(to_light);
            if (d < p.w) {
                let att = (1.0 - d / p.w) * (1.0 - d / p.w);
                let ndl = max(dot(n, to_light / max(d, 1e-3)), 0.0) * 0.7 + 0.3;
                light += c.rgb * att * ndl;
            }
        }
    }
    return rgb * light;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let dir = in.world_position - view.world_position;
    let modern = globals.params.w != 0.0;
    if ((material.flags & 1u) != 0u) {
        // Background: sky bands above the horizon, ground gradient below.
        if ((material.flags & 4u) != 0u) {
            return vec4<f32>(pal(select(0x7Cu, 0x88u, dir.y >= 0.0)), 1.0);
        }
        if (modern) {
            let b = band_f(dir);
            let k = u32(b.y);
            let k1 = min(k + 1u, 15u);
            return vec4<f32>(mix(pal(u32(b.x) + k), pal(u32(b.x) + k1), b.z), 1.0);
        }
        return vec4<f32>(pal(band(dir).x), 1.0);
    }

    let word = material.word;
    let hi = (word >> 8u) & 0xFFu;
    let lo = word & 0xFFu;
    let px = vec2<u32>(in.clip_position.xy);
    let is_line = (material.flags & 2u) != 0u;
    var rgb: vec3<f32>;
    if (is_line || hi == 0xFFu) {
        if (modern) {
            rgb = haze_smooth(lo, in.fog_f);
        } else {
            rgb = pal(haze(lo, in.fog));
        }
    } else if (hi == 0xFBu) {
        discard;
    } else if (hi == 0xFCu) {
        if (((px.x + px.y) & 1u) == 1u) { discard; }
        rgb = pal(lo);
    } else if (hi == 0xFEu || hi == 0xFDu) {
        // AND / OR with the ground gradient underneath.
        if (modern) {
            let b = band_f(dir);
            let k = u32(b.x) + u32(b.y);
            let k1 = u32(b.x) + min(u32(b.y) + 1u, 15u);
            let c0 = select(k | lo, k & lo, hi == 0xFEu);
            let c1 = select(k1 | lo, k1 & lo, hi == 0xFEu);
            rgb = mix(pal(c0), pal(c1), b.z);
        } else {
            rgb = pal(select(band(dir).x | lo, band(dir).x & lo, hi == 0xFEu));
        }
    } else {
        // Two-colour checkerboard dither (modern: the average of both colours).
        if (modern) {
            rgb = (pal(lo) + pal(hi)) * 0.5;
        } else {
            rgb = pal(select(lo, hi, ((px.x + px.y) & 1u) == 1u));
        }
    }
    let flags = u32(globals.ambient.w);
    if (!is_line && flags != 0u) {
        rgb = lit(rgb, in.world_position, dir, in.clip_position, flags);
    } else if (modern && !is_line) {
        rgb = rgb * shade(in.world_position, dir);
    }
    return vec4<f32>(rgb, 1.0);
}
