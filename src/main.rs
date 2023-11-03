use bevy::{
    asset::LoadState,
    core_pipeline::{clear_color::ClearColorConfig, Skybox},
    prelude::{shape::Quad, *},
    render::{
        camera::ScalingMode,
        render_resource::{
            Extent3d, Texture, TextureDimension, TextureFormat, TextureViewDescriptor,
            TextureViewDimension,
        },
        renderer::RenderDevice,
        texture::CompressedImageFormats,
        view::visibility::RenderLayers,
    },
};
use bevy_mod_billboard::{prelude::*, BillboardDepth};
use bevy_prototype_debug_lines::DebugLinesPlugin;
use bevy_rapier3d::prelude::*;
use bevy_scene_hook::{HookPlugin, HookedSceneBundle, SceneHook};
use bevy_third_person_camera::*;
use std::collections::HashMap;

mod aircraft;
mod billboard;
mod hud;
mod missile;
mod player;
mod sam;
mod util;
mod vehicle;

use crate::aircraft::*;
use crate::billboard::*;
use crate::hud::*;
use crate::missile::*;
use crate::player::*;
use crate::vehicle::*;

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins,
            bevy::diagnostic::FrameTimeDiagnosticsPlugin,
            bevy::diagnostic::EntityCountDiagnosticsPlugin,
            RapierPhysicsPlugin::<NoUserData>::default(),
            //    RapierDebugRenderPlugin::default(),
            ThirdPersonCameraPlugin,
            DebugLinesPlugin::default(),
            BillboardPlugin,
            HookPlugin,
        ))
        .add_systems(
            Startup,
            (setup_graphics, setup_terrain, spawn_player, setup_hud),
        )
        .add_systems(
            Update,
            (
                apply_skybox,
                handle_camera_controls,
                update_cockpit_camera,
                update_player_aircraft_controls,
                update_player_weapon_controls,
                update_missiles,
                update_aircraft_forces,
                update_hud,
                auto_scale_billboards,
                update_light_billboards,
            ),
        )
        .run()
}

fn handle_camera_controls(
    main_cameras: Query<Entity, With<MainCamera>>,
    mut commands: Commands,
    mut aircrafts: Query<&mut Visibility, With<Player>>,
    mut vehicles: Query<(Entity, &Vehicle)>,
    mut camera_settings: ResMut<CameraSettings>,
    input: Res<Input<KeyCode>>,
) {
    for mut aircraft_visibility in aircrafts.iter_mut() {
        if input.just_pressed(KeyCode::F1) {
            *aircraft_visibility = Visibility::Hidden;
            camera_settings.render_hud = true;
            for main_camera in main_cameras.iter() {
                commands.entity(main_camera).remove::<ThirdPersonCamera>();
                commands.entity(main_camera).insert(CockpitCamera);
                commands.entity(main_camera).remove::<RenderLayers>();
                commands
                    .entity(main_camera)
                    .insert(RenderLayers::from_layers(&[0, 2]));
            }
        } else if input.just_pressed(KeyCode::F2) {
            *aircraft_visibility = Visibility::Visible;
            camera_settings.render_hud = false;
            for main_camera in main_cameras.iter() {
                commands.entity(main_camera).remove::<CockpitCamera>();
                commands.entity(main_camera).insert(ThirdPersonCamera {
                    zoom: Zoom::new(0.2, 500.0),
                    ..default()
                });
                commands.entity(main_camera).remove::<RenderLayers>();
                commands
                    .entity(main_camera)
                    .insert(RenderLayers::from_layers(&[0, 2, 3])); //TODO: Remove Layer 1 to remove debug line display
            }
            let mut i: i32 = 0;
            let mut vehicles_sorted = vehicles.iter_mut().collect::<Vec<_>>();
            vehicles_sorted
                .sort_by(|(_, a), (_, b)| (a.serialnumber).partial_cmp(&b.serialnumber).unwrap());
            for (entity, _vehicle) in vehicles_sorted.iter() {
                if camera_settings.target_index == i {
                    commands.entity(*entity).insert(ThirdPersonCameraTarget);
                } else {
                    commands.entity(*entity).remove::<ThirdPersonCameraTarget>();
                }
                i += 1;
            }
            camera_settings.target_index += 1;
            if camera_settings.target_index >= i {
                camera_settings.target_index = 0;
            }
        }
    }
}

