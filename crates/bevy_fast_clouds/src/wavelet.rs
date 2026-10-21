//! Ridge packets: DST-IV along `n`, Hann in the tangent plane.

use bevy::math::Vec3;
use std::f32::consts::PI;

pub const H_PACK: usize = 4;
pub const NUM_ORIENT: usize = 6;

#[inline]
pub fn hann_window(tau: f32) -> f32 {
    if tau.abs() >= 1.0 { 0.0 } else { 0.5 * (1.0 + (PI * tau).cos()) }
}

#[inline]
pub fn malvar_window(tau: f32) -> f32 {
    if tau.abs() >= 1.0 { 0.0 } else { (0.5 * PI * (tau + 1.0)).sin() }
}

#[inline]
pub fn dst_iv_row(tau: f32) -> [f32; H_PACK] {
    let a = PI * tau;
    [(0.5 * a).sin(), (1.5 * a).sin(), (2.5 * a).sin(), (3.5 * a).sin()]
}

#[inline]
pub fn dst_iv_dot(tau: f32, c: [f32; H_PACK]) -> f32 {
    let r = dst_iv_row(tau);
    r[0] * c[0] + r[1] * c[1] + r[2] * c[2] + r[3] * c[3]
}

#[inline]
pub fn h_mask(h_max: u32) -> [f32; H_PACK] {
    [
        if h_max >= 1 { 1.0 } else { 0.0 },
        if h_max >= 2 { 1.0 } else { 0.0 },
        if h_max >= 3 { 1.0 } else { 0.0 },
        if h_max >= 4 { 1.0 } else { 0.0 },
    ]
}

#[inline]
pub fn scale_amplitude(j: u32) -> f32 {
    1.0 / (1.0 + j as f32)
}

#[inline]
pub fn hash_u32(mut x: u32) -> f32 {
    x = x.wrapping_mul(747796405).wrapping_add(2891336453);
    let mut word = ((x >> ((x >> 28) + 4)) ^ x).wrapping_mul(277803737);
    word = (word >> 22) ^ word;
    (word >> 8) as f32 / 16_777_215.0
}

#[inline]
pub fn cell_hash(j: u32, k: i32, orient: u32, seed: u32) -> f32 {
    let packed = seed
        .wrapping_add(j.wrapping_mul(1973))
        .wrapping_add((k as u32).wrapping_mul(9277))
        .wrapping_add(orient.wrapping_mul(3697));
    hash_u32(packed)
}

pub fn harmonic_coeffs(j: u32, k: i32, orient: u32, seed: u32, occ: f32, h_max: u32) -> [f32; 4] {
    let mask = h_mask(h_max);
    let amp = occ * scale_amplitude(j);
    [
        amp * (0.40 + 0.60 * cell_hash(j, k, orient, seed)) * mask[0],
        amp * (0.40 + 0.60 * cell_hash(j + 17, k, orient, seed)) * 0.70 * mask[1],
        amp * (0.40 + 0.60 * cell_hash(j + 34, k, orient, seed)) * 0.45 * mask[2],
        amp * (0.40 + 0.60 * cell_hash(j + 51, k, orient, seed)) * 0.28 * mask[3],
    ]
}

pub fn steering_set() -> [Vec3; NUM_ORIENT] {
    let d = 1.0 / 3.0f32.sqrt();
    [Vec3::X, Vec3::Y, Vec3::Z, Vec3::new(d, d, d), Vec3::new(d, -d, d), Vec3::new(d, d, -d)]
}

pub fn ridge_frame(n: Vec3) -> (Vec3, Vec3) {
    let a = if n.z.abs() > 0.9 { Vec3::X } else { Vec3::Z };
    let e1 = n.cross(a).normalize();
    let e2 = n.cross(e1);
    (e1, e2)
}

#[inline]
pub fn lapped_scaling(t: f32) -> f32 {
    let k0 = t.floor();
    hann_window(t - k0) + hann_window(t - (k0 + 1.0))
}

pub fn ridge_packet_value(
    uv: Vec3,
    n: Vec3,
    j: u32,
    seed: u32,
    orient: u32,
    occ: f32,
    h_max: u32,
    ray_dir: Vec3,
) -> f32 {
    let (e1, e2) = ridge_frame(n);
    let align = ray_dir.dot(n).abs();
    let orient_keep = 1.0 - smoothstep(0.55, 0.92, align);
    if orient_keep <= 0.001 {
        return 0.0;
    }

    let scale = 2f32.powi(j as i32);
    let t_n = scale * uv.dot(n);
    let k0 = t_n.floor();
    let tau0 = t_n - k0;
    let tau1 = t_n - (k0 + 1.0);
    let c0 = harmonic_coeffs(j, k0 as i32, orient, seed, occ, h_max);
    let c1 = harmonic_coeffs(j, k0 as i32 + 1, orient, seed, occ, h_max);
    let ridge =
        malvar_window(tau0) * dst_iv_dot(tau0, c0) + malvar_window(tau1) * dst_iv_dot(tau1, c1);
    let trans = lapped_scaling(scale * uv.dot(e1)) * lapped_scaling(scale * uv.dot(e2));
    ridge * trans * orient_keep
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hann_pou_two_tap() {
        for i in 0..8 {
            let f = i as f32 / 8.0;
            let sum = hann_window(f) + hann_window(f - 1.0);
            assert!((sum - 1.0).abs() < 1e-5, "{sum}");
        }
    }

    #[test]
    fn dst_iv_row_is_odd_half_integers() {
        let at_zero = dst_iv_row(0.0);
        assert!(at_zero.iter().all(|v| v.abs() < 1e-5));
        let r = dst_iv_row(1.0);
        assert!((r[0] - 1.0).abs() < 1e-5);
        assert!((r[1] + 1.0).abs() < 1e-5);
        assert!(malvar_window(1.0).abs() < 1e-5);
    }

    #[test]
    fn aligned_ray_prunes_ridge() {
        let uv = Vec3::new(0.4, 0.5, 0.5);
        let n = Vec3::X;
        let keep = ridge_packet_value(uv, n, 2, 1, 0, 1.0, 4, Vec3::Y);
        let drop = ridge_packet_value(uv, n, 2, 1, 0, 1.0, 4, Vec3::X);
        assert!(keep.abs() > drop.abs());
    }
}
