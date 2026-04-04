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
use crate::player::Player;
use crate::pointlight::*;

// ============================================================
// Constants
// ============================================================

/// Heightmap covers 100 km × 100 km
const TERRAIN_SIZE: f32 = 100_000.0;
const CHUNKS_PER_SIDE: usize = 16;
const CELLS_PER_CHUNK: usize = 64;
const GRID_RES: usize = CHUNKS_PER_SIDE * CELLS_PER_CHUNK; // 1024
const CELL_SIZE: f32 = TERRAIN_SIZE / GRID_RES as f32;     // ~97.6
const HALF_SIZE: f32 = TERRAIN_SIZE / 2.0;

const OCEAN_SIZE: f32 = 400_000.0;

const BASE_HEIGHT: f32 = -1.0;
const WATER_LEVEL: f32 = -2.5;
const MOUNTAIN_THRESHOLD: f32 = 0.42;
const MAX_MOUNTAIN_HEIGHT: f32 = 80.0;
const TERRAIN_SEED: u64 = 117;

const AIRBASE_FLAT_RADIUS: f32 = 500.0;
const AIRBASE_TRANSITION: f32 = 300.0;
const LAND_RADIUS: f32 = 40_000.0;
const COAST_WIDTH: f32 = 2000.0;

const NUM_CITIES: usize = 15;
const MIN_CITY_DIST: f32 = 4000.0;
const CITY_RADIUS_MIN: f32 = 120.0;
const CITY_RADIUS_MAX: f32 = 400.0;
const BUILDINGS_PER_CITY_MIN: usize = 15;
const BUILDINGS_PER_CITY_MAX: usize = 50;

const ROAD_WIDTH: f32 = 0.4;
const ROAD_LIGHT_SPACING: f32 = 200.0;
const ROAD_MAX_HEIGHT: f32 = 6.0;
const ROAD_SEGMENT_LEN: f32 = 50.0;

const FIELD_COUNT: usize = 80;
const FIELD_SIZE_MIN: f32 = 60.0;
const FIELD_SIZE_MAX: f32 = 200.0;

const RUNWAY_LENGTH: f32 = 172.0;
const RUNWAY_WIDTH: f32 = 5.0;
const RUNWAY_Y: f32 = -0.96;
const RUNWAY_Z: f32 = 0.5;

const NUM_TREES: usize = 1500;
const NUM_FARMS: usize = 60;
const NUM_COMM_TOWERS: usize = 12;
const NUM_SHIPS: usize = 15;

const ORIGIN_SHIFT_THRESHOLD: f32 = 10_000.0;

// ============================================================
// Noise
// ============================================================

fn hash2d(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = seed;
    h ^= x as u32; h = h.wrapping_mul(0x9E3779B9);
    h ^= y as u32; h = h.wrapping_mul(0x517CC1B7);
    h ^= h >> 16;  h = h.wrapping_mul(0x85EBCA6B);
    h ^= h >> 13;
    (h & 0x7FFFFFFF) as f32 / 0x7FFFFFFF as f32
}

fn smoothstep(t: f32) -> f32 { t * t * (3.0 - 2.0 * t) }

fn value_noise(x: f32, y: f32, seed: u32) -> f32 {
    let ix = x.floor() as i32; let iy = y.floor() as i32;
    let fx = smoothstep(x - ix as f32); let fy = smoothstep(y - iy as f32);
    let v00 = hash2d(ix, iy, seed);     let v10 = hash2d(ix+1, iy, seed);
    let v01 = hash2d(ix, iy+1, seed);   let v11 = hash2d(ix+1, iy+1, seed);
    let a = v00 + fx * (v10 - v00);
    let b = v01 + fx * (v11 - v01);
    a + fy * (b - a)
}

fn fbm(x: f32, y: f32, octaves: u32, seed: u32) -> f32 {
    let (mut val, mut amp, mut freq, mut ma) = (0.0, 1.0, 1.0, 0.0);
    for i in 0..octaves {
        val += value_noise(x*freq, y*freq, seed.wrapping_add(i*31)) * amp;
        ma += amp; amp *= 0.5; freq *= 2.0;
    }
    val / ma
}

// ============================================================
// Terrain Data
// ============================================================

/// Marker for terrain mesh chunks (excluded from billboard raycasts).
#[derive(Component)]
pub struct TerrainChunk;

#[derive(Resource)]
pub struct TerrainData {
    pub heights: Vec<f32>,
    pub width: usize,
    pub depth: usize,
    /// Accumulated origin-shift offset (add to current world coords
    /// to recover original heightmap coords).
    pub origin_shift: Vec3,
    /// City centre positions (original terrain coords) for the map MFD.
    pub city_positions: Vec<Vec2>,
}

impl TerrainData {
    fn idx(w: usize, gx: usize, gz: usize) -> usize { gz * w + gx }

    /// Sample height at original-terrain coordinates (accounts for origin shift).
    pub fn get_height_world(&self, wx: f32, wz: f32) -> f32 {
        let tx = wx + self.origin_shift.x;
        let tz = wz + self.origin_shift.z;
        let fx = (tx + HALF_SIZE) / CELL_SIZE;
        let fz = (tz + HALF_SIZE) / CELL_SIZE;
        if fx < 0.0 || fz < 0.0 { return WATER_LEVEL - 3.0; }
        let gx = (fx as usize).min(self.width - 2);
        let gz = (fz as usize).min(self.depth - 2);
        if gx >= self.width - 1 || gz >= self.depth - 1 { return WATER_LEVEL - 3.0; }
        let lx = (fx - gx as f32).clamp(0.0, 1.0);
        let lz = (fz - gz as f32).clamp(0.0, 1.0);
        let w = self.width;
        let h00 = self.heights[Self::idx(w, gx, gz)];
        let h10 = self.heights[Self::idx(w, gx+1, gz)];
        let h01 = self.heights[Self::idx(w, gx, gz+1)];
        let h11 = self.heights[Self::idx(w, gx+1, gz+1)];
        let a = h00 + lx * (h10 - h00);
        let b = h01 + lx * (h11 - h01);
        a + lz * (b - a)
    }

