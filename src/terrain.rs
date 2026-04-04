use bevy::{
    prelude::*,
    asset::RenderAssetUsages,
    camera::visibility::RenderLayers,
    render::render_resource::PrimitiveTopology,
};
use bevy_rapier3d::prelude::*;
use rand::prelude::*;
use rand::rngs::StdRng;

use crate::billboard::Billboard;
use crate::definitions::RENDERLAYER_POINTLIGHTS;
use crate::pointlight::*;

// ============================================================
// Constants
// ============================================================

const TERRAIN_SIZE: f32 = 10000.0;
const CHUNKS_PER_SIDE: usize = 8;
const CELLS_PER_CHUNK: usize = 64;
const GRID_RES: usize = CHUNKS_PER_SIDE * CELLS_PER_CHUNK; // 512
const CELL_SIZE: f32 = TERRAIN_SIZE / GRID_RES as f32;
const HALF_SIZE: f32 = TERRAIN_SIZE / 2.0;
const BASE_HEIGHT: f32 = -1.0;
const WATER_LEVEL: f32 = -2.5;
const MOUNTAIN_THRESHOLD: f32 = 0.42;
const MAX_MOUNTAIN_HEIGHT: f32 = 80.0;
const TERRAIN_SEED: u64 = 117;

// Airbase exclusion — mountains are suppressed within this radius of the origin
const AIRBASE_FLAT_RADIUS: f32 = 500.0;
const AIRBASE_TRANSITION: f32 = 300.0;

const NUM_CITIES: usize = 7;
const MIN_CITY_DIST: f32 = 800.0;
const CITY_RADIUS_MIN: f32 = 120.0;
const CITY_RADIUS_MAX: f32 = 300.0;
const BUILDINGS_PER_CITY_MIN: usize = 15;
const BUILDINGS_PER_CITY_MAX: usize = 45;

const ROAD_WIDTH: f32 = 0.4;
const ROAD_LIGHT_SPACING: f32 = 80.0;
const ROAD_MAX_HEIGHT: f32 = 6.0; // roads won't go above BASE_HEIGHT + this
const ROAD_SEGMENT_LEN: f32 = 25.0;

const FIELD_COUNT: usize = 30;
const FIELD_SIZE_MIN: f32 = 40.0;
const FIELD_SIZE_MAX: f32 = 120.0;

const RUNWAY_LENGTH: f32 = 172.0;
const RUNWAY_WIDTH: f32 = 5.0;
const RUNWAY_Y: f32 = -0.96;
const RUNWAY_Z: f32 = 0.5;

const NUM_TREES: usize = 400;
const NUM_FARMS: usize = 20;
const NUM_COMM_TOWERS: usize = 6;

// ============================================================
// Noise
// ============================================================

fn hash2d(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = seed;
    h ^= x as u32;
    h = h.wrapping_mul(0x9E3779B9);
    h ^= y as u32;
    h = h.wrapping_mul(0x517CC1B7);
    h ^= h >> 16;
    h = h.wrapping_mul(0x85EBCA6B);
    h ^= h >> 13;
    (h & 0x7FFFFFFF) as f32 / 0x7FFFFFFF as f32
}

