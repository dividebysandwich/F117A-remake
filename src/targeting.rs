use bevy::prelude::*;
use crate::vehicle::*;

#[derive(Component)]
pub struct Targetable;

#[derive(Component)]
pub struct SensorTarget;

#[derive(Resource)]
pub struct TargetSettings {
    pub target_index: i32, // Keeps track of which object is currently being targeted by the player
    //TODO: X/Z target position / SPI (Sensor Point of Interest) for "realistic" targeting
}


pub fn handle_targeting_controls(
    mut commands: Commands,
    mut vehicles: Query<(Entity, &Vehicle), With<Targetable>>,
    mut target_settings: ResMut<TargetSettings>,
    input: Res<Input<KeyCode>>,
) {
    if input.just_pressed(KeyCode::Back) { // Clear current target
        target_settings.target_index = -1;
        let vehicles_unsorted = vehicles.iter_mut().collect::<Vec<_>>();
        for (entity, _vehicle) in vehicles_unsorted.iter() {
            commands.entity(*entity).remove::<SensorTarget>();
        }
    } else if input.just_pressed(KeyCode::N) { // Toggle to next target
        let mut i: i32 = 0;
        cycle_nearby_target(&mut target_settings, &mut vehicles, &mut i, &mut commands);
        target_settings.target_index += 1;
        if target_settings.target_index >= i {
            target_settings.target_index = 0;
        }
    } else if input.just_pressed(KeyCode::M) { // Toggle to previous target
        let mut i: i32 = 0;
        cycle_nearby_target(&mut target_settings, &mut vehicles, &mut i, &mut commands);
        target_settings.target_index -= 1;
        if target_settings.target_index < 0 {
            target_settings.target_index = i-1;
        }
    } else if input.just_pressed(KeyCode::T) { // TODO: Lock target near crosshair
    }
}

fn cycle_nearby_target(target_settings: &mut ResMut<'_, TargetSettings>, vehicles: &mut Query<'_, '_, (Entity, &Vehicle), With<Targetable>>, i: &mut i32, commands: &mut Commands<'_, '_>) {
    if target_settings.target_index == -1 {
        target_settings.target_index = 0;
    }
    let mut vehicles_sorted = vehicles.iter_mut().collect::<Vec<_>>();
    vehicles_sorted
        .sort_by(|(_, a), (_, b)| (a.serialnumber).partial_cmp(&b.serialnumber).unwrap());
    for (entity, _vehicle) in vehicles_sorted.iter() {
        // TODO: Disregard targets that are too far away for arcade targeting
        if target_settings.target_index == *i {
            commands.entity(*entity).insert(SensorTarget);
        } else {
            commands.entity(*entity).remove::<SensorTarget>();
        }
        *i += 1;
    }
}
