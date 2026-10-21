#![doc = concat!(include_str!("../docs/crate.md"), "\n\n", include_str!("../docs/ridge_march.svg"))]

pub mod lod;
pub mod render;
pub mod volume;
pub mod wavelet;

use bevy::{prelude::*, render::render_resource::TextureUsages};

pub use lod::{CloudRenderSettings, LodPolicy, lod_bands};
pub use render::{CloudCamera, CloudShading, CloudVolume};
pub use volume::{CloudSphere, WaveletVolume, anvil_spheres, cumulus_spheres};
pub use wavelet::steering_set;

pub struct FastCloudsPlugin;

/// Picks the volumes' LOD and gathers them and [`CloudShading`] for drawing (`PostUpdate`,
/// after transform propagation).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct CloudSystems;

impl Plugin for FastCloudsPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<WaveletVolume>()
            .init_resource::<LodPolicy>()
            .init_resource::<CloudRenderSettings>()
            .add_systems(PostUpdate, enable_depth_sampling);
        render::build_render(app);
    }
}

/// The march reads the scene's depth.
fn enable_depth_sampling(mut cameras: Query<&mut Camera3d, With<CloudCamera>>) {
    for mut camera in &mut cameras {
        let usages = TextureUsages::from(camera.depth_texture_usages);
        if !usages.contains(TextureUsages::TEXTURE_BINDING) {
            camera.depth_texture_usages =
                (usages | TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT).into();
        }
    }
}
