// Blurred rotor discs (rotor_blur.rs): a ring around the rotor axis, child of the turning
// rotor part (so the ghost blades turn with the drawn rotor angle), with uv the position in the
// rotor's plane in rotor radii, the rotor turning towards increasing atan2(uv.y, uv.x).
// The look is layered from several effects of blades too fast to follow:
//   - the sweep haze: how much of the time a blade covers each point (denser at the root),
//   - ghost blades: a sharp leading edge and a motion-blur trail behind each blade,
//   - the blade roots and grips, a denser band just outside the hub,
//   - the tip path: the painted tip caps as a light ring at the rim,
//   - faint bands along the span (blade twist catching the light differently),
//   - a sun glint: a bright radial streak where the blades' faces reflect the sun to the eye,
//   - edge on, the disc thickens (the view ray crosses more of the swept volume),
//   - from below, the blades' shaded undersides.

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
    // z: FLIR (tracking camera), w: modern mode.
    params: vec4<f32>,
    // Direction to the sun (xyz), strength (w).
    sun: vec4<f32>,
    ambient: vec4<f32>,
    // y: daylight 0 (night) .. 1 (day), z: night vision goggles.
    light_info: vec4<f32>,
    // y: animation clock, zw: tracking camera picture size (FLIR view).
    dots: vec4<f32>,
    local_lights: array<vec4<f32>, 64>,
    clouds: vec4<f32>,
    weather: vec4<f32>,
    noise: vec4<f32>,
    stratus: vec4<f32>,
    bolt_a: vec4<f32>,
    bolt_b: vec4<f32>,
    sky_sun: vec4<f32>,
    sky_moon: vec4<f32>,
    // x: darkness of a night without moonlight, zw: the recon cameras' picture size.
    sky_night: vec4<f32>,
};

struct RotorMaterial {
    colour: u32,
    blades: u32,
    // Hub radius and blade chord (fractions of the rotor radius).
    hub: f32,
    chord: f32,
    // 0 (blades) .. 1 (disc).
    blur: f32,
    // Rotor axis in the part's frame: 0 = y, 1 = x, 2 = z.
    axis: u32,
    _pad: vec2<f32>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> material: RotorMaterial;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var<storage, read> globals: Globals;

const TAU: f32 = 6.28318530718;
// Brightness kept by aircraft on a dark night (palette.wgsl).
const AIRCRAFT_NIGHT: f32 = 0.95;
// Opacity of the sweep haze relative to the blades' coverage.
const SWEEP: f32 = 0.9;
// Opacity of the ghost blades, and their trail as a fraction of the angle between blades.
const GHOST: f32 = 0.2;
const TRAIL: f32 = 0.22;
// The blade roots: opacity and width (rotor radii) past the hub.
const ROOT: f32 = 0.22;
const ROOT_WIDTH: f32 = 0.1;
// The tip caps: opacity, inner edge (rotor radii) and brightness relative to the blades.
const TIP: f32 = 0.06;
const TIP_FROM: f32 = 0.945;
const TIP_LIGHT: f32 = 2.2;
// Span bands: strength and count across the radius.
const BANDS: f32 = 0.12;
const BAND_FREQ: f32 = 46.0;
// The sun glint: brightness, width (sine of its half angle) and how far from the disc normal
// the blades' faces turn (the half vector's cosine).
const GLINT: f32 = 0.7;
const GLINT_WIDTH: f32 = 0.035;
const GLINT_FACE: vec2<f32> = vec2<f32>(0.8, 0.97);
// Brightness of the blades' undersides.
const UNDERSIDE: f32 = 0.6;
const MAX_ALPHA: f32 = 0.9;

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    // The rotor plane's u and v directions and its normal, in the world.
    @location(2) @interpolate(flat) axis_u: vec3<f32>,
    @location(3) @interpolate(flat) axis_v: vec3<f32>,
    @location(4) @interpolate(flat) normal: vec3<f32>,
};

@vertex
fn vertex(v: Vertex) -> VertexOutput {
    let world_from_local = mesh_functions::get_world_from_local(v.instance_index);
    let world = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(v.position, 1.0));
    let m = mat3x3<f32>(world_from_local[0].xyz, world_from_local[1].xyz, world_from_local[2].xyz);
    // rotor_blur.rs `plane`: (u, v, axis) by the part's axis.
    var u = vec3<f32>(1.0, 0.0, 0.0);
    var w = vec3<f32>(0.0, 0.0, 1.0);
    var n = vec3<f32>(0.0, 1.0, 0.0);
    if (material.axis == 1u) {
        u = vec3<f32>(0.0, 0.0, 1.0);
        w = vec3<f32>(0.0, 1.0, 0.0);
        n = vec3<f32>(1.0, 0.0, 0.0);
    } else if (material.axis == 2u) {
        u = vec3<f32>(0.0, 1.0, 0.0);
        w = vec3<f32>(1.0, 0.0, 0.0);
        n = vec3<f32>(0.0, 0.0, 1.0);
    }
    var out: VertexOutput;
    out.world_position = world.xyz;
    out.clip_position = position_world_to_clip(world.xyz);
    out.uv = v.uv;
    out.axis_u = normalize(m * u);
    out.axis_v = normalize(m * w);
    out.normal = normalize(m * n);
    return out;
}

// Night vision goggles (trail.wgsl).
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
    let c = mix(vec3<f32>(0.0, 0.04, 0.01), vec3<f32>(0.3, 0.85, 0.2), v) + vec3<f32>(0.5, 0.15, 0.4) * v * v * v;
    return pow(min(c, vec3<f32>(1.0)), vec3<f32>(2.2));
}

