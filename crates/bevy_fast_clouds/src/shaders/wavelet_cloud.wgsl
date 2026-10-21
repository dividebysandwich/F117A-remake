// Hull march. J1/J2/J3 compile out fine scales, MARCH_STEPS is set per LOD, MULTISAMPLED
// reads a multisampled depth buffer, DITHER draws whole ramp colours and screen-door coverage
// with a 4x4 ordered dither (no blending), SUN_SHADOW samples the occupancy towards the sun
// for self shadowing (otherwise the occupancy at the sample stands in). RES_SCALE > 1 marches
// one pixel per RES_SCALE x RES_SCALE view pixels (clipped at the nearest of them).

#import bevy_render::view::View

struct GpuVolume {
    aabb_min: vec3<f32>,
    j_max: f32,
    aabb_max: vec3<f32>,
    h_max: f32,
    // Sphere offset, sphere count, seed, camera inside.
    misc: vec4<u32>,
    // Fill, core, darken (0..1).
    fill: vec4<f32>,
}

struct CloudGlobals {
    // Direction to the sun, haze distance.
    sun: vec4<f32>,
    // Max march span, sigma_t, phase g, LOD false colour.
    params0: vec4<f32>,
    // j cap, keep alpha, interior span, frame counter.
    params1: vec4<f32>,
    // Resolution scale.
    params2: vec4<f32>,
    flash: vec4<f32>,
    // Line light: one end and intensity, the other end and reach, colour.
    line_a: vec4<f32>,
    line_b: vec4<f32>,
    line_color: vec4<f32>,
    // Fog over the clouds: colour, amount.
    fog: vec4<f32>,
    // Shade ramp: 0 sunlit (and the haze colour) .. 15 darkest.
    ramp: array<vec4<f32>, 16>,
}

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) @interpolate(flat) volume_index: u32,
}

#ifdef MULTISAMPLED
@group(0) @binding(0) var depth_texture: texture_depth_multisampled_2d;
#else
@group(0) @binding(0) var depth_texture: texture_depth_2d;
#endif
@group(0) @binding(1) var<uniform> view: View;
@group(0) @binding(2) var<uniform> globals: CloudGlobals;
@group(0) @binding(3) var<storage, read> volumes: array<GpuVolume>;
@group(0) @binding(4) var<storage, read> spheres: array<vec4<f32>>;

const PI: f32 = 3.141592653589793;
const HALF_PI: f32 = 1.5707963267948966;
const INV_SQRT3: f32 = 0.57735026919;
const INV_SQRT2: f32 = 0.70710678118;
const MARCH_STEPS: i32 = #{MARCH_STEPS};
const RES_SCALE: i32 = #{RES_SCALE};

const N0: vec3<f32> = vec3<f32>(1.0, 0.0, 0.0);
const E1_0: vec3<f32> = vec3<f32>(0.0, 1.0, 0.0);
const N1: vec3<f32> = vec3<f32>(0.0, 1.0, 0.0);
const E1_1: vec3<f32> = vec3<f32>(0.0, 0.0, 1.0);
const N2: vec3<f32> = vec3<f32>(0.0, 0.0, 1.0);
const E1_2: vec3<f32> = vec3<f32>(1.0, 0.0, 0.0);
const N3: vec3<f32> = vec3<f32>(INV_SQRT3, INV_SQRT3, INV_SQRT3);
const E1_3: vec3<f32> = vec3<f32>(INV_SQRT2, -INV_SQRT2, 0.0);
const N4: vec3<f32> = vec3<f32>(INV_SQRT3, -INV_SQRT3, INV_SQRT3);
const E1_4: vec3<f32> = vec3<f32>(INV_SQRT2, INV_SQRT2, 0.0);
const N5: vec3<f32> = vec3<f32>(INV_SQRT3, INV_SQRT3, -INV_SQRT3);
const E1_5: vec3<f32> = vec3<f32>(INV_SQRT2, -INV_SQRT2, 0.0);

