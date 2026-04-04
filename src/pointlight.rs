use bevy::{prelude::*, asset::RenderAssetUsages, camera::visibility::RenderLayers, render::render_resource::{Extent3d, TextureDimension, TextureFormat}};

use crate::{MainCamera, billboard::Billboard, terrain::TerrainChunk, util::get_time_millis, definitions::RENDERLAYER_POINTLIGHTS};

#[allow(non_camel_case_types)]
#[derive(Debug, Copy, Clone)]
pub enum LightType {
    SOLID,
    BLINKING,
    FLASH_SINGLE,
    FLASH_ALT_SINGLE,
    FLASH_DOUBLE,
}

#[derive(Debug, Copy, Clone)]
pub enum LightColor {
    WHITE,
    RED,
    GREEN,
    BLUE,
    YELLOW,
}

#[derive(Component)]
pub struct LightBillboardToBeAdded {
    pub light_color: LightColor,
    pub light_type: LightType,
    pub lightsource_type: LightSourceType,
}

#[derive(Component)]
pub struct LightBillboard {
    pub light_color: LightColor,
    pub light_type: LightType,
    pub lightsource_type: LightSourceType,
    pub active: bool,
    pub occluded: bool,
}

#[allow(dead_code)]
#[derive(Debug, Copy, Clone)]
pub enum LightSourceType{
    POINT,
    SPOT,
    DIRECTIONAL,
    NONE
}

#[derive(Resource)]
pub struct PrefabImages {
    red: Handle<Image>,
    green: Handle<Image>,
    blue: Handle<Image>,
    yellow: Handle<Image>,
    white: Handle<Image>,
}


pub fn initialize_textures(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
) {
    commands.insert_resource(PrefabImages {
        red: images.add(create_texture(LightColor::RED)),
        green: images.add(create_texture(LightColor::GREEN)),
        blue: images.add(create_texture(LightColor::BLUE)),
        yellow: images.add(create_texture(LightColor::YELLOW)),
        white: images.add(create_texture(LightColor::WHITE)),
    });

}