fn bayer4(p: vec2<u32>) -> f32 {
    var m = array<f32, 16>(0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0);
    return (m[(p.y & 3u) * 4u + (p.x & 3u)] + 0.5) / 16.0;
}

// Opacity of a band between radii lo and hi, with soft edges `soft` wide.
fn band(r: f32, lo: f32, hi: f32, soft: f32) -> f32 {
    return smoothstep(lo - soft, lo + soft, r) * (1.0 - smoothstep(hi - soft, hi + soft, r));
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let r = length(in.uv);
    // Radii per pixel (derivatives before any branch: WebGPU's uniform control flow).
    let aa = max(fwidth(r), 1e-4);
    if (material.blur <= 0.0) {
        discard;
    }
    let n_blades = f32(material.blades);
    let chord = material.chord;
    let hub = material.hub;
    let theta = atan2(in.uv.y, in.uv.x);

    // Sweep haze: the fraction of time a blade covers this radius.
    let cover = clamp(n_blades * chord / (TAU * max(r, 0.05)), 0.0, 1.0);
    var a = cover * SWEEP;

    // Ghost blades: s is 0 on a blade, rising ahead of it (the way it turns) to 1 at the next.
    let s = fract(theta / TAU * n_blades);
    let gap = TAU * r / n_blades;
    let ahead = s * gap;
    let lead = 1.0 - smoothstep(chord * 0.5, chord * 0.5 + max(chord * 0.6, aa * 2.0), ahead);
    let trail = exp(-(1.0 - s) / TRAIL);
    a += max(lead, trail) * GHOST * mix(1.0, 0.55, r);

    // Blade roots, just outside the hub.
    let root = band(r, hub * 0.7, hub + ROOT_WIDTH, 0.02);
    a += root * ROOT;

    // Tip path ring.
    let tip = band(r, TIP_FROM, 0.995, max(aa, 0.006));
    a += tip * TIP;

    // Bands along the span (only where they don't shimmer: fade out when finer than pixels).
    let band_fade = 1.0 - smoothstep(0.2, 0.6, aa * BAND_FREQ);
    a *= 1.0 + BANDS * band_fade * sin(r * BAND_FREQ);

    // Edge on, the view ray crosses more of the swept volume: transmittance to the power of
    // the path length.
    let to_eye = view.world_position - in.world_position;
    let eye = normalize(to_eye);
    let facing = dot(eye, in.normal);
    let path = 1.0 / max(abs(facing), 0.12);
    a = 1.0 - pow(1.0 - clamp(a, 0.0, 0.99), path);

    // Soft rim and hub edges.
    a *= (1.0 - smoothstep(1.0 - max(aa * 1.5, 0.008), 1.0 + aa, r)) * smoothstep(hub * 0.5, hub * 0.75, r);
    a = min(a, MAX_ALPHA) * material.blur;

    // FLIR (tracking camera): the blades are cool.
    if (globals.params.z != 0.0 && all(abs(view.viewport.zw - globals.dots.zw) < vec2<f32>(0.5))) {
        return vec4<f32>(vec3<f32>(0.1), a);
    }

    var rgb = globals.palette[material.colour & 0xFFu].rgb;
    rgb = mix(rgb, rgb * TIP_LIGHT, tip);
    rgb = rgb * mix(1.0, 0.75, root);
    if (facing < 0.0) {
        rgb = rgb * UNDERSIDE;
    }
    // A dark night blacks out the aircraft (palette.wgsl `night_dark`, not in recon photos).
    let recon = all(abs(view.viewport.zw - globals.sky_night.zw) < vec2<f32>(0.5));
    let dark = select(globals.sky_night.x, 0.0, recon) * AIRCRAFT_NIGHT;
    rgb = rgb * (1.0 - dark);

    // Sun glint: a blade face reflects the sun to the eye where the blade runs across the half
    // vector's direction in the rotor plane, with the half vector near the disc normal.
    let sun = globals.sun.xyz;
    let sun_side = dot(sun, in.normal);
    if (globals.sun.w > 0.0 && sun_side * facing > 0.0) {
        let h = normalize(sun + eye);
        let hn = abs(dot(h, in.normal));
        let hp = vec2<f32>(dot(h, in.axis_u), dot(h, in.axis_v));
        let radial = in.uv / max(r, 1e-4);
        let across = dot(normalize(hp + vec2<f32>(1e-5, 0.0)), radial);
        let streak = exp(-across * across / (2.0 * GLINT_WIDTH * GLINT_WIDTH));
        let glint = streak * smoothstep(GLINT_FACE.x, GLINT_FACE.y, hn) * GLINT
            * clamp(globals.light_info.y, 0.0, 1.0) * (1.0 - dark) * smoothstep(hub, hub + 0.1, r)
            * (1.0 - smoothstep(0.97, 1.0, r)) * material.blur;
        rgb = rgb + vec3<f32>(1.0, 0.96, 0.88) * glint / max(a, 0.05);
        a = min(a + glint * 0.35, MAX_ALPHA);
    }

    // Classic mode: dithered like the smoke.
    if (globals.params.w == 0.0) {
        if (a < bayer4(vec2<u32>(in.clip_position.xy))) {
            discard;
        }
        return vec4<f32>(nvg(min(rgb, vec3<f32>(1.0)), in.clip_position), 1.0);
    }
    if (a <= 0.002) {
        discard;
    }
    return vec4<f32>(nvg(rgb, in.clip_position), a);
}
