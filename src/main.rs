use bevy::prelude::*;
use bevy::asset::Assets;
use bevy::render::camera::ScalingMode;
use bevy_editor_pls::*;
use bevy_rapier3d::prelude::*;
use bevy_atmosphere::prelude::*;
use bevy_third_person_camera::*;
use bevy_prototype_debug_lines::DebugLinesPlugin;
use bevy::core_pipeline::clear_color::ClearColorConfig;
use bevy::render::view::visibility::RenderLayers;

mod aircraft;
mod player;
mod hud;

use crate::aircraft::*;
use crate::player::*;
use crate::hud::*;

fn main() {
    App::new()
    .add_plugins(DefaultPlugins)
    .add_plugins(EditorPlugin::default())
    .add_plugins(bevy::diagnostic::FrameTimeDiagnosticsPlugin)
    .add_plugins(bevy::diagnostic::EntityCountDiagnosticsPlugin)
    .add_plugins(RapierPhysicsPlugin::<NoUserData>::default())
//    .add_plugins(RapierDebugRenderPlugin::default())
    .add_plugins(AtmospherePlugin)
    .add_plugins(ThirdPersonCameraPlugin)
    .add_plugins(DebugLinesPlugin::default())
    .add_systems(Startup, setup_graphics)
    .add_systems(Startup, setup_physics)
    .add_systems(Startup, spawn_player)
    .add_systems(Startup, setup_hud)
    .add_systems(Update, update_player_aircraft_controls)
    .add_systems(Update, update_aircraft_forces)
    .add_systems(Update, update_hud)
    .add_systems(Update, keyboard_input)
    .run()
}

fn keyboard_input(mut external_impulses: Query<&mut ExternalImpulse, With<Player>>, mut transform: Query<&mut Transform, With<Player>>, input: Res<Input<KeyCode>>) {
    if input.just_pressed(KeyCode::Space) {
        println!("Boing!");

        for mut external_impulse in external_impulses.iter_mut() {
            println!("Applied!");
            let object_rotation = transform.get_single().unwrap().rotation;
            let impulse_vector = Vec3::new(0.0, 80.0, 0.0);
            let rotated_impulse_vector = Quat::mul_vec3(object_rotation, impulse_vector);
            external_impulse.impulse = rotated_impulse_vector;
            external_impulse.torque_impulse = Vec3::new(0.0, 5.0, 10.0);
        }
    }
}

fn setup_graphics(mut commands: Commands) {
    // Main 3d camera
    commands.spawn((
        ThirdPersonCamera {
            ..default()
        },
        Camera3dBundle {
            transform: Transform::from_xyz(-3.0, 3.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
            ..Default::default()
    }, AtmosphereCamera::default()))
    .insert(UiCameraConfig {
        show_ui: false,
        ..default()
    });

    // HUD camera
    commands.spawn((Camera2dBundle {
        camera_2d: Camera2d {
            // Don't clear the canvas before drawing
            clear_color: ClearColorConfig::None,
        },
        camera: Camera {
            // renders after / on top of the 3d camera
            order: 1,
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
    commands.insert_resource(AtmosphereModel::new(Gradient {
        ground: Color::rgb(0.0, 0.0, 0.0),
        horizon: Color::rgb(0.333, 0.11, 0.294),
        sky: Color::rgb(0.004, 0.027, 0.12),
    }));

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
    mut meshes: ResMut<Assets<Mesh>>,
) {

//    let mesh: Handle<Mesh> = asset_server.load("models/planes/f117a.gltf#Scene0");
//    let m = &meshes.get(&mesh);
//    let x_shape = Collider::from_bevy_mesh(m.unwrap(), &ComputedColliderShape::TriMesh).unwrap();
    commands.spawn(SceneBundle {
        scene: asset_server.load("models/planes/f117a.gltf#Scene0"),
        transform: Transform::from_scale(Vec3::splat(0.01)),
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
    .insert(Collider::cuboid(200.0, 30.0, 200.0))
    .insert(Restitution::coefficient(0.4))
    .insert(RigidBody::Dynamic)
    .insert(GravityScale(0.0))
    .insert(Damping { linear_damping: 0.3, angular_damping: 1.0 });
//    .insert(TransformBundle::from(Transform::from_xyz(0.0, 4.0, 0.0)));


}

