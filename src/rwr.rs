use bevy::{prelude::*, sprite::MaterialMesh2dBundle, render::view::RenderLayers};

use crate::{definitions::RENDERLAYER_COCKPIT, radar::RadarDetectable, player::Player};

#[derive(Component)]
pub struct RwrRcsBar;

#[derive(Component)]
pub struct RwrReturnEnergyBar;

pub fn setup_rwr(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    let bar = shape::Box::new(600.0, 20., 0.);
    commands.spawn(MaterialMesh2dBundle {
        mesh: meshes.add(bar.into()).into(),
        material: materials.add(ColorMaterial::from(Color::ORANGE_RED)),
        transform: Transform::from_translation(Vec3::new(0., -500., 0.)),
        ..default()
        }
    )
    .insert(RwrRcsBar)
    .insert(RenderLayers::layer(RENDERLAYER_COCKPIT));

}

pub fn update_rwr(
    detectables: Query<&RadarDetectable, With<Player>>,
    mut rwr_rcs_bars: Query<(&mut Transform, &RwrRcsBar)>,
) {
    for (mut transform, rwr_rcs_bar) in rwr_rcs_bars.iter_mut() {
        for detectable in detectables.iter() {
            transform.scale.x = detectable.radar_cross_section;
            transform.translation.x = 200.0 - detectable.radar_cross_section * 300.;
        }
    }

}