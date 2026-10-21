//! `j_max` / `H_max` from camera distance or projected size.

use bevy::prelude::*;

#[derive(Resource, Clone, Copy, Debug)]
pub struct LodPolicy {
    pub d_ref: f32,
    pub j_min: u32,
    pub use_projected_size: bool,
    pub pixel_ref: f32,
}

impl Default for LodPolicy {
    fn default() -> Self {
        Self { d_ref: 8.0, j_min: 0, use_projected_size: false, pixel_ref: 220.0 }
    }
}

#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct CloudRenderSettings {
    pub j_cap: u32,
    pub h_cap: u32,
    pub keep_alpha: f32,
    /// March steps at the finest LOD; coarser LODs take fewer (`steps_for_lod`).
    pub step_count: u32,
    /// Longest stretch of a ray marched through one volume from outside (world units).
    pub max_march_span: f32,
    /// Stretch marched from a camera inside a volume (world units).
    pub interior_span: f32,
    /// Extinction per world unit at density 1.
    pub sigma_t: f32,
    pub phase_g: f32,
    pub lod_false_color: bool,
    /// Classic look: whole ramp colours and screen-door coverage with a 4x4 ordered dither
    /// instead of blending.
    pub dither: bool,
    /// Self shadow from a second occupancy sample towards the sun at every step.
    pub sun_shadow: bool,
    /// View pixels per marched pixel along each axis (1: full resolution; 2 marches a quarter
    /// of the pixels and upsamples them).
    pub resolution_scale: u32,
}

impl Default for CloudRenderSettings {
    fn default() -> Self {
        Self {
            j_cap: 3,
            h_cap: 4,
            keep_alpha: 0.72,
            step_count: 16,
            max_march_span: 22.0,
            interior_span: 6.0,
            sigma_t: 2.6,
            phase_g: 0.4,
            lod_false_color: false,
            dither: false,
            sun_shadow: true,
            resolution_scale: 1,
        }
    }
}

impl CloudRenderSettings {
    /// March steps of a volume drawn at LOD `j` (0 coarse .. 3 fine).
    pub fn steps_for_lod(&self, j: u32) -> u32 {
        (self.step_count * (j.min(3) + 5) / 8).max(4)
    }
}

pub fn lod_bands(
    distance: f32,
    extent: f32,
    camera_fov_y: f32,
    viewport_height: f32,
    policy: &LodPolicy,
    j_cap: u32,
    h_cap: u32,
) -> (u32, u32) {
    let j_cap = j_cap.max(policy.j_min);
    let (j_max, dist_for_h) = if policy.use_projected_size {
        let pixels = projected_height(distance, extent, camera_fov_y, viewport_height);
        let drops = (policy.pixel_ref / pixels.max(1.0)).log2().floor().max(0.0) as u32;
        (j_cap.saturating_sub(drops).max(policy.j_min), policy.pixel_ref / pixels.max(1.0))
    } else {
        let ratio = (distance / policy.d_ref).max(1.0);
        let drops = ratio.log2().floor().max(0.0) as u32;
        (j_cap.saturating_sub(drops).max(policy.j_min), ratio)
    };
    let h_max = if dist_for_h > 4.0 { 1 } else { h_cap.max(1) };
    (j_max.min(j_cap), h_max.min(h_cap.max(1)))
}

pub fn projected_height(distance: f32, extent: f32, fov_y: f32, viewport_height: f32) -> f32 {
    let world_height = (fov_y * 0.5).tan() * 2.0 * distance.max(0.001);
    extent / world_height.max(0.001) * viewport_height
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn farther_drops_bands() {
        let policy = LodPolicy::default();
        let near = lod_bands(4.0, 4.0, 0.8, 720.0, &policy, 4, 2);
        let far = lod_bands(64.0, 4.0, 0.8, 720.0, &policy, 4, 2);
        assert!(near.0 > far.0);
        assert_eq!(far.1, 1);
    }

    #[test]
    fn coarse_lods_march_fewer_steps() {
        let s = CloudRenderSettings::default();
        assert_eq!(s.steps_for_lod(3), 16);
        assert!(s.steps_for_lod(0) < s.steps_for_lod(3));
    }
}