    /// Grid-index bounds for a world-space AABB (clamped to grid).
    fn grid_bounds(min_w: f32, max_w: f32, min_d: f32, max_d: f32, width: usize, depth: usize) -> (usize,usize,usize,usize) {
        let gx0 = ((min_w + HALF_SIZE) / CELL_SIZE).floor().max(0.0) as usize;
        let gx1 = ((max_w + HALF_SIZE) / CELL_SIZE).ceil().min(width as f32 - 1.0) as usize;
        let gz0 = ((min_d + HALF_SIZE) / CELL_SIZE).floor().max(0.0) as usize;
        let gz1 = ((max_d + HALF_SIZE) / CELL_SIZE).ceil().min(depth as f32 - 1.0) as usize;
        (gx0, gx1, gz0, gz1)
    }

    fn flatten_rect(&mut self, min_x: f32, min_z: f32, max_x: f32, max_z: f32, target_h: f32, margin: f32) {
        let w = self.width;
        let (gx0,gx1,gz0,gz1) = Self::grid_bounds(min_x-margin, max_x+margin, min_z-margin, max_z+margin, w, self.depth);
        for gz in gz0..=gz1 { for gx in gx0..=gx1 {
            let wx = gx as f32 * CELL_SIZE - HALF_SIZE;
            let wz = gz as f32 * CELL_SIZE - HALF_SIZE;
            let dx = if wx < min_x { min_x-wx } else if wx > max_x { wx-max_x } else { 0.0 };
            let dz = if wz < min_z { min_z-wz } else if wz > max_z { wz-max_z } else { 0.0 };
            let d = (dx*dx + dz*dz).sqrt();
            if d <= 0.0 { self.heights[Self::idx(w,gx,gz)] = target_h; }
            else if d < margin {
                let b = smoothstep(d/margin);
                let i = Self::idx(w,gx,gz);
                self.heights[i] = target_h*(1.0-b) + self.heights[i]*b;
            }
        }}
    }

    fn flatten_circle(&mut self, cx: f32, cz: f32, radius: f32, target_h: f32) {
        let outer = radius*1.3; let o2 = outer*outer; let r2 = radius*radius;
        let w = self.width;
        let (gx0,gx1,gz0,gz1) = Self::grid_bounds(cx-outer, cx+outer, cz-outer, cz+outer, w, self.depth);
        for gz in gz0..=gz1 { for gx in gx0..=gx1 {
            let wx = gx as f32*CELL_SIZE - HALF_SIZE;
            let wz = gz as f32*CELL_SIZE - HALF_SIZE;
            let d2 = (wx-cx)*(wx-cx) + (wz-cz)*(wz-cz);
            if d2 < r2 { self.heights[Self::idx(w,gx,gz)] = target_h; }
            else if d2 < o2 {
                let t = ((d2.sqrt()-radius)/(outer-radius)).clamp(0.0,1.0);
                let b = smoothstep(t); let i = Self::idx(w,gx,gz);
                self.heights[i] = target_h*(1.0-b) + self.heights[i]*b;
            }
        }}
    }

    fn flatten_path(&mut self, points: &[Vec2], half_w: f32, target_h: f32) {
        let margin = half_w*1.5; let w = self.width;
        // Compute tight bounding box of the path + margin
        let (mut bmin_x, mut bmax_x) = (f32::MAX, f32::MIN);
        let (mut bmin_z, mut bmax_z) = (f32::MAX, f32::MIN);
        for p in points {
            bmin_x = bmin_x.min(p.x); bmax_x = bmax_x.max(p.x);
            bmin_z = bmin_z.min(p.y); bmax_z = bmax_z.max(p.y);
        }
        let (gx0,gx1,gz0,gz1) = Self::grid_bounds(bmin_x-margin, bmax_x+margin, bmin_z-margin, bmax_z+margin, w, self.depth);
        for gz in gz0..=gz1 { for gx in gx0..=gx1 {
            let wx = gx as f32*CELL_SIZE - HALF_SIZE;
            let wz = gz as f32*CELL_SIZE - HALF_SIZE;
            let p = Vec2::new(wx,wz);
            let mut md = f32::MAX;
            for s in points.windows(2) {
                let ab = s[1]-s[0]; let ap = p-s[0];
                let t = (ap.dot(ab)/ab.dot(ab)).clamp(0.0,1.0);
                md = md.min((p - (s[0]+ab*t)).length());
            }
            if md < half_w { self.heights[Self::idx(w,gx,gz)] = target_h; }
            else if md < margin {
                let b = smoothstep((md-half_w)/(margin-half_w));
                let i = Self::idx(w,gx,gz);
                self.heights[i] = target_h*(1.0-b) + self.heights[i]*b;
            }
        }}
    }
}

// ============================================================
// Feature Structures
// ============================================================

struct CityData { pos: Vec2, radius: f32 }
struct RoadPath { waypoints: Vec<Vec2> }
struct FieldRect { center: Vec2, half_size: Vec2, color: Color }

// ============================================================
// Heightmap Generation
// ============================================================

