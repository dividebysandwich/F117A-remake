use bevy::{
    prelude::*,
    render::{
        camera::RenderTarget,
        render_resource::{
            Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
        },
        view::RenderLayers,
    }, reflect::TypeUuid,
};

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
pub fn setup_mfd(
    mut commands: Commands,
    image_handles: Option<Res<FlirImage>>,
    query: Query<Entity, With<MfdSprite>>,
) {
    match query.get_single() {
        Ok(_) => {},
        Err(_) => {
            match image_handles {
                Some(_resource) => {
                    info!("Spawning MFD");
                    commands.spawn(SpriteBundle {
                        texture: _resource.image.clone(),
                        ..Default::default()
                    })
                    // This ensures the 2D sprite is rendered in the UI camera
                    .insert(UiCameraConfig { show_ui: true })
                    .insert(MfdSprite);
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
            camera: Camera {
                // render before the "main pass" camera
                order: -1,
                target: RenderTarget::Image(image_handle.clone()),
                ..default()
            },
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