fn smoothstep(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

fn value_noise(x: f32, y: f32, seed: u32) -> f32 {
    let ix = x.floor() as i32;
    let iy = y.floor() as i32;
    let fx = smoothstep(x - ix as f32);
    let fy = smoothstep(y - iy as f32);
    let v00 = hash2d(ix, iy, seed);
    let v10 = hash2d(ix + 1, iy, seed);
    let v01 = hash2d(ix, iy + 1, seed);
    let v11 = hash2d(ix + 1, iy + 1, seed);
    let a = v00 + fx * (v10 - v00);
    let b = v01 + fx * (v11 - v01);
    a + fy * (b - a)
}

fn fbm(x: f32, y: f32, octaves: u32, seed: u32) -> f32 {
    let mut val = 0.0;
    let mut amp = 1.0;
    let mut freq = 1.0;
    let mut max_amp = 0.0;
    for i in 0..octaves {
        val += value_noise(x * freq, y * freq, seed.wrapping_add(i * 31)) * amp;
        max_amp += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    val / max_amp
}

// ============================================================
// Terrain Data
// ============================================================

#[derive(Resource)]
pub struct TerrainData {
    pub heights: Vec<f32>,
    pub width: usize,
    pub depth: usize,
}

impl TerrainData {
    fn idx(width: usize, gx: usize, gz: usize) -> usize {
        gz * width + gx
    }

    pub fn get_height_world(&self, wx: f32, wz: f32) -> f32 {
        let fx = (wx + HALF_SIZE) / CELL_SIZE;
        let fz = (wz + HALF_SIZE) / CELL_SIZE;
        let gx = (fx.max(0.0) as usize).min(self.width - 2);
        let gz = (fz.max(0.0) as usize).min(self.depth - 2);
        let lx = (fx - gx as f32).clamp(0.0, 1.0);
        let lz = (fz - gz as f32).clamp(0.0, 1.0);
        let h00 = self.heights[Self::idx(self.width, gx, gz)];
        let h10 = self.heights[Self::idx(self.width, gx + 1, gz)];
        let h01 = self.heights[Self::idx(self.width, gx, gz + 1)];
        let h11 = self.heights[Self::idx(self.width, gx + 1, gz + 1)];
        let a = h00 + lx * (h10 - h00);
        let b = h01 + lx * (h11 - h01);
        a + lz * (b - a)
    }

    fn flatten_rect(&mut self, min_x: f32, min_z: f32, max_x: f32, max_z: f32, target_h: f32, margin: f32) {
        let w = self.width;
        for gz in 0..self.depth {
            for gx in 0..self.width {
                let wx = gx as f32 * CELL_SIZE - HALF_SIZE;
                let wz = gz as f32 * CELL_SIZE - HALF_SIZE;
                let dx = if wx < min_x { min_x - wx } else if wx > max_x { wx - max_x } else { 0.0 };
                let dz = if wz < min_z { min_z - wz } else if wz > max_z { wz - max_z } else { 0.0 };
                let d = (dx * dx + dz * dz).sqrt();
                if d <= 0.0 {
                    self.heights[Self::idx(w, gx, gz)] = target_h;
                } else if d < margin {
                    let blend = smoothstep(d / margin);
                    let idx = Self::idx(w, gx, gz);
                    self.heights[idx] = target_h * (1.0 - blend) + self.heights[idx] * blend;
                }
            }
        }
    }

    fn flatten_circle(&mut self, cx: f32, cz: f32, radius: f32, target_h: f32) {
        let outer = radius * 1.3;
        let outer2 = outer * outer;
        let r2 = radius * radius;
        let w = self.width;
        for gz in 0..self.depth {
            for gx in 0..self.width {
                let wx = gx as f32 * CELL_SIZE - HALF_SIZE;
                let wz = gz as f32 * CELL_SIZE - HALF_SIZE;
                let d2 = (wx - cx) * (wx - cx) + (wz - cz) * (wz - cz);
                if d2 < r2 {
                    self.heights[Self::idx(w, gx, gz)] = target_h;
                } else if d2 < outer2 {
                    let t = ((d2.sqrt() - radius) / (outer - radius)).clamp(0.0, 1.0);
                    let blend = smoothstep(t);
                    let idx = Self::idx(w, gx, gz);
                    self.heights[idx] = target_h * (1.0 - blend) + self.heights[idx] * blend;
                }
            }
        }
    }

    /// Flatten a corridor along a polyline path
    fn flatten_path(&mut self, points: &[Vec2], half_width: f32, target_h: f32) {
        let margin = half_width * 1.5;
        let w = self.width;
        for gz in 0..self.depth {
            for gx in 0..self.width {
                let wx = gx as f32 * CELL_SIZE - HALF_SIZE;
                let wz = gz as f32 * CELL_SIZE - HALF_SIZE;
                let p = Vec2::new(wx, wz);
                // Find closest distance to any segment of the path
                let mut min_dist = f32::MAX;
                for seg in points.windows(2) {
                    let a = seg[0];
                    let b = seg[1];
                    let ab = b - a;
                    let ap = p - a;
                    let t = (ap.dot(ab) / ab.dot(ab)).clamp(0.0, 1.0);
                    let closest = a + ab * t;
                    min_dist = min_dist.min((p - closest).length());
                }
                if min_dist < half_width {
                    self.heights[Self::idx(w, gx, gz)] = target_h;
                } else if min_dist < margin {
                    let blend = smoothstep((min_dist - half_width) / (margin - half_width));
                    let idx = Self::idx(w, gx, gz);
                    self.heights[idx] = target_h * (1.0 - blend) + self.heights[idx] * blend;
                }
            }
        }
    }
}

// ============================================================
// Feature Structures
// ============================================================

struct CityData {
    pos: Vec2,
    radius: f32,
}

struct RoadPath {
    waypoints: Vec<Vec2>,
}

struct FieldRect {
    center: Vec2,
    half_size: Vec2,
    color: Color,
}

// ============================================================
// Heightmap Generation
// ============================================================

fn generate_heightmap(seed: u32) -> TerrainData {
    let w = GRID_RES + 1;
    let d = GRID_RES + 1;
    let mut heights = vec![BASE_HEIGHT; w * d];

    for gz in 0..d {
        for gx in 0..w {
            let wx = gx as f32 * CELL_SIZE - HALF_SIZE;
            let wz = gz as f32 * CELL_SIZE - HALF_SIZE;

            let mountain_noise = fbm(wx * 0.0003, wz * 0.0003, 4, seed);
            let detail = fbm(wx * 0.001, wz * 0.001, 3, seed + 100);

            // Suppress mountains near the airbase so the runway is on flat ground
            let dist_from_origin = (wx * wx + wz * wz).sqrt();
            let suppression = if dist_from_origin < AIRBASE_FLAT_RADIUS {
                0.0
            } else if dist_from_origin < AIRBASE_FLAT_RADIUS + AIRBASE_TRANSITION {
                smoothstep((dist_from_origin - AIRBASE_FLAT_RADIUS) / AIRBASE_TRANSITION)
            } else {
                1.0
            };

            let mut h = BASE_HEIGHT;

            if mountain_noise > MOUNTAIN_THRESHOLD {
                let s = (mountain_noise - MOUNTAIN_THRESHOLD) / (1.0 - MOUNTAIN_THRESHOLD);
                h += s * s * MAX_MOUNTAIN_HEIGHT * suppression;
            }

            // Gentle undulation (also suppressed near airbase)
            h += (detail - 0.5) * 3.0 * suppression;

            // Lakes — only away from airbase
            let lake_noise = fbm(wx * 0.0005 + 50.0, wz * 0.0005 + 50.0, 3, seed + 200);
            if lake_noise < 0.25 && h < BASE_HEIGHT + 2.0 && dist_from_origin > AIRBASE_FLAT_RADIUS {
                h = BASE_HEIGHT + (lake_noise - 0.25) * 12.0;
            }

            heights[gz * w + gx] = h;
        }
    }

    TerrainData { heights, width: w, depth: d }
}

// ============================================================
// Feature Generation
// ============================================================

fn generate_cities(terrain: &TerrainData, rng: &mut StdRng) -> Vec<CityData> {
    let mut cities = Vec::new();
    let mut attempts = 0;
    while cities.len() < NUM_CITIES && attempts < 500 {
        attempts += 1;
        let x = rng.gen_range(-3000.0..4000.0_f32);
        let z = rng.gen_range(-3000.0..3000.0_f32);
        if x.abs() < 400.0 && z.abs() < 200.0 { continue; }
        let h = terrain.get_height_world(x, z);
        if h < WATER_LEVEL + 1.0 || h > BASE_HEIGHT + 8.0 { continue; }
        if cities.iter().any(|c: &CityData| (c.pos - Vec2::new(x, z)).length() < MIN_CITY_DIST) { continue; }
        cities.push(CityData { pos: Vec2::new(x, z), radius: rng.gen_range(CITY_RADIUS_MIN..CITY_RADIUS_MAX) });
    }
    cities
}

/// Route a road between two points, pushing waypoints around elevated terrain.
fn route_road(start: Vec2, end: Vec2, terrain: &TerrainData) -> Vec<Vec2> {
    let total_dir = end - start;
    let total_len = total_dir.length();
    let n = (total_len / ROAD_SEGMENT_LEN).ceil().max(2.0) as usize;
    let perp = Vec2::new(-total_dir.y, total_dir.x).normalize_or_zero();
    let max_h = BASE_HEIGHT + ROAD_MAX_HEIGHT;

    // Start with straight-line waypoints
    let mut pts: Vec<Vec2> = (0..=n).map(|i| start.lerp(end, i as f32 / n as f32)).collect();

    // Iteratively push interior waypoints away from high terrain
    for _ in 0..15 {
        let mut changed = false;
        for i in 1..pts.len() - 1 {
            let h = terrain.get_height_world(pts[i].x, pts[i].y);
            if h > max_h {
                let offset = 35.0;
                let c1 = pts[i] + perp * offset;
                let c2 = pts[i] - perp * offset;
                let h1 = terrain.get_height_world(c1.x, c1.y);
                let h2 = terrain.get_height_world(c2.x, c2.y);
                let best = if h1 < h2 { c1 } else { c2 };
                let best_h = h1.min(h2);
                if best_h < h {
                    pts[i] = best;
                    changed = true;
                }
            }
        }
        if !changed { break; }
    }

    // Smooth the path to avoid sharp zigzags (Laplacian smoothing)
    for _ in 0..3 {
        for i in 1..pts.len() - 1 {
            pts[i] = (pts[i - 1] + pts[i] * 2.0 + pts[i + 1]) / 4.0;
        }
    }

    pts
}

fn generate_roads(cities: &[CityData], terrain: &TerrainData) -> Vec<RoadPath> {
    let mut roads = Vec::new();
    if cities.is_empty() { return roads; }

    // Track which connections we've made to avoid duplicates
    let mut connected = std::collections::HashSet::<(usize, usize)>::new();

    for (i, city) in cities.iter().enumerate() {
        let mut dists: Vec<(usize, f32)> = cities.iter().enumerate()
            .filter(|(j, _)| *j != i)
            .map(|(j, other)| (j, (city.pos - other.pos).length()))
            .collect();
        dists.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
        for &(j, _) in dists.iter().take(2) {
            let key = if i < j { (i, j) } else { (j, i) };
            if connected.insert(key) {
                let waypoints = route_road(cities[key.0].pos, cities[key.1].pos, terrain);
                roads.push(RoadPath { waypoints });
            }
        }
    }

    // Road from airbase to nearest city
    if let Some(nearest) = cities.iter().min_by(|a, b| a.pos.length().partial_cmp(&b.pos.length()).unwrap()) {
        let waypoints = route_road(Vec2::ZERO, nearest.pos, terrain);
        roads.push(RoadPath { waypoints });
    }

    roads
}

fn generate_fields(terrain: &TerrainData, cities: &[CityData], rng: &mut StdRng) -> Vec<FieldRect> {
    let palette = [
        Color::srgb(0.55, 0.50, 0.25),
        Color::srgb(0.30, 0.50, 0.20),
        Color::srgb(0.45, 0.40, 0.20),
        Color::srgb(0.35, 0.55, 0.25),
    ];
    let mut fields = Vec::new();
    let mut attempts = 0;
    while fields.len() < FIELD_COUNT && attempts < 300 {
        attempts += 1;
        let x = rng.gen_range(-3500.0..4500.0_f32);
        let z = rng.gen_range(-3500.0..3500.0_f32);
        let h = terrain.get_height_world(x, z);
        if h < WATER_LEVEL + 1.0 || h > BASE_HEIGHT + 5.0 { continue; }
        if cities.iter().any(|c| (c.pos - Vec2::new(x, z)).length() < c.radius + 50.0) { continue; }
        if x.abs() < 300.0 && z.abs() < 100.0 { continue; }
        let hw = rng.gen_range(FIELD_SIZE_MIN..FIELD_SIZE_MAX) / 2.0;
        let hd = rng.gen_range(FIELD_SIZE_MIN..FIELD_SIZE_MAX) / 2.0;
        fields.push(FieldRect {
            center: Vec2::new(x, z), half_size: Vec2::new(hw, hd),
            color: palette[rng.gen_range(0..palette.len())],
        });
    }
    fields
}

// ============================================================
// Terrain Color
// ============================================================

fn terrain_color(height: f32, normal_y: f32) -> [f32; 4] {
    let rel = height - BASE_HEIGHT;
    if normal_y < 0.85 { return [0.45, 0.40, 0.35, 1.0]; }
    if height < WATER_LEVEL {
        [0.15, 0.22, 0.12, 1.0]
    } else if rel < 1.0 {
        [0.28, 0.38, 0.18, 1.0]
    } else if rel < 10.0 {
        let t = (rel - 1.0) / 9.0;
        [0.28 + t * 0.15, 0.38 - t * 0.05, 0.18 + t * 0.08, 1.0]
    } else if rel < 40.0 {
        let t = (rel - 10.0) / 30.0;
        [0.43 + t * 0.12, 0.33 + t * 0.12, 0.26 + t * 0.12, 1.0]
    } else {
        [0.58, 0.55, 0.50, 1.0]
    }
}

// ============================================================
// Mesh Creation
// ============================================================

fn build_chunk_mesh(data: &TerrainData, chunk_x: usize, chunk_z: usize) -> Mesh {
    let start_gx = chunk_x * CELLS_PER_CHUNK;
    let start_gz = chunk_z * CELLS_PER_CHUNK;
    let end_gx = (start_gx + CELLS_PER_CHUNK).min(data.width - 1);
    let end_gz = (start_gz + CELLS_PER_CHUNK).min(data.depth - 1);
    let cols = end_gx - start_gx;
    let rows = end_gz - start_gz;
    let vw = cols + 1;
    let vh = rows + 1;

    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(vw * vh);
    let mut normals: Vec<[f32; 3]> = Vec::with_capacity(vw * vh);
    let mut colors: Vec<[f32; 4]> = Vec::with_capacity(vw * vh);
    let mut indices: Vec<u32> = Vec::with_capacity(cols * rows * 6);

    for lz in 0..vh {
        for lx in 0..vw {
            let gx = start_gx + lx;
            let gz = start_gz + lz;
            let wx = gx as f32 * CELL_SIZE - HALF_SIZE;
            let wz = gz as f32 * CELL_SIZE - HALF_SIZE;
            let h = data.heights[TerrainData::idx(data.width, gx, gz)];

            positions.push([wx, h, wz]);

            let h_l = if gx > 0 { data.heights[TerrainData::idx(data.width, gx - 1, gz)] } else { h };
            let h_r = if gx < data.width - 1 { data.heights[TerrainData::idx(data.width, gx + 1, gz)] } else { h };
            let h_d = if gz > 0 { data.heights[TerrainData::idx(data.width, gx, gz - 1)] } else { h };
            let h_u = if gz < data.depth - 1 { data.heights[TerrainData::idx(data.width, gx, gz + 1)] } else { h };
            let n = Vec3::new(h_l - h_r, 2.0 * CELL_SIZE, h_d - h_u).normalize();
            normals.push(n.to_array());
            colors.push(terrain_color(h, n.y));
        }
    }

    for lz in 0..rows {
        for lx in 0..cols {
            let tl = (lz * vw + lx) as u32;
            let tr = tl + 1;
            let bl = tl + vw as u32;
            let br = bl + 1;
            indices.extend_from_slice(&[tl, bl, tr, tr, bl, br]);
        }
    }

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::all());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(bevy::mesh::Indices::U32(indices));
    mesh
}

fn make_ground_quad(width: f32, length: f32) -> Mesh {
    let hw = width / 2.0;
    let hl = length / 2.0;
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::all());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![
        [-hw, 0.0, -hl], [hw, 0.0, -hl], [hw, 0.0, hl], [-hw, 0.0, hl],
    ]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; 4]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
    mesh.insert_indices(bevy::mesh::Indices::U32(vec![0, 2, 1, 0, 3, 2]));
    mesh
}