/// Compute the height for one grid cell (pure function, safe to call from any thread).
fn compute_height(gx: usize, gz: usize, seed: u32) -> f32 {
    let wx = gx as f32 * CELL_SIZE - HALF_SIZE;
    let wz = gz as f32 * CELL_SIZE - HALF_SIZE;
    let dist = (wx*wx + wz*wz).sqrt();

    let suppress = if dist < AIRBASE_FLAT_RADIUS { 0.0 }
        else if dist < AIRBASE_FLAT_RADIUS+AIRBASE_TRANSITION {
            smoothstep((dist-AIRBASE_FLAT_RADIUS)/AIRBASE_TRANSITION)
        } else { 1.0 };

    let mnoise = fbm(wx*0.0003, wz*0.0003, 4, seed);
    let detail = fbm(wx*0.001, wz*0.001, 3, seed+100);
    let mut h = BASE_HEIGHT;
    if mnoise > MOUNTAIN_THRESHOLD {
        let s = (mnoise-MOUNTAIN_THRESHOLD)/(1.0-MOUNTAIN_THRESHOLD);
        h += s*s * MAX_MOUNTAIN_HEIGHT * suppress;
    }
    h += (detail-0.5)*3.0 * suppress;

    let ln = fbm(wx*0.0005+50.0, wz*0.0005+50.0, 3, seed+200);
    if ln < 0.25 && h < BASE_HEIGHT+2.0 && dist > AIRBASE_FLAT_RADIUS {
        h = BASE_HEIGHT + (ln-0.25)*12.0;
    }

    let coast_noise = fbm(wx*0.0008, wz*0.0008, 2, seed+300) * 800.0;
    let land_r = LAND_RADIUS + coast_noise;
    if dist > land_r + COAST_WIDTH {
        h = WATER_LEVEL - 3.0;
    } else if dist > land_r {
        let t = (dist - land_r) / COAST_WIDTH;
        h = h*(1.0-smoothstep(t)) + (WATER_LEVEL-3.0)*smoothstep(t);
    }
    h
}

fn generate_heightmap(seed: u32) -> TerrainData {
    let w = GRID_RES+1; let d = GRID_RES+1;
    let cpus = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(16);
    let rows_per = (d + cpus - 1) / cpus;

    // Each thread generates its slice of rows and returns a Vec<f32>
    let row_chunks: Vec<Vec<f32>> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..cpus).map(|t| {
            let start = t * rows_per;
            let end = ((t+1) * rows_per).min(d);
            scope.spawn(move || {
                let mut buf = Vec::with_capacity((end - start) * w);
                for gz in start..end {
                    for gx in 0..w {
                        buf.push(compute_height(gx, gz, seed));
                    }
                }
                buf
            })
        }).collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });

    let heights: Vec<f32> = row_chunks.into_iter().flatten().collect();
    TerrainData { heights, width: w, depth: d, origin_shift: Vec3::ZERO, city_positions: Vec::new() }
}

// ============================================================
// Feature Generation
// ============================================================

fn generate_cities(t: &TerrainData, rng: &mut StdRng) -> Vec<CityData> {
    let range = LAND_RADIUS * 0.85;
    let mut c = Vec::new(); let mut att = 0;
    while c.len() < NUM_CITIES && att < 1000 { att += 1;
        let x = rng.gen_range(-range..range);
        let z = rng.gen_range(-range..range);
        if x.abs() < 600.0 && z.abs() < 300.0 { continue; }
        let h = t.get_height_world(x,z);
        if h < WATER_LEVEL+1.0 || h > BASE_HEIGHT+8.0 { continue; }
        if c.iter().any(|ci: &CityData| (ci.pos-Vec2::new(x,z)).length() < MIN_CITY_DIST) { continue; }
        c.push(CityData { pos: Vec2::new(x,z), radius: rng.gen_range(CITY_RADIUS_MIN..CITY_RADIUS_MAX) });
    }
    c
}

fn route_road(start: Vec2, end: Vec2, terrain: &TerrainData) -> Vec<Vec2> {
    let dir = end-start; let len = dir.length();
    let n = (len/ROAD_SEGMENT_LEN).ceil().max(2.0) as usize;
    let perp = Vec2::new(-dir.y, dir.x).normalize_or_zero();
    let max_h = BASE_HEIGHT + ROAD_MAX_HEIGHT;
    let mut pts: Vec<Vec2> = (0..=n).map(|i| start.lerp(end, i as f32/n as f32)).collect();
    let airbase_exclusion = AIRBASE_FLAT_RADIUS + 100.0;
    for _ in 0..20 { let mut changed = false;
        for i in 1..pts.len()-1 {
            // Push away from high terrain
            let h = terrain.get_height_world(pts[i].x, pts[i].y);
            if h > max_h {
                let c1 = pts[i]+perp*50.0; let c2 = pts[i]-perp*50.0;
                let h1 = terrain.get_height_world(c1.x,c1.y);
                let h2 = terrain.get_height_world(c2.x,c2.y);
                let best = if h1<h2 { c1 } else { c2 };
                if h1.min(h2) < h { pts[i] = best; changed = true; }
            }
            // Push away from airbase area
            let dist = pts[i].length();
            if dist < airbase_exclusion {
                let out_dir = pts[i].normalize_or(Vec2::X);
                pts[i] = out_dir * (airbase_exclusion + 50.0);
                changed = true;
            }
        }
        if !changed { break; }
    }
    for _ in 0..3 { for i in 1..pts.len()-1 { pts[i] = (pts[i-1]+pts[i]*2.0+pts[i+1])/4.0; } }
    pts
}

fn generate_roads(cities: &[CityData], terrain: &TerrainData) -> Vec<RoadPath> {
    let mut roads = Vec::new();
    if cities.is_empty() { return roads; }
    let mut connected = std::collections::HashSet::<(usize,usize)>::new();
    for (i,city) in cities.iter().enumerate() {
        let mut ds: Vec<(usize,f32)> = cities.iter().enumerate()
            .filter(|(j,_)| *j!=i).map(|(j,o)| (j,(city.pos-o.pos).length())).collect();
        ds.sort_by(|a,b| a.1.partial_cmp(&b.1).unwrap());
        for &(j,_) in ds.iter().take(2) {
            let k = if i<j {(i,j)} else {(j,i)};
            if connected.insert(k) {
                roads.push(RoadPath { waypoints: route_road(cities[k.0].pos, cities[k.1].pos, terrain) });
            }
        }
    }
    // Road from airbase to nearest city — start beyond the runway, not at origin
    if let Some(near) = cities.iter().min_by(|a,b| a.pos.length().partial_cmp(&b.pos.length()).unwrap()) {
        let exit = Vec2::new(RUNWAY_LENGTH + 50.0, 0.0);
        roads.push(RoadPath { waypoints: route_road(exit, near.pos, terrain) });
    }
    roads
}

