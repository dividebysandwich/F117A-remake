use bevy::{prelude::{*, shape::Quad}, render::render_resource::{TextureDimension, Extent3d, TextureFormat}, reflect::TypeUuid};
use bevy_mod_billboard::{prelude::*, BillboardDepth};
use bevy_mod_raycast::prelude::*;

use crate::{MainCamera, util::get_time_millis};

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


#[derive(Debug, Copy, Clone)]
pub enum LightSourceType{
    POINT,
    SPOT,
    DIRECTIONAL,
    NONE
}

#[derive(Resource, TypeUuid, Reflect)]
#[uuid="58b43f34-80b3-4886-b9a0-93a48bf3ae6f"]
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
    );
    let color: [u8; 4];
    match light_color {
        LightColor::BLUE => color = [0, 0, 255, 255], // RGBA blue
        LightColor::GREEN => color = [0, 255, 0, 255], // RGBA green
        LightColor::RED => color = [255, 0, 0, 255],  // RGBA red
        LightColor::WHITE => color = [255, 255, 255, 255], // RGBA white
        LightColor::YELLOW => color = [255, 255, 0, 255], // RGBA yellow
    }
    image.data = (0..16 * 16).flat_map(|_| color).collect();
    image
}



pub fn auto_scale_and_hide_billboards(
    mut billboards: Query<(&mut Visibility, &GlobalTransform, &mut Transform, &mut LightBillboard), Without<PointLight>>,
    camera: Query<(&MainCamera, &GlobalTransform, &Transform, Without<LightBillboard>)>,
    raycast_query: Query<Entity, With<LightBillboard>>,
    mut raycast: Raycast,
) {
    let (_cam, c_global_transform, c_transform, _) = camera.single();

    for (mut b_visibility, b_global_transform, mut b_transform, mut billboard) in billboards.iter_mut() {
        let cam_distance = c_global_transform.translation().distance(b_global_transform.translation()) * 0.4;
//        let direction = (b_transform.translation - c_transform.translation).normalize();
//        let cam_up = c_transform.rotation * Vec3::Y;
//        let cam_right = cam_up.cross(direction).normalize();
//        let orthogonal = direction.cross(cam_right).normalize();

        let filter = |entity| !raycast_query.contains(entity);
        let early_exit_test = |_entity| true;
        let settings = RaycastSettings::default()
            .with_filter(&filter)
            .with_early_exit_test(&early_exit_test);

        let hits = raycast.cast_ray(Ray3d::new(c_global_transform.translation(), b_global_transform.translation() - c_global_transform.translation()), &settings);
        billboard.occluded = false;
        if billboard.active == true { // Don't make inactive billboards visible
            *b_visibility = Visibility::Visible;
        }
        b_transform.scale = Vec3::new(cam_distance, cam_distance, cam_distance);
        for (is_first, intersection) in hits {
            *b_visibility = Visibility::Hidden;
            billboard.occluded = true;
        }

    }
}

pub fn update_light_billboards(
    lights_to_add: Query<(Entity, &LightBillboardToBeAdded)>,
    mut commands: Commands,
    mut billboard_textures: ResMut<Assets<BillboardTexture>>,
    mut meshes: ResMut<Assets<Mesh>>,
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
        let light = commands
            .spawn(BillboardTextureBundle {
                texture: billboard_textures.add(BillboardTexture::Single(image_handle.clone())),
                mesh: meshes.add(Quad::new(Vec2::new(0.01, 0.01)).into()).into(),
                billboard_depth: BillboardDepth(false),
                ..default()
            })
            .insert(LightBillboard {
                light_color: light_billboard_to_be_added.light_color,
                light_type: light_billboard_to_be_added.light_type,
                lightsource_type : light_billboard_to_be_added.lightsource_type,
                active: true,
                occluded: false,
            })
            .id();
        commands.entity(entity).push_children(&[light]);
        if let LightSourceType::POINT = light_billboard_to_be_added.lightsource_type  {
            info!("Adding light source!");
            let lightsource = commands.spawn(PointLightBundle {
                point_light: PointLight {
                    color: Color::rgb(1.0, 1.0, 1.0),
                    intensity: 20000.,
                    range: 10.,
                    shadows_enabled: true,
                    ..default()
                },
                ..default()
            })
            .insert(LightBillboard {
                light_color: light_billboard_to_be_added.light_color,
                light_type: light_billboard_to_be_added.light_type,
                lightsource_type : light_billboard_to_be_added.lightsource_type,
                active: true,
                occluded: false,
            })
            .id();
            commands.entity(entity).push_children(&[lightsource]);
        }
        commands.entity(entity).remove::<LightBillboardToBeAdded>();
    }
}

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


    for (mut visibility, billboard, mut lightsource) in lightsources.iter_mut() {
        match billboard.light_type {        
            LightType::FLASH_SINGLE => {
                if first_flash_active {
                    lightsource.intensity = 1000.0;
//                    *visibility = Visibility::Visible;
                } else {
                    lightsource.intensity = 0.0;
//                    *visibility = Visibility::Hidden;
                }
            },
            LightType::FLASH_ALT_SINGLE => {
                if first_flash_alt_active {
                    lightsource.intensity = 1000.0;
//                    *visibility = Visibility::Visible;
                } else {
                    lightsource.intensity = 0.0;
//                    *visibility = Visibility::Hidden;
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