fn update_cockpit_camera(
    mut camera_transforms: Query<
        &mut Transform,
        (With<CockpitCamera>, Without<Aircraft>, Without<Player>),
    >,
    aircraft_transforms: Query<&Transform, (With<Aircraft>, With<Player>, Without<CockpitCamera>)>,
) {
    for aircraft_transform in aircraft_transforms.iter() {
        for mut camera_transform in camera_transforms.iter_mut() {
            camera_transform.translation = aircraft_transform.translation;
            camera_transform.rotation =
                aircraft_transform.rotation * Quat::from_rotation_y(f32::to_radians(-90.0));
        }
    }
}

const CUBEMAPS: &[(&str, CompressedImageFormats)] =
    &[("skybox/night.png", CompressedImageFormats::NONE)];

#[derive(Resource)]
struct Cubemap {
    is_loaded: bool,
    image_handle: Handle<Image>,
}

#[derive(Resource)]
pub struct CameraSettings {
    pub target_index: i32, // Keeps track of which target is currently being followed
    pub render_hud: bool,
}

#[derive(Component)]
pub struct MainCamera;

#[derive(Component)]
pub struct CockpitCamera;

#[derive(Debug, Copy, Clone)]
enum LightType {
    SOLID,
    BLINKING,
    FLASH_SINGLE,
    FLASH_DOUBLE,
}

#[derive(Debug, Copy, Clone)]
enum LightColor {
    WHITE,
    RED,
    GREEN,
    BLUE,
    YELLOW,
}

#[derive(Component)]
pub struct LightBillboardToBeAdded {
    light_color: LightColor,
    light_type: LightType,
}

#[derive(Component)]
pub struct LightBillboard {
    light_color: LightColor,
    light_type: LightType,
}

#[derive(Resource)]
struct LightTextures {
    map: HashMap<LightColor, Handle<Image>>,
}

fn apply_skybox(
    main_cameras: Query<Entity, With<MainCamera>>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
    mut cubemap: ResMut<Cubemap>,
) {
    if !cubemap.is_loaded && asset_server.get_load_state(&cubemap.image_handle) == LoadState::Loaded
    {
        info!("Applying skybox");
        let image = images.get_mut(&cubemap.image_handle).unwrap();
        // NOTE: PNGs do not have any metadata that could indicate they contain a cubemap texture,
        // so they appear as one texture. The following code reconfigures the texture as necessary.
        if image.texture_descriptor.array_layer_count() == 1 {
            image.reinterpret_stacked_2d_as_array(
                image.texture_descriptor.size.height / image.texture_descriptor.size.width,
            );
            image.texture_view_descriptor = Some(TextureViewDescriptor {
                dimension: Some(TextureViewDimension::Cube),
                ..default()
            });
        }

        for main_camera in main_cameras.iter() {
            commands
                .entity(main_camera)
                .insert(Skybox(cubemap.image_handle.clone()));
        }
        cubemap.is_loaded = true;
    }
}