fn generate_fields(t: &TerrainData, cities: &[CityData], rng: &mut StdRng) -> Vec<FieldRect> {
    let pal = [Color::srgb(0.55,0.50,0.25),Color::srgb(0.30,0.50,0.20),
               Color::srgb(0.45,0.40,0.20),Color::srgb(0.35,0.55,0.25)];
    let field_range = LAND_RADIUS * 0.8;
    let mut f = Vec::new(); let mut att = 0;
    while f.len() < FIELD_COUNT && att < 500 { att += 1;
        let x = rng.gen_range(-field_range..field_range);
        let z = rng.gen_range(-field_range..field_range);
        let h = t.get_height_world(x,z);
        if h < WATER_LEVEL+1.0 || h > BASE_HEIGHT+5.0 { continue; }
        if cities.iter().any(|c| (c.pos-Vec2::new(x,z)).length() < c.radius+50.0) { continue; }
        if x.abs() < 300.0 && z.abs() < 100.0 { continue; }
        let hw = rng.gen_range(FIELD_SIZE_MIN..FIELD_SIZE_MAX)/2.0;
        let hd = rng.gen_range(FIELD_SIZE_MIN..FIELD_SIZE_MAX)/2.0;
        f.push(FieldRect { center: Vec2::new(x,z), half_size: Vec2::new(hw,hd),
            color: pal[rng.gen_range(0..pal.len())] });
    }
    f
}

// ============================================================
// Terrain Color
// ============================================================

fn terrain_color(height: f32, normal_y: f32) -> [f32; 4] {
    let rel = height - BASE_HEIGHT;
    if normal_y < 0.85 { return [0.45,0.40,0.35,1.0]; }
    if height < WATER_LEVEL { [0.15,0.22,0.12,1.0] }
    else if rel < 1.0 { [0.28,0.38,0.18,1.0] }
    else if rel < 10.0 { let t=(rel-1.0)/9.0; [0.28+t*0.15,0.38-t*0.05,0.18+t*0.08,1.0] }
    else if rel < 40.0 { let t=(rel-10.0)/30.0; [0.43+t*0.12,0.33+t*0.12,0.26+t*0.12,1.0] }
    else { [0.58,0.55,0.50,1.0] }
}

// ============================================================
// Mesh Creation — chunks with LOCAL-SPACE vertices
// ============================================================

/// Returns (mesh, chunk_center_world_pos)
fn build_chunk_mesh(data: &TerrainData, cx: usize, cz: usize) -> (Mesh, Vec3) {
    let sgx = cx*CELLS_PER_CHUNK; let sgz = cz*CELLS_PER_CHUNK;
    let egx = (sgx+CELLS_PER_CHUNK).min(data.width-1);
    let egz = (sgz+CELLS_PER_CHUNK).min(data.depth-1);
    let cols = egx-sgx; let rows = egz-sgz;
    let vw = cols+1; let vh = rows+1;

    // Chunk center in original world coords
    let cx_w = (sgx as f32 + cols as f32*0.5)*CELL_SIZE - HALF_SIZE;
    let cz_w = (sgz as f32 + rows as f32*0.5)*CELL_SIZE - HALF_SIZE;
    let center = Vec3::new(cx_w, 0.0, cz_w);

    let mut pos: Vec<[f32;3]> = Vec::with_capacity(vw*vh);
    let mut nor: Vec<[f32;3]> = Vec::with_capacity(vw*vh);
    let mut col: Vec<[f32;4]> = Vec::with_capacity(vw*vh);
    let mut idx: Vec<u32> = Vec::with_capacity(cols*rows*6);

    for lz in 0..vh { for lx in 0..vw {
        let gx = sgx+lx; let gz = sgz+lz;
        let wx = gx as f32*CELL_SIZE - HALF_SIZE;
        let wz = gz as f32*CELL_SIZE - HALF_SIZE;
        let h = data.heights[TerrainData::idx(data.width,gx,gz)];
        pos.push([wx - center.x, h, wz - center.z]); // LOCAL coords

        let hl = if gx>0 { data.heights[TerrainData::idx(data.width,gx-1,gz)] } else { h };
        let hr = if gx<data.width-1 { data.heights[TerrainData::idx(data.width,gx+1,gz)] } else { h };
        let hd = if gz>0 { data.heights[TerrainData::idx(data.width,gx,gz-1)] } else { h };
        let hu = if gz<data.depth-1 { data.heights[TerrainData::idx(data.width,gx,gz+1)] } else { h };
        let n = Vec3::new(hl-hr, 2.0*CELL_SIZE, hd-hu).normalize();
        nor.push(n.to_array());
        col.push(terrain_color(h, n.y));
    }}
    for lz in 0..rows { for lx in 0..cols {
        let tl = (lz*vw+lx) as u32; let tr = tl+1;
        let bl = tl+vw as u32; let br = bl+1;
        idx.extend_from_slice(&[tl,bl,tr, tr,bl,br]);
    }}
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::all());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nor);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
    mesh.insert_indices(bevy::mesh::Indices::U32(idx));
    (mesh, center)
}

fn make_ground_quad(w: f32, l: f32) -> Mesh {
    let hw=w/2.0; let hl=l/2.0;
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::all());
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[-hw,0.0,-hl],[hw,0.0,-hl],[hw,0.0,hl],[-hw,0.0,hl]]);
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0,1.0,0.0];4]);
    m.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0,0.0],[1.0,0.0],[1.0,1.0],[0.0,1.0]]);
    m.insert_indices(bevy::mesh::Indices::U32(vec![0,2,1,0,3,2]));
    m
}

// ============================================================
// Spawn helpers
// ============================================================