// ============================================================
// Spawn Helpers
// ============================================================

fn spawn_billboard_light_entity(
    commands: &mut Commands,
    mesh: &Handle<Mesh>,
    material: &Handle<StandardMaterial>,
    position: Vec3,
    light_color: LightColor,
    light_type: LightType,
) {
    commands.spawn((
        Mesh3d(mesh.clone()),
        MeshMaterial3d(material.clone()),
        Transform::from_translation(position),
        Billboard,
        LightBillboard {
            light_color, light_type,
            lightsource_type: LightSourceType::NONE,
            active: true, occluded: false,
        },
        RenderLayers::layer(RENDERLAYER_POINTLIGHTS),
    ));
}

// ============================================================
// Spawning Functions
// ============================================================

fn spawn_terrain_chunks(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    terrain: &TerrainData,
) {
    let material = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.9,
        ..default()
    });
    for cz in 0..CHUNKS_PER_SIDE {
        for cx in 0..CHUNKS_PER_SIDE {
            let mesh_handle = meshes.add(build_chunk_mesh(terrain, cx, cz));
            commands.spawn((Mesh3d(mesh_handle), MeshMaterial3d(material.clone())));
        }
    }
    // Single physics heightfield
    commands.spawn((
        RigidBody::Fixed,
        Collider::heightfield(
            terrain.heights.clone(), terrain.depth, terrain.width,
            Vec3::new(TERRAIN_SIZE, 1.0, TERRAIN_SIZE),
        ),
    ));
}

