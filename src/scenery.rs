
use bevy::{prelude::*, camera::visibility::RenderLayers};
use bevy_rapier3d::prelude::*;

use crate::bevy_scene_hook::{SceneHook};
use crate::billboard::Billboard;
use crate::{pointlight::{LightBillboardToBeAdded, LightColor, LightType, LightBillboard, create_texture, get_light_color_from_name, get_light_type_from_name, LightSourceType, get_lightsource_type_from_name}, definitions::RENDERLAYER_POINTLIGHTS};

pub fn setup_terrain(mut commands: Commands, asset_server: Res<AssetServer>) {
    let gltf_handle = asset_server.load("terrain/testmap.glb#Scene0");

    commands.spawn((
        SceneRoot(gltf_handle.clone()),
        SceneHook::new(|entity, cmds| {
            for n in entity.get::<Name>().iter() {
                let name = n.as_str();
                if name.starts_with("PointLight") {
                    cmds.insert(LightBillboardToBeAdded {
                        light_color: get_light_color_from_name(name),
                        light_type: get_light_type_from_name(name),
                        lightsource_type: get_lightsource_type_from_name(name)
                    });
                }
            }
        }),
        RigidBody::Fixed,
        AsyncSceneCollider {
            shape: Some(ComputedColliderShape::TriMesh(TriMeshFlags::empty())),
            ..default()
        },
    ));
}



pub fn setup_scenery(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    asset_server: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let billboard_mesh = meshes.add(Rectangle::new(0.01, 0.01));

    let mut image_handle = asset_server.load("test.png");
    let mut billboard_material = materials.add(StandardMaterial {
        base_color_texture: Some(image_handle.clone()),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });

    let mut i = 0.0;
    while i < 86.0 {
        commands
            .spawn((
                Mesh3d(billboard_mesh.clone()),
                MeshMaterial3d(billboard_material.clone()),
                Transform::from_translation(Vec3::new(-0.2 + (i * 2.0), -0.96, -2.0)),
                Billboard,
                LightBillboard {
                    light_color: LightColor::YELLOW,
                    light_type: LightType::SOLID,
                    lightsource_type: LightSourceType::NONE,
                    active: true,
                    occluded: false,
                },
                RenderLayers::layer(RENDERLAYER_POINTLIGHTS),
            ));

        commands
            .spawn((
                Mesh3d(billboard_mesh.clone()),
                MeshMaterial3d(billboard_material.clone()),
                Transform::from_translation(Vec3::new(-0.2 + (i * 2.0), -0.96, 2.75)),
                Billboard,
                LightBillboard {
                    light_color: LightColor::YELLOW,
                    light_type: LightType::SOLID,
                    lightsource_type: LightSourceType::NONE,
                    active: true,
                    occluded: false,
                },
                RenderLayers::layer(RENDERLAYER_POINTLIGHTS),
            ));

        commands
            .spawn((
                Mesh3d(billboard_mesh.clone()),
                MeshMaterial3d(billboard_material.clone()),
                Transform::from_translation(Vec3::new(-0.2 + (i * 2.0), -0.96, 7.3)),
                Billboard,
                LightBillboard {
                    light_color: LightColor::YELLOW,
                    light_type: LightType::SOLID,
                    lightsource_type: LightSourceType::NONE,
                    active: true,
                    occluded: false,
                },
                RenderLayers::layer(RENDERLAYER_POINTLIGHTS),
            ));

        i += 1.0;
    }
    i = 0.0;
    image_handle = images.add(create_texture(LightColor::GREEN));
    billboard_material = materials.add(StandardMaterial {
        base_color_texture: Some(image_handle.clone()),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    while i < 20.0 {
        commands
            .spawn((
                Mesh3d(billboard_mesh.clone()),
                MeshMaterial3d(billboard_material.clone()),
                Transform::from_translation(Vec3::new(-2.5, -0.96, -2.0 + (i * 0.49))),
                Billboard,
                LightBillboard {
                    light_color: LightColor::YELLOW,
                    light_type: LightType::SOLID,
                    lightsource_type: LightSourceType::NONE,
                    active: true,
                    occluded: false,
                },
                RenderLayers::layer(RENDERLAYER_POINTLIGHTS),
            ));
        i += 1.0;
    }
    i = 0.0;
    image_handle = images.add(create_texture(LightColor::RED));
    billboard_material = materials.add(StandardMaterial {
        base_color_texture: Some(image_handle.clone()),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    while i < 20.0 {
        commands
            .spawn((
                Mesh3d(billboard_mesh.clone()),
                MeshMaterial3d(billboard_material.clone()),
                Transform::from_translation(Vec3::new(173.0, -0.96, -2.0 + (i * 0.49))),
                Billboard,
                LightBillboard {
                    light_color: LightColor::YELLOW,
                    light_type: LightType::SOLID,
                    lightsource_type: LightSourceType::NONE,
                    active: true,
                    occluded: false,
                },
                RenderLayers::layer(RENDERLAYER_POINTLIGHTS),
            ));
        i += 1.0;
    }

}
