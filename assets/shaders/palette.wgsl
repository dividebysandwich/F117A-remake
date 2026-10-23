// Palette-indexed rendering that reproduces classic MCGA drawing:
// colour words (high byte = fill mode), per-object distance haze down 16-colour ramps,
// and the sky/ground band gradient (background sphere and AND/OR terrain overlays).

#import bevy_pbr::{
    mesh_functions,
    mesh_view_bindings::{view, lights, directional_shadow_textures, directional_shadow_textures_comparison_sampler},
    view_transformations::position_world_to_clip,
    mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT,
}

const MAX_LOCAL_LIGHTS: u32 = 32u;
const LIGHT_SHADOWS: u32 = 1u;
const LIGHT_LOCAL: u32 = 2u;
const LIGHT_MOON_KEY: u32 = 4u;
const FLAG_CLOUD: u32 = 8u;
// Smoke plume volume (fires.rs), optionally lit by a fire at its base.
const TAG_SMOKE: u32 = 8192u;
const TAG_SMOKE_FIRE: u32 = 16384u;
// Warm object (palette_material.rs `TAG_WARM`).
const TAG_WARM: u32 = 2048u;
const TAG_DOT_NEAR: u32 = 4096u;
// Aircraft spotting dot (palette_material.rs `TAG_DOT_SPOT`).
const TAG_DOT_SPOT: u32 = 32768u;
// Dot only drawn from DOT_FAR_MIN metres away (palette_material.rs `TAG_DOT_FAR`).
const TAG_DOT_FAR: u32 = 2147483648u;
const DOT_FAR_MIN: f32 = 3000.0;
// Share of a moonless night's darkness on aircraft and their spotting dots: barely visible.
const AIRCRAFT_NIGHT: f32 = 0.95;
// Cloud colours: the grey ramp (its base entry is the horizon haze colour).
const CLOUD_RAMP: u32 = 0xE0u;

struct Globals {
    // Live palette (linear RGB).
    palette: array<vec4<f32>, 256>,
    // Sky band boundaries (16, packed 4 per vec4), in 320x200 screen units at f = 256.
    sky_ring: array<vec4<f32>, 4>,
    // Ground band boundaries below the horizon (16), same units; depend on altitude.
    ground_g: array<vec4<f32>, 4>,
    // x: haze enabled, y: metres per world unit, z: FLIR (tracking camera), w: modern mode
    params: vec4<f32>,
    // Direction to the sun (xyz), strength (w).
    sun: vec4<f32>,
    // Ambient (rgb), lighting flags (w).
    ambient: vec4<f32>,
    // x: local light count, y: daylight 0 (night) .. 1 (day), z: night vision goggles,
    // w: flat colours (SM89_FLAT_SHADER bits: 1 all, 2 background, 4 objects, 8 clouds and smoke).
    light_info: vec4<f32>,
    // x: XR dot angular size in radians (0 = flat screen), y: animation clock (seconds,
    // wraps every hour), zw: tracking camera picture size (pixels).
    dots: vec4<f32>,
    // Two entries per light: (position, range), (colour * intensity, unused).
    local_lights: array<vec4<f32>, 64>,
    // Weather: deck base / top (metres, eye height frame), deck present (z), whiteout 0..1 (w).
    clouds: vec4<f32>,
    // x: haze distance scale, y: lightning flash 0..1, z: visibility under clouds (metres),
    // w: gloom 0..1 (grey, dim days under an overcast and in storms).
    weather: vec4<f32>,
    // Cloud texture: floating origin (x, z) modulo NOISE_PERIOD, seed (z); stratus layer
    // coverage 0..1 (w).
    noise: vec4<f32>,
    // Stratus layer coverage: threshold 0..1 (x), noise seed (y), floating origin x/z (z, w).
    stratus: vec4<f32>,
    // Lightning channel as a line light: one end (xyz) and intensity (w), the other end (xyz)
    // and reach in metres (w).
    bolt_a: vec4<f32>,
    bolt_b: vec4<f32>,
    // Continuous time of day: direction to the sun (xyz), on (w = 1); direction to the moon
    // (xyz), star visibility 0..1 (w).
    sky_sun: vec4<f32>,
    sky_moon: vec4<f32>,
    // x: darkness of a night without moonlight 0..1 (0 under night vision goggles), y: the
    // strength of its red horizon glow, zw: the recon cameras' viewport size.
    sky_night: vec4<f32>,
    // The recon cameras' palette (linear RGB): the day's, as in the near infrared.
    recon_palette: array<vec4<f32>, 256>,
    // Direction to the moon (xyz) and its light's strength (w), for modern shading.
    moon: vec4<f32>,
    // The key light's shadow maps: its index among Bevy's directional lights (x), number of
    // cascades (y, 0 without shadows); the cascades' far bounds.
    shadow: vec4<f32>,
    shadow_bounds: vec4<f32>,
};

struct PaletteMaterial {
    // Colour word: low byte = palette index, high byte = fill mode (FF solid, FE AND, FD OR,
    // FC screen door, FB invisible, < FB two-colour checkerboard).
    word: u32,
    // bit 0: background sphere (sky/ground gradient), bit 1: line/dot (always solid low byte)
    flags: u32,
    // The lighting options are on (the PALETTE_LIT variant; unused here).
    lit: u32,
    _pad1: u32,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> material: PaletteMaterial;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var<storage, read> globals: Globals;

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
#ifdef DOT_BATCH
    // Batched dots: four vertices at each dot's centre, spread by their quad corner.
    @location(1) corner: vec2<f32>,
#endif
#ifdef DOT_COLOUR
    // Palette colour of each dot in a batch of several colours (dots.rs `dot_batch_coloured`).
    @location(2) colour: u32,
#endif
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) @interpolate(flat) fog: u32,
    // Unrounded fog level (modern mode: smooth haze between ramp entries).
    @location(2) @interpolate(flat) fog_f: f32,
    // Cloud volumes, in the cloud's local space (unit radius, y 0..1): position (w: noise
    // offset of this cloud), eye (w: 1 cumulus, 2 tower, 3 smoke, 4 smoke over a fire), sun
    // direction, scale (metres per local unit on each axis).
    @location(3) puff_pos: vec4<f32>,
    @location(4) @interpolate(flat) puff_eye: vec4<f32>,
    @location(5) @interpolate(flat) puff_sun: vec3<f32>,
    @location(6) @interpolate(flat) puff_scale: vec3<f32>,
    // Bit 0: warm object or dot light (FLIR); bit 1: aircraft (level code 5); bit 2: spotting
    // dot.
    @location(7) @interpolate(flat) warm: u32,
#ifdef DOT_COLOUR
    @location(8) @interpolate(flat) dot_colour: u32,
