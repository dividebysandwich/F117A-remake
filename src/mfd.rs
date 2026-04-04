use bevy::{
    prelude::*,
    camera::{ScalingMode, RenderTarget},
    camera::visibility::RenderLayers,
    render::render_resource::{
        Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
    },
};

use crate::{definitions::{COLOR_GREEN, RENDERLAYER_COCKPIT, RENDERLAYER_MFD, RENDERLAYER_WORLD}, player::Player, targeting::SensorTarget};

#[derive(Component)]
pub struct FlirCamera;

#[derive(Resource)]
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
    match query.single() {
        Ok(_) => {
            for mut transform in flir_cameras.iter_mut() {
                transform.translation = player_transform.single().unwrap().translation;
                match sensor_target.single() {
                    Ok(target_transform) => {
                        let los = target_transform.translation - transform.translation;
                        *transform = transform.looking_to(los.normalize(), Vec3::Y);
                    },
                    Err(_) => {
                    }
                }
            }
        },
        Err(_) => {
            match image_handles {
                Some(_resource) => {
                    info!("Spawning MFD");

                    commands.spawn((
                        Sprite::from_image(_resource.image.clone()),
                        Transform::from_translation(Vec3::new(600.0, -290.0, 0.0)),
                    ))
                    .insert(RenderLayers::layer(RENDERLAYER_COCKPIT))
                    .insert(MfdSprite); // This stops setup_mfd from being called again

                    let font = asset_server.load("fonts/Brickshapers-eXPx.ttf");
                    commands.spawn((
                        Text2d::new("L"),
                        TextFont {
                            font: font.clone(),
                            font_size: 30.0,
                            ..default()
                        },
                        TextColor(COLOR_GREEN),
                        TextLayout::new_with_justify(Justify::Right),
                        Transform::from_translation(Vec3::new(-100.0, -100.0, 0.0)),
                    )).insert(RenderLayers::layer(RENDERLAYER_MFD));

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
    commands.spawn((
        Mesh2d(meshes.add(Rectangle::new(50., 4.))),
        MeshMaterial2d(materials.add(ColorMaterial::from(COLOR_GREEN))),
        Transform::from_translation(Vec3::new(50., 0., 0.)),
    )).insert(RenderLayers::layer(RENDERLAYER_MFD));
    commands.spawn((
        Mesh2d(meshes.add(Rectangle::new(50., 4.))),
        MeshMaterial2d(materials.add(ColorMaterial::from(COLOR_GREEN))),
        Transform::from_translation(Vec3::new(-50., 0., 0.)),
    )).insert(RenderLayers::layer(RENDERLAYER_MFD));
    commands.spawn((
        Mesh2d(meshes.add(Rectangle::new(4., 50.))),
        MeshMaterial2d(materials.add(ColorMaterial::from(COLOR_GREEN))),
        Transform::from_translation(Vec3::new(0., 50., 0.)),
    )).insert(RenderLayers::layer(RENDERLAYER_MFD));
    commands.spawn((
        Mesh2d(meshes.add(Rectangle::new(4., 50.))),
        MeshMaterial2d(materials.add(ColorMaterial::from(COLOR_GREEN))),
        Transform::from_translation(Vec3::new(0., -50., 0.)),
    )).insert(RenderLayers::layer(RENDERLAYER_MFD));
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
        .spawn((
            Camera3d::default(),
            Camera {
                clear_color: ClearColorConfig::Custom(Color::srgb(0.0, 0.0, 0.0)),
                // render before the "main pass" camera and the mfd 2d camera
                order: -2,
                ..default()
            },
            RenderTarget::from(image_handle.clone()),
            bevy::prelude::Projection::Perspective(PerspectiveProjection {
                fov: start_fov.to_radians(),
                ..default()
            }),
            Transform::from_translation(Vec3::new(0.0, 0.0, 15.0))
                .looking_at(Vec3::ZERO, Vec3::Y),
        ))
        .insert(FlirCamera)
        .insert(RenderLayers::layer(RENDERLAYER_WORLD));

        // HUD camera
        commands
        .spawn((
            Camera2d,
            Camera {
                // Don't clear the canvas before drawing
                clear_color: ClearColorConfig::None,
                // renders after the mfd 3d camera and before the main cameras
                order: -1,
                ..default()
            },
            RenderTarget::from(image_handle.clone()),
            Projection::Orthographic({
                let mut ortho = OrthographicProjection::default_2d();
                ortho.scale = 1.0;
                ortho.scaling_mode = ScalingMode::Fixed {
                    width: 512.,
                    height: 512.,
                };
                ortho.far = 1000.0;
                ortho.near = -1000.0;
                ortho
            }),
        ))
        .insert(RenderLayers::layer(RENDERLAYER_MFD));

}