const CUBE_UNIT: array<vec3<f32>, 36> = array<vec3<f32>, 36>(
    vec3(0.0, 0.0, 1.0), vec3(1.0, 0.0, 1.0), vec3(1.0, 1.0, 1.0),
    vec3(0.0, 0.0, 1.0), vec3(1.0, 1.0, 1.0), vec3(0.0, 1.0, 1.0),
    vec3(1.0, 0.0, 0.0), vec3(0.0, 0.0, 0.0), vec3(0.0, 1.0, 0.0),
    vec3(1.0, 0.0, 0.0), vec3(0.0, 1.0, 0.0), vec3(1.0, 1.0, 0.0),
    vec3(1.0, 0.0, 1.0), vec3(1.0, 0.0, 0.0), vec3(1.0, 1.0, 0.0),
    vec3(1.0, 0.0, 1.0), vec3(1.0, 1.0, 0.0), vec3(1.0, 1.0, 1.0),
    vec3(0.0, 0.0, 0.0), vec3(0.0, 0.0, 1.0), vec3(0.0, 1.0, 1.0),
    vec3(0.0, 0.0, 0.0), vec3(0.0, 1.0, 1.0), vec3(0.0, 1.0, 0.0),
    vec3(0.0, 1.0, 1.0), vec3(1.0, 1.0, 1.0), vec3(1.0, 1.0, 0.0),
    vec3(0.0, 1.0, 1.0), vec3(1.0, 1.0, 0.0), vec3(0.0, 1.0, 0.0),
    vec3(0.0, 0.0, 0.0), vec3(1.0, 0.0, 0.0), vec3(1.0, 0.0, 1.0),
    vec3(0.0, 0.0, 0.0), vec3(1.0, 0.0, 1.0), vec3(0.0, 0.0, 1.0),
);

@vertex
fn vertex(@builtin(vertex_index) vid: u32, @builtin(instance_index) inst: u32) -> VsOut {
    let vol = volumes[inst];
    let local = CUBE_UNIT[vid];
    let world = mix(vol.aabb_min, vol.aabb_max, local);
    var out: VsOut;
    out.clip = view.clip_from_world * vec4(world, 1.0);
    out.world = world;
    out.volume_index = inst;
    return out;
}

fn sat(x: f32) -> f32 {
    return clamp(x, 0.0, 1.0);
}

fn malvar_window(tau: f32) -> f32 {
    let t = clamp(tau, -1.0, 1.0);
    return sin(HALF_PI * (t + 1.0));
}

fn dst_iv_dot(tau: f32, c: vec4<f32>) -> f32 {
    let a = PI * tau;
    return sin(0.5 * a) * c.x + sin(1.5 * a) * c.y + sin(2.5 * a) * c.z + sin(3.5 * a) * c.w;
}

fn hash_u32(x_in: u32) -> f32 {
    var x = x_in * 747796405u + 2891336453u;
    var word = ((x >> ((x >> 28u) + 4u)) ^ x) * 277803737u;
    word = (word >> 22u) ^ word;
    return f32(word >> 8u) / 16777215.0;
}

fn cell_hash(j: u32, k: i32, orient: u32, seed: u32) -> f32 {
    return hash_u32(seed + j * 1973u + u32(k) * 9277u + orient * 3697u);
}

fn coeffs(j: u32, k: i32, orient: u32, seed: u32, amp: f32, mask: vec4<f32>) -> vec4<f32> {
    let h = cell_hash(j, k, orient, seed);
    return vec4<f32>(1.0, 0.70, 0.45, 0.28) * (0.40 + 0.60 * h) * amp * mask;
}

fn ridge(
    uv: vec3<f32>,
    n: vec3<f32>,
    e1: vec3<f32>,
    j: u32,
    orient: u32,
    seed: u32,
    amp: f32,
    mask: vec4<f32>,
    rd: vec3<f32>,
) -> f32 {
    let orient_keep = 1.0 - smoothstep(0.55, 0.92, abs(dot(n, rd)));
    let scale = exp2(f32(j));
    let t_n = scale * dot(uv, n);
    let k0 = floor(t_n);
    let tau0 = t_n - k0;
    let tau1 = tau0 - 1.0;
    let c0 = coeffs(j, i32(k0), orient, seed, amp, mask);
    let c1 = coeffs(j, i32(k0) + 1, orient, seed, amp, mask);
    let packet = malvar_window(tau0) * dst_iv_dot(tau0, c0)
        + malvar_window(tau1) * dst_iv_dot(tau1, c1);
    let frac = scale * dot(uv, e1) - floor(scale * dot(uv, e1));
    let trans = 0.5 * (1.0 + cos(PI * clamp(2.0 * frac - 1.0, -1.0, 1.0)));
    return packet * trans * orient_keep;
}