#endif
};

// Dot lights: size relative to one 320x200 screen pixel and lower bound in pixels.
const DOT_SIZE: f32 = 0.6;
const DOT_MIN_PIXELS: f32 = 2.0;
// Aircraft lights (level code 5) up close: drawn as a lamp this many metres across, up to
// AIRCRAFT_DOT_MAX times their size far away.
const AIRCRAFT_DOT_METRES: f32 = 0.3;
const AIRCRAFT_DOT_MAX: f32 = 5.0;
// Dot lights are seen up to DOT_RANGE metres, or DOT_VIS_FACTOR times the weather's visibility
// if less (lights carry further than the visibility); dim ones (TAG_DOT_NEAR: runway edges,
// taxiways) DOT_NEAR times that.
const DOT_RANGE: f32 = 30000.0;
const DOT_VIS_FACTOR: f32 = 1.5;
const DOT_NEAR: f32 = 0.4;
// Spotting dots (TAG_DOT_SPOT) vanish once their aircraft (its size in metres in tag bits
// 16..) spans more than SPOT_PIXELS in the view, and sit that size towards the eye so its
// model can't hide them.
const SPOT_PIXELS: f32 = 12.0;

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
    out.warm = select(0u, 1u, (tag & (TAG_WARM | 128u)) != 0u) | select(0u, 2u, (tag & 7u) == 5u)
        | select(0u, 4u, (tag & (TAG_DOT_SPOT | 128u)) == (TAG_DOT_SPOT | 128u));
#ifdef DOT_COLOUR
    out.dot_colour = v.colour;
#endif
    // Smoke volumes: the view ray and the sun in the plume's local space (inverse of the
    // scaled rotation m = diag(1/s^2) m^T).
    out.puff_pos = vec4<f32>(0.0);
    out.puff_eye = vec4<f32>(0.0);
    out.puff_sun = vec3<f32>(0.0);
    out.puff_scale = vec3<f32>(1.0);
    if ((tag & TAG_SMOKE) != 0u) {
        let m = mat3x3<f32>(world_from_local[0].xyz, world_from_local[1].xyz, world_from_local[2].xyz);
        let s2 = vec3<f32>(dot(m[0], m[0]), dot(m[1], m[1]), dot(m[2], m[2]));
        let o = world_from_local[3].xyz;
        let eye_local = (transpose(m) * (view.world_position - o)) / s2;
        // Plumes carry their noise offset in the tag's high bits (stays put when the floating
        // origin moves). puff_eye.w: 1 smoke, 2 smoke over a fire.
        out.puff_pos = vec4<f32>(v.position, f32(tag >> 16u) * 0.01);
        out.puff_eye = vec4<f32>(eye_local, select(1.0, 2.0, (tag & TAG_SMOKE_FIRE) != 0u));
        out.puff_sun = normalize((transpose(m) * globals.sun.xyz) / s2);
        out.puff_scale = sqrt(s2);
    }

    // Dot lights (dots.rs `dot_batch`: four vertices at each dot's centre) spread by their quad
    // corner to a constant size in this view's pixels (so the tracking camera's zoom doesn't
    // magnify them); in XR at most a 320x200 pixel's angular size.
#ifdef DOT_BATCH
    if (((tag >> 7u) & 1u) == 1u) {
        var center = world.xyz;
        let spot = (tag & TAG_DOT_SPOT) != 0u;
        let spot_m = f32((tag >> 16u) & 0xFFu);
        if (spot) {
            center = center + normalize(view.world_position - center) * spot_m;
        }
        let corner = v.corner;
        let clip = position_world_to_clip(center);
        let vp = view.viewport.zw;
        var px = max(vp.y / 200.0 * DOT_SIZE, DOT_MIN_PIXELS);
        if (globals.dots.x > 0.0) {
            px = min(px, max(globals.dots.x * 0.5 * vp.y * view.clip_from_view[1][1], DOT_MIN_PIXELS));
        }
        if ((tag & 7u) == 5u) {
            let lamp = AIRCRAFT_DOT_METRES * 0.5 * vp.y * view.clip_from_view[1][1] / max(clip.w, 0.01);
            px = max(px, min(lamp, px * AIRCRAFT_DOT_MAX));
        }
        // Larger lights (PAPI): size multiplier in tag bits 8..10, or the size in tenths in
        // bits 24..30 (palette_material.rs `dot_size_tenths`).
        let tenths = (tag >> 24u) & 0x7Fu;
        if (tenths != 0u) {
            px = px * f32(tenths) / 10.0;
        } else {
            px = px * f32(((tag >> 8u) & 7u) + 1u);
        }
        out.world_position = center;
        out.clip_position = clip + vec4<f32>(corner * px * 2.0 / vp * clip.w, 0.0, 0.0);
        var range = DOT_RANGE;
        if (globals.weather.z > 0.0) {
            range = min(range, globals.weather.z * DOT_VIS_FACTOR);
        }
        if ((tag & TAG_DOT_NEAR) != 0u) {
            range = range * DOT_NEAR;
        }
        let span = spot_m * 0.5 * vp.y * view.clip_from_view[1][1] / max(clip.w, 0.01);
        let dist = distance(center, view.world_position);
        let near = (tag & TAG_DOT_FAR) != 0u && dist < DOT_FAR_MIN;
        if (dist > range || near || (spot && span > SPOT_PIXELS)) {
            // Out of range (or a spotted aircraft close enough to see, or a far-only dot too
            // close): a degenerate quad outside the clip volume.
            out.clip_position = vec4<f32>(0.0, 0.0, -1.0, 1.0);
        }
    }
#endif

    // Per-object haze: fog = clamp((depth/512 in object units) - sizeClass, 0, 31) >> 1
    let size_class = (tag >> 3u) & 7u;
    let no_fog = (tag >> 6u) & 1u;
    let origin = world_from_local[3].xyz;
    let forward = -view.world_from_view[2].xyz;
    let depth_m = max(dot(origin - view.world_position, forward), 0.0);
    let depth_units = depth_m * max(globals.weather.x, 1.0) / (globals.params.y * level_unit(tag & 7u));
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
// direction, using the classic screen-unit band boundaries: R = 256 * tan(angle).
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
    if (recon_view()) {
        return globals.recon_palette[i & 0xFFu].rgb;
    }
    return globals.palette[i & 0xFFu].rgb;
}

