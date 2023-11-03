
use bevy::prelude::{*, shape::Quad};
use bevy_mod_billboard::{prelude::BillboardTexture, BillboardTextureBundle, BillboardDepth};
use bevy_rapier3d::prelude::*;
use bevy_scene_hook::{HookedSceneBundle, SceneHook};

use crate::pointlight::{LightBillboardToBeAdded, LightColor, LightType, LightBillboard, create_texture, get_light_color_from_name, get_light_type_from_name};

pub fn setup_terrain(mut commands: Commands, asset_server: Res<AssetServer>) {
    let gltf_handle = asset_server.load("terrain/testmap.glb#Scene0");

    commands.spawn((
        HookedSceneBundle {
            scene: SceneBundle {
                scene: gltf_handle.clone(),
                ..default()
            },
            hook: SceneHook::new(|entity, cmds| {
                for n in entity.get::<Name>().iter() {
                    let name = n.as_str();
                    if name.starts_with("PointLight") {
                        cmds.insert(LightBillboardToBeAdded {
                            light_color: get_light_color_from_name(name),
                            light_type: get_light_type_from_name(name),
                        });
                    }
                }
            }),
        },
        RigidBody::Fixed,
        AsyncSceneCollider {
            shape: Some(ComputedColliderShape::TriMesh),
            ..default()
        },
    ));
}



pub fn setup_scenery(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut billboard_textures: ResMut<Assets<BillboardTexture>>,
    asset_server: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
) {
    let mut image_handle = asset_server.load("test.png");

    let mut i = 0.0;
    while i < 86.0 {
        commands
            .spawn(BillboardTextureBundle {
                transform: Transform::from_translation(Vec3::new(-0.2 + (i * 2.0), -0.96, -2.0)),
                texture: billboard_textures.add(BillboardTexture::Single(image_handle.clone())),
                mesh: meshes.add(Quad::new(Vec2::new(0.01, 0.01)).into()).into(),
                billboard_depth: BillboardDepth(false),
                ..default()
            })
            .insert(LightBillboard {
                light_color: LightColor::YELLOW,
                light_type: LightType::SOLID,
                active: true,
                occluded: false,
            });

        commands
            .spawn(BillboardTextureBundle {
                transform: Transform::from_translation(Vec3::new(-0.2 + (i * 2.0), -0.96, 2.75)),
                texture: billboard_textures.add(BillboardTexture::Single(image_handle.clone())),
                mesh: meshes.add(Quad::new(Vec2::new(0.01, 0.01)).into()).into(),
                billboard_depth: BillboardDepth(false),
                ..default()
            })
            .insert(LightBillboard {
                light_color: LightColor::YELLOW,
                light_type: LightType::SOLID,
                active: true,
                occluded: false,
            });

        commands
            .spawn(BillboardTextureBundle {
                transform: Transform::from_translation(Vec3::new(-0.2 + (i * 2.0), -0.96, 7.3)),
                texture: billboard_textures.add(BillboardTexture::Single(image_handle.clone())),
                mesh: meshes.add(Quad::new(Vec2::new(0.01, 0.01)).into()).into(),
                billboard_depth: BillboardDepth(false),
                ..default()
            })
            .insert(LightBillboard {
                light_color: LightColor::YELLOW,
                light_type: LightType::SOLID,
                active: true,
                occluded: false,
            });

        i += 1.0;
    }
    i = 0.0;
    image_handle = images.add(create_texture(LightColor::RED));
    while i < 20.0 {
        commands
            .spawn(BillboardTextureBundle {
                transform: Transform::from_translation(Vec3::new(-2.5, -0.96, -2.0 + (i * 0.49))),
                texture: billboard_textures.add(BillboardTexture::Single(image_handle.clone())),
                mesh: meshes.add(Quad::new(Vec2::new(0.01, 0.01)).into()).into(),
                billboard_depth: BillboardDepth(false),
                ..default()
            })
            .insert(LightBillboard {
                light_color: LightColor::YELLOW,
                light_type: LightType::SOLID,
                active: true,
                occluded: false,
            });
        i += 1.0;
    }
    i = 0.0;
    image_handle = images.add(create_texture(LightColor::GREEN));
    while i < 20.0 {
        commands
            .spawn(BillboardTextureBundle {
                transform: Transform::from_translation(Vec3::new(173.0, -0.96, -2.0 + (i * 0.49))),
                texture: billboard_textures.add(BillboardTexture::Single(image_handle.clone())),
                mesh: meshes.add(Quad::new(Vec2::new(0.01, 0.01)).into()).into(),
                billboard_depth: BillboardDepth(false),
                ..default()
            })
            .insert(LightBillboard {
                light_color: LightColor::YELLOW,
                light_type: LightType::SOLID,
                active: true,
                occluded: false,
            });
        i += 1.0;
    }

}