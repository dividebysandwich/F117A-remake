use bevy::{
    prelude::*,
    asset::RenderAssetUsages,
    camera::visibility::RenderLayers,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

use crate::definitions::*;
use crate::player::Player;
use crate::terrain::TerrainData;

// ── Constants ──

/// Pixel resolution of the map texture (updated every frame)
const MAP_TEX_SIZE: u32 = 128;
/// Display size on the HUD
const MAP_DISPLAY_SIZE: f32 = 240.0;
/// HUD position (left side, matching FLIR on right)
const MAP_HUD_X: f32 = -600.0;
const MAP_HUD_Y: f32 = -290.0;
/// World radius shown around the player (in game units).
/// The map shows a square 2 × VIEW_RADIUS on each side.
const VIEW_RADIUS: f32 = 8000.0;
const PLAYER_MARKER_SCALE: f32 = 8.0;

// ── Components ──

#[derive(Component)]
pub struct MapSprite;

/// Anything with this component + a visual on RENDERLAYER_COCKPIT
/// will be positioned on the map.
#[derive(Component)]
pub struct MapMarker {
    pub world_pos: Vec2,
    pub heading: f32,
}

#[derive(Component)]
pub struct PlayerMapMarker;

#[derive(Component)]
pub struct HomebaseMapMarker;

/// Handle to the map image asset so we can write to it each frame
#[derive(Resource)]
pub struct MapImageHandle(Handle<Image>);

// ── Setup ──

pub fn setup_map_mfd(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut color_mats: ResMut<Assets<ColorMaterial>>,
    terrain: Res<TerrainData>,
) {
    // Create blank map image (filled on first frame by update_map_mfd)
    let s = MAP_TEX_SIZE as usize;
    let map_image = Image::new(
        Extent3d { width: MAP_TEX_SIZE, height: MAP_TEX_SIZE, depth_or_array_layers: 1 },
        TextureDimension::D2,
        vec![0u8; s * s * 4],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::all(),
    );
    let map_handle = images.add(map_image);
    // Draw initial content centred on origin
    if let Some(img) = images.get_mut(&map_handle) {
        redraw_map(img, &terrain, Vec2::ZERO);
    }
    commands.insert_resource(MapImageHandle(map_handle.clone()));

    // Map sprite
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

    // Border
    let border_mat = color_mats.add(ColorMaterial::from(Color::srgb(0.0, 0.6, 0.0)));
    let half = MAP_DISPLAY_SIZE / 2.0;
    for (ox, oy, w, h) in [
        (0.0, half, MAP_DISPLAY_SIZE + 4.0, 2.0),
        (0.0, -half, MAP_DISPLAY_SIZE + 4.0, 2.0),
        (-half, 0.0, 2.0, MAP_DISPLAY_SIZE),
        (half, 0.0, 2.0, MAP_DISPLAY_SIZE),
    ] {
        commands.spawn((
            Mesh2d(meshes.add(Rectangle::new(w, h))),
            MeshMaterial2d(border_mat.clone()),
            Transform::from_translation(Vec3::new(MAP_HUD_X + ox, MAP_HUD_Y + oy, 0.5)),
            RenderLayers::layer(RENDERLAYER_COCKPIT),
        ));
    }

    // Player marker (green triangle, always at map centre)
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

    // Homebase marker (white dot, moves relative to player)
    let home_mat = color_mats.add(ColorMaterial::from(Color::srgb(1.0, 1.0, 1.0)));
    let home_mesh = meshes.add(Rectangle::new(5.0, 5.0));
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
    player_q: Query<&Transform, With<Player>>,
    terrain: Res<TerrainData>,
    map_handle: Option<Res<MapImageHandle>>,
    mut images: ResMut<Assets<Image>>,
    mut markers: Query<
        (&mut Transform, &MapMarker),
        (Without<Player>, Without<MapSprite>, Without<PlayerMapMarker>),
    >,
    mut player_marker: Query<
        (&mut Transform, &mut MapMarker),
        (With<PlayerMapMarker>, Without<Player>, Without<MapSprite>),
    >,
) {
    let Ok(ptf) = player_q.single() else { return };
    let Some(mh) = map_handle else { return };

    // Player position in original terrain coords
    let player_terrain = Vec2::new(
        ptf.translation.x + terrain.origin_shift.x,
        ptf.translation.z + terrain.origin_shift.z,
    );
    let fwd = ptf.rotation * Vec3::X;
    let heading = fwd.z.atan2(fwd.x);

    // Re-render the map texture centred on the player
    if let Some(image) = images.get_mut(&mh.0) {
        redraw_map(image, &terrain, player_terrain);
    }

    // Player marker stays at map centre, just rotates
    if let Ok((mut mtf, mut mm)) = player_marker.single_mut() {
        mm.world_pos = player_terrain;
        mm.heading = heading;
        mtf.translation.x = MAP_HUD_X;
        mtf.translation.y = MAP_HUD_Y;
        mtf.rotation = Quat::from_rotation_z(-heading + std::f32::consts::FRAC_PI_2);
    }

    // Other markers position relative to player
    for (mut mtf, mm) in markers.iter_mut() {
        let rel = mm.world_pos - player_terrain;
        let (mx, my) = rel_to_hud(rel);
        mtf.translation.x = mx;
        mtf.translation.y = my;
        // Hide if off the map
        let h = MAP_DISPLAY_SIZE / 2.0;
        let vis = (mx - MAP_HUD_X).abs() < h && (my - MAP_HUD_Y).abs() < h;
        mtf.scale = if vis { Vec3::ONE } else { Vec3::ZERO };
        mtf.rotation = Quat::from_rotation_z(-mm.heading + std::f32::consts::FRAC_PI_2);
    }
}

// ── Helpers ──

/// Convert a world-space offset (relative to player) to HUD pixel position.
fn rel_to_hud(rel: Vec2) -> (f32, f32) {
    let nx = rel.x / VIEW_RADIUS;
    let ny = rel.y / VIEW_RADIUS;
    (
        MAP_HUD_X + nx * (MAP_DISPLAY_SIZE / 2.0),
        MAP_HUD_Y + ny * (MAP_DISPLAY_SIZE / 2.0),
    )
}

/// Redraw the map texture centred on `center` (terrain coords).
fn redraw_map(image: &mut Image, terrain: &TerrainData, center: Vec2) {
    let s = MAP_TEX_SIZE as usize;
    let water_level = -2.5_f32;
    let base_height = -1.0_f32;

    // We need mutable access to the pixel data
    let data = image.data.get_or_insert_with(|| vec![0u8; s * s * 4]);
    if data.len() < s * s * 4 {
        data.resize(s * s * 4, 0);
    }

    for py in 0..s {
        for px in 0..s {
            let wx = center.x + (px as f32 / s as f32 - 0.5) * VIEW_RADIUS * 2.0;
            let wz = center.y + (py as f32 / s as f32 - 0.5) * VIEW_RADIUS * 2.0;
            let h = terrain.get_height_world(wx, wz);

            let (r, g, b) = if h < water_level {
                (8u8, 25, 55)
            } else {
                let rel = h - base_height;
                if rel < 2.0 { (40, 70, 35) }
                else if rel < 15.0 {
                    let t = (rel - 2.0) / 13.0;
                    (40 + (t * 30.0) as u8, 70 - (t * 15.0) as u8, 35 + (t * 10.0) as u8)
                } else { (75, 55, 48) }
            };

            let idx = (py * s + px) * 4;
            data[idx] = r; data[idx+1] = g; data[idx+2] = b; data[idx+3] = 255;
        }
    }

    // Mark cities that are within view
    for city_pos in &terrain.city_positions {
        let rel = *city_pos - center;
        if rel.x.abs() > VIEW_RADIUS || rel.y.abs() > VIEW_RADIUS { continue; }
        let cpx = ((rel.x / (VIEW_RADIUS * 2.0) + 0.5) * s as f32) as i32;
        let cpy = ((rel.y / (VIEW_RADIUS * 2.0) + 0.5) * s as f32) as i32;
        for dy in -2..=2_i32 { for dx in -2..=2_i32 {
            if dx*dx + dy*dy > 4 { continue; }
            let x = (cpx+dx).clamp(0, s as i32-1) as usize;
            let y = (cpy+dy).clamp(0, s as i32-1) as usize;
            let i = (y*s+x)*4;
            data[i] = 120; data[i+1] = 120; data[i+2] = 110;
        }}
    }

    // Homebase (origin) indicator
    let home_rel = -center;
    if home_rel.x.abs() < VIEW_RADIUS && home_rel.y.abs() < VIEW_RADIUS {
        let hx = ((home_rel.x / (VIEW_RADIUS * 2.0) + 0.5) * s as f32) as i32;
        let hy = ((home_rel.y / (VIEW_RADIUS * 2.0) + 0.5) * s as f32) as i32;
        for dy in -2..=2_i32 { for dx in -3..=3_i32 {
            let x = (hx+dx).clamp(0, s as i32-1) as usize;
            let y = (hy+dy).clamp(0, s as i32-1) as usize;
            let i = (y*s+x)*4;
            data[i] = 200; data[i+1] = 200; data[i+2] = 180;
        }}
    }
}
