use bevy::prelude::*;
use bevy_rapier3d::prelude::*;
use bevy::render::view::visibility::RenderLayers;

use crate::aircraft::*;
use crate::vehicle::*;
use crate::sam::*;

#[derive(Component)]
pub struct Player;


pub fn spawn_player(mut commands: Commands,    
    asset_server: Res<AssetServer>,
) {

//    let mesh: Handle<Mesh> = asset_server.load("models/planes/f117a.gltf#Scene0");
//    let m = &meshes.get(&mesh);
//    let x_shape = Collider::from_bevy_mesh(m.unwrap(), &ComputedColliderShape::TriMesh).unwrap();
    commands.spawn(SceneBundle {
        scene: asset_server.load("models/planes/f117a.gltf#Scene0"),
        transform: Transform::from_scale(Vec3::splat(0.005)),
        visibility: Visibility::Hidden,
        ..default()
    })
    .insert(Player)
    .insert(Vehicle{..default()})
    .insert(Aircraft{name: String::from("GHOST 1-1"), aircraft_type: AircraftType::F117A, fuel: 35500.0, ..default() })
    .insert(ExternalImpulse {
        ..default()
    })
    .insert(ExternalForce {
        ..default()
    })
    .insert(Velocity{..default()})
    .insert(Collider::cuboid(100.0, 30.0, 100.0))
    .insert(Restitution::coefficient(0.4))
    .insert(RigidBody::Dynamic)
    .insert(GravityScale(0.0)) 
    .insert(Damping { linear_damping: 0.3, angular_damping: 1.0 })
    .insert(ColliderMassProperties::Density(35.0))
    // Player airplane is layer 3 so it can be skipped when rendering cockpit view
    .insert(RenderLayers::layer(3));

//    .insert(TransformBundle::from(Transform::from_xyz(0.0, 4.0, 0.0)));

    spawn_sam(commands, asset_server, 100.0, 10.0)

}
