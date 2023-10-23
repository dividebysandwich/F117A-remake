use bevy::{
        prelude::*,
        asset::LoadState,
        core_pipeline::{
            clear_color::ClearColorConfig,
            Skybox,
        },
        render::{
            camera::ScalingMode,
            render_resource::{TextureViewDescriptor, TextureViewDimension},
            view::visibility::RenderLayers,
            texture::CompressedImageFormats,
        }
    };
use bevy_editor_pls::*;
use bevy_rapier3d::prelude::*;
use bevy_third_person_camera::*;
use bevy_prototype_debug_lines::DebugLinesPlugin;

mod aircraft;
mod player;
mod hud;
mod sam;

use crate::aircraft::*;
use crate::player::*;
use crate::hud::*;
use crate::sam::*;

fn main() {
    App::new()
    .add_plugins((
        DefaultPlugins, 
        EditorPlugin::default(),
        bevy::diagnostic::FrameTimeDiagnosticsPlugin,
        bevy::diagnostic::EntityCountDiagnosticsPlugin,
        RapierPhysicsPlugin::<NoUserData>::default(),
//        RapierDebugRenderPlugin::default(),
        ThirdPersonCameraPlugin,
        DebugLinesPlugin::default()
    ))
    .add_systems(Startup, (
        setup_graphics, 
        setup_physics,
        spawn_player,
        setup_hud,
    ))
    .add_systems(Update, (
        apply_skybox,
        handle_camera_controls,
        update_cockpit_camera,
        update_player_aircraft_controls, 
        update_aircraft_forces,
        update_hud, 
    ))
    .run()
}

fn handle_camera_controls(
    main_cameras: Query<Entity, With<MainCamera>>,
    mut commands: Commands,
    mut aircrafts: Query<&mut Visibility, With<Player>>,
    input: Res<Input<KeyCode>>) {
    for mut aircraft_visibility in aircrafts.iter_mut() {
        if input.just_pressed(KeyCode::F1) {
            *aircraft_visibility = Visibility::Hidden;
            for main_camera in main_cameras.iter() {
                commands.entity(main_camera).remove::<ThirdPersonCamera>();
                commands.entity(main_camera).insert(CockpitCamera);
                commands.entity(main_camera).remove::<RenderLayers>();
                commands.entity(main_camera).insert(RenderLayers::from_layers(&[0, 2]));

            }
        } else if input.just_pressed(KeyCode::F2) {
            *aircraft_visibility = Visibility::Visible;
            for main_camera in main_cameras.iter() {
                commands.entity(main_camera).remove::<CockpitCamera>();
                commands.entity(main_camera).insert(ThirdPersonCamera{
                    ..default()
                });
                commands.entity(main_camera).remove::<RenderLayers>();
                commands.entity(main_camera).insert(RenderLayers::from_layers(&[0, 2, 3]));
            }
        }

    }
}

fn update_cockpit_camera(
    mut camera_transforms: Query<&mut Transform, (With<CockpitCamera>, Without<Aircraft>, Without<Player>)>,
    aircraft_transforms: Query<&Transform, (With<Aircraft>, With<Player>, Without<CockpitCamera>)>) {
    for aircraft_transform in aircraft_transforms.iter() {
        for mut camera_transform in camera_transforms.iter_mut() {
            camera_transform.translation = aircraft_transform.translation;
            camera_transform.rotation = aircraft_transform.rotation * Quat::from_rotation_y(f32::to_radians(-90.0));
        }
    }
}


const CUBEMAPS: &[(&str, CompressedImageFormats)] = &[
    (
        "skybox/night.png",
        CompressedImageFormats::NONE,
    )
];

#[derive(Resource)]
struct Cubemap {
    is_loaded: bool,
    image_handle: Handle<Image>,
}

#[derive(Component)]
struct MainCamera;

#[derive(Component)]
struct CockpitCamera;


