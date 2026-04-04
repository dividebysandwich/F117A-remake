use bevy::{
    prelude::*,
    asset::RenderAssetUsages,
    camera::visibility::RenderLayers,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

use crate::definitions::*;
use crate::player::Player;
use crate::terrain::TerrainData;

// ── Map constants ──

/// Pixel resolution of the generated map texture
const MAP_TEX_SIZE: u32 = 192;
/// Display size on the HUD (pixels in 1920×1080 HUD space)
const MAP_DISPLAY_SIZE: f32 = 240.0;
/// Position on the HUD — left side, matching FLIR on right
const MAP_HUD_X: f32 = -600.0;
const MAP_HUD_Y: f32 = -290.0;
/// Size of the terrain the map covers (same as TERRAIN_SIZE in terrain.rs)
const MAP_WORLD_EXTENT: f32 = 10000.0;
const MAP_HALF: f32 = MAP_WORLD_EXTENT / 2.0;
/// How large the player arrow is on the map
const PLAYER_MARKER_SCALE: f32 = 8.0;
/// Homebase marker size
const HOMEBASE_MARKER_SCALE: f32 = 6.0;

// ── Components ──

/// Marks the map sprite on the HUD
#[derive(Component)]
pub struct MapSprite;

/// A marker displayed on the overhead map.
/// Add this component (along with a Mesh2d or Sprite) to any entity
/// that should appear on the map.  The `update_map_markers` system
/// will keep its HUD-space position in sync with `world_pos`.
#[derive(Component)]
pub struct MapMarker {
    /// Position in original terrain coordinates (not shifted).
    /// For moving objects, update this each frame.
    pub world_pos: Vec2,
    /// Rotation in radians (0 = north / +Z, clockwise).
    pub heading: f32,
}

/// Specifically tags the player's map marker so we can update it.
#[derive(Component)]
pub struct PlayerMapMarker;

/// Tags the homebase marker.
#[derive(Component)]
pub struct HomebaseMapMarker;

// ── Setup ──

pub fn setup_map_mfd(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut color_mats: ResMut<Assets<ColorMaterial>>,
    terrain: Res<TerrainData>,
) {
    // Generate the map texture from the heightmap
    let map_image = generate_map_image(&terrain);
    let map_handle = images.add(map_image);

    // Map sprite on the cockpit HUD layer
    commands.spawn((
        Sprite {
            image: map_handle,
            custom_size: Some(Vec2::splat(MAP_DISPLAY_SIZE)),
            ..default()
        },
        Transform::from_translation(Vec3::new(MAP_HUD_X, MAP_HUD_Y, 0.0)),
        RenderLayers::layer(RENDERLAYER_COCKPIT),
        MapSprite,
    ));

    // Border around the map
    let border_color = Color::srgb(0.0, 0.6, 0.0);
    let half = MAP_DISPLAY_SIZE / 2.0;
    let border_mat = color_mats.add(ColorMaterial::from(border_color));
    for (ox, oy, w, h) in [
        (0.0, half, MAP_DISPLAY_SIZE + 4.0, 2.0),   // top
        (0.0, -half, MAP_DISPLAY_SIZE + 4.0, 2.0),  // bottom
        (-half, 0.0, 2.0, MAP_DISPLAY_SIZE),          // left
        (half, 0.0, 2.0, MAP_DISPLAY_SIZE),           // right
    ] {
        commands.spawn((
            Mesh2d(meshes.add(Rectangle::new(w, h))),
            MeshMaterial2d(border_mat.clone()),
            Transform::from_translation(Vec3::new(MAP_HUD_X + ox, MAP_HUD_Y + oy, 0.5)),
            RenderLayers::layer(RENDERLAYER_COCKPIT),
        ));
    }

    // Player marker — small green triangle
    let player_mat = color_mats.add(ColorMaterial::from(Color::srgb(0.0, 1.0, 0.0)));
    let player_mesh = meshes.add(Triangle2d::new(
        Vec2::new(0.0, PLAYER_MARKER_SCALE),
        Vec2::new(-PLAYER_MARKER_SCALE * 0.5, -PLAYER_MARKER_SCALE * 0.5),
        Vec2::new(PLAYER_MARKER_SCALE * 0.5, -PLAYER_MARKER_SCALE * 0.5),
    ));
    commands.spawn((
        Mesh2d(player_mesh),
        MeshMaterial2d(player_mat),
        Transform::from_translation(Vec3::new(MAP_HUD_X, MAP_HUD_Y, 1.0)),
        RenderLayers::layer(RENDERLAYER_COCKPIT),
        MapMarker { world_pos: Vec2::ZERO, heading: 0.0 },
        PlayerMapMarker,
    ));

    // Homebase marker — small white square at origin
    let home_mat = color_mats.add(ColorMaterial::from(Color::srgb(1.0, 1.0, 1.0)));
    let home_mesh = meshes.add(Rectangle::new(HOMEBASE_MARKER_SCALE, HOMEBASE_MARKER_SCALE));
    commands.spawn((
        Mesh2d(home_mesh),
        MeshMaterial2d(home_mat),
        Transform::from_translation(Vec3::new(MAP_HUD_X, MAP_HUD_Y, 0.8)),
        RenderLayers::layer(RENDERLAYER_COCKPIT),
        MapMarker { world_pos: Vec2::ZERO, heading: 0.0 },
        HomebaseMapMarker,
    ));
}

// ── Update ──

pub fn update_map_mfd(
    player_q: Query<(&Transform, &GlobalTransform), With<Player>>,
    terrain: Res<TerrainData>,
    mut markers: Query<(&mut Transform, &MapMarker), (Without<Player>, Without<MapSprite>, Without<PlayerMapMarker>)>,
    mut player_marker: Query<(&mut Transform, &mut MapMarker), (With<PlayerMapMarker>, Without<Player>, Without<MapSprite>)>,
) {
    // Update the player marker's world position and heading
    if let Ok((ptf, _pgt)) = player_q.single() {
        if let Ok((mut mtf, mut mm)) = player_marker.single_mut() {
            // Recover original terrain coords by adding origin_shift
            mm.world_pos = Vec2::new(
                ptf.translation.x + terrain.origin_shift.x,
                ptf.translation.z + terrain.origin_shift.z,
            );
            // Heading from aircraft forward (local +X)
            let fwd = ptf.rotation * Vec3::X;
            mm.heading = fwd.z.atan2(fwd.x);

            let (mx, my) = world_to_map(mm.world_pos);
            mtf.translation.x = mx;
            mtf.translation.y = my;
            mtf.rotation = Quat::from_rotation_z(-mm.heading + std::f32::consts::FRAC_PI_2);
        }
    }

    // Update all other map markers
    for (mut mtf, mm) in markers.iter_mut() {
        let (mx, my) = world_to_map(mm.world_pos);
        mtf.translation.x = mx;
        mtf.translation.y = my;
        mtf.rotation = Quat::from_rotation_z(-mm.heading + std::f32::consts::FRAC_PI_2);
    }
}

// ── Helpers ──

/// Convert original terrain world coords to HUD pixel position on the map.
fn world_to_map(world_pos: Vec2) -> (f32, f32) {
    let nx = world_pos.x / MAP_HALF; // −1 … +1
    let ny = world_pos.y / MAP_HALF;
    let mx = MAP_HUD_X + nx * (MAP_DISPLAY_SIZE / 2.0);
    let my = MAP_HUD_Y + ny * (MAP_DISPLAY_SIZE / 2.0);
    (mx, my)
}

/// Build a small overhead map image from the terrain heightmap.
fn generate_map_image(terrain: &TerrainData) -> Image {
    let s = MAP_TEX_SIZE as usize;
    let mut data = vec![0u8; s * s * 4];
    let water_level = -2.5_f32;
    let base_height = -1.0_f32;

    for py in 0..s {
        for px in 0..s {
            let wx = (px as f32 / s as f32 - 0.5) * MAP_WORLD_EXTENT;
            let wz = (py as f32 / s as f32 - 0.5) * MAP_WORLD_EXTENT;
            let h = terrain.get_height_world(wx, wz);

            let (r, g, b) = if h < water_level {
                (8, 25, 55) // ocean
            } else {
                let rel = h - base_height;
                if rel < 2.0 {
                    (40, 70, 35) // lowland
                } else if rel < 15.0 {
                    let t = (rel - 2.0) / 13.0;
                    (40 + (t * 30.0) as u8, 70 - (t * 15.0) as u8, 35 + (t * 10.0) as u8)
                } else {
                    (75, 55, 48) // mountains
                }
            };

            let idx = (py * s + px) * 4;
            data[idx] = r;
            data[idx + 1] = g;
            data[idx + 2] = b;
            data[idx + 3] = 255;
        }
    }

    // Mark cities as grey patches
    for city_pos in &terrain.city_positions {
        let cpx = ((city_pos.x / MAP_WORLD_EXTENT + 0.5) * s as f32) as i32;
        let cpy = ((city_pos.y / MAP_WORLD_EXTENT + 0.5) * s as f32) as i32;
        let cr = 3_i32; // city dot radius in pixels
        for dy in -cr..=cr {
            for dx in -cr..=cr {
                if dx * dx + dy * dy <= cr * cr {
                    let px = (cpx + dx).clamp(0, s as i32 - 1) as usize;
                    let py = (cpy + dy).clamp(0, s as i32 - 1) as usize;
                    let idx = (py * s + px) * 4;
                    data[idx] = 90;
                    data[idx + 1] = 90;
                    data[idx + 2] = 85;
                }
            }
        }
    }

    // Mark airbase at origin
    let abx = (0.5 * s as f32) as i32;
    let aby = (0.5 * s as f32) as i32;
    for dy in -2..=2_i32 {
        for dx in -4..=4_i32 {
            let px = (abx + dx).clamp(0, s as i32 - 1) as usize;
            let py = (aby + dy).clamp(0, s as i32 - 1) as usize;
            let idx = (py * s + px) * 4;
            data[idx] = 50;
            data[idx + 1] = 50;
            data[idx + 2] = 48;
        }
    }

    Image::new(
        Extent3d { width: MAP_TEX_SIZE, height: MAP_TEX_SIZE, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::all(),
    )
}