fn sphere_occ(p: vec3<f32>, offset: u32, local: u32, count: u32) -> f32 {
    let live = select(0.0, 1.0, local < count);
    let s = spheres[min(offset + local, arrayLength(&spheres) - 1u)];
    let d = p - s.xyz;
    let x = 1.0 - sqrt(dot(d, d)) / max(s.w, 0.001);
    let t = sat(x);
    return live * t * t * (3.0 - 2.0 * t);
}

fn occupancy_volume(vol: GpuVolume, p: vec3<f32>) -> f32 {
    let offset = vol.misc.x;
    let count = vol.misc.y;
    var d = 0.0;
    d = max(d, sphere_occ(p, offset, 0u, count));
    d = max(d, sphere_occ(p, offset, 1u, count));
    d = max(d, sphere_occ(p, offset, 2u, count));
    d = max(d, sphere_occ(p, offset, 3u, count));
    d = max(d, sphere_occ(p, offset, 4u, count));
    d = max(d, sphere_occ(p, offset, 5u, count));
    d = max(d, sphere_occ(p, offset, 6u, count));
    d = max(d, sphere_occ(p, offset, 7u, count));
    d = max(d, sphere_occ(p, offset, 8u, count));
    d = max(d, sphere_occ(p, offset, 9u, count));
    d = max(d, sphere_occ(p, offset, 10u, count));
    d = max(d, sphere_occ(p, offset, 11u, count));
    return d;
}

fn ridge_band(
    uv: vec3<f32>,
    j: u32,
    seed: u32,
    occ: f32,
    mask: vec4<f32>,
    rd: vec3<f32>,
) -> f32 {
    let amp = occ / (1.0 + f32(j));
    let keep = mix(1.0, sat((globals.params1.y - cell_hash(j, 0, 0u, seed) + 0.001) * 80.0), sat(f32(j) - 1.0));
    return keep * (
        ridge(uv, N0, E1_0, j, 0u, seed, amp, mask, rd)
        + ridge(uv, N1, E1_1, j, 1u, seed, amp, mask, rd)
        + ridge(uv, N2, E1_2, j, 2u, seed, amp, mask, rd)
        + ridge(uv, N3, E1_3, j, 3u, seed, amp, mask, rd)
        + ridge(uv, N4, E1_4, j, 4u, seed, amp, mask, rd)
        + ridge(uv, N5, E1_5, j, 5u, seed, amp, mask, rd)
    );
}

// Density and occupancy at p.
fn density_volume(vol: GpuVolume, p: vec3<f32>, rd: vec3<f32>) -> vec2<f32> {
    let fill = max(vol.fill.x, 0.05);
    let occ = sat(occupancy_volume(vol, p) * fill);
    if occ < 0.02 {
        return vec2<f32>(0.0, occ);
    }
    let extent = max(vol.aabb_max - vol.aabb_min, vec3<f32>(0.001));
    let uv_raw = (p - vol.aabb_min) / extent;
    let uv = clamp(uv_raw, vec3<f32>(0.0), vec3<f32>(1.0));
    let outside = min(uv_raw, vec3<f32>(1.0) - uv_raw);
    let inside = sat(min(outside.x, min(outside.y, outside.z)) * 64.0);
    let mask = vec4<f32>(
        sat(vol.h_max),
        sat(vol.h_max - 1.0),
        sat(vol.h_max - 2.0),
        sat(vol.h_max - 3.0),
    );
    let seed = vol.misc.z;
    var rho = ridge_band(uv, 0u, seed, occ, mask, rd);
#ifdef J1
    rho += ridge_band(uv, 1u, seed, occ, mask, rd);
#endif
#ifdef J2
    rho += ridge_band(uv, 2u, seed, occ, mask, rd);
#endif
#ifdef J3
    rho += ridge_band(uv, 3u, seed, occ, mask, rd);
#endif
    // Solid core from the occupancy, the ridges eroding and billowing its edges.
    let core = vol.fill.y;
    return vec2<f32>(max(mix(rho * (0.35 + 0.75 * occ), occ * 1.4 - 0.45 + rho * 0.9, core), 0.0) * inside * fill, occ);
}

