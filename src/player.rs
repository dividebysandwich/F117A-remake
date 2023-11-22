use bevy::prelude::*;
use bevy_rapier3d::prelude::*;
use bevy::render::view::visibility::RenderLayers;
use bevy_scene_hook::HookedSceneBundle;
use bevy_scene_hook::SceneHook;

use crate::coalition::Coalition;
use crate::coalition::CoalitionType;
use crate::definitions::*;
use crate::aircraft::*;
use crate::pointlight::LightBillboardToBeAdded;
use crate::pointlight::get_light_color_from_name;
use crate::pointlight::get_light_type_from_name;
use crate::pointlight::get_lightsource_type_from_name;
use crate::radar::RadarDetectable;
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
    let gltf_handle = asset_server.load("models/planes/f117a.glb#Scene0");

    commands.spawn((
        HookedSceneBundle {
            scene: SceneBundle {
                scene: gltf_handle.clone(),
                visibility: Visibility::Hidden,
                ..default()
            },
            hook: SceneHook::new(|entity, cmds| {
                for n in entity.get::<Name>().iter() {
                    let name = n.as_str();
                    if name.starts_with("PointLight") {
                        cmds.insert(LightBillboardToBeAdded {
                            light_color: get_light_color_from_name(name),
                            light_type: get_light_type_from_name(name),
                            lightsource_type: get_lightsource_type_from_name(name)
                        });
                    }
                }
            }),
        },
    ))
    .insert(Player)
    .insert(Coalition{side: CoalitionType::BLUE})
    .insert(RadarDetectable {
        base_radar_cross_section: 0.2,
        radar_visibility: 0.1,
    })
    .insert(Vehicle{..default()})
    .insert(Aircraft{name: String::from("GHOST 1-1"), aircraft_type: AircraftType::F117A, fuel: 35500.0, ..default() })
    .insert(ExternalImpulse {
        ..default()
    })
    .insert(ExternalForce {
        ..default()
    })
    .insert(Velocity{..default()})
    .insert(Collider::cuboid(0.5, 0.15, 0.5))
    .insert(CollisionGroups::new(Group::from_bits_truncate(COLLISION_MASK_PLAYER), 
        Group::from_bits_truncate(
            COLLISION_MASK_TERRAIN |
            COLLISION_MASK_AIRCRAFT |
            COLLISION_MASK_GROUNDVEHICLE |
            COLLISION_MASK_MISSILE
        )))
    .insert(Restitution::coefficient(0.4))
    .insert(RigidBody::Dynamic)
    .insert(GravityScale(0.0)) 
    .insert(Damping { linear_damping: 0.25, angular_damping: 3.0 })
    .insert(ColliderMassProperties::Density(35.0))
    // Player airplane is layer 3 so it can be skipped when rendering cockpit view
    .insert(RenderLayers::layer(3));

//    .insert(TransformBundle::from(Transform::from_xyz(0.0, 4.0, 0.0)));

    spawn_sam(&mut commands, &asset_server, 100.0, 10.0, CoalitionType::RED);
    spawn_sam(&mut commands, &asset_server, 10.0, 10.0, CoalitionType::RED);

}
