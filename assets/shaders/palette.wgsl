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
const FLAG_CLOUD: u32 = 8u;
// Mesh tag bit of cloud puffs: smooth normals from the puff's centre, soft edges.
const TAG_CLOUD_VOLUME: u32 = 2048u;
// Cloud volume shape: cumulus (else a cumulonimbus tower).
const TAG_CLOUD_CUMULUS: u32 = 4096u;
// Ray-march steps through a cloud volume.
const CLOUD_STEPS: u32 = 14u;
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
    // x: local light count, y: daylight 0 (night) .. 1 (day).
    light_info: vec4<f32>,
    // x: XR dot angular size in radians (0 = flat screen).
    dots: vec4<f32>,
    // Two entries per light: (position, range), (colour * intensity, unused).
    local_lights: array<vec4<f32>, 64>,
    // Weather: deck base / top (metres, eye height frame), deck present (z), whiteout 0..1 (w).
    clouds: vec4<f32>,
    // x: haze distance scale, y: lightning flash 0..1, z: visibility under clouds (metres).
    weather: vec4<f32>,
    // Cloud texture: floating origin (x, z) modulo NOISE_PERIOD, seed (z).
    noise: vec4<f32>,
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
    // offset of this cloud), eye (w: 1 cumulus, 2 tower), sun direction, scale (metres per
    // local unit on each axis).
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
    // Cloud volumes: the view ray and the sun in the cloud's local space (inverse of the
    // scaled rotation m = diag(1/s^2) m^T).
    out.puff_pos = vec4<f32>(0.0);
    out.puff_eye = vec4<f32>(0.0);
    out.puff_sun = vec3<f32>(0.0);
    out.puff_scale = vec3<f32>(1.0);
    if ((tag & TAG_CLOUD_VOLUME) != 0u) {
        let m = mat3x3<f32>(world_from_local[0].xyz, world_from_local[1].xyz, world_from_local[2].xyz);
        let s2 = vec3<f32>(dot(m[0], m[0]), dot(m[1], m[1]), dot(m[2], m[2]));
        let o = world_from_local[3].xyz;
        let eye_local = (transpose(m) * (view.world_position - o)) / s2;
        out.puff_pos = vec4<f32>(v.position, fract((o.x + o.z) * 0.000137) * 50.0);
        out.puff_eye = vec4<f32>(eye_local, select(2.0, 1.0, (tag & TAG_CLOUD_CUMULUS) != 0u));
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

    // Per-object haze (2AA0:0120): fog = clamp((depth/512 in object units) - sizeClass, 0, 31) >> 1
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

// --- Cloud volumes ---------------------------------------------------------------------------
// Cheap volumetric clouds: the cloud's proxy mesh (an ellipsoid or cylinder enclosing it) is
// drawn; per pixel the view ray is intersected with the shape analytically and marched in a
// few steps through a noise density, with Beer's law opacity and one sample towards the sun
// for self-shadowing. The light picks the grey palette ramp entry like any other cloud.

// Cumulus: unit sphere around (0, 0.45, 0) with vertical radius 0.55, cut flat at y = 0.
const CU_CENTER: f32 = 0.45;
const CU_RY: f32 = 0.55;
// Cumulonimbus: a column of radius ~0.9 with an anvil of 1.35 near the top.
const CB_BOUND: f32 = 1.4;

fn tower_radius(y: f32) -> f32 {
    let anvil = smoothstep(0.7, 0.86, y) * 0.45;
    let top = 1.0 - smoothstep(0.93, 1.0, y) * 0.45;
    return (0.9 + anvil) * top;
}

// Ray parameter range (t0, t1) inside the cloud's bounds (empty when t0 >= t1).
fn volume_bounds(ro: vec3<f32>, rd: vec3<f32>, cumulus: bool) -> vec2<f32> {
    if (cumulus) {
        let k = vec3<f32>(1.0, 1.0 / CU_RY, 1.0);
        let qo = (ro - vec3<f32>(0.0, CU_CENTER, 0.0)) * k;
        let qd = rd * k;
        let a = dot(qd, qd);
        let b = dot(qo, qd);
        let disc = b * b - a * (dot(qo, qo) - 1.0);
        if (disc < 0.0) { return vec2<f32>(1.0, 0.0); }
        let sq = sqrt(disc);
        var t = vec2<f32>((-b - sq) / a, (-b + sq) / a);
        // The flat base.
        if (abs(rd.y) > 1e-6) {
            let tb = -ro.y / rd.y;
            if (rd.y > 0.0) { t.x = max(t.x, tb); } else { t.y = min(t.y, tb); }
        } else if (ro.y < 0.0) {
            return vec2<f32>(1.0, 0.0);
        }
        return t;
    }
    let a = rd.x * rd.x + rd.z * rd.z;
    let b = ro.x * rd.x + ro.z * rd.z;
    let c = ro.x * ro.x + ro.z * ro.z - CB_BOUND * CB_BOUND;
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

// Density 0..1 at local point p. `aspect` scales the noise to the cloud's real proportions.
fn volume_density(p: vec3<f32>, cumulus: bool, aspect: vec3<f32>, seed: f32, detail: bool) -> f32 {
    var shape: f32;
    if (cumulus) {
        let q = (p - vec3<f32>(0.0, CU_CENTER, 0.0)) * vec3<f32>(1.0, 1.0 / CU_RY, 1.0);
        shape = (1.0 - length(q)) * smoothstep(0.0, 0.06, p.y);
    } else {
        shape = 1.0 - length(p.xz) / tower_radius(p.y);
    }
    if (shape < -0.45) { return 0.0; }
    let pn = p * aspect + vec3<f32>(seed);
    var n = perlin(pn * 1.6, 1024);
    if (detail) { n += 0.5 * perlin(pn * 4.1, 1024); }
    return clamp((shape + 0.55 * n) * 4.0, 0.0, 1.0);
}

fn cloud_volume(in: VertexOutput, front: bool, dir: vec3<f32>, modern: bool) -> vec4<f32> {
    let dist = length(dir);
    let cumulus = in.puff_eye.w < 1.5;
    let ro = in.puff_eye.xyz;
    let rd = in.puff_pos.xyz - ro;
    let tb = volume_bounds(ro, rd, cumulus);
    // The proxy is double sided: front faces from outside the bounds, back faces from inside.
    if (front == (tb.x <= 0.0)) { discard; }
    let t0 = max(tb.x, 0.0);
    // The march runs from the eye (t = 0) through this proxy point (t = 1); world height
    // eye.y + dir.y t. An overcast deck hides the part of a tower on its far side.
    var t1 = tb.y;
    let eye_y = view.world_position.y;
    if (globals.clouds.z != 0.0 && abs(dir.y) > 1e-6) {
        if (eye_y >= globals.clouds.y && dir.y < 0.0) {
            t1 = min(t1, (globals.clouds.y - eye_y) / dir.y);
        } else if (eye_y <= globals.clouds.x && dir.y > 0.0) {
            t1 = min(t1, (globals.clouds.x - eye_y) / dir.y);
        }
    }
    if (t1 <= t0) { discard; }
    // Noise in units of the horizontal radius; metres per unit of t.
    let aspect = in.puff_scale / in.puff_scale.x;
    let m_per_t = length(rd * in.puff_scale);
    let dt = (t1 - t0) / f32(CLOUD_STEPS);
    let ds = dt * m_per_t;
    let jitter = bayer4(vec2<u32>(in.clip_position.xy));
    let sun = in.puff_sun * 0.12;
    var trans = 1.0;
    var light = 0.0;
    for (var i = 0u; i < CLOUD_STEPS; i++) {
        let p = ro + rd * (t0 + (f32(i) + jitter) * dt);
        let dens = volume_density(p, cumulus, aspect, in.puff_pos.w, true);
        if (dens <= 0.001) { continue; }
        // Opaque after a few hundred metres of dense cloud.
        let a = 1.0 - exp(-dens * ds / 120.0);
        // Lit where the cloud thins towards the sun; tops brighter than bases.
        let towards_sun = volume_density(p + sun, cumulus, aspect, in.puff_pos.w, false);
        let lit = clamp(0.45 + (dens - towards_sun) * 1.2 + 0.35 * (p.y - 0.4), 0.0, 1.0);
        light += trans * a * lit;
        trans *= 1.0 - a;
        if (trans < 0.03) { break; }
    }
    var alpha = 1.0 - trans;
    if (alpha < 0.01) { discard; }
    let rgb = flash(whiteout(cloud_ramp((1.0 - light / alpha) * 11.0, dist, modern), dist));
    if (!modern) {
        // Classic: a screen door instead of blending.
        if (alpha < bayer4(vec2<u32>(in.clip_position.xy) + vec2<u32>(2u, 1u))) { discard; }
        alpha = 1.0;
    }
    return vec4<f32>(rgb, alpha);
}

// Lightning: a brief flash over the world.
fn flash(rgb: vec3<f32>) -> vec3<f32> {
    return rgb + vec3<f32>(0.55, 0.55, 0.65) * globals.weather.y;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let dir = in.world_position - view.world_position;
    let modern = globals.params.w != 0.0;
    if ((material.flags & FLAG_CLOUD) != 0u) {
        let d = length(dir);
        if (in.puff_eye.w > 0.0) {
            return cloud_volume(in, front, dir, modern);
        }
        var n = normalize(cross(dpdx(in.world_position), dpdy(in.world_position)));
        if (dot(n, dir) > 0.0) { n = -n; }
        let noise = cloud_noise(in.world_position, d);
        return vec4<f32>(flash(whiteout(cloud_color(noise, n, d, modern), d)), 1.0);
    }
    if ((material.flags & 1u) != 0u) {
        // Background: sky bands above the horizon, ground gradient below.
        if ((material.flags & 4u) != 0u) {
            return vec4<f32>(pal(select(0x7Cu, 0x88u, dir.y >= 0.0)), 1.0);
        }
        // Overcast: the deck's underside instead of the sky below it, its top instead of the
        // ground above it, grey inside it.
        let eye_y = view.world_position.y;
        let nd = normalize(dir);
        if (globals.clouds.z != 0.0) {
            if (eye_y > globals.clouds.x && eye_y < globals.clouds.y) {
                return vec4<f32>(flash(pal(CLOUD_RAMP + 2u)), 1.0);
            }
            if (eye_y <= globals.clouds.x && nd.y >= 0.0) {
                let d = (globals.clouds.x - eye_y) / max(nd.y, 1e-4);
                let hit = view.world_position + nd * d;
                let c = cloud_color(cloud_noise(hit, d), vec3<f32>(0.0, -1.0, 0.0), d, modern);
                return vec4<f32>(flash(whiteout(c, d)), 1.0);
            }
            if (eye_y >= globals.clouds.y && nd.y < 0.0) {
                let d = (eye_y - globals.clouds.y) / max(-nd.y, 1e-4);
                let hit = view.world_position + nd * d;
                let c = cloud_color(cloud_noise(hit, d), vec3<f32>(0.0, 1.0, 0.0), d, modern);
                return vec4<f32>(whiteout(c, d), 1.0);
            }
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
        return vec4<f32>(flash(whiteout(bg, 1e4)), 1.0);
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
    return vec4<f32>(flash(whiteout(rgb, length(dir))), 1.0);
}