fn ray_aabb(ro: vec3<f32>, rd: vec3<f32>, bmin: vec3<f32>, bmax: vec3<f32>) -> vec2<f32> {
    let inv = 1.0 / rd;
    let t0 = (bmin - ro) * inv;
    let t1 = (bmax - ro) * inv;
    let tmin3 = min(t0, t1);
    let tmax3 = max(t0, t1);
    let tmin = max(max(tmin3.x, tmin3.y), tmin3.z);
    let tmax = min(min(tmax3.x, tmax3.y), tmax3.z);
    return vec2<f32>(tmin, tmax);
}

// Where the ray is inside any of the volume's spheres: (first entry, last exit).
fn sphere_span(vol: GpuVolume, ro: vec3<f32>, rd: vec3<f32>) -> vec2<f32> {
    var span = vec2<f32>(1e20, -1e20);
    let last = arrayLength(&spheres) - 1u;
    for (var i = 0u; i < 12u; i++) {
        if (i >= vol.misc.y) { break; }
        let s = spheres[min(vol.misc.x + i, last)];
        let oc = ro - s.xyz;
        let b = dot(oc, rd);
        let h = b * b - (dot(oc, oc) - s.w * s.w);
        if (h > 0.0) {
            let q = sqrt(h);
            span = vec2<f32>(min(span.x, -b - q), max(span.y, -b + q));
        }
    }
    return span;
}

fn lod_tint(j_max: f32, j_cap: f32) -> vec3<f32> {
    let t = j_max / max(j_cap, 1.0);
    return mix(vec3<f32>(0.25, 0.45, 1.0), vec3<f32>(1.0, 0.85, 0.35), t);
}

fn hg_phase(cos_theta: f32, g: f32) -> f32 {
    let g2 = g * g;
    return (1.0 - g2) / max(pow(1.0 + g2 - 2.0 * g * cos_theta, 1.5), 0.001);
}

// Light of the line light at p (a bright core around the channel and a wide, weaker halo).
fn line_glow(p: vec3<f32>) -> f32 {
    let i = globals.line_a.w;
    if (i <= 0.0) { return 0.0; }
    let a = globals.line_a.xyz;
    let ab = globals.line_b.xyz - a;
    let t = sat(dot(p - a, ab) / max(dot(ab, ab), 1e-3));
    let d = length(p - (a + ab * t));
    let reach = globals.line_b.w;
    let core = d / (reach * 0.06);
    let halo = d / (reach * 0.3);
    return i * (1.0 - smoothstep(0.3 * reach, reach, d)) * (1.6 / (1.0 + core * core) + 0.3 / (1.0 + halo * halo));
}

fn ramp(k: f32) -> vec3<f32> {
    let f = clamp(k, 0.0, 15.0);
    let i = u32(floor(f));
    return mix(globals.ramp[i].rgb, globals.ramp[min(i + 1u, 15u)].rgb, fract(f));
}

// 4x4 ordered dither threshold.
fn bayer4(p: vec2<u32>) -> f32 {
    var m = array<f32, 16>(0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0);
    return (m[(p.y & 3u) * 4u + (p.x & 3u)] + 0.5) / 16.0;
}

// Interleaved gradient noise, moving each frame: the march's start offset.
fn ign(p: vec2<f32>) -> f32 {
    let q = p + 5.588238 * f32(u32(globals.params1.w) % 64u);
    return fract(52.9829189 * fract(dot(q, vec2<f32>(0.06711056, 0.00583715))));
}

// Depth of the scene under a cloud pixel: at reduced resolution the nearest of its block's
// corners, so clouds never spread over the edges of nearer geometry.
fn scene_depth_at(pixel: vec2<i32>) -> f32 {
    if (RES_SCALE == 1) {
        return textureLoad(depth_texture, pixel, 0);
    }
    let last = vec2<i32>(textureDimensions(depth_texture)) - 1;
    let p = pixel * RES_SCALE;
    let q = min(p + (RES_SCALE - 1), last);
    return max(
        max(textureLoad(depth_texture, min(p, last), 0), textureLoad(depth_texture, vec2(q.x, p.y), 0)),
        max(textureLoad(depth_texture, vec2(p.x, q.y), 0), textureLoad(depth_texture, q, 0)),
    );
}

