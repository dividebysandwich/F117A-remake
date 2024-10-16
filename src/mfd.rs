use bevy::{
    prelude::*,
    render::{
        camera::{RenderTarget, ScalingMode},
        render_resource::{
            Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
        },
        view::RenderLayers,
    }, sprite::MaterialMesh2dBundle,
};

use crate::{definitions::{COLOR_GREEN, RENDERLAYER_COCKPIT, RENDERLAYER_MFD, RENDERLAYER_WORLD}, player::Player, targeting::SensorTarget};

#[derive(Component)]
pub struct FlirCamera;

#[derive(Resource, TypePath)]
#[type_path = "f117::mfd::FlirImage"]
pub struct FlirImage {
    pub image: Handle<Image>,
}

#[derive(Component)]
pub struct MfdSprite;

//Set up the MFD displaying the correct texture
pub fn update_mfd(
    mut commands: Commands,
    image_handles: Option<Res<FlirImage>>,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    query: Query<Entity, With<MfdSprite>>,
    player_transform: Query<&Transform, (With<Player>, Without<FlirCamera>, Without<SensorTarget>)>,
    mut flir_cameras: Query<&mut Transform, (With<FlirCamera>, Without<Player>, Without<SensorTarget>)>,
    sensor_target: Query<&Transform, (With<SensorTarget>, Without<Player>, Without<FlirCamera>)>,
) {
    match query.get_single() {
        Ok(_) => {
            for mut transform in flir_cameras.iter_mut() {
                transform.translation = player_transform.single().translation;
                match sensor_target.get_single() {
                    Ok(target_transform) => {
//                        info!("Target found");
                        let los = target_transform.translation - transform.translation;
                        *transform = transform.looking_to(los.normalize(), Vec3::Y);
                    },
                    Err(_) => {
//                        info!("No target");
//                        transform.rotation = player_transform.single().rotation;
                    }
                }
            }
        },
        Err(_) => {
            match image_handles {
                Some(_resource) => {
                    info!("Spawning MFD");

                    commands.spawn(SpriteBundle {
                        transform: Transform::from_translation(Vec3::new(600.0, -290.0, 0.0)),
                        texture: _resource.image.clone(),
                        ..Default::default()
                    })
                    .insert(RenderLayers::layer(RENDERLAYER_COCKPIT))
                    .insert(MfdSprite); // This stops setup_mfd from being called again

                    let font = asset_server.load("fonts/Brickshapers-eXPx.ttf");
                    let text_style = TextStyle {
                        font: font.clone(),
                        font_size: 30.0,
                        color: COLOR_GREEN,
                    };
                    commands.spawn(
                        Text2dBundle {
                            text: Text::from_section("L", text_style.clone()).with_justify(JustifyText::Right),
                            transform: Transform::from_translation(Vec3::new(-100.0, -100.0, 0.0)),
                            ..default()
                        }
                    ).insert(RenderLayers::layer(RENDERLAYER_MFD));

                    draw_crosshair(&mut commands, &mut meshes, &mut materials);

                }
                _ => {
                    info!("FLIR image not loaded yet");
                }
            }        
        }
    }
    
}

fn draw_crosshair(commands: &mut Commands<'_, '_>, meshes: &mut ResMut<'_, Assets<Mesh>>, materials: &mut ResMut<'_, Assets<ColorMaterial>>) {
    commands.spawn(MaterialMesh2dBundle {
        mesh: meshes.add(Rectangle::new(50., 4.)).into(),
        material: materials.add(ColorMaterial::from(COLOR_GREEN)),
        transform: Transform::from_translation(Vec3::new(50., 0., 0.)),
        ..default()
    }).insert(RenderLayers::layer(RENDERLAYER_MFD));
    commands.spawn(MaterialMesh2dBundle {
        mesh: meshes.add(Rectangle::new(50., 4.)).into(),
        material: materials.add(ColorMaterial::from(COLOR_GREEN)),
        transform: Transform::from_translation(Vec3::new(-50., 0., 0.)),
        ..default()
    }).insert(RenderLayers::layer(RENDERLAYER_MFD));
    commands.spawn(MaterialMesh2dBundle {
        mesh: meshes.add(Rectangle::new(4., 50.)).into(),
        material: materials.add(ColorMaterial::from(COLOR_GREEN)),
        transform: Transform::from_translation(Vec3::new(0., 50., 0.)),
        ..default()
    }).insert(RenderLayers::layer(RENDERLAYER_MFD));
    commands.spawn(MaterialMesh2dBundle {
        mesh: meshes.add(Rectangle::new(4., 50.)).into(),
        material: materials.add(ColorMaterial::from(COLOR_GREEN)),
        transform: Transform::from_translation(Vec3::new(0., -50., 0.)),
        ..default()
    }).insert(RenderLayers::layer(RENDERLAYER_MFD));
}

//Set up the camera and render target for the FLIR
pub fn setup_flir(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
) {
    info!("setup_flir");
    let size = Extent3d {
        width: 512,
        height: 512,
        ..default()
    };

    // This is the texture that will be rendered to.
    let mut image = Image {
        texture_descriptor: TextureDescriptor {
            label: None,
            size,
            dimension: TextureDimension::D2,
            format: TextureFormat::Bgra8UnormSrgb,
            mip_level_count: 1,
            sample_count: 1,
            usage: TextureUsages::TEXTURE_BINDING
                | TextureUsages::COPY_DST
                | TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        },
        ..default()
    };

    // fill image.data with zeroes
    image.resize(size);

    let image_handle = images.add(image);
    info!("Setting FLIR image");
    commands.insert_resource(FlirImage {
        image: image_handle.clone(),
    });

    let start_fov: f32 = 2.0;
    commands
        .spawn(Camera3dBundle {
            camera: Camera {
                clear_color: ClearColorConfig::Custom(Color::srgb(0.0, 0.0, 0.0)),
                // render before the "main pass" camera and the mfd 2d camera
                order: -2,
                target: RenderTarget::Image(image_handle.clone()),
                ..default()
            },
            projection: bevy::prelude::Projection::Perspective(PerspectiveProjection {
                fov: start_fov.to_radians(),
                ..default()
            }),
            transform: Transform::from_translation(Vec3::new(0.0, 0.0, 15.0))
            .looking_at(Vec3::ZERO, Vec3::Y),
            ..Default::default()
        })
//TODO: Hide Gizmos on this camera, for example using GizmoConfig
//        .insert(UiCameraConfig {
//            show_ui: false,
//            ..default()
//        })
        .insert(FlirCamera)
        .insert(RenderLayers::layer(RENDERLAYER_WORLD));

        // HUD camera
        commands
        .spawn(
            Camera2dBundle {
                camera: Camera {
                    // Don't clear the canvas before drawing
                    clear_color: ClearColorConfig::None,
                    // renders after the mfd 3d camera and before the main cameras
                    order: -1,
                    target: RenderTarget::Image(image_handle.clone()),
                    ..default()
                },
                projection: OrthographicProjection {
                    // Make sure the HUD scales with the window size
                    scale: 1.0,
                    scaling_mode: ScalingMode::Fixed {
                        width: 512.,
                        height: 512.,
                    },
                    far: 1000.0, // Changing far and near planes is required to make spritebundles work
                    near: -1000.0,
                    ..default()
                }
                .into(),
                ..Default::default()
            }
        )
//TODO: Hide Gizmos on this camera, for example using GizmoConfig
//        .insert(UiCameraConfig {
//            show_ui: false,
//            ..default()
//        })
        .insert(RenderLayers::layer(RENDERLAYER_MFD));


}