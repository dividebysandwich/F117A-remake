use bevy::prelude::*;
use bevy_rapier3d::prelude::*;
use std::collections::HashMap;

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

pub fn spawn_sam(mut commands: Commands,    
    asset_server: Res<AssetServer>,
    xpos: f32,
    zpos: f32,
) {

//    let mesh: Handle<Mesh> = asset_server.load("models/planes/f117a.gltf#Scene0");
//    let m = &meshes.get(&mesh);
//    let x_shape = Collider::from_bevy_mesh(m.unwrap(), &ComputedColliderShape::TriMesh).unwrap();
    commands.spawn(SceneBundle {
        scene: asset_server.load("models/vehicles/SA6.gltf#Scene0"),
//        transform: Transform::from_scale(Vec3::splat(1.0)),
        ..default()
    })
    .insert(SAM{name: String::from("SA-6 #1"), ..default() })
    .insert(Collider::cuboid(30.0, 30.0, 30.0))
    .insert(RigidBody::Dynamic)
    .insert(TransformBundle::from(Transform::from_xyz(xpos, 4.0, zpos)));


}