fn spawn_water(commands: &mut Commands, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) {
    let mesh_handle = meshes.add(make_ground_quad(TERRAIN_SIZE * 1.2, TERRAIN_SIZE * 1.2));
    let material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.05, 0.12, 0.25, 0.85),
        alpha_mode: AlphaMode::Blend,
        perceptual_roughness: 0.3,
        ..default()
    });
    commands.spawn((Mesh3d(mesh_handle), MeshMaterial3d(material),
        Transform::from_translation(Vec3::new(0.0, WATER_LEVEL, 0.0))));
}

fn spawn_moonlight(commands: &mut Commands) {
    commands.spawn((
        DirectionalLight {
            color: Color::srgb(0.4, 0.4, 0.55), illuminance: 300.0,
            shadows_enabled: false, ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.8, 0.3, 0.0)),
    ));
}

fn spawn_cities(
    commands: &mut Commands, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>,
    bb_mesh: &Handle<Mesh>, bb_mat_white: &Handle<StandardMaterial>, bb_mat_yellow: &Handle<StandardMaterial>,
    cities: &[CityData], terrain: &TerrainData, rng: &mut StdRng,
) {
    let building_colors = [
        Color::srgb(0.55, 0.55, 0.50), Color::srgb(0.60, 0.58, 0.52),
        Color::srgb(0.50, 0.48, 0.45), Color::srgb(0.65, 0.63, 0.55),
        Color::srgb(0.45, 0.44, 0.42),
    ];
    let unit_box = meshes.add(Cuboid::new(1.0, 1.0, 1.0));

    for city in cities {
        let n = rng.gen_range(BUILDINGS_PER_CITY_MIN..BUILDINGS_PER_CITY_MAX);
        for _ in 0..n {
            let angle = rng.gen_range(0.0..std::f32::consts::TAU);
            let dist = rng.gen_range(0.0..city.radius);
            let bx = city.pos.x + angle.cos() * dist;
            let bz = city.pos.y + angle.sin() * dist;
            let gh = terrain.get_height_world(bx, bz);
            if gh < WATER_LEVEL + 0.5 { continue; }
            let w = rng.gen_range(0.5..2.5_f32);
            let h = rng.gen_range(0.5..3.0_f32);
            let d = rng.gen_range(0.5..2.5_f32);
            let mat = materials.add(StandardMaterial {
                base_color: building_colors[rng.gen_range(0..building_colors.len())],
                perceptual_roughness: 0.8, ..default()
            });
            commands.spawn((Mesh3d(unit_box.clone()), MeshMaterial3d(mat),
                Transform::from_translation(Vec3::new(bx, gh + h / 2.0, bz)).with_scale(Vec3::new(w, h, d))));
        }
        let nl = (city.radius / 30.0) as usize;
        for _ in 0..nl {
            let angle = rng.gen_range(0.0..std::f32::consts::TAU);
            let dist = rng.gen_range(0.0..city.radius);
            let lx = city.pos.x + angle.cos() * dist;
            let lz = city.pos.y + angle.sin() * dist;
            let gh = terrain.get_height_world(lx, lz);
            if gh < WATER_LEVEL + 0.5 { continue; }
            let (mat, lc) = if rng.gen_bool(0.5) {
                (bb_mat_white, LightColor::WHITE)
            } else {
                (bb_mat_yellow, LightColor::YELLOW)
            };
            spawn_billboard_light_entity(commands, bb_mesh, mat, Vec3::new(lx, gh + 0.3, lz), lc, LightType::SOLID);
        }
    }
}

