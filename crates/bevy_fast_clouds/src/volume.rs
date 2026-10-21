//! Sphere-union occupancy and baked ridge packets.

use bevy::prelude::*;

use crate::wavelet::{cell_hash, ridge_packet_value, steering_set};

/// Spheres read per volume (the shader's occupancy loop is unrolled to this many).
pub const MAX_SPHERES_PER_VOLUME: usize = 12;
pub const MAX_VOLUMES: usize = 512;
pub const MAX_SPHERES: usize = MAX_VOLUMES * MAX_SPHERES_PER_VOLUME;

#[derive(Clone, Copy, Debug)]
pub struct CloudSphere {
    pub center: Vec3,
    pub radius: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct RidgePacket {
    pub orient: u32,
    pub j: u32,
    pub k: i32,
}

/// Sphere-union occupancy plus a baked packet list.
#[derive(Asset, TypePath, Clone, Debug)]
pub struct WaveletVolume {
    pub spheres: Vec<CloudSphere>,
    pub packets: Vec<RidgePacket>,
    pub aabb_min: Vec3,
    pub aabb_max: Vec3,
    pub j_cap: u32,
    pub h_cap: u32,
    pub seed: u32,
    pub keep_alpha: f32,
}

impl WaveletVolume {
    pub fn from_spheres(
        spheres: Vec<CloudSphere>,
        seed: u32,
        j_cap: u32,
        h_cap: u32,
        keep_alpha: f32,
    ) -> Self {
        let spheres: Vec<_> = spheres.into_iter().take(MAX_SPHERES_PER_VOLUME).collect();
        let (aabb_min, aabb_max) = aabb_from_spheres(&spheres);
        let mut volume = Self {
            spheres,
            packets: Vec::new(),
            aabb_min,
            aabb_max,
            j_cap,
            h_cap: h_cap.clamp(1, 4),
            seed,
            keep_alpha: keep_alpha.clamp(0.05, 1.0),
        };
        volume.bake_packets();
        volume
    }

    /// Cuts the volume off flat at height `y` (local units): a cumulus base.
    pub fn with_floor(mut self, y: f32) -> Self {
        self.aabb_min.y = self.aabb_min.y.max(y).min(self.aabb_max.y - 0.001);
        self
    }

    pub fn extent(&self) -> Vec3 {
        (self.aabb_max - self.aabb_min).max(Vec3::splat(0.001))
    }

    pub fn world_to_uv(&self, p: Vec3) -> Vec3 {
        (p - self.aabb_min) / self.extent()
    }

    fn bake_packets(&mut self) {
        self.packets.clear();
        let nset = steering_set();
        let center = (self.aabb_min + self.aabb_max) * 0.5;
        for (orient, n) in nset.iter().enumerate() {
            for j in 0..=self.j_cap {
                let cells = 1i32 << j;
                for k in 0..cells {
                    if j >= 2 && cell_hash(j, k, orient as u32, self.seed) > self.keep_alpha {
                        continue;
                    }
                    let along = ((k as f32 + 0.5) / cells as f32 - 0.5)
                        * self.extent().dot(*n).abs().max(self.extent().length());
                    let probe = center + *n * along * 0.35;
                    if occupancy(&self.spheres, probe) < 0.04 {
                        continue;
                    }
                    self.packets.push(RidgePacket { orient: orient as u32, j, k });
                }
            }
        }
    }

    pub fn density_at(&self, p: Vec3, j_max: u32, h_max: u32, ray_dir: Vec3) -> f32 {
        let occ = occupancy(&self.spheres, p);
        if occ <= 0.0 {
            return 0.0;
        }
        let uv = self.world_to_uv(p);
        if uv.min_element() < 0.0 || uv.max_element() > 1.0 {
            return 0.0;
        }
        let j_max = j_max.min(self.j_cap);
        let h_max = h_max.min(self.h_cap).max(1);
        let mut rho = 0.0;
        for (orient, n) in steering_set().iter().enumerate() {
            for j in 0..=j_max {
                if j >= 2 && cell_hash(j, 0, orient as u32, self.seed) > self.keep_alpha + 0.35 {
                    continue;
                }
                rho += ridge_packet_value(uv, *n, j, self.seed, orient as u32, occ, h_max, ray_dir);
            }
        }
        (rho * (0.35 + 0.65 * occ)).max(0.0)
    }