pub fn create_texture(light_color: LightColor) -> Image {
    let mut image = Image::new_fill(
        Extent3d {
            width: 16,
            height: 16,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &vec![0; 16 * 16 * 4],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::all()
    );
    let color: [u8; 4];
    match light_color {
        LightColor::BLUE => color = [0, 0, 255, 255], // RGBA blue
        LightColor::GREEN => color = [0, 255, 0, 255], // RGBA green
        LightColor::RED => color = [255, 0, 0, 255],  // RGBA red
        LightColor::WHITE => color = [255, 255, 255, 255], // RGBA white
        LightColor::YELLOW => color = [255, 255, 0, 255], // RGBA yellow
    }
    image.data = Some((0..16 * 16).flat_map(|_| color).collect());
    image
}



pub fn auto_scale_and_hide_billboards(
    mut billboards: Query<(&mut Visibility, &GlobalTransform, &mut Transform, &mut LightBillboard), Without<PointLight>>,
    camera: Query<(&MainCamera, &GlobalTransform, &Transform), Without<LightBillboard>>,
    raycast_query: Query<Entity, With<LightBillboard>>,
    terrain_chunks: Query<Entity, With<TerrainChunk>>,
    mut raycast: MeshRayCast,
) {
    let Ok((_cam, c_global_transform, _c_transform)) = camera.single() else { return };

    for (mut b_visibility, b_global_transform, mut b_transform, mut billboard) in billboards.iter_mut() {
        let cam_distance = c_global_transform.translation().distance(b_global_transform.translation()) * 0.4;

        // Exclude billboard entities AND terrain chunks from the raycast so
        // ground-level lights are not hidden by the terrain surface they sit on.
        let filter = |entity: Entity| {
            !raycast_query.contains(entity) && !terrain_chunks.contains(entity)
        };
        let settings = MeshRayCastSettings::default()
            .with_filter(&filter)
            .with_early_exit_test(&|_entity| true);

        let ray_dir = b_global_transform.translation() - c_global_transform.translation();
        let billboard_dist = ray_dir.length();
        let hits = raycast.cast_ray(
            Ray3d::new(c_global_transform.translation(), Dir3::new(ray_dir).unwrap_or(Dir3::Z)),
            &settings,
        );
        billboard.occluded = false;
        if billboard.active { // Don't make inactive billboards visible
            *b_visibility = Visibility::Visible;
        }
        b_transform.scale = Vec3::new(cam_distance, cam_distance, cam_distance);
        // Only occlude if a hit is actually between camera and billboard,
        // not behind the billboard (e.g. the surface it sits on).
        for (_entity, intersection) in hits {
            if intersection.distance < billboard_dist * 0.95 {
                *b_visibility = Visibility::Hidden;
                billboard.occluded = true;
            }
        }
    }
}

pub fn update_light_billboards(
    lights_to_add: Query<(Entity, &LightBillboardToBeAdded)>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    image_handles: Res<PrefabImages>,
) {
    for (entity, light_billboard_to_be_added) in lights_to_add.iter() {
        let image_handle: Handle<Image>;
        match light_billboard_to_be_added.light_color {
            LightColor::BLUE => image_handle = image_handles.blue.clone(),
            LightColor::GREEN => image_handle = image_handles.green.clone(),
            LightColor::RED => image_handle = image_handles.red.clone(),
            LightColor::WHITE => image_handle = image_handles.white.clone(),
            LightColor::YELLOW => image_handle = image_handles.yellow.clone(),
        }
        let material = materials.add(StandardMaterial {
            base_color_texture: Some(image_handle),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            double_sided: true,
            cull_mode: None,
            ..default()
        });
        let light = commands
            .spawn((
                Mesh3d(meshes.add(Rectangle::new(0.01, 0.01))),
                MeshMaterial3d(material),
                Billboard,
            ))
            .insert(LightBillboard {
                light_color: light_billboard_to_be_added.light_color,
                light_type: light_billboard_to_be_added.light_type,
                lightsource_type : light_billboard_to_be_added.lightsource_type,
                active: true,
                occluded: false,
            })
            .insert(RenderLayers::layer(RENDERLAYER_POINTLIGHTS))
            .id();
        commands.entity(light).insert(ChildOf(entity));
        if let LightSourceType::POINT = light_billboard_to_be_added.lightsource_type  {
            info!("Adding light source!");
            let lightsource = commands.spawn(
                PointLight {
                    color: Color::srgb(1.0, 1.0, 1.0),
                    intensity: 20000.,
                    range: 10.,
                    shadows_enabled: true,
                    ..default()
                },
            )
            .insert(LightBillboard {
                light_color: light_billboard_to_be_added.light_color,
                light_type: light_billboard_to_be_added.light_type,
                lightsource_type : light_billboard_to_be_added.lightsource_type,
                active: true,
                occluded: false,
            })
            .insert(RenderLayers::layer(RENDERLAYER_POINTLIGHTS))
            .id();
            commands.entity(lightsource).insert(ChildOf(entity));
        }
        commands.entity(entity).remove::<LightBillboardToBeAdded>();
    }
}

#[allow(unused_mut)]
pub fn update_blinking_lights(
    mut billboards: Query<(&mut Visibility, &mut LightBillboard), Without<PointLight>>,
    mut lightsources: Query<(&mut Visibility, &LightBillboard, &mut PointLight), With<PointLight>>,
) {
    let milliseconds = get_time_millis();

    let slow_blink_active: bool;
    match milliseconds % 2000 {
        0..=1000 => slow_blink_active = true,
        _ => slow_blink_active = false,
    }

    let first_flash_active: bool;
    match milliseconds % 2000 {
        0..=50 => first_flash_active = true,
        _ => first_flash_active = false,
    }
    let second_flash_active: bool;
    match milliseconds % 2000 {
        300..=350 => second_flash_active = true,
        _ => second_flash_active = false,
    }
    let first_flash_alt_active: bool;
    match milliseconds % 2000 {
        400..=450 => first_flash_alt_active = true,
        _ => first_flash_alt_active = false,
    }


for (mut _visibility, billboard, mut lightsource) in lightsources.iter_mut() {
        match billboard.light_type {
            LightType::FLASH_SINGLE => {
                if first_flash_active {
                    lightsource.intensity = 1000.0;
                } else {
                    lightsource.intensity = 0.0;
                }
            },
            LightType::FLASH_ALT_SINGLE => {
                if first_flash_alt_active {
                    lightsource.intensity = 1000.0;
                } else {
                    lightsource.intensity = 0.0;
                }
            },
            _ => {},
        }
    }

    for (mut visibility, mut billboard) in billboards.iter_mut() {
        match billboard.light_type {
            LightType::BLINKING => {
                if slow_blink_active {
                    billboard.active = true;
                    if billboard.occluded == false {
                        *visibility = Visibility::Visible;
                    }
                } else {
                    billboard.active = false;
                    *visibility = Visibility::Hidden;
                }
            },
            LightType::FLASH_SINGLE => {
                if first_flash_active {
                    billboard.active = true;
                    if billboard.occluded == false {
                        *visibility = Visibility::Visible;
                    }
                } else {
                    billboard.active = false;
                    *visibility = Visibility::Hidden;
                }
            },
            LightType::FLASH_ALT_SINGLE => {
                if first_flash_alt_active {
                    billboard.active = true;
                    if billboard.occluded == false {
                        *visibility = Visibility::Visible;
                    }
                } else {
                    billboard.active = false;
                    *visibility = Visibility::Hidden;
                }
            },
            LightType::FLASH_DOUBLE => {
                if first_flash_active || second_flash_active {
                    billboard.active = true;
                    if billboard.occluded == false {
                        *visibility = Visibility::Visible;
                    }
                } else {
                    billboard.active = false;
                    *visibility = Visibility::Hidden;
                }
            },
            _ => {},
        }
    }

}


pub fn get_light_color_from_name(name: &str) -> LightColor{
    if name.contains("_RED") {
        return LightColor::RED;
    } else if name.contains("_GREEN") {
        return LightColor::GREEN;
    } else if name.contains("_BLUE") {
        return LightColor::BLUE;
    } else if name.contains("_YELLOW") {
        return LightColor::YELLOW;
    } else if name.contains("_WHITE") {
        return LightColor::WHITE;
    } else {
        return LightColor::WHITE;
    }
}

pub fn get_light_type_from_name(name: &str) -> LightType{
    if name.contains("_BLINKING") {
        return LightType::BLINKING;
    } else if name.contains("_FLASH_SINGLE") {
        return LightType::FLASH_SINGLE;
    } else if name.contains("_FLASH_ALT_SINGLE") {
        return LightType::FLASH_ALT_SINGLE;
    } else if name.contains("_FLASH_DOUBLE") {
        return LightType::FLASH_DOUBLE;
    } else {
        return LightType::SOLID;
    }
}

pub fn get_lightsource_type_from_name(name: &str) -> LightSourceType{
    if name.contains("_ILLUMINATING") {
        return LightSourceType::POINT;
    } else {
        return LightSourceType::NONE;
    }
}