fn spawn_roads(
    commands: &mut Commands, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>,
    bb_mesh: &Handle<Mesh>, bb_mat_yellow: &Handle<StandardMaterial>,
    roads: &[RoadPath], terrain: &TerrainData,
) {
    let road_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.25, 0.25, 0.25), perceptual_roughness: 0.95, ..default()
    });

    for road in roads {
        if road.waypoints.len() < 2 { continue; }

        // Render each sub-segment as a terrain-following quad
        for seg in road.waypoints.windows(2) {
            let a = seg[0];
            let b = seg[1];
            let dir = b - a;
            let seg_len = dir.length();
            if seg_len < 0.1 { continue; }
            let mid = (a + b) / 2.0;
            let h = terrain.get_height_world(mid.x, mid.y);
            let angle = dir.y.atan2(dir.x);

            let road_mesh = meshes.add(make_ground_quad(ROAD_WIDTH, seg_len));
            commands.spawn((
                Mesh3d(road_mesh), MeshMaterial3d(road_mat.clone()),
                Transform::from_translation(Vec3::new(mid.x, h + 0.05, mid.y))
                    .with_rotation(Quat::from_rotation_y(-angle + std::f32::consts::FRAC_PI_2)),
            ));
        }

        // Road lights along the full path
        let total_len: f32 = road.waypoints.windows(2).map(|s| (s[1] - s[0]).length()).sum();
        let n_lights = (total_len / ROAD_LIGHT_SPACING) as usize;
        for i in 0..n_lights {
            let target_dist = (i as f32 + 0.5) / n_lights as f32 * total_len;
            let mut accum = 0.0;
            for seg in road.waypoints.windows(2) {
                let seg_len = (seg[1] - seg[0]).length();
                if accum + seg_len >= target_dist {
                    let t = (target_dist - accum) / seg_len;
                    let p = seg[0].lerp(seg[1], t);
                    let h = terrain.get_height_world(p.x, p.y);
                    if h > WATER_LEVEL + 0.5 {
                        spawn_billboard_light_entity(commands, bb_mesh, bb_mat_yellow,
                            Vec3::new(p.x, h + 0.3, p.y), LightColor::YELLOW, LightType::SOLID);
                    }
                    break;
                }
                accum += seg_len;
            }
        }
    }
}