// A recon camera's view (recon.rs): it sees by day and night alike, like a near infrared camera.
fn recon_view() -> bool {
    return all(abs(view.viewport.zw - globals.sky_night.zw) < vec2<f32>(0.5));
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

// Moonlight's colour: a cool blue white.
const MOON_TINT: vec3<f32> = vec3<f32>(0.75, 0.85, 1.0);

fn sun_moon(n: vec3<f32>) -> vec3<f32> {
    return globals.ambient.rgb
        + vec3<f32>(globals.sun.w * max(dot(n, globals.sun.xyz), 0.0))
        + MOON_TINT * globals.moon.w * max(dot(n, globals.moon.xyz), 0.0);
}

// Modern mode: flat lighting by the sun and the moon (no shadows) on a face with normal n,
// relative to flat ground (the palette's brightness): faces turned to the light brighter,
// turned away darker. A low sun lights walls facing it far more than the ground: capped.
const SKY_LIGHT_MAX: f32 = 2.0;

fn sky_light(n: vec3<f32>) -> vec3<f32> {
    return min(sun_moon(n) / sun_moon(vec3<f32>(0.0, 1.0, 0.0)), vec3<f32>(SKY_LIGHT_MAX));
}

// Modern mode: flat lighting of a face. `face`: the face normal from the position's
// derivatives (`Deriv`).
fn shade(face: vec3<f32>, dir: vec3<f32>) -> vec3<f32> {
    var n = normalize(face);
    if (dot(n, dir) > 0.0) { n = -n; }
    return sky_light(n);
}

// Specular highlights from the local lights: strength (x) and Blinn-Phong exponent (y).
const GLOSS_AIRCRAFT: vec2<f32> = vec2<f32>(0.6, 48.0);
const GLOSS_OBJECT: vec2<f32> = vec2<f32>(0.2, 16.0);
const GLOSS_GROUND: vec2<f32> = vec2<f32>(0.15, 24.0);

// Lighting options: the palette colour lit by the sun (with shadow maps) and local lights,
// using the flat face normal. Unlit parts keep roughly the classic brightness.
// `dark`: the night's darkness on everything but the local lights; `gloss`: their specular
// highlights (`GLOSS_*`).
fn lit(rgb: vec3<f32>, world: vec3<f32>, face: vec3<f32>, dir: vec3<f32>, clip: vec4<f32>, flags: u32, dark: f32, gloss: vec2<f32>) -> vec3<f32> {
    var n = normalize(face);
    if (dot(n, dir) > 0.0) { n = -n; }
    return lit_surface(rgb, world, n, dir, clip, flags, dark, gloss);
}

// The key light's shadow at `world` (1 lit, 0 shadowed): Bevy's cascade lookup cut down to
// one cascade and one sample (its own, with cascade blending and the filter and PCSS paths,
// costs the Steam Frame about 8 ms). Classic shaders: hard edged (the nearest texel); modern:
// the hardware's 2x2 comparison filter.
fn sun_shadow(world: vec3<f32>, n: vec3<f32>, view_z: f32, hard: bool) -> f32 {
    // The index and the cascade bounds come from the CPU (lighting.rs): indexing Bevy's light
    // array in a loop costs the Steam Frame most of the time the lookup takes.
    let z = vec4<f32>(-view_z);
    let ci = u32(dot(select(vec4<f32>(0.0), vec4<f32>(1.0), z >= globals.shadow_bounds), vec4<f32>(1.0)));
    if (ci >= u32(globals.shadow.y)) {
        return 1.0;
    }
    let light = &lights.directional_lights[u32(globals.shadow.x)];
    let cascade = &(*light).cascades[ci];
    let p = world + (*light).shadow_normal_bias * (*cascade).texel_size * n
        + (*light).shadow_depth_bias * (*light).direction_to_light;
    let c = (*cascade).clip_from_world * vec4<f32>(p, 1.0);
    let ndc = c.xyz / c.w;
    if (any(abs(ndc.xy) > vec2<f32>(1.0)) || ndc.z < 0.0 || ndc.z > 1.0) {
        return 1.0;
    }
    let uv = ndc.xy * vec2<f32>(0.5, -0.5) + 0.5;
    let layer = i32((*light).depth_texture_base_index + ci);
#ifdef NO_ARRAY_TEXTURES_SUPPORT
    if (hard) {
        let size = vec2<f32>(textureDimensions(directional_shadow_textures));
        let d = textureLoad(directional_shadow_textures, vec2<i32>(uv * size), 0);
        return select(0.0, 1.0, ndc.z >= d);
    }
    return textureSampleCompareLevel(directional_shadow_textures, directional_shadow_textures_comparison_sampler, uv, ndc.z);
#else
    if (hard) {
        let size = vec2<f32>(textureDimensions(directional_shadow_textures));
        let d = textureLoad(directional_shadow_textures, vec2<i32>(uv * size), layer, 0);
        return select(0.0, 1.0, ndc.z >= d);
    }
    return textureSampleCompareLevel(directional_shadow_textures, directional_shadow_textures_comparison_sampler, uv, layer, ndc.z);
#endif
}

fn lit_surface(rgb: vec3<f32>, world: vec3<f32>, n: vec3<f32>, dir: vec3<f32>, clip: vec4<f32>, flags: u32, dark: f32, gloss: vec2<f32>) -> vec3<f32> {
    var light = globals.ambient.rgb;
    var spec = vec3<f32>(0.0);
    if ((flags & LIGHT_SHADOWS) != 0u) {
        // The shadow maps are the key light's: the sun, or the moon when it is the brighter.
        let moon_key = (flags & LIGHT_MOON_KEY) != 0u;
        let sun_l = vec3<f32>(globals.sun.w * max(dot(n, globals.sun.xyz), 0.0));
        let moon_l = MOON_TINT * globals.moon.w * max(dot(n, globals.moon.xyz), 0.0);
        var shadow = 1.0;
#ifdef PALETTE_SHADOWS
        if (globals.shadow.y > 0.0 && !offscreen_view() && dot(select(sun_l, moon_l, moon_key), vec3<f32>(1.0)) > 0.0) {
            let view_z = (view.view_from_world * vec4<f32>(world, 1.0)).z;
            shadow = sun_shadow(world, n, view_z, globals.params.w == 0.0);
        }
#endif
        light += select(sun_l * shadow, sun_l, moon_key) + select(moon_l, moon_l * shadow, moon_key);
        // Sunlit flat ground keeps the palette's brightness, shadows go darker (lighting.rs
        // sets the ambient light for that).
        light = min(light / sun_moon(vec3<f32>(0.0, 1.0, 0.0)), vec3<f32>(SKY_LIGHT_MAX));
    } else if (globals.params.w != 0.0) {
        light = sky_light(n);
    } else {
        light = vec3<f32>(1.0);
    }
    light *= 1.0 - dark;
#ifdef PALETTE_LOCAL
    if ((flags & LIGHT_LOCAL) != 0u) {
        let count = min(u32(globals.light_info.x), MAX_LOCAL_LIGHTS);
        let to_eye = -normalize(dir);
        for (var i = 0u; i < count; i++) {
            let p = globals.local_lights[2u * i];
            let c = globals.local_lights[2u * i + 1u];
            let to_light = p.xyz - world;
            let d = length(to_light);
            if (d < p.w) {
                let att = (1.0 - d / p.w) * (1.0 - d / p.w);
                let l = to_light / max(d, 1e-3);
                let facing = dot(n, l);
                light += c.rgb * att * (max(facing, 0.0) * 0.7 + 0.3);
                // Blinn-Phong highlight in the light's own colour.
                if (facing > 0.0) {
                    let h = normalize(l + to_eye);
                    spec += c.rgb * att * gloss.x * pow(max(dot(n, h), 0.0), gloss.y);
                }
            }
        }
    }
#endif
    return rgb * light + spec;
}

// Cloud texture: Perlin noise in world space. The lattice wraps every NOISE_PERIOD metres
// horizontally (the floating origin is handed over modulo that), so the pattern stays put
// when the origin moves.
const NOISE_PERIOD: f32 = 81920.0;
const NOISE_CELL: f32 = 1280.0;
const NOISE_OCTAVES: u32 = 5u;

fn noise_hash(p: vec3<i32>) -> u32 {
    var h = (u32(p.x) * 0x8DA6B343u) ^ (u32(p.y) * 0xD8163841u) ^ (u32(p.z) * 0xCB1AB31Fu) ^ u32(globals.noise.z);
    h ^= h >> 16u;
    h *= 0x7FEB352Du;
    h ^= h >> 15u;
    h *= 0x846CA68Bu;
    h ^= h >> 16u;
    return h;
}

// Gradient at a lattice corner (Perlin's 12 cube-edge directions) dotted with the offset.
fn noise_grad(h: u32, f: vec3<f32>) -> f32 {
    let g = h % 12u;
    let u = select(f.y, f.x, g < 8u);
    let v = select(select(f.z, f.x, g == 12u || g == 14u), f.y, g < 4u);
    return select(u, -u, (g & 1u) != 0u) + select(v, -v, (g & 2u) != 0u);
}

fn perlin(p: vec3<f32>, period: i32) -> f32 {
    let i = vec3<i32>(floor(p));
    let f = fract(p);
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    var c: array<f32, 8>;
    for (var k = 0u; k < 8u; k++) {
        let o = vec3<i32>(i32(k & 1u), i32((k >> 1u) & 1u), i32((k >> 2u) & 1u));
        let q = i + o;
        let w = vec3<i32>(((q.x % period) + period) % period, q.y, ((q.z % period) + period) % period);
        c[k] = noise_grad(noise_hash(w), f - vec3<f32>(o));
    }
    let x0 = mix(mix(c[0], c[1], u.x), mix(c[2], c[3], u.x), u.y);
    let x1 = mix(mix(c[4], c[5], u.x), mix(c[6], c[7], u.x), u.y);
    return mix(x0, x1, u.z);
}

// About -1..1; octaves finer than a few pixels fade out with distance.
// --- Stratus coverage (same field as stealth_mission::Weather::stratus_field) ------------------

fn value_hash(ix: i32, iy: i32, seed: u32) -> f32 {
    var h = (u32(ix) * 0x8DA6B343u) ^ (u32(iy) * 0xD8163841u) ^ (seed * 0xCB1AB31Fu);
    h ^= h >> 16u;
    h *= 0x7FEB352Du;
    h ^= h >> 15u;
    h *= 0x846CA68Bu;
    h ^= h >> 16u;
    return f32(h >> 8u) / 16777216.0;
}

fn value_noise(p: vec2<f32>, seed: u32) -> f32 {
    let f = floor(p);
    let i = vec2<i32>(f);
    let t = p - f;
    let u = t * t * (3.0 - 2.0 * t);
    let a = value_hash(i.x, i.y, seed);
    let b = value_hash(i.x + 1, i.y, seed);
    let c = value_hash(i.x, i.y + 1, seed);
    let d = value_hash(i.x + 1, i.y + 1, seed);
    let top = a + (b - a) * u.x;
    let bottom = c + (d - c) * u.x;
    return top + (bottom - top) * u.y;
}

// The field at a render-space position, in map units (64 per km, y southward).
fn stratus_field(world: vec3<f32>) -> f32 {
    let map_unit = 32.0 * globals.params.y;
    let east = world.x + globals.stratus.z;
    let north = -(world.z + globals.stratus.w);
    let m = vec2<f32>(east / map_unit, 32768.0 - north / map_unit);
    let seed = u32(globals.stratus.y);
    return 0.6 * value_noise(m / 512.0, seed) + 0.3 * value_noise(m / 160.0, seed + 1u) + 0.1 * value_noise(m / 48.0, seed + 2u);
}

// Stratus coverage 0..1 at a position (soft edge just above the threshold). Far away the
// gaps would alias into speckle: there it fades to the layer's average coverage.
fn stratus_cover(world: vec3<f32>) -> f32 {
    if (globals.clouds.z == 0.0) { return 0.0; }
    let thr = globals.stratus.x;
    let local = smoothstep(thr, thr + 0.035, stratus_field(world));
    let d = length(world - view.world_position);
    return mix(local, globals.noise.w, smoothstep(25000.0, 70000.0, d));
}

fn cloud_noise(world: vec3<f32>, dist: f32) -> f32 {
    let p = world + vec3<f32>(globals.noise.x, 0.0, globals.noise.y);
    var sum = 0.0;
    var amp = 0.6;
    var cell = NOISE_CELL;
    for (var o = 0u; o < NOISE_OCTAVES; o++) {
        let fade = 1.0 - smoothstep(cell * 15.0, cell * 40.0, dist);
        sum += amp * fade * perlin(p / cell, i32(NOISE_PERIOD / cell));
        amp *= 0.5;
        cell *= 0.5;
    }
    return sum;
}

// Cloud colour at `world` on a face with normal n (facing the viewer) seen `dist` metres
// away: tops light, undersides dark, sun side lighter, textured by noise, hazing to the
// horizon colour with distance.
fn cloud_color(noise: f32, n: vec3<f32>, dist: f32, modern: bool) -> vec3<f32> {
    let shading = (1.0 - n.y) * 4.0 + 4.0 - 3.0 * max(dot(n, globals.sun.xyz), 0.0);
    return cloud_ramp(shading + 4.5 * noise, dist, modern);
}

// Grey ramp entry k (0 lightest), hazed with distance.
fn cloud_ramp(shade: f32, dist: f32, modern: bool) -> vec3<f32> {
    let k = clamp(shade, 0.0, 14.0);
    let fog = clamp(dist / max(globals.weather.z, 1.0) * 15.0, 0.0, 15.0);
    let f = max(k - fog, 0.0);
    if (modern) {
        return mix(pal(CLOUD_RAMP + u32(floor(f))), pal(CLOUD_RAMP + u32(floor(f)) + 1u), fract(f));
    }
    return pal(CLOUD_RAMP + u32(round(f)));
}

// 4x4 ordered dither threshold (classic mode's soft cloud edges).
// Angular radius of the sun's disc (drawn about three times its real size).
const SUN_RADIUS: f32 = 0.014;

// The sun's colour at an elevation (sine): orange red at the horizon, warm white up high.
fn sun_colour(h: f32) -> vec3<f32> {
    return mix(vec3<f32>(1.0, 0.42, 0.1), vec3<f32>(1.0, 0.93, 0.78), smoothstep(-0.02, 0.3, h));
}

// Modern shaders, continuous time of day: the glare of the sun over everything in its
// direction, a bright core and wide soft halos, dimmed by an overcast and gone below the horizon.
fn sun_glare(dir: vec3<f32>) -> vec3<f32> {
    if (globals.sky_sun.w == 0.0 || globals.params.w == 0.0) {
        return vec3<f32>(0.0);
    }
    let s = normalize(globals.sky_sun.xyz);
    let up = smoothstep(-0.03, 0.02, s.y);
    if (up <= 0.0) {
        return vec3<f32>(0.0);
    }
    let c = max(dot(normalize(dir), s), 0.0);
    let strength = clamp(globals.sun.w / 0.3, 0.0, 1.0) * up;
    let glow = pow(c, 900.0) * 0.9 + pow(c, 60.0) * 0.16 + pow(c, 8.0) * 0.05;
    return sun_colour(s.y) * glow * strength;
}

// Angular radius of the moon's disc (drawn about five times its real size).
const MOON_RADIUS: f32 = 0.022;
const MOON_COLOR: vec3<f32> = vec3<f32>(0.82, 0.80, 0.74);
// Star cells (radians) and the share of cells holding a star.
const STAR_CELL: f32 = 0.012;
const STAR_SHARE: f32 = 0.035;

fn hash2(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

// Continuous time of day: the sun, stars and the moon over the sky colour (`nd` normalised, above the
// horizon). The moon is lit from the true sun direction, so its phase shows by itself. `px`:
// radians per pixel (`Deriv`).
fn night_sky(bg_in: vec3<f32>, nd: vec3<f32>, px: f32) -> vec3<f32> {
    var bg = bg_in;
    let daylight = clamp(globals.light_info.y, 0.0, 1.0);
    let stars = globals.sky_moon.w;
    if (stars > 0.0) {
        let az = atan2(nd.x, -nd.z);
        let el = asin(clamp(nd.y, -1.0, 1.0));
        // Rows of cells by elevation; narrower azimuth cells near the zenith would crowd the
        // stars, so each row's cells are as wide as the row's middle is around.
        let row = floor(el / STAR_CELL);
        let w = STAR_CELL / max(cos((row + 0.5) * STAR_CELL), 0.05);
        let cell = vec2<f32>(floor(az / w), row);
        let h = hash2(cell);
        if (h < STAR_SHARE) {
            let c_az = (cell.x + 0.5 + 0.6 * (hash2(cell + 7.1) - 0.5)) * w;
            let c_el = (cell.y + 0.5 + 0.6 * (hash2(cell + 3.3) - 0.5)) * STAR_CELL;
            let star = vec3<f32>(cos(c_el) * sin(c_az), sin(c_el), -cos(c_el) * cos(c_az));
            let d = length(nd - star);
            // A hard edged point about a pixel across: unfiltered, it pops between pixels as the
            // view moves, so the stars twinkle.
            let r = max(STAR_CELL * 0.03, px * 0.55);
            let core = select(0.0, 1.0, d < r);
            let mag = 0.35 + 0.65 * pow(h / STAR_SHARE, 3.0);
            // Fainter towards the horizon (haze).
            let alt_fade = smoothstep(0.0, 0.15, nd.y);
            bg += vec3<f32>(0.9, 0.92, 1.0) * core * mag * stars * alt_fade;
        }
    }
    // The sun's disc, a soft edge against the sky.
    let sun = normalize(globals.sky_sun.xyz);
    if (sun.y > -SUN_RADIUS) {
        let a = acos(clamp(dot(nd, sun), -1.0, 1.0));
        let edge = 1.0 - smoothstep(SUN_RADIUS - px, SUN_RADIUS + px, a);
        bg = mix(bg, sun_colour(sun.y) * 1.6, edge);
    }
    let m = normalize(globals.sky_moon.xyz);
    let c = dot(nd, m);
    if (m.y > -MOON_RADIUS && c > cos(MOON_RADIUS * 8.0)) {
        // The disc as a sphere seen from the earth: its normal at this pixel.
        let o = (nd - m * c) / MOON_RADIUS;
        let r2 = dot(o, o);
        let fw = px / MOON_RADIUS;
        let edge = 1.0 - smoothstep(1.0 - fw, 1.0 + fw, sqrt(r2));
        if (edge > 0.0) {
            let n = normalize(o - m * sqrt(max(1.0 - r2, 0.0)));
            let lit = smoothstep(-0.05, 0.05, dot(n, normalize(globals.sky_sun.xyz)));
            // Earthshine on the dark side; faint by day.
            let shade = mix(0.04, 1.0, lit) * mix(1.0, 0.35, daylight);
            let disc = MOON_COLOR * shade;
            bg = mix(bg, max(bg, disc), edge);
        }
        // A soft halo around the lit moon at night, fading out before the edge of this test.
        let a = acos(min(c, 1.0)) / MOON_RADIUS;
        let halo = exp(-a * a * 0.25) * smoothstep(8.0, 5.0, a) * 0.12 * (1.0 - daylight);
        bg += MOON_COLOR * halo * globals.sky_moon.w;
    }
    return bg;
}

// A night without moonlight (continuous time of day): sky, ground and clouds go black, only
// lights and stars show, aircraft barely; the horizon keeps a deep red glow. 0 (moonlit) .. 1.
// Never for the recon cameras.
fn night_dark() -> f32 {
    if (recon_view()) {
        return 0.0;
    }
    return globals.sky_night.x;
}

// The red glow over the horizon of a dark night by sky band (8-bit sRGB red).
fn night_red(k: u32) -> vec3<f32> {
    var red = array<f32, 10>(64.0, 48.0, 36.0, 28.0, 24.0, 16.0, 12.0, 8.0, 6.0, 3.0);
    if (k >= 10u) { return vec3<f32>(0.0); }
    let r = red[k] / 255.0 * globals.sky_night.y;
    return pow(min(vec3<f32>(r, 0.0, r * 0.07), vec3<f32>(1.0)), vec3<f32>(2.2));
}

// The sky or ground colour `bg` darkened for the night (`dir`: view direction; sky band `b`
// as from `band_f`, its fraction ignored in classic mode).
fn night_background(bg: vec3<f32>, dir: vec3<f32>, b: vec3<f32>, modern: bool) -> vec3<f32> {
    let dark = night_dark();
    if (dark <= 0.0) {
        return bg;
    }
    var c = bg * (1.0 - dark);
    if (dir.y >= 0.0) {
        let k = u32(b.y);
        var red = night_red(k);
        if (modern) {
            red = mix(red, night_red(k + 1u), b.z);
        }
        c += red * dark;
    }
    return c;
}

fn bayer4(p: vec2<u32>) -> f32 {
    var m = array<f32, 16>(0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0);
    return (m[(p.y & 3u) * 4u + (p.x & 3u)] + 0.5) / 16.0;
}

// Inside a cloud: everything but the cockpit fades into it. Under an overcast or in a storm
// the day turns greyer and dimmer.
fn whiteout(rgb: vec3<f32>, dist: f32) -> vec3<f32> {
    let g = globals.weather.w;
    let grey = mix(rgb, vec3<f32>(dot(rgb, vec3<f32>(0.2126, 0.7152, 0.0722))), 0.4 * g) * (1.0 - 0.3 * g);
    let w = globals.clouds.w * smoothstep(3.0, 60.0, dist);
    return mix(grey, pal(CLOUD_RAMP + 2u) * (1.0 - night_dark()), w);
}

// --- Smoke plumes ----------------------------------------------------------------------------
// Smoke of burning wrecks and fires (fires.rs): a column rising from the ground (local y 0..1
// inside a unit cylinder), widening with height and leaning downwind (local +x). The noise
// scrolls upwards so the smoke billows; dark grey from the cloud ramp, self-shadowed, its base lit orange by the fire underneath.

const SMOKE_STEPS: u32 = 12u;
// XR (two large eye views on a mobile GPU): fewer steps, a single noise octave and no
// self-shadow sample — about a sixth of the noise evaluations, so a screen full of plumes
// can't stall the GPU long enough to lose the device.
const SMOKE_STEPS_XR: u32 = 6u;
// Plume lean (local x at the top) and radius at the base / top.
const SMOKE_LEAN: f32 = 0.4;
const SMOKE_R0: f32 = 0.06;
const SMOKE_R1: f32 = 0.58;
// Rise speed of the noise (metres per second).
const SMOKE_RISE: f32 = 9.0;
const FIRE_COLOR: vec3<f32> = vec3<f32>(1.0, 0.42, 0.1);
// The proxy mesh (fires.rs) circumscribes the unit cylinder (y 0..1) the plume lives in:
// radius 1 / cos(pi / 16).
const SMOKE_PROXY: f32 = 1.0196;

// Ray parameter range (t0, t1) inside the unit cylinder (empty when t0 >= t1).
fn cylinder_bounds(ro: vec3<f32>, rd: vec3<f32>) -> vec2<f32> {
    let a = rd.x * rd.x + rd.z * rd.z;
    let b = ro.x * rd.x + ro.z * rd.z;
    let c = ro.x * ro.x + ro.z * ro.z - 1.0;
    var t = vec2<f32>(-1e9, 1e9);
    if (a > 1e-9) {
        let disc = b * b - a * c;
        if (disc < 0.0) { return vec2<f32>(1.0, 0.0); }
        let sq = sqrt(disc);
        t = vec2<f32>((-b - sq) / a, (-b + sq) / a);
    } else if (c > 0.0) {
        return vec2<f32>(1.0, 0.0);
    }
    if (abs(rd.y) > 1e-6) {
        let ty = vec2<f32>(-ro.y / rd.y, (1.0 - ro.y) / rd.y);
        t = vec2<f32>(max(t.x, min(ty.x, ty.y)), min(t.y, max(ty.x, ty.y)));
    } else if (ro.y < 0.0 || ro.y > 1.0) {
        return vec2<f32>(1.0, 0.0);
    }
    return t;
}

// The proxy is drawn with both windings and back faces culled, so from outside both the near
// side and (reversed) the far side are rasterised, from inside only the far side, and every
// fragment is front facing: whether to march from this fragment (local point p, eye ro) is
// decided by its geometry instead — from outside the side facing the eye, from inside the far
// side.
fn proxy_fragment(p: vec3<f32>, ro: vec3<f32>) -> bool {
    let inside = dot(ro.xz, ro.xz) < SMOKE_PROXY * SMOKE_PROXY && ro.y > 0.0 && ro.y < 1.0;
    var n: vec3<f32>;
    if (p.y > 0.9999) {
        n = vec3<f32>(0.0, 1.0, 0.0);
    } else if (p.y < 0.0001) {
        n = vec3<f32>(0.0, -1.0, 0.0);
    } else {
        n = vec3<f32>(p.x, 0.0, p.z);
    }
    return inside || dot(n, ro - p) > 0.0;
}

// `start`: local height where the smoke has risen clear of the flames.
fn smoke_density(p: vec3<f32>, aspect: vec3<f32>, seed: f32, rise: f32, start: f32, detail: bool) -> f32 {
    let y = p.y;
    if (y <= 0.0 || y >= 1.0) { return 0.0; }
    let cx = SMOKE_LEAN * pow(y, 1.6);
    let r = mix(SMOKE_R0, SMOKE_R1, pow(y, 0.7));
    var shape = 1.0 - length(vec2<f32>(p.x - cx, p.z)) / r;
    // Thins out and ends towards the top.
    shape -= smoothstep(0.55, 1.0, y) * 1.1;
    if (shape < -0.6) { return 0.0; }
    // Noise in units of the plume's width, rising (faster at the core).
    let pn = p * aspect + vec3<f32>(seed, -rise, seed * 0.7);
    var n = perlin(pn * 2.2, 1024);
    if (detail) {
        n += 0.5 * perlin((pn + vec3<f32>(0.0, -rise * 0.6, 0.0)) * 5.3, 1024);
    }
    return clamp((shape + 0.65 * n) * 3.0, 0.0, 1.0) * (1.0 - 0.55 * y) * smoothstep(start * 0.3, start * 2.0, y);
}

fn smoke_volume(in: VertexOutput, dir: vec3<f32>, modern: bool) -> vec4<f32> {
    let dist = length(dir);
    let fire = in.puff_eye.w > 1.5;
    let ro = in.puff_eye.xyz;
    let rd = in.puff_pos.xyz - ro;
    if (!proxy_fragment(in.puff_pos.xyz, ro)) { discard; }
    let tb = cylinder_bounds(ro, rd);
    let t0 = max(tb.x, 0.0);
    let t1 = tb.y;
    if (t1 <= t0) { discard; }
    let aspect = in.puff_scale / in.puff_scale.x;
    let m_per_t = length(rd * in.puff_scale);
    let xr = globals.dots.x > 0.0;
    let steps = select(SMOKE_STEPS, SMOKE_STEPS_XR, xr);
    let dt = (t1 - t0) / f32(steps);
    let ds = dt * m_per_t;
    // Opaque within about a fifth of the plume's width.
    let thick = in.puff_scale.x * 0.2;
    let rise = globals.dots.y * SMOKE_RISE / in.puff_scale.x;
    let jitter = bayer4(vec2<u32>(in.clip_position.xy));
    let sun = in.puff_sun * 0.08;
    // Smoke starts above the flames (about one base width up); the fire lights the bottom of
    // the plume.
    let start = select(0.0, in.puff_scale.x * SMOKE_R0 / in.puff_scale.y, fire);
    let glow_top = start * 4.0;
    var trans = 1.0;
    var light = 0.0;
    var glow = 0.0;
    for (var i = 0u; i < steps; i++) {
        let p = ro + rd * (t0 + (f32(i) + jitter) * dt);
        let dens = smoke_density(p, aspect, in.puff_pos.w, rise, start, !xr);
        if (dens <= 0.001) { continue; }
        let a = 1.0 - exp(-dens * ds / thick);
        var lit = clamp(0.5 + 0.25 * p.y, 0.0, 1.0);
        if (!xr) {
            let towards_sun = smoke_density(p + sun, aspect, in.puff_pos.w, rise, start, false);
            lit = clamp(0.4 + (dens - towards_sun) * 1.4 + 0.25 * p.y, 0.0, 1.0);
        }
        light += trans * a * lit;
        if (fire) {
            let g = 1.0 - smoothstep(start, glow_top, p.y);
            glow += trans * a * g * g;
        }
        trans *= 1.0 - a;
        if (trans < 0.03) { break; }
    }
    var alpha = 1.0 - trans;
    if (alpha < 0.01) { discard; }
    // Darker than cloud: the lower half of the grey ramp.
    var rgb = cloud_ramp(7.0 + (1.0 - light / alpha) * 6.5, dist, modern);
    // The fire lights the smoke above it, most at night.
    rgb += FIRE_COLOR * min(glow / alpha, 1.0) * mix(0.5, 0.15, globals.light_info.y);
    rgb = flash(whiteout(rgb, dist), view.world_position + dir);
    if (!modern) {
        // Classic: a screen door instead of blending.
        if (alpha < bayer4(vec2<u32>(in.clip_position.xy) + vec2<u32>(2u, 1u))) { discard; }
        alpha = 1.0;
    }
    return vec4<f32>(rgb, alpha);
}

const BOLT_COLOR: vec3<f32> = vec3<f32>(0.55, 0.58, 0.75);

// Light of the current lightning channel at a point: falls off with the distance from the
// channel (a line light), zero beyond its reach.
fn bolt_glow(world: vec3<f32>) -> f32 {
    let i = globals.bolt_a.w;
    if (i <= 0.0) { return 0.0; }
    let a = globals.bolt_a.xyz;
    let ab = globals.bolt_b.xyz - a;
    let t = clamp(dot(world - a, ab) / max(dot(ab, ab), 1e-3), 0.0, 1.0);
    let d = length(world - (a + ab * t));
    let reach = globals.bolt_b.w;
    // A bright core around the channel and a wide, weaker halo.
    let core = d / (reach * 0.06);
    let halo = d / (reach * 0.3);
    return i * (1.0 - smoothstep(0.3 * reach, reach, d)) * (1.6 / (1.0 + core * core) + 0.3 / (1.0 + halo * halo));
}

// Lightning reads far brighter at night than by day.
fn flash_scale() -> f32 {
    return mix(1.0, 0.3, globals.light_info.y);
}

// Lightning: the far-reaching flash over everything plus the channel's local light.
fn flash(rgb: vec3<f32>, world: vec3<f32>) -> vec3<f32> {
    return rgb + BOLT_COLOR * (globals.weather.y + bolt_glow(world)) * flash_scale();
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

// FLIR (tracking camera only, picked by its picture size): a black and white thermal picture
// with warm objects (mesh tag `TAG_WARM`, dot lights) bright and the scenery dark; haze
// fades warm objects into the background.
// The targeting pod's or a recon camera's picture: no shadow maps there (lighting.rs
// `no_offscreen_shadows`).
fn offscreen_view() -> bool {
    return recon_view() || all(abs(view.viewport.zw - globals.dots.zw) < vec2<f32>(0.5));
}

fn flir_view() -> bool {
    return globals.params.z != 0.0 && all(abs(view.viewport.zw - globals.dots.zw) < vec2<f32>(0.5));
}

fn flir(rgb: vec3<f32>, warm: u32, fog_f: f32) -> vec3<f32> {
    let s = sqrt(dot(rgb, vec3<f32>(0.2126, 0.7152, 0.0722)));
    let cold = 0.3 + 0.25 * s;
    var v = cold;
    if ((warm & 1u) != 0u) {
        v = mix(0.85 + 0.15 * s, cold, clamp(fog_f / 24.0, 0.0, 0.6));
    }
    return vec3<f32>(pow(v, 2.2));
}

// Screen-space derivatives, taken at the start of `fragment`: WebGPU only allows them in
// uniform control flow, which a branch or return on a varying ends.
struct Deriv {
    // Face normal (unnormalised, either side).
    face: vec3<f32>,
    // Radians per pixel of the view direction (from the smooth direction: cell distances in
    // the night sky jump at cell edges).
    sky_px: f32,
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    var deriv: Deriv;
    deriv.face = cross(dpdx(in.world_position), dpdy(in.world_position));
    deriv.sky_px = max(length(fwidth(normalize(in.world_position - view.world_position))), 1e-5);
    if ((u32(globals.light_info.w) & 1u) != 0u) {
        return vec4<f32>(pal(material.word & 0xFFu), 1.0);
    }
    if (flir_view()) {
        // Background: cold sky, cool ground.
        if ((material.flags & 4u) != 0u) {
            let dir = in.world_position - view.world_position;
            return vec4<f32>(vec3<f32>(pow(select(0.2, 0.35, dir.y < 0.0), 2.2)), 1.0);
        }
        let c = shade_fragment(in, deriv);
        return vec4<f32>(flir(c.rgb, in.warm, in.fog_f), c.a);
    }
    let c = shade_fragment(in, deriv);
    let glare = sun_glare(in.world_position - view.world_position);
    return vec4<f32>(nvg(c.rgb + glare * c.a, in.clip_position), c.a);
}

fn shade_fragment(in: VertexOutput, deriv: Deriv) -> vec4<f32> {
    let dir = in.world_position - view.world_position;
    let modern = globals.params.w != 0.0;
    // One pipeline per kind of material (palette_material.rs `PaletteKey`): a shader with
    // every branch needs so many registers that the Frame's GPU runs half as many fragments
    // at once.
#ifdef PALETTE_CLOUD
    {
        if ((u32(globals.light_info.w) & 8u) != 0u) {
            return vec4<f32>(pal(CLOUD_RAMP + 4u), 1.0);
        }
        let d = length(dir);
        if (in.puff_eye.w > 0.0) {
            return smoke_volume(in, dir, modern);
        }
        // Stratus sheet: only where the layer's coverage field says so.
        var cover = stratus_cover(in.world_position);
        if (modern) {
            if (cover <= 0.0) { discard; }
        } else {
            if (cover < bayer4(vec2<u32>(in.clip_position.xy))) { discard; }
            cover = 1.0;
        }
        var n = normalize(deriv.face);
        if (dot(n, dir) > 0.0) { n = -n; }
        let noise = cloud_noise(in.world_position, d);
        let c = cloud_color(noise, n, d, modern) * (1.0 - night_dark());
        return vec4<f32>(flash(whiteout(c, d), in.world_position), cover);
    }
#else ifdef PALETTE_BACKGROUND
    {
        if ((u32(globals.light_info.w) & 2u) != 0u) {
            return vec4<f32>(pal(select(0x7Cu, 0x88u, dir.y >= 0.0)), 1.0);
        }
        // Background: sky bands above the horizon, ground gradient below.
        if ((material.flags & 4u) != 0u) {
            return vec4<f32>(pal(select(0x7Cu, 0x88u, dir.y >= 0.0)), 1.0);
        }
        var bg: vec3<f32>;
        let b = band_f(dir);
        if (modern) {
            let k = u32(b.y);
            let k1 = min(k + 1u, 15u);
            bg = mix(pal(u32(b.x) + k), pal(u32(b.x) + k1), b.z);
        } else {
            bg = pal(band(dir).x);
        }
        // Lighting options: the ground gradient is lit like flat ground where the view ray
        // meets the ground plane (y = 0), so it takes shadows and local lights.
        let eye = view.world_position;
        var ground_lit = false;
#ifdef PALETTE_LIT
        let flags = u32(globals.ambient.w);
        if (flags != 0u && dir.y < 0.0 && eye.y > 0.0) {
            let ground = eye + dir * (-eye.y / dir.y);
            bg = lit_surface(bg, ground, vec3<f32>(0.0, 1.0, 0.0), dir, in.clip_position, flags, night_dark(), GLOSS_GROUND);
            ground_lit = true;
        }
#endif
        if (!ground_lit) {
            bg = night_background(bg, dir, b, modern);
        }
        if (globals.sky_sun.w > 0.0 && dir.y > 0.0) {
            bg = night_sky(bg, normalize(dir), deriv.sky_px);
        }
        // Stratus layer beyond its planes: its underside over the sky below it, its top over
        // the ground above it, where the layer's coverage field has cloud.
        if (globals.clouds.z != 0.0) {
            let eye_y = eye.y;
            let nd = normalize(dir);
            var d = -1.0;
            var n = vec3<f32>(0.0, -1.0, 0.0);
            if (eye_y <= globals.clouds.x && nd.y > 0.0) {
                d = (globals.clouds.x - eye_y) / nd.y;
            } else if (eye_y >= globals.clouds.y && nd.y < 0.0) {
                d = (eye_y - globals.clouds.y) / -nd.y;
                n = vec3<f32>(0.0, 1.0, 0.0);
            } else if (eye_y > globals.clouds.x && eye_y < globals.clouds.y) {
                bg = mix(bg, pal(CLOUD_RAMP + 2u) * (1.0 - night_dark()), stratus_cover(eye));
            }
            if (d > 0.0) {
                let hit = eye + nd * d;
                let c = cloud_color(cloud_noise(hit, d), n, d, modern) * (1.0 - night_dark());
                bg = mix(bg, c, stratus_cover(hit));
            }
        }
        // Lightning: the channel's light where the view ray meets cloud or ground, and the
        // glow of a strike over the sky in its direction.
        let to_bolt = (globals.bolt_a.xyz + globals.bolt_b.xyz) * 0.5 - eye;
        let bolt_d = length(to_bolt);
        let along = min(bolt_d, 60000.0);
        let sheet = pow(max(dot(normalize(dir), to_bolt / max(bolt_d, 1.0)), 0.0), 6.0) * globals.bolt_a.w * 0.25 * flash_scale();
        bg = flash(whiteout(bg, 1e4), eye + normalize(dir) * along) + BOLT_COLOR * sheet;
        return vec4<f32>(bg, 1.0);
    }
#else
    let word = material.word;
    let hi = (word >> 8u) & 0xFFu;
    var lo = word & 0xFFu;
#ifdef DOT_COLOUR
    lo = in.dot_colour;
#endif
    if ((u32(globals.light_info.w) & 4u) != 0u) {
        return vec4<f32>(pal(haze(lo, in.fog)), 1.0);
    }
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
    // A dark night blacks out the scenery; lights (lines and dots) show, aircraft barely.
    // Lines in scenery colours (rivers, roads: material flag 64) go dark with the ground.
    var dark = select(night_dark(), 0.0, is_line && (material.flags & 64u) == 0u);
    if ((in.warm & 4u) != 0u) {
        dark = night_dark() * AIRCRAFT_NIGHT;
    } else if ((in.warm & 2u) != 0u) {
        dark *= AIRCRAFT_NIGHT;
    }
#ifdef PALETTE_LIT
    let flags = u32(globals.ambient.w);
    if (!is_line && flags != 0u) {
        let gloss = select(GLOSS_OBJECT, GLOSS_AIRCRAFT, (in.warm & 2u) != 0u);
        rgb = lit(rgb, in.world_position, deriv.face, dir, in.clip_position, flags, dark, gloss);
    } else
#endif
    {
        if (modern && !is_line) {
            rgb = rgb * shade(deriv.face, dir);
        }
        rgb = rgb * (1.0 - dark);
    }
    return vec4<f32>(flash(whiteout(rgb, length(dir)), in.world_position), 1.0);
#endif
}