fn spawn_bb(cmd: &mut Commands, m: &Handle<Mesh>, mat: &Handle<StandardMaterial>,
    pos: Vec3, lc: LightColor, lt: LightType)
{
    cmd.spawn((Mesh3d(m.clone()), MeshMaterial3d(mat.clone()),
        Transform::from_translation(pos), Billboard,
        LightBillboard { light_color: lc, light_type: lt,
            lightsource_type: LightSourceType::NONE, active: true, occluded: false },
        RenderLayers::layer(RENDERLAYER_POINTLIGHTS)));
}

fn bb_mat(mats: &mut Assets<StandardMaterial>, img: Handle<Image>) -> Handle<StandardMaterial> {
    mats.add(StandardMaterial {
        base_color_texture: Some(img), unlit: true,
        alpha_mode: AlphaMode::Blend, double_sided: true, cull_mode: None, ..default()
    })
}

// ============================================================
// Spawning
// ============================================================

fn spawn_terrain_chunks(cmd: &mut Commands, meshes: &mut Assets<Mesh>,
    mats: &mut Assets<StandardMaterial>, terrain: &TerrainData)
{
    let mat = mats.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.9, ..default() });

    // Build all chunk meshes in parallel, then register serially
    let total = CHUNKS_PER_SIDE * CHUNKS_PER_SIDE;
    let chunk_data: Vec<(Mesh, Vec3)> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..total).map(|i| {
            let tref = terrain;
            let cx = i % CHUNKS_PER_SIDE;
            let cz = i / CHUNKS_PER_SIDE;
            scope.spawn(move || build_chunk_mesh(tref, cx, cz))
        }).collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });

    for (mesh, center) in chunk_data {
        cmd.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(mat.clone()),
            Transform::from_translation(center), TerrainChunk));
    }
    cmd.spawn((RigidBody::Fixed,
        Collider::heightfield(terrain.heights.clone(), terrain.depth, terrain.width,
            Vec3::new(TERRAIN_SIZE, 1.0, TERRAIN_SIZE))));
}

fn spawn_water(cmd: &mut Commands, meshes: &mut Assets<Mesh>, mats: &mut Assets<StandardMaterial>) {
    let mat = mats.add(StandardMaterial {
        base_color: Color::srgba(0.04, 0.10, 0.22, 0.88),
        alpha_mode: AlphaMode::Blend, perceptual_roughness: 0.3, ..default()
    });
    cmd.spawn((Mesh3d(meshes.add(make_ground_quad(OCEAN_SIZE, OCEAN_SIZE))),
        MeshMaterial3d(mat), Transform::from_translation(Vec3::new(0.0, WATER_LEVEL, 0.0))));
}

fn spawn_moonlight(cmd: &mut Commands) {
    cmd.spawn((DirectionalLight { color: Color::srgb(0.4,0.4,0.55), illuminance: 300.0,
        shadows_enabled: false, ..default() },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ,-0.8,0.3,0.0))));
}

fn spawn_cities(cmd: &mut Commands, meshes: &mut Assets<Mesh>, mats: &mut Assets<StandardMaterial>,
    bbm: &Handle<Mesh>, bbw: &Handle<StandardMaterial>, bby: &Handle<StandardMaterial>,
    cities: &[CityData], terrain: &TerrainData, rng: &mut StdRng)
{
    let bcols = [Color::srgb(0.55,0.55,0.50),Color::srgb(0.60,0.58,0.52),
        Color::srgb(0.50,0.48,0.45),Color::srgb(0.65,0.63,0.55),Color::srgb(0.45,0.44,0.42)];
    let ub = meshes.add(Cuboid::new(1.0,1.0,1.0));
    // Dark paved ground underneath each city
    let pavement_mat = mats.add(StandardMaterial { base_color: Color::srgb(0.18,0.18,0.17), perceptual_roughness: 0.95, ..default() });
    for city in cities {
        let gh = terrain.get_height_world(city.pos.x, city.pos.y);
        let pad_size = city.radius * 2.2;
        cmd.spawn((Mesh3d(meshes.add(make_ground_quad(pad_size, pad_size))),
            MeshMaterial3d(pavement_mat.clone()),
            Transform::from_translation(Vec3::new(city.pos.x, gh + 0.02, city.pos.y))));

        let nb = rng.gen_range(BUILDINGS_PER_CITY_MIN..BUILDINGS_PER_CITY_MAX);
        for _ in 0..nb {
            let a = rng.gen_range(0.0..std::f32::consts::TAU);
            let d = rng.gen_range(0.0..city.radius);
            let (bx,bz) = (city.pos.x+a.cos()*d, city.pos.y+a.sin()*d);
            let gh = terrain.get_height_world(bx,bz);
            if gh < WATER_LEVEL+0.5 { continue; }
            let (w,h,dp) = (rng.gen_range(0.5..2.5_f32), rng.gen_range(0.5..3.0_f32), rng.gen_range(0.5..2.5_f32));
            let m = mats.add(StandardMaterial { base_color: bcols[rng.gen_range(0..bcols.len())],
                perceptual_roughness: 0.8, ..default() });
            cmd.spawn((Mesh3d(ub.clone()), MeshMaterial3d(m),
                Transform::from_translation(Vec3::new(bx,gh+h/2.0,bz)).with_scale(Vec3::new(w,h,dp))));
        }
        for _ in 0..(city.radius/30.0) as usize {
            let a = rng.gen_range(0.0..std::f32::consts::TAU);
            let d = rng.gen_range(0.0..city.radius);
            let (lx,lz) = (city.pos.x+a.cos()*d, city.pos.y+a.sin()*d);
            let gh = terrain.get_height_world(lx,lz);
            if gh < WATER_LEVEL+0.5 { continue; }
            let (mat,lc) = if rng.gen_bool(0.5) { (bbw,LightColor::WHITE) } else { (bby,LightColor::YELLOW) };
            spawn_bb(cmd, bbm, mat, Vec3::new(lx,gh+0.3,lz), lc, LightType::SOLID);
        }
    }
}

