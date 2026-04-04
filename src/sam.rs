use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::coalition::{CoalitionType, Coalition};
use crate::definitions::*;
use crate::radar::*;
use crate::targeting::Targetable;
use crate::vehicle::*;

#[derive(Debug, Eq, PartialEq, Hash)]
pub enum SAMType {
    SA6
}

#[derive(Component)]
pub struct SAM {
    pub name: String,
    pub sam_type: SAMType,
    pub health: f32,
}

impl Default for SAM {
    fn default() -> Self {
        SAM {
            name: String::from("Default SAM"),
            sam_type: SAMType::SA6,
            health: 100.0,
        }
    }
}

pub fn spawn_sam(commands: &mut Commands,
    asset_server: &Res<AssetServer>,
    xpos: f32,
    zpos: f32,
    side: CoalitionType,
) {
    commands.spawn(SceneRoot(asset_server.load("models/vehicles/SA6.gltf#Scene0")))
    .insert(Vehicle{..default()})
    .insert(Coalition{side: side})
    .insert(SAM{name: String::from("SA-6 #1"), ..default() })
    .insert(Collider::cuboid(0.25, 0.35, 0.4))
    .insert(CollisionGroups::new(Group::from_bits_truncate(COLLISION_MASK_GROUNDVEHICLE),
        Group::from_bits_truncate(
            COLLISION_MASK_TERRAIN |
            COLLISION_MASK_AIRCRAFT |
            COLLISION_MASK_GROUNDVEHICLE |
            COLLISION_MASK_MISSILE |
            COLLISION_MASK_PLAYER
        )))
    .insert(RigidBody::Dynamic)
    .insert(ColliderMassProperties::Density(100.0))
    .insert(Transform::from_xyz(xpos, 0.0, zpos))
    .insert(Targetable)
    .insert(RadarEmitter{
        radar_type: RadarEmitterType::PULSE,
        radar_gain: 10.0,
        scan_interval: 3.0,
        ..default()
    });

}