fn spawn_fields(
    commands: &mut Commands, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>,
    fields: &[FieldRect], terrain: &TerrainData,
) {
    for field in fields {
        let w = field.half_size.x * 2.0;
        let d = field.half_size.y * 2.0;
        let mat = materials.add(StandardMaterial { base_color: field.color, perceptual_roughness: 0.95, ..default() });
        let h = terrain.get_height_world(field.center.x, field.center.y);
        let field_mesh = meshes.add(make_ground_quad(w, d));
        commands.spawn((Mesh3d(field_mesh), MeshMaterial3d(mat),
            Transform::from_translation(Vec3::new(field.center.x, h + 0.03, field.center.y))));
    }
}

fn spawn_airbase(
    commands: &mut Commands, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>,
    bb_mesh: &Handle<Mesh>,
    bb_mat_yellow: &Handle<StandardMaterial>, bb_mat_green: &Handle<StandardMaterial>,
    bb_mat_red: &Handle<StandardMaterial>, bb_mat_white: &Handle<StandardMaterial>,
) {
    let concrete_mat = materials.add(StandardMaterial { base_color: Color::srgb(0.35, 0.35, 0.33), perceptual_roughness: 0.9, ..default() });
    let building_mat = materials.add(StandardMaterial { base_color: Color::srgb(0.50, 0.48, 0.44), perceptual_roughness: 0.85, ..default() });
    let tower_mat = materials.add(StandardMaterial { base_color: Color::srgb(0.55, 0.55, 0.52), perceptual_roughness: 0.7, ..default() });

    // Runway
    commands.spawn((Mesh3d(meshes.add(make_ground_quad(RUNWAY_WIDTH, RUNWAY_LENGTH))), MeshMaterial3d(concrete_mat.clone()),
        Transform::from_translation(Vec3::new(RUNWAY_LENGTH / 2.0 - 0.2, RUNWAY_Y + 0.02, RUNWAY_Z))));
    // Taxiway
    commands.spawn((Mesh3d(meshes.add(make_ground_quad(3.0, RUNWAY_LENGTH))), MeshMaterial3d(concrete_mat.clone()),
        Transform::from_translation(Vec3::new(RUNWAY_LENGTH / 2.0 - 0.2, RUNWAY_Y + 0.01, 7.3))));

    // Hangars
    let hangar_mesh = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    for i in 0..2 {
        commands.spawn((Mesh3d(hangar_mesh.clone()), MeshMaterial3d(building_mat.clone()),
            Transform::from_translation(Vec3::new(30.0 + i as f32 * 40.0, RUNWAY_Y + 1.5, -8.0)).with_scale(Vec3::new(6.0, 3.0, 4.0))));
    }

    // Control Tower
    let tower_box = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    commands.spawn((Mesh3d(tower_box.clone()), MeshMaterial3d(tower_mat.clone()),
        Transform::from_translation(Vec3::new(140.0, RUNWAY_Y + 2.5, -12.0)).with_scale(Vec3::new(2.0, 5.0, 2.0))));
    commands.spawn((Mesh3d(tower_box.clone()), MeshMaterial3d(tower_mat),
        Transform::from_translation(Vec3::new(140.0, RUNWAY_Y + 5.5, -12.0)).with_scale(Vec3::new(3.0, 1.0, 3.0))));
    spawn_billboard_light_entity(commands, bb_mesh, bb_mat_red,
        Vec3::new(140.0, RUNWAY_Y + 6.5, -12.0), LightColor::RED, LightType::FLASH_SINGLE);

    // Runway lights (yellow center lines)
    let mut x = -0.2;
    while x < RUNWAY_LENGTH {
        for &z in &[-2.0, 2.75, 7.3] {
            spawn_billboard_light_entity(commands, bb_mesh, bb_mat_yellow,
                Vec3::new(x, RUNWAY_Y, z), LightColor::YELLOW, LightType::SOLID);
        }
        x += 2.0;
    }
    // Threshold (green)
    let mut z = -2.0;
    while z < 8.0 {
        spawn_billboard_light_entity(commands, bb_mesh, bb_mat_green,
            Vec3::new(-2.5, RUNWAY_Y, z), LightColor::GREEN, LightType::SOLID);
        z += 0.49;
    }
    // End (red)
    z = -2.0;
    while z < 8.0 {
        spawn_billboard_light_entity(commands, bb_mesh, bb_mat_red,
            Vec3::new(173.0, RUNWAY_Y, z), LightColor::RED, LightType::SOLID);
        z += 0.49;
    }
    // Approach (white flashing)
    for i in 1..=5 {
        spawn_billboard_light_entity(commands, bb_mesh, bb_mat_white,
            Vec3::new(-2.5 - i as f32 * 8.0, RUNWAY_Y + 0.2, RUNWAY_Z), LightColor::WHITE, LightType::FLASH_SINGLE);
    }
    // PAPI
    for i in 0..4 {
        let (mat, lc) = if i < 2 { (bb_mat_red, LightColor::RED) } else { (bb_mat_white, LightColor::WHITE) };
        spawn_billboard_light_entity(commands, bb_mesh, mat,
            Vec3::new(5.0, RUNWAY_Y + 0.1, -4.0 - i as f32 * 0.8), lc, LightType::SOLID);
    }
}