fn spawn_roads(cmd: &mut Commands, meshes: &mut Assets<Mesh>, mats: &mut Assets<StandardMaterial>,
    bbm: &Handle<Mesh>, bby: &Handle<StandardMaterial>, roads: &[RoadPath], terrain: &TerrainData)
{
    let rm = mats.add(StandardMaterial { base_color: Color::srgb(0.25,0.25,0.25),
        perceptual_roughness: 0.95, ..default() });
    for road in roads {
        if road.waypoints.len() < 2 { continue; }
        for seg in road.waypoints.windows(2) {
            let d = seg[1]-seg[0]; let sl = d.length();
            if sl < 0.1 { continue; }
            let mid = (seg[0]+seg[1])/2.0;
            let h = terrain.get_height_world(mid.x, mid.y);
            let ang = d.y.atan2(d.x);
            cmd.spawn((Mesh3d(meshes.add(make_ground_quad(ROAD_WIDTH, sl))),
                MeshMaterial3d(rm.clone()),
                Transform::from_translation(Vec3::new(mid.x,h+0.05,mid.y))
                    .with_rotation(Quat::from_rotation_y(-ang+std::f32::consts::FRAC_PI_2))));
        }
        let tl: f32 = road.waypoints.windows(2).map(|s| (s[1]-s[0]).length()).sum();
        let nl = (tl/ROAD_LIGHT_SPACING) as usize;
        for i in 0..nl {
            let td = (i as f32+0.5)/nl as f32*tl; let mut acc = 0.0;
            for s in road.waypoints.windows(2) {
                let sl = (s[1]-s[0]).length();
                if acc+sl >= td {
                    let p = s[0].lerp(s[1], (td-acc)/sl);
                    let h = terrain.get_height_world(p.x,p.y);
                    if h > WATER_LEVEL+0.5 { spawn_bb(cmd,bbm,bby,Vec3::new(p.x,h+0.3,p.y),LightColor::YELLOW,LightType::SOLID); }
                    break;
                }
                acc += sl;
            }
        }
    }
}

fn spawn_fields(cmd: &mut Commands, meshes: &mut Assets<Mesh>, mats: &mut Assets<StandardMaterial>,
    fields: &[FieldRect], terrain: &TerrainData)
{
    for f in fields {
        let m = mats.add(StandardMaterial { base_color: f.color, perceptual_roughness: 0.95, ..default() });
        let h = terrain.get_height_world(f.center.x,f.center.y);
        cmd.spawn((Mesh3d(meshes.add(make_ground_quad(f.half_size.x*2.0,f.half_size.y*2.0))),
            MeshMaterial3d(m), Transform::from_translation(Vec3::new(f.center.x,h+0.03,f.center.y))));
    }
}

fn spawn_airbase(cmd: &mut Commands, meshes: &mut Assets<Mesh>, mats: &mut Assets<StandardMaterial>,
    bbm: &Handle<Mesh>, bby: &Handle<StandardMaterial>, bbg: &Handle<StandardMaterial>,
    bbr: &Handle<StandardMaterial>, bbw: &Handle<StandardMaterial>)
{
    let cm = mats.add(StandardMaterial { base_color: Color::srgb(0.35,0.35,0.33), perceptual_roughness: 0.9, ..default() });
    let bm = mats.add(StandardMaterial { base_color: Color::srgb(0.50,0.48,0.44), perceptual_roughness: 0.85, ..default() });
    let tm = mats.add(StandardMaterial { base_color: Color::srgb(0.55,0.55,0.52), perceptual_roughness: 0.7, ..default() });

    cmd.spawn((Mesh3d(meshes.add(make_ground_quad(RUNWAY_WIDTH,RUNWAY_LENGTH))), MeshMaterial3d(cm.clone()),
        Transform::from_translation(Vec3::new(RUNWAY_LENGTH/2.0-0.2,RUNWAY_Y+0.02,RUNWAY_Z))));
    cmd.spawn((Mesh3d(meshes.add(make_ground_quad(3.0,RUNWAY_LENGTH))), MeshMaterial3d(cm),
        Transform::from_translation(Vec3::new(RUNWAY_LENGTH/2.0-0.2,RUNWAY_Y+0.01,7.3))));

    let hm = meshes.add(Cuboid::new(1.0,1.0,1.0));
    for i in 0..2 { cmd.spawn((Mesh3d(hm.clone()), MeshMaterial3d(bm.clone()),
        Transform::from_translation(Vec3::new(30.0+i as f32*40.0,RUNWAY_Y+1.5,-8.0)).with_scale(Vec3::new(6.0,3.0,4.0)))); }

    let tb = meshes.add(Cuboid::new(1.0,1.0,1.0));
    cmd.spawn((Mesh3d(tb.clone()), MeshMaterial3d(tm.clone()),
        Transform::from_translation(Vec3::new(140.0,RUNWAY_Y+2.5,-12.0)).with_scale(Vec3::new(2.0,5.0,2.0))));
    cmd.spawn((Mesh3d(tb), MeshMaterial3d(tm),
        Transform::from_translation(Vec3::new(140.0,RUNWAY_Y+5.5,-12.0)).with_scale(Vec3::new(3.0,1.0,3.0))));
    spawn_bb(cmd,bbm,bbr,Vec3::new(140.0,RUNWAY_Y+6.5,-12.0),LightColor::RED,LightType::FLASH_SINGLE);

    let mut x = -0.2;
    while x < RUNWAY_LENGTH { for &z in &[-2.0,2.75,7.3] {
        spawn_bb(cmd,bbm,bby,Vec3::new(x,RUNWAY_Y,z),LightColor::YELLOW,LightType::SOLID);
    } x += 2.0; }
    let mut z = -2.0;
    while z < 8.0 { spawn_bb(cmd,bbm,bbg,Vec3::new(-2.5,RUNWAY_Y,z),LightColor::GREEN,LightType::SOLID); z+=0.49; }
    z = -2.0;
    while z < 8.0 { spawn_bb(cmd,bbm,bbr,Vec3::new(173.0,RUNWAY_Y,z),LightColor::RED,LightType::SOLID); z+=0.49; }
    for i in 1..=5 { spawn_bb(cmd,bbm,bbw,Vec3::new(-2.5-i as f32*8.0,RUNWAY_Y+0.2,RUNWAY_Z),LightColor::WHITE,LightType::FLASH_SINGLE); }
    for i in 0..4 {
        let (mat,lc) = if i<2 {(bbr,LightColor::RED)} else {(bbw,LightColor::WHITE)};
        spawn_bb(cmd,bbm,mat,Vec3::new(5.0,RUNWAY_Y+0.1,-4.0-i as f32*0.8),lc,LightType::SOLID);
    }
}