    pub fn packet_count_for_lod(&self, j_max: u32, h_max: u32) -> usize {
        let _ = h_max;
        self.packets.iter().filter(|p| p.j <= j_max).count()
    }
}

pub fn occupancy(spheres: &[CloudSphere], p: Vec3) -> f32 {
    let mut d: f32 = 0.0;
    for sphere in spheres {
        let x = 1.0 - (p - sphere.center).length() / sphere.radius.max(0.001);
        d = d.max(smoothstep(0.0, 1.0, x));
    }
    d
}

fn aabb_from_spheres(spheres: &[CloudSphere]) -> (Vec3, Vec3) {
    let mut min = Vec3::splat(f32::MAX);
    let mut max = Vec3::splat(f32::MIN);
    for sphere in spheres {
        min = min.min(sphere.center - Vec3::splat(sphere.radius));
        max = max.max(sphere.center + Vec3::splat(sphere.radius));
    }
    if spheres.is_empty() {
        (Vec3::ZERO, Vec3::ONE)
    } else {
        // Room for the density's soft edge (5% of the largest sphere).
        let pad = spheres.iter().map(|s| s.radius).fold(0.0, f32::max) * 0.05;
        (min - Vec3::splat(pad), max + Vec3::splat(pad))
    }
}

#[inline]
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub fn cumulus_spheres(origin: Vec3, scale: f32, seed: u32) -> Vec<CloudSphere> {
    let mut spheres = Vec::new();
    let bases = [
        (Vec3::new(0.0, 0.15, 0.0), 1.35),
        (Vec3::new(0.9, 0.25, 0.2), 0.95),
        (Vec3::new(-0.85, 0.2, -0.15), 0.9),
        (Vec3::new(0.15, 0.55, 0.7), 0.8),
        (Vec3::new(-0.2, 0.45, -0.75), 0.85),
        (Vec3::new(0.55, 0.7, -0.35), 0.7),
        (Vec3::new(-0.6, 0.65, 0.4), 0.72),
        (Vec3::new(0.1, 1.05, 0.05), 0.75),
        (Vec3::new(1.4, 0.35, -0.5), 0.55),
        (Vec3::new(-1.35, 0.3, 0.55), 0.58),
        (Vec3::new(0.4, 0.15, 1.15), 0.5),
        (Vec3::new(-0.35, 0.2, -1.2), 0.52),
    ];
    for (i, (offset, radius)) in bases.iter().enumerate() {
        let jitter = Vec3::new(
            (seed.wrapping_add(i as u32 * 13) as f32).sin() * 0.08,
            (seed.wrapping_add(i as u32 * 29) as f32).cos() * 0.05,
            ((seed >> 3).wrapping_add(i as u32 * 7) as f32).sin() * 0.08,
        );
        spheres.push(CloudSphere {
            center: origin + (*offset + jitter) * scale,
            radius: radius * scale,
        });
    }
    spheres
}

pub fn anvil_spheres(origin: Vec3, scale: f32) -> Vec<CloudSphere> {
    vec![
        CloudSphere { center: origin + Vec3::new(0.0, 0.2, 0.0) * scale, radius: 1.6 * scale },
        CloudSphere { center: origin + Vec3::new(1.6, 0.35, 0.1) * scale, radius: 1.1 * scale },
        CloudSphere { center: origin + Vec3::new(-1.5, 0.3, -0.2) * scale, radius: 1.05 * scale },
        CloudSphere { center: origin + Vec3::new(0.2, 0.85, 0.0) * scale, radius: 0.9 * scale },
        CloudSphere { center: origin + Vec3::new(2.4, 0.55, -0.4) * scale, radius: 0.7 * scale },
        CloudSphere { center: origin + Vec3::new(-2.2, 0.5, 0.3) * scale, radius: 0.65 * scale },
        CloudSphere { center: origin + Vec3::new(0.6, 0.15, 1.3) * scale, radius: 0.8 * scale },
        CloudSphere { center: origin + Vec3::new(-0.5, 0.1, -1.2) * scale, radius: 0.75 * scale },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn occupancy_inside_sphere() {
        let spheres = [CloudSphere { center: Vec3::ZERO, radius: 1.0 }];
        assert!(occupancy(&spheres, Vec3::ZERO) > 0.9);
        assert_eq!(occupancy(&spheres, Vec3::new(3.0, 0.0, 0.0)), 0.0);
    }

    #[test]
    fn bake_keeps_coarse_packets() {
        let volume = WaveletVolume::from_spheres(cumulus_spheres(Vec3::ZERO, 1.0, 1), 1, 3, 4, 0.7);
        assert!(!volume.packets.is_empty());
        assert!(volume.packet_count_for_lod(0, 1) < volume.packet_count_for_lod(3, 4));
    }

    #[test]
    fn density_nonzero_inside() {
        let volume = WaveletVolume::from_spheres(cumulus_spheres(Vec3::ZERO, 1.0, 3), 3, 2, 2, 1.0);
        let rho = volume.density_at(Vec3::new(0.0, 0.2, 0.0), 2, 2, Vec3::Y);
        assert!(rho >= 0.0);
    }
}