// ============================================================
// Terrain Features — trees, farms, comm towers
// ============================================================

fn spawn_terrain_features(
    commands: &mut Commands, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>,
    bb_mesh: &Handle<Mesh>, bb_mat_red: &Handle<StandardMaterial>,
    terrain: &TerrainData, cities: &[CityData], rng: &mut StdRng,
) {
    let tree_mesh = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let tree_colors = [
        materials.add(StandardMaterial { base_color: Color::srgb(0.12, 0.28, 0.10), perceptual_roughness: 0.9, ..default() }),
        materials.add(StandardMaterial { base_color: Color::srgb(0.15, 0.32, 0.12), perceptual_roughness: 0.9, ..default() }),
        materials.add(StandardMaterial { base_color: Color::srgb(0.10, 0.24, 0.08), perceptual_roughness: 0.9, ..default() }),
    ];
    let farm_mesh = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let farm_mat = materials.add(StandardMaterial { base_color: Color::srgb(0.55, 0.45, 0.35), perceptual_roughness: 0.9, ..default() });
    let tower_mesh = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let tower_mat = materials.add(StandardMaterial { base_color: Color::srgb(0.40, 0.40, 0.40), perceptual_roughness: 0.6, ..default() });

    // Trees — clustered using noise for "forest" areas
    let mut tree_count = 0;
    let mut attempts = 0;
    while tree_count < NUM_TREES && attempts < NUM_TREES * 3 {
        attempts += 1;
        let x = rng.gen_range(-4000.0..4000.0_f32);
        let z = rng.gen_range(-4000.0..4000.0_f32);
        let h = terrain.get_height_world(x, z);
        // Only on flat, dry ground
        if h < WATER_LEVEL + 0.5 || h > BASE_HEIGHT + 15.0 { continue; }
        // Not on airbase or in cities
        if (x * x + z * z).sqrt() < AIRBASE_FLAT_RADIUS { continue; }
        if cities.iter().any(|c| (c.pos - Vec2::new(x, z)).length() < c.radius + 20.0) { continue; }
        // Use noise to cluster trees into forest areas
        let forest_noise = fbm(x * 0.002, z * 0.002, 2, 999);
        if forest_noise < 0.45 { continue; }

        let scale_h = rng.gen_range(0.4..1.2_f32);
        let scale_w = rng.gen_range(0.15..0.35_f32);
        let mat = tree_colors[rng.gen_range(0..tree_colors.len())].clone();
        commands.spawn((Mesh3d(tree_mesh.clone()), MeshMaterial3d(mat),
            Transform::from_translation(Vec3::new(x, h + scale_h / 2.0, z))
                .with_scale(Vec3::new(scale_w, scale_h, scale_w))));
        tree_count += 1;
    }

    // Farms — small isolated buildings
    let mut farm_count = 0;
    attempts = 0;
    while farm_count < NUM_FARMS && attempts < NUM_FARMS * 5 {
        attempts += 1;
        let x = rng.gen_range(-3500.0..4000.0_f32);
        let z = rng.gen_range(-3500.0..3500.0_f32);
        let h = terrain.get_height_world(x, z);
        if h < WATER_LEVEL + 0.5 || h > BASE_HEIGHT + 5.0 { continue; }
        if (x * x + z * z).sqrt() < AIRBASE_FLAT_RADIUS { continue; }
        if cities.iter().any(|c| (c.pos - Vec2::new(x, z)).length() < c.radius + 50.0) { continue; }

        let w = rng.gen_range(1.0..3.0_f32);
        let bh = rng.gen_range(0.5..1.5_f32);
        let d = rng.gen_range(1.0..2.5_f32);
        commands.spawn((Mesh3d(farm_mesh.clone()), MeshMaterial3d(farm_mat.clone()),
            Transform::from_translation(Vec3::new(x, h + bh / 2.0, z)).with_scale(Vec3::new(w, bh, d))));
        farm_count += 1;
    }

    // Communication towers — tall thin poles with red flashing lights
    let mut tower_count = 0;
    attempts = 0;
    while tower_count < NUM_COMM_TOWERS && attempts < NUM_COMM_TOWERS * 10 {
        attempts += 1;
        let x = rng.gen_range(-3000.0..4000.0_f32);
        let z = rng.gen_range(-3000.0..3000.0_f32);
        let h = terrain.get_height_world(x, z);
        if h < WATER_LEVEL + 0.5 || h > BASE_HEIGHT + 20.0 { continue; }
        if (x * x + z * z).sqrt() < AIRBASE_FLAT_RADIUS + 200.0 { continue; }

        let tower_h = rng.gen_range(6.0..12.0_f32);
        commands.spawn((Mesh3d(tower_mesh.clone()), MeshMaterial3d(tower_mat.clone()),
            Transform::from_translation(Vec3::new(x, h + tower_h / 2.0, z))
                .with_scale(Vec3::new(0.15, tower_h, 0.15))));
        spawn_billboard_light_entity(commands, bb_mesh, bb_mat_red,
            Vec3::new(x, h + tower_h + 0.2, z), LightColor::RED, LightType::FLASH_ALT_SINGLE);
        tower_count += 1;
    }
}