fn spawn_terrain_features(cmd: &mut Commands, meshes: &mut Assets<Mesh>, mats: &mut Assets<StandardMaterial>,
    bbm: &Handle<Mesh>, bbr: &Handle<StandardMaterial>,
    terrain: &TerrainData, cities: &[CityData], rng: &mut StdRng)
{
    let tree_mesh = meshes.add(Cuboid::new(1.0,1.0,1.0));
    let tc = [
        mats.add(StandardMaterial { base_color: Color::srgb(0.12,0.28,0.10), perceptual_roughness: 0.9, ..default() }),
        mats.add(StandardMaterial { base_color: Color::srgb(0.15,0.32,0.12), perceptual_roughness: 0.9, ..default() }),
        mats.add(StandardMaterial { base_color: Color::srgb(0.10,0.24,0.08), perceptual_roughness: 0.9, ..default() }),
    ];
    let fm = meshes.add(Cuboid::new(1.0,1.0,1.0));
    let fmat = mats.add(StandardMaterial { base_color: Color::srgb(0.55,0.45,0.35), perceptual_roughness: 0.9, ..default() });
    let twm = meshes.add(Cuboid::new(1.0,1.0,1.0));
    let twmat = mats.add(StandardMaterial { base_color: Color::srgb(0.40,0.40,0.40), perceptual_roughness: 0.6, ..default() });

    let feature_range = LAND_RADIUS * 0.85;
    let mut cnt = 0; let mut att = 0;
    while cnt < NUM_TREES && att < NUM_TREES*3 { att += 1;
        let x = rng.gen_range(-feature_range..feature_range);
        let z = rng.gen_range(-feature_range..feature_range);
        let h = terrain.get_height_world(x,z);
        if h < WATER_LEVEL+0.5 || h > BASE_HEIGHT+15.0 { continue; }
        if (x*x+z*z).sqrt() < AIRBASE_FLAT_RADIUS { continue; }
        if cities.iter().any(|c| (c.pos-Vec2::new(x,z)).length() < c.radius+20.0) { continue; }
        if fbm(x*0.002,z*0.002,2,999) < 0.45 { continue; }
        let sh = rng.gen_range(0.4..1.2_f32); let sw = rng.gen_range(0.15..0.35_f32);
        cmd.spawn((Mesh3d(tree_mesh.clone()), MeshMaterial3d(tc[rng.gen_range(0..tc.len())].clone()),
            Transform::from_translation(Vec3::new(x,h+sh/2.0,z)).with_scale(Vec3::new(sw,sh,sw))));
        cnt += 1;
    }
    cnt = 0; att = 0;
    while cnt < NUM_FARMS && att < NUM_FARMS*5 { att += 1;
        let x = rng.gen_range(-feature_range..feature_range);
        let z = rng.gen_range(-feature_range..feature_range);
        let h = terrain.get_height_world(x,z);
        if h < WATER_LEVEL+0.5 || h > BASE_HEIGHT+5.0 { continue; }
        if (x*x+z*z).sqrt() < AIRBASE_FLAT_RADIUS { continue; }
        if cities.iter().any(|c| (c.pos-Vec2::new(x,z)).length() < c.radius+50.0) { continue; }
        let (w,bh,d) = (rng.gen_range(1.0..3.0_f32), rng.gen_range(0.5..1.5_f32), rng.gen_range(1.0..2.5_f32));
        cmd.spawn((Mesh3d(fm.clone()), MeshMaterial3d(fmat.clone()),
            Transform::from_translation(Vec3::new(x,h+bh/2.0,z)).with_scale(Vec3::new(w,bh,d))));
        cnt += 1;
    }
    cnt = 0; att = 0;
    while cnt < NUM_COMM_TOWERS && att < NUM_COMM_TOWERS*10 { att += 1;
        let x = rng.gen_range(-feature_range..feature_range);
        let z = rng.gen_range(-feature_range..feature_range);
        let h = terrain.get_height_world(x,z);
        if h < WATER_LEVEL+0.5 || h > BASE_HEIGHT+20.0 { continue; }
        if (x*x+z*z).sqrt() < AIRBASE_FLAT_RADIUS+200.0 { continue; }
        let th = rng.gen_range(6.0..12.0_f32);
        cmd.spawn((Mesh3d(twm.clone()), MeshMaterial3d(twmat.clone()),
            Transform::from_translation(Vec3::new(x,h+th/2.0,z)).with_scale(Vec3::new(0.15,th,0.15))));
        spawn_bb(cmd,bbm,bbr,Vec3::new(x,h+th+0.2,z),LightColor::RED,LightType::FLASH_ALT_SINGLE);
        cnt += 1;
    }
}

