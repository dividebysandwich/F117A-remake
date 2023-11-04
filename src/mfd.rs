use bevy::{
    prelude::*,
    render::{
        camera::RenderTarget,
        render_resource::{
            Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
        },
        view::RenderLayers,
    }, reflect::TypeUuid, core_pipeline::clear_color::ClearColorConfig,
};

use crate::{player::Player, missile::SensorTarget};

#[derive(Component)]
pub struct FlirCamera;

#[derive(Resource, TypeUuid, Reflect)]
#[uuid="58b43f34-80b3-4886-b9a0-93a48bf3ae7f"]
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
                        info!("Target found");
                        let los = target_transform.translation - transform.translation;
                        *transform = transform.looking_to(los.normalize(), Vec3::Y);
                    },
                    Err(_) => {
                        info!("No target");
                        transform.rotation = player_transform.single().rotation;
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
                    .insert(RenderLayers::layer(1))
                    .insert(MfdSprite); // This stops setup_mfd from being called again

                    let font = asset_server.load("fonts/Brickshapers-eXPx.ttf");
                    let text_style = TextStyle {
                        font: font.clone(),
                        font_size: 30.0,
                        color: Color::YELLOW,
                    };
                    commands.spawn(
                        Text2dBundle {
                            text: Text::from_section("MFD TEST", text_style.clone()).with_alignment(TextAlignment::Right),
                            transform: Transform::from_translation(Vec3::new(200.0, 100.0, 0.0)),
                            ..default()
                        }
                    ).insert(RenderLayers::layer(1));


                }
                _ => {
                    info!("FLIR image not loaded yet");
                }
            }        
        }
    }
    
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

    commands
        .spawn(Camera3dBundle {
            camera_3d: Camera3d {
                clear_color: ClearColorConfig::Custom(Color::rgb(0.0, 0.0, 0.0)),
                ..default()
            },
            camera: Camera {
                // render before the "main pass" camera
                order: -1,
                target: RenderTarget::Image(image_handle.clone()),
                ..default()
            },
//            projection: PerspectiveProjection {
//                fov: 10.0,
//                ..default()
//            },
            transform: Transform::from_translation(Vec3::new(0.0, 0.0, 15.0))
            .looking_at(Vec3::ZERO, Vec3::Y),
            ..Default::default()
        })
        .insert(UiCameraConfig {
            show_ui: false,
            ..default()
        })
        .insert(FlirCamera)
        .insert(RenderLayers::from_layers(&[0, 2]));

}