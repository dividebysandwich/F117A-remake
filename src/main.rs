use bevy::{
    asset::LoadState,
    core_pipeline::{clear_color::ClearColorConfig, Skybox},
    prelude::{*},
    render::{
        camera::ScalingMode,
        render_resource::{
            TextureViewDescriptor,
            TextureViewDimension,
        },
        texture::CompressedImageFormats,
        view::visibility::RenderLayers,
    },
};
use bevy_mod_billboard::prelude::*;
//use bevy_prototype_debug_lines::DebugLinesPlugin;
use bevy_rapier3d::prelude::*;
use bevy_scene_hook::HookPlugin;
use bevy_third_person_camera::*;
use bevy_common_assets::toml::TomlAssetPlugin;
use definitions::{RENDERLAYER_WORLD, RENDERLAYER_POINTLIGHTS, RENDERLAYER_COCKPIT, RENDERLAYER_AIRCRAFT};
use radar::{update_rcs, update_radar};

mod definitions;
mod explosion;
mod aircraft;
mod hud;
mod missile;
mod player;
mod sam;
mod util;
mod vehicle;
mod pointlight;
mod scenery;
mod mfd;
mod targeting;
mod health;
mod coalition;
mod radar;
mod rwr;
mod f117_ai;

use crate::aircraft::*;
use crate::hud::*;
use crate::missile::*;
use crate::player::*;
use crate::vehicle::*;
use crate::pointlight::*;
use crate::scenery::*;
use crate::mfd::*;
use crate::targeting::*;
use crate::explosion::*;
use crate::rwr::*;
use crate::f117_ai::*;

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins,
            bevy::diagnostic::FrameTimeDiagnosticsPlugin,
            bevy::diagnostic::EntityCountDiagnosticsPlugin,
            RapierPhysicsPlugin::<NoUserData>::default(),
//            RapierDebugRenderPlugin::default(),
            ThirdPersonCameraPlugin,
//            DebugLinesPlugin::default(),
            BillboardPlugin,
            HookPlugin,
            TomlAssetPlugin::<F117AI>::new(&["toml"]),
        ))
        .add_systems(
            PreStartup, 
            (
                load_f117_ai,
            )
        )
        .add_systems(
            Startup,
            (
                setup_graphics,
                initialize_textures, 
                setup_terrain, 
                setup_scenery,
                spawn_player, 
                setup_hud, 
                setup_flir,
                setup_sounds,
                setup_rwr,
                prepare_takeoff,
            ),
        )
        .add_systems(
            Update,
            (
                apply_skybox,
                handle_camera_controls,
                handle_targeting_controls,
                update_cockpit_camera,
                update_player_aircraft_controls,
                update_player_weapon_controls,
                update_missiles,
                update_aircraft_forces,
                update_rcs,
                update_radar,
                update_hud,
                update_blinking_lights,
                auto_scale_and_hide_billboards,
                update_light_billboards,
                update_mfd,
                update_rwr,
                handle_explosion_test,
                handle_collision_events,
                update_explosion_effects,
                update_f117_ai,
            ),
        )
        .run()
}

fn setup_sounds(
    asset_server: Res<AssetServer>, 
    mut commands: Commands
) {
    commands.spawn(AudioBundle {
        source: asset_server.load("sounds/radio_takeoff.ogg"),
        ..default()
    });
}

//Render layers:
// 0: World
// 1: Cockpit HUD / UI
// 2: MFD Text / UI
// 3: Player aircraft
// 4: Point lights

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
                    .insert(RenderLayers::from_layers(&[
                        RENDERLAYER_WORLD, 
                        RENDERLAYER_POINTLIGHTS]));
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
                    .insert(RenderLayers::from_layers(&[
                        RENDERLAYER_WORLD, 
                        RENDERLAYER_COCKPIT, 
                        RENDERLAYER_AIRCRAFT, 
                        RENDERLAYER_POINTLIGHTS])); //TODO: Remove Cockpit Layer (1) to remove debug line display
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

#[derive(Component)]
pub struct HudCamera;


fn apply_skybox(
    main_cameras: Query<Entity, With<MainCamera>>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
    mut cubemap: ResMut<Cubemap>,
) {
    if !cubemap.is_loaded {
        let (a_load, _a_deps, _a_rec_deps) = asset_server.get_load_states(&cubemap.image_handle).unwrap();
        if a_load == LoadState::Loaded
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
}

fn setup_graphics(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
) {
    commands.insert_resource(Cubemap {
        is_loaded: false,
        image_handle: asset_server.load(CUBEMAPS[0].0),
    });

    // Initialize the third person target storage
    commands.insert_resource(CameraSettings { target_index: 0, render_hud: true });

    // Initialize the sensor target storage
    commands.insert_resource(TargetSettings { target_index: -1 });

    // Main 3d camera
    commands
        .spawn(Camera3dBundle {
            camera: Camera {
                // renders first
                order: 0,
                ..default()
            },
            ..Default::default()
        })
        .insert(UiCameraConfig {
            show_ui: false,
            ..default()
        })
        .insert(MainCamera)
        .insert(CockpitCamera)
        .insert(RenderLayers::from_layers(&[RENDERLAYER_WORLD, RENDERLAYER_POINTLIGHTS]));

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
                    far: 1000.0, // Changing far and near planes is required to make spritebundles work
                    near: -1000.0,
                    ..default()
                }
                .into(),
                ..Default::default()
            },
            RenderLayers::layer(RENDERLAYER_COCKPIT),
        ))
        .insert(HudCamera)
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
        render_layers: RenderLayers::layer(RENDERLAYER_COCKPIT),
        ..default()
    });

}