// ============================================================
// Main Entry Point
// ============================================================

pub fn setup_procedural_world(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let mut rng = StdRng::seed_from_u64(TERRAIN_SEED);

    // Generate heightmap (mountains suppressed near origin)
    let mut terrain = generate_heightmap(rng.gen());

    // Hard-flatten airbase area
    terrain.flatten_rect(-20.0, -20.0, RUNWAY_LENGTH + 20.0, 20.0, RUNWAY_Y, 60.0);

    // Generate & flatten cities
    let cities = generate_cities(&terrain, &mut rng);
    for city in &cities {
        terrain.flatten_circle(city.pos.x, city.pos.y, city.radius, BASE_HEIGHT);
    }

    // Generate roads (routed around mountains)
    let roads = generate_roads(&cities, &terrain);

    // Flatten road corridors using actual waypoints
    for road in &roads {
        terrain.flatten_path(&road.waypoints, 8.0, BASE_HEIGHT);
    }

    let fields = generate_fields(&terrain, &cities, &mut rng);

    // Billboard materials
    let bb_mesh_handle = meshes.add(Rectangle::new(0.01, 0.01));
    let bb_mat_yellow = materials.add(StandardMaterial {
        base_color_texture: Some(images.add(create_texture(LightColor::YELLOW))),
        unlit: true, alpha_mode: AlphaMode::Blend, ..default()
    });
    let bb_mat_green = materials.add(StandardMaterial {
        base_color_texture: Some(images.add(create_texture(LightColor::GREEN))),
        unlit: true, alpha_mode: AlphaMode::Blend, ..default()
    });
    let bb_mat_red = materials.add(StandardMaterial {
        base_color_texture: Some(images.add(create_texture(LightColor::RED))),
        unlit: true, alpha_mode: AlphaMode::Blend, ..default()
    });
    let bb_mat_white = materials.add(StandardMaterial {
        base_color_texture: Some(images.add(create_texture(LightColor::WHITE))),
        unlit: true, alpha_mode: AlphaMode::Blend, ..default()
    });

    // Spawn world
    spawn_moonlight(&mut commands);
    spawn_terrain_chunks(&mut commands, &mut meshes, &mut materials, &terrain);
    spawn_water(&mut commands, &mut meshes, &mut materials);
    spawn_cities(&mut commands, &mut meshes, &mut materials, &bb_mesh_handle, &bb_mat_white, &bb_mat_yellow, &cities, &terrain, &mut rng);
    spawn_roads(&mut commands, &mut meshes, &mut materials, &bb_mesh_handle, &bb_mat_yellow, &roads, &terrain);
    spawn_fields(&mut commands, &mut meshes, &mut materials, &fields, &terrain);
    spawn_airbase(&mut commands, &mut meshes, &mut materials, &bb_mesh_handle, &bb_mat_yellow, &bb_mat_green, &bb_mat_red, &bb_mat_white);
    spawn_terrain_features(&mut commands, &mut meshes, &mut materials, &bb_mesh_handle, &bb_mat_red, &terrain, &cities, &mut rng);

    commands.insert_resource(terrain);
}