fn setup_graphics(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut billboard_textures: ResMut<Assets<BillboardTexture>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
) {
    commands.insert_resource(Cubemap {
        is_loaded: false,
        image_handle: asset_server.load(CUBEMAPS[0].0),
    });

    // Initialize the third person target storage
    commands.insert_resource(CameraSettings { target_index: 0, render_hud: true });

    // Main 3d camera
    commands
        .spawn(Camera3dBundle {
            camera: Camera {
                // renders first
                order: 0,
                ..default()
            },
            //            transform: Transform::from_xyz(-3.0, 3.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
            ..Default::default()
        })
        .insert(UiCameraConfig {
            show_ui: false,
            ..default()
        })
        .insert(MainCamera)
        .insert(CockpitCamera)
        .insert(RenderLayers::from_layers(&[0, 2]));

    // HUD camera
    commands
        .spawn((
            Camera2dBundle {
                camera_2d: Camera2d {
                    // Don't clear the canvas before drawing
                    clear_color: ClearColorConfig::None,
                },
                camera: Camera {
                    // renders after / on top of the 3d camera
                    order: 2,
                    ..default()
                },
                projection: OrthographicProjection {
                    // Make sure the HUD scales with the window size
                    scale: 1.0,
                    scaling_mode: ScalingMode::Fixed {
                        width: 1920.,
                        height: 1080.,
                    },
                    ..default()
                }
                .into(),
                ..Default::default()
            },
            RenderLayers::layer(1),
        ))
        .insert(UiCameraConfig {
            show_ui: true,
            ..default()
        });

    // light
    /*commands.spawn(PointLightBundle {
        transform: Transform::from_xyz(50.0, 50.0, 50.0),
        point_light: PointLight {
            intensity: 600000.,
            range: 100.,
            shadows_enabled: true,
            ..default()
        },
        ..default()
    });*/

    commands.insert_resource(GizmoConfig {
        render_layers: RenderLayers::layer(1),
        ..default()
    });

    // Pixel shader render test
    /*    let material = materials.add(StandardMaterial {
        base_color: Color::hex("#ff0000").unwrap(),
        emissive: Color::hex("#ff0000").unwrap(),
        ..Default::default()
    });

    // Test entity for scaled point light textures
    commands.spawn(PbrBundle {
        transform: Transform::from_xyz(500.0, 1.0, 5.0),
        material: material,
        mesh: meshes.add(Mesh::from(shape::Cube { size: 1.0 })),
        ..Default::default()
    }).insert(Vehicle{..default()});
    */
    let mut image_handle = asset_server.load("test.png");
    commands
        .spawn(BillboardTextureBundle {
            transform: Transform::from_translation(Vec3::new(500.0, 0.5, 5.0)),
            texture: billboard_textures.add(BillboardTexture::Single(image_handle.clone())),
            mesh: meshes.add(Quad::new(Vec2::new(0.01, 0.01)).into()).into(),
            billboard_depth: BillboardDepth(false),
            ..default()
        })
        .insert(LightBillboard {
            light_color: LightColor::YELLOW,
            light_type: LightType::SOLID,
        });

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
            });
        i += 1.0;
    }
}

fn create_texture(light_color: LightColor) -> Image {
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
/* TODO
fn load_textures(
mut commands: Commands,
asset_server: Res<AssetServer>,
mut texture_handles: ResMut<LightTextures>,
) {
let texture_red = create_texture(LightColor::RED);
texture_handles.map.insert(LightColor::RED, texture_red);
}
*/

fn update_light_billboards(
    lights_to_add: Query<(Entity, &LightBillboardToBeAdded)>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut billboard_textures: ResMut<Assets<BillboardTexture>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let image_handle: Handle<Image> = asset_server.load("test.png");
    for (entity, light_billboard_to_be_added) in lights_to_add.iter() {
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
            })
            .id();
        commands.entity(entity).push_children(&[light]);
        commands.entity(entity).remove::<LightBillboardToBeAdded>();
    }
}

fn setup_terrain(mut commands: Commands, asset_server: Res<AssetServer>) {
    let gltf_handle = asset_server.load("terrain/testmap.gltf#Scene0");

    /*    commands.spawn((HookedSceneBundle {
        scene: SceneBundle { scene: gltf_handle, ..default() },
        hook: SceneHook::new(|entity, cmds| {
            for n in entity.get::<Name>().iter() {
                let name = n.as_str();
                if name.starts_with("PointLight") {
                    cmds.insert(BillboardTextureBundle {
                        transform: Transform::from_translation(Vec3::new(500.0, 0.5, 5.0)),
                        texture: billboard_textures.add(BillboardTexture::Single(image_handle)),
                        mesh: meshes.add(Quad::new(Vec2::new(0.01, 0.01)).into()).into(),
                        billboard_depth: BillboardDepth(false),
                        ..default()
                    }).insert(LightBillboard);

                }
            }
        }),
    },
    RigidBody::Fixed,
    AsyncSceneCollider {
        shape: Some(ComputedColliderShape::TriMesh),
        ..default()
    }
    ));
    */

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
                            light_color: LightColor::YELLOW,
                            light_type: LightType::SOLID,
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

    /*
    commands.spawn((SceneBundle {
        scene: gltf_handle.clone(),
        ..default()
        },
        RigidBody::Fixed,
        AsyncSceneCollider {
            shape: Some(ComputedColliderShape::TriMesh),
            ..default()
        }
    ))
    .insert(CollisionGroups::new(Group::from_bits_truncate(0b0001), Group::from_bits_truncate(0b1111)));
    */
}
