// Palette-indexed rendering that reproduces the original's MCGA drawing:
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
const FLAG_CLOUD: u32 = 8u;
// Faceted cumulus / cumulonimbus mesh: no stratus coverage cutout.
const FLAG_CLOUD_SOLID: u32 = 64u;
// Smoke plume volume (fires.rs), optionally lit by a fire at its base.
const TAG_SMOKE: u32 = 8192u;
const TAG_SMOKE_FIRE: u32 = 16384u;
// Cloud colours: the grey ramp (its base entry is the horizon haze colour).
const CLOUD_RAMP: u32 = 0xE0u;

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
    // x: local light count, y: daylight 0 (night) .. 1 (day), z: night vision goggles.
    light_info: vec4<f32>,
    // x: XR dot angular size in radians (0 = flat screen), y: animation clock (seconds,
    // wraps every hour).
    dots: vec4<f32>,
    // Two entries per light: (position, range), (colour * intensity, unused).
    local_lights: array<vec4<f32>, 64>,
    // Weather: deck base / top (metres, eye height frame), deck present (z), whiteout 0..1 (w).
    clouds: vec4<f32>,
    // x: haze distance scale, y: lightning flash 0..1, z: visibility under clouds (metres).
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
#ifdef DOT_BATCH
    // Batched dots: four vertices at each dot's centre, spread by their quad corner.
    @location(1) corner: vec2<f32>,
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

    // Dot lights: the unit quad expanded around the origin to a constant size in this view's
    // pixels (so the tracking camera's zoom doesn't magnify them); in XR at most the original
    // pixel's angular size.
    if (((tag >> 7u) & 1u) == 1u) {
#ifdef DOT_BATCH
        let center = world.xyz;
        let corner = v.corner;
#else
        let center = world_from_local[3].xyz;
        let corner = v.position.xy;
#endif
        let clip = position_world_to_clip(center);
        let vp = view.viewport.zw;
        var px = max(vp.y / 200.0 * DOT_SIZE, DOT_MIN_PIXELS);
        if (globals.dots.x > 0.0) {
            px = min(px, max(globals.dots.x * 0.5 * vp.y * view.clip_from_view[1][1], DOT_MIN_PIXELS));
        }
        // Larger lights (PAPI): size multiplier in tag bits 8..10.
        px = px * f32(((tag >> 8u) & 7u) + 1u);
        out.world_position = center;
        out.clip_position = clip + vec4<f32>(corner * px * 2.0 / vp * clip.w, 0.0, 0.0);
    }

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
    return lit_surface(rgb, world, n, clip, flags);
}

fn lit_surface(rgb: vec3<f32>, world: vec3<f32>, n: vec3<f32>, clip: vec4<f32>, flags: u32) -> vec3<f32> {
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
// --- Stratus coverage (same field as f117_mission::Weather::stratus_field) ------------------

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
fn bayer4(p: vec2<u32>) -> f32 {
    var m = array<f32, 16>(0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0);
    return (m[(p.y & 3u) * 4u + (p.x & 3u)] + 0.5) / 16.0;
}

// Inside a cloud: everything but the cockpit fades into it.
fn whiteout(rgb: vec3<f32>, dist: f32) -> vec3<f32> {
    let w = globals.clouds.w * smoothstep(3.0, 60.0, dist);
    return mix(rgb, pal(CLOUD_RAMP + 2u), w);
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

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let c = shade_fragment(in);
    return vec4<f32>(nvg(c.rgb, in.clip_position), c.a);
}

fn shade_fragment(in: VertexOutput) -> vec4<f32> {
    let dir = in.world_position - view.world_position;
    let modern = globals.params.w != 0.0;
    if ((material.flags & FLAG_CLOUD) != 0u) {
        let d = length(dir);
        if (in.puff_eye.w > 0.0) {
            return smoke_volume(in, dir, modern);
        }
        // Stratus sheet: only where the layer's coverage field says so (cumulus and towers
        // are solid).
        var cover = 1.0;
        if ((material.flags & FLAG_CLOUD_SOLID) == 0u) {
            cover = stratus_cover(in.world_position);
            if (modern) {
                if (cover <= 0.0) { discard; }
            } else {
                if (cover < bayer4(vec2<u32>(in.clip_position.xy))) { discard; }
                cover = 1.0;
            }
        }
        var n = normalize(cross(dpdx(in.world_position), dpdy(in.world_position)));
        if (dot(n, dir) > 0.0) { n = -n; }
        let noise = cloud_noise(in.world_position, d);
        return vec4<f32>(flash(whiteout(cloud_color(noise, n, d, modern), d), in.world_position), cover);
    }
    if ((material.flags & 1u) != 0u) {
        // Background: sky bands above the horizon, ground gradient below.
        if ((material.flags & 4u) != 0u) {
            return vec4<f32>(pal(select(0x7Cu, 0x88u, dir.y >= 0.0)), 1.0);
        }
        var bg: vec3<f32>;
        if (modern) {
            let b = band_f(dir);
            let k = u32(b.y);
            let k1 = min(k + 1u, 15u);
            bg = mix(pal(u32(b.x) + k), pal(u32(b.x) + k1), b.z);
        } else {
            bg = pal(band(dir).x);
        }
        // Lighting options: the ground gradient is lit like flat ground where the view ray
        // meets the ground plane (y = 0), so it takes shadows and local lights.
        let flags = u32(globals.ambient.w);
        let eye = view.world_position;
        if (flags != 0u && dir.y < 0.0 && eye.y > 0.0) {
            let ground = eye + dir * (-eye.y / dir.y);
            bg = lit_surface(bg, ground, vec3<f32>(0.0, 1.0, 0.0), in.clip_position, flags);
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
                bg = mix(bg, pal(CLOUD_RAMP + 2u), stratus_cover(eye));
            }
            if (d > 0.0) {
                let hit = eye + nd * d;
                let c = cloud_color(cloud_noise(hit, d), n, d, modern);
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
    return vec4<f32>(flash(whiteout(rgb, length(dir)), in.world_position), 1.0);
}