@fragment
fn fragment(in: VsOut) -> @location(0) vec4<f32> {
    let vol = volumes[in.volume_index];
    let cam = view.world_position;
    let interior = vol.misc.w != 0u;
    let to_hull = in.world - cam;
    let rd = to_hull / max(length(to_hull), 1e-5);

    // Distance to the opaque scene along the ray (reverse infinite z: depth = near / view z).
    let scene_depth = scene_depth_at(vec2<i32>(in.clip.xy));
    let near = view.clip_from_view[3][2];
    let forward = -view.world_from_view[2].xyz;
    let t_scene = select(1e9, near / scene_depth / max(dot(rd, forward), 1e-4), scene_depth > 0.0);

    let hull = ray_aabb(cam, rd, vol.aabb_min, vol.aabb_max);
    var t0 = max(hull.x, length(to_hull));
    var span_cap = globals.params0.x;
    if (interior) {
        t0 = 0.0;
        span_cap = globals.params1.z;
    }
    // Only where the ray passes through the spheres (the box's corners are empty).
    let spheres_hit = sphere_span(vol, cam, rd);
    t0 = max(t0, spheres_hit.x);
    let t1 = min(min(min(hull.y, spheres_hit.y), t_scene), t0 + span_cap);
    if (t1 <= t0) { discard; }
    let span = t1 - t0;
    let step = span / f32(MARCH_STEPS);

    let p_px = vec2<u32>(in.clip.xy);
#ifdef DITHER
    let jitter = bayer4(p_px + vec2<u32>(1u, 2u));
#else
    let jitter = ign(in.clip.xy);
#endif

    let sigma_t = globals.params0.y;
    let sun = globals.sun.xyz;
    let haze = globals.sun.w;
    // Forward scattering lightens clouds seen towards the sun.
    let forward_lift = 0.8 * (hg_phase(dot(rd, sun), globals.params0.z) - 1.0);
    let extent = max(vol.aabb_max - vol.aabb_min, vec3<f32>(0.001));
#ifdef SUN_SHADOW
    // Self shadow: occupancy a little way towards the sun.
    let shadow_reach = 0.3 * min(extent.x, min(extent.y, extent.z));
#endif

    var transmittance = 1.0;
    var shade_sum = 0.0;
    var glow_sum = 0.0;
    for (var s = 0; s < MARCH_STEPS; s = s + 1) {
        if transmittance < 0.03 {
            break;
        }
        let t = t0 + (f32(s) + jitter) * step;
        let p = cam + rd * t;
        let d = density_volume(vol, p, rd);
        let dens = d.x;
        if (dens <= 0.0) {
            continue;
        }
        let a = 1.0 - exp(-sigma_t * dens * step);
        let w = transmittance * a;
        // Ramp entry: tops lit, bases and the side away from the sun in shadow.
        let height = sat((p.y - vol.aabb_min.y) / extent.y);
#ifdef SUN_SHADOW
        let shadow = occupancy_volume(vol, p + sun * shadow_reach);
#else
        let shadow = 0.7 * d.y;
#endif
        var k = 1.0 + 6.0 * shadow + 4.0 * (1.0 - height) - forward_lift * (1.0 - shadow);
        // Haze: down the ramp towards entry 0 with distance.
        if (haze > 0.0) {
            k -= t / haze * 15.0;
        }
        shade_sum += w * max(k, 0.0);
        glow_sum += w * line_glow(p);
        transmittance *= 1.0 - a;
    }

    let alpha = 1.0 - transmittance;
    if (alpha <= 0.0) { discard; }
    let k = shade_sum / alpha;
    let glow = glow_sum / alpha;
#ifdef DITHER
    // Screen door coverage and whole ramp entries, as the original drew its dithered fills.
    if (alpha < bayer4(p_px)) { discard; }
    var rgb = globals.ramp[u32(clamp(floor(k + bayer4(p_px + vec2<u32>(2u, 3u))), 0.0, 15.0))].rgb;
#else
    var rgb = ramp(k);
#endif
    // Dark clouds (storm cells) stay dark whatever the ramp and the haze.
    rgb *= 1.0 - vol.fill.z;
    rgb += globals.flash.rgb + globals.line_color.rgb * glow;
    rgb = mix(rgb, globals.fog.rgb, globals.fog.a);
    if (globals.params0.w > 0.0) {
        rgb = lod_tint(vol.j_max, globals.params1.x);
    }
#ifdef DITHER
    return vec4<f32>(rgb, 1.0);
#else
    return vec4<f32>(rgb * alpha, alpha);
#endif
}