fn apply_skybox(
    main_cameras: Query<Entity, With<MainCamera>>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
    mut cubemap: ResMut<Cubemap>,
) {
    if !cubemap.is_loaded && asset_server.get_load_state(&cubemap.image_handle) == LoadState::Loaded {
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
            commands.entity(main_camera).insert(Skybox(cubemap.image_handle.clone()));
        }
        cubemap.is_loaded = true;
    }
}



fn setup_graphics(
    mut commands: Commands, 
    asset_server: Res<AssetServer>, 
) {
    commands.insert_resource(Cubemap {
        is_loaded: false,
        image_handle: asset_server.load(CUBEMAPS[0].0),
    });

    // Main 3d camera
    commands.spawn(
        Camera3dBundle {
            camera: Camera {
                // renders first
                order: 0,
                ..default()
            },
//            transform: Transform::from_xyz(-3.0, 3.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
            ..Default::default()
        }
    )
    .insert(UiCameraConfig {
        show_ui: false,
        ..default()
    })
    .insert(MainCamera)
    .insert(CockpitCamera)
    .insert(RenderLayers::from_layers(&[0, 2]));

    // HUD camera
    commands.spawn((Camera2dBundle {
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
            scaling_mode: ScalingMode::Fixed {width: 1920., height: 1080.},
            ..default()
        }.into(),        ..Default::default()
    }, RenderLayers::layer(1))).insert(UiCameraConfig {
        show_ui: true,
        ..default()
    });


    // light
    commands.spawn(PointLightBundle {
        transform: Transform::from_xyz(50.0, 50.0, 50.0),
        point_light: PointLight {
            intensity: 600000.,
            range: 100.,
            shadows_enabled: true,
            ..default()
        },
        ..default()
    });

    commands.insert_resource(GizmoConfig {
        render_layers: RenderLayers::layer(1),
        ..default()
    })

}

fn setup_physics(mut commands: Commands,    
    asset_server: Res<AssetServer>,
) {
    let gltf_handle = asset_server.load("terrain/testmap.gltf#Scene0");

    commands.spawn((SceneBundle {
        scene: gltf_handle,
        ..default()
        },
        RigidBody::Fixed,
        AsyncSceneCollider {
            shape: Some(ComputedColliderShape::TriMesh),
            ..default()
        }
    ));
}

fn spawn_player(mut commands: Commands,    
    asset_server: Res<AssetServer>,
) {

//    let mesh: Handle<Mesh> = asset_server.load("models/planes/f117a.gltf#Scene0");
//    let m = &meshes.get(&mesh);
//    let x_shape = Collider::from_bevy_mesh(m.unwrap(), &ComputedColliderShape::TriMesh).unwrap();
    commands.spawn(SceneBundle {
        scene: asset_server.load("models/planes/f117a.gltf#Scene0"),
        transform: Transform::from_scale(Vec3::splat(0.005)),
        visibility: Visibility::Hidden,
        ..default()
    }).insert(Player)
    .insert(Aircraft{name: String::from("GHOST 1-1"), aircraft_type: AircraftType::F117A, fuel: 35500.0, ..default() })
    .insert(ExternalImpulse {
        ..default()
    })
    .insert(ExternalForce {
        ..default()
    })
    .insert(ThirdPersonCameraTarget)
    .insert(Velocity{..default()})
    .insert(Collider::cuboid(100.0, 30.0, 100.0))
    .insert(Restitution::coefficient(0.4))
    .insert(RigidBody::Dynamic)
    .insert(GravityScale(0.0)) 
    .insert(Damping { linear_damping: 0.3, angular_damping: 1.0 })
    .insert(ColliderMassProperties::Density(35.0))
    // Player airplane is layer 3 so it can be skipped when rendering cockpit view
    .insert(RenderLayers::layer(3));

//    .insert(TransformBundle::from(Transform::from_xyz(0.0, 4.0, 0.0)));

    spawn_sam(commands, asset_server, 150.0, 150.0)

}
