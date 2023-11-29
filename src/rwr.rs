use bevy::{prelude::*, sprite::MaterialMesh2dBundle, render::view::RenderLayers};

use crate::{definitions::{RENDERLAYER_COCKPIT, RADAR_PULSE_TIMEOUT}, radar::RadarDetectable, player::Player, util::get_time_millis};

#[derive(Component)]
pub struct RwrRcsBar;

#[derive(Component)]
pub struct RwrReturnEnergyBar;

pub fn setup_rwr(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    let rcr_bar = shape::Box::new(600.0, 20., 0.);
    commands.spawn(MaterialMesh2dBundle {
        mesh: meshes.add(rcr_bar.into()).into(),
        material: materials.add(ColorMaterial::from(Color::ORANGE_RED)),
        transform: Transform::from_translation(Vec3::new(0., -500., 0.)),
        ..default()
        }
    )
    .insert(RwrRcsBar)
    .insert(RenderLayers::layer(RENDERLAYER_COCKPIT));

    let return_energy_bar = shape::Box::new(600.0, 20., 0.);
    commands.spawn(MaterialMesh2dBundle {
        mesh: meshes.add(return_energy_bar.into()).into(),
        material: materials.add(ColorMaterial::from(Color::YELLOW)),
        transform: Transform::from_translation(Vec3::new(0., -500., 0.)),
        ..default()
        }
    )
    .insert(RwrReturnEnergyBar)
    .insert(RenderLayers::layer(RENDERLAYER_COCKPIT));

}

pub fn update_rwr(
    mut detectables: Query<&mut RadarDetectable, With<Player>>,
    mut rwr_rcs_bars: Query<&mut Transform, (With<RwrRcsBar>, Without<RwrReturnEnergyBar>)>,
    mut rwr_return_energy_bars: Query<&mut Transform, (With<RwrReturnEnergyBar>, Without<RwrRcsBar>)>,
) {
    for mut transform in rwr_rcs_bars.iter_mut() {
        for detectable in detectables.iter() {
            transform.scale.x = detectable.radar_cross_section;
            transform.translation.x = 200. - detectable.radar_cross_section * 300.;
        }
    }

    for mut transform in rwr_return_energy_bars.iter_mut() {
        for mut detectable in detectables.iter_mut() {
            let milliseconds = get_time_millis();
            if (milliseconds - detectable.last_impulse_time) > RADAR_PULSE_TIMEOUT {
                detectable.reflected_energy = 0.;
            }
            let energy = detectable.reflected_energy.clamp(0.0, 1.0);
            transform.scale.x = energy;
            transform.translation.x = energy * 300. - 200.;
        }
    }


}