fn spawn_ships(cmd: &mut Commands, meshes: &mut Assets<Mesh>, mats: &mut Assets<StandardMaterial>,
    bbm: &Handle<Mesh>, bbr: &Handle<StandardMaterial>, bbg: &Handle<StandardMaterial>,
    bbw: &Handle<StandardMaterial>, rng: &mut StdRng)
{
    let hull_mat = mats.add(StandardMaterial { base_color: Color::srgb(0.30,0.30,0.32), perceptual_roughness: 0.8, ..default() });
    let bridge_mat = mats.add(StandardMaterial { base_color: Color::srgb(0.45,0.42,0.40), perceptual_roughness: 0.7, ..default() });
    let hull_mesh = meshes.add(Cuboid::new(1.0,1.0,1.0));

    for _ in 0..NUM_SHIPS {
        // Place ships in the ocean (outside the land radius)
        let angle = rng.gen_range(0.0..std::f32::consts::TAU);
        let dist = rng.gen_range(LAND_RADIUS+1000.0..LAND_RADIUS+8000.0);
        let sx = angle.cos() * dist;
        let sz = angle.sin() * dist;
        let ship_y = WATER_LEVEL + 0.15;
        let heading = rng.gen_range(0.0..std::f32::consts::TAU);

        let hull_len = rng.gen_range(3.0..8.0_f32);
        let hull_w = hull_len * 0.2;
        let hull_h = hull_len * 0.08;

        // Hull
        cmd.spawn((Mesh3d(hull_mesh.clone()), MeshMaterial3d(hull_mat.clone()),
            Transform::from_translation(Vec3::new(sx, ship_y, sz))
                .with_rotation(Quat::from_rotation_y(heading))
                .with_scale(Vec3::new(hull_len, hull_h, hull_w))));

        // Bridge/superstructure
        let bridge_h = hull_h * 2.0;
        let bridge_offset = hull_len * 0.2;
        let bridge_pos = Vec3::new(
            sx + heading.cos() * bridge_offset,
            ship_y + hull_h/2.0 + bridge_h/2.0,
            sz + heading.sin() * bridge_offset,
        );
        cmd.spawn((Mesh3d(hull_mesh.clone()), MeshMaterial3d(bridge_mat.clone()),
            Transform::from_translation(bridge_pos)
                .with_rotation(Quat::from_rotation_y(heading))
                .with_scale(Vec3::new(hull_len*0.2, bridge_h, hull_w*0.7))));

        // Navigation lights: red (port), green (starboard), white (masthead)
        let port_offset = Vec3::new(-heading.sin()*hull_w*0.6, 0.3, heading.cos()*hull_w*0.6);
        let stbd_offset = Vec3::new(heading.sin()*hull_w*0.6, 0.3, -heading.cos()*hull_w*0.6);
        let mast_pos = Vec3::new(sx + heading.cos()*bridge_offset, ship_y+hull_h+bridge_h+0.3, sz + heading.sin()*bridge_offset);

        spawn_bb(cmd, bbm, bbr, Vec3::new(sx,ship_y,sz)+port_offset, LightColor::RED, LightType::SOLID);
        spawn_bb(cmd, bbm, bbg, Vec3::new(sx,ship_y,sz)+stbd_offset, LightColor::GREEN, LightType::SOLID);
        spawn_bb(cmd, bbm, bbw, mast_pos, LightColor::WHITE, LightType::SOLID);
    }
}

// ============================================================
// Origin Shifting
// ============================================================

/// Shifts root entities back toward the origin when the player flies far away.
/// Only root entities (no parent) are shifted — children move automatically
/// with their parents.  2D elements (cameras, text, sprites, meshes) are
/// excluded so the HUD stays in place.
pub fn origin_shift(
    mut shiftable: Query<
        &mut Transform,
        (Without<ChildOf>, Without<Camera2d>, Without<Text2d>, Without<Sprite>, Without<Mesh2d>),
    >,
    player_q: Query<&GlobalTransform, With<Player>>,
    mut terrain: ResMut<TerrainData>,
) {
    let Ok(pg) = player_q.single() else { return };
    let pos: Vec3 = pg.translation();
    if pos.x.abs() < ORIGIN_SHIFT_THRESHOLD && pos.z.abs() < ORIGIN_SHIFT_THRESHOLD { return; }

    let shift = Vec3::new(pos.x, 0.0, pos.z);
    for mut t in shiftable.iter_mut() {
        t.translation -= shift;
    }
    terrain.origin_shift += shift;
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
    let mut terrain = generate_heightmap(rng.gen());

    terrain.flatten_rect(-20.0,-20.0,RUNWAY_LENGTH+20.0,20.0,RUNWAY_Y,60.0);

    let cities = generate_cities(&terrain, &mut rng);
    terrain.city_positions = cities.iter().map(|c| c.pos).collect();
    for c in &cities { terrain.flatten_circle(c.pos.x,c.pos.y,c.radius,BASE_HEIGHT); }

    let roads = generate_roads(&cities, &terrain);
    for r in &roads { terrain.flatten_path(&r.waypoints, 8.0, BASE_HEIGHT); }

    let fields = generate_fields(&terrain, &cities, &mut rng);

    let bbm = meshes.add(Rectangle::new(0.01,0.01));
    let bby = bb_mat(&mut materials, images.add(create_texture(LightColor::YELLOW)));
    let bbg = bb_mat(&mut materials, images.add(create_texture(LightColor::GREEN)));
    let bbr = bb_mat(&mut materials, images.add(create_texture(LightColor::RED)));
    let bbw = bb_mat(&mut materials, images.add(create_texture(LightColor::WHITE)));

    spawn_moonlight(&mut commands);
    spawn_terrain_chunks(&mut commands, &mut meshes, &mut materials, &terrain);
    spawn_water(&mut commands, &mut meshes, &mut materials);
    spawn_cities(&mut commands, &mut meshes, &mut materials, &bbm,&bbw,&bby, &cities, &terrain, &mut rng);
    spawn_roads(&mut commands, &mut meshes, &mut materials, &bbm,&bby, &roads, &terrain);
    spawn_fields(&mut commands, &mut meshes, &mut materials, &fields, &terrain);
    spawn_airbase(&mut commands, &mut meshes, &mut materials, &bbm,&bby,&bbg,&bbr,&bbw);
    spawn_terrain_features(&mut commands, &mut meshes, &mut materials, &bbm,&bbr, &terrain, &cities, &mut rng);
    spawn_ships(&mut commands, &mut meshes, &mut materials, &bbm,&bbr,&bbg,&bbw, &mut rng);

    commands.insert_resource(terrain);
}
