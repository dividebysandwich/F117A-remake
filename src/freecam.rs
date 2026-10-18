//! `--freecam`: debug camera to inspect the world. WASD/QE move, right mouse drag looks around,
//! Shift = fast. Start options: `--at <WLD object name>`, `--pos x,z`, `--alt <metres>`, `--yaw/--look-pitch <deg>`.

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::math::DVec3;
use bevy::prelude::*;

use crate::settings::{AppState, MissionSetup, SetupSet};
use crate::world::{Theater, WorldAnchor, WorldPos};

pub struct FreeCamPlugin;

impl Plugin for FreeCamPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            OnEnter(AppState::Flight),
            spawn.in_set(SetupSet::Spawn).run_if(|s: Res<MissionSetup>| s.freecam),
        )
        .add_systems(Update, fly.run_if(in_state(AppState::Flight)));
    }
}

#[derive(Component)]
struct FreeCam {
    yaw: f32,
    pitch: f32,
}

fn arg(name: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn spawn(
    mut commands: Commands,
    theater: Res<Theater>,
    mut meshes: ResMut<Assets<Mesh>>,
    palette: Res<crate::palette::Palette>,
    mission: Option<Res<crate::mission::ActiveMission>>,
) {
    let at = arg("--at").unwrap_or_default();
    // "--at primary" / "--at secondary": the mission's target positions.
    let target = mission.and_then(|m| {
        let t = match at.as_str() {
            "primary" => &m.0.primary,
            "secondary" => &m.0.secondary,
            _ => return None,
        };
        let (e, n) = crate::world::map_to_wu(t.position.x as u16, t.position.y as u16);
        Some(crate::world::wu_to_bevy(e, n, 0.0))
    });
    // "--pos x,z": Bevy ground position in metres.
    let pos = arg("--pos").and_then(|s| {
        let (x, z) = s.split_once(',')?;
        Some(DVec3::new(x.parse().ok()?, 0.0, z.parse().ok()?))
    });
    let ground = target
        .or(pos)
        .or_else(|| theater.object_position(&at))
        .or_else(|| theater.world.objects.iter().find(|o| o.target_flags & 0x100 != 0).map(|o| {
            let (e, n) = crate::world::map_to_wu(o.x, o.y);
            crate::world::wu_to_bevy(e, n, 0.0)
        }))
        .unwrap_or(DVec3::new(256_000.0, 0.0, -256_000.0));
    let alt: f64 = arg("--alt").and_then(|s| s.parse().ok()).unwrap_or(300.0);
    let yaw = arg("--yaw").and_then(|s| s.parse::<f32>().ok()).unwrap_or(0.0).to_radians();
    let pitch = arg("--look-pitch").and_then(|s| s.parse::<f32>().ok()).unwrap_or(-15.0).to_radians();
    let cam = commands.spawn((
        Camera3d::default(),
        bevy::core_pipeline::tonemapping::Tonemapping::None,
        Projection::Perspective(PerspectiveProjection { near: 0.5, far: 600_000.0, ..default() }),
        FreeCam { yaw, pitch },
        WorldAnchor,
        WorldPos(ground + DVec3::Y * alt),
        Transform::from_rotation(Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0)),
    )).id();
    crate::camera::spawn_background(&mut commands, cam, &mut meshes, &palette, crate::camera::MAIN_BACKGROUND_LAYER);
    commands.entity(cam).insert(bevy::camera::visibility::RenderLayers::from_layers(&[0, crate::camera::MAIN_BACKGROUND_LAYER, crate::camera::MAIN_LOD_LAYER]));
}

fn fly(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    time: Res<Time>,
    mut q: Query<(&mut FreeCam, &mut WorldPos, &mut Transform)>,
    mut viewpoint: ResMut<crate::palette::Viewpoint>,
) {
    let Ok((mut cam, mut pos, mut tf)) = q.single_mut() else { return };
    if buttons.pressed(MouseButton::Right) {
        cam.yaw -= motion.delta.x * 0.003;
        cam.pitch = (cam.pitch - motion.delta.y * 0.003).clamp(-1.55, 1.55);
    }
    tf.rotation = Quat::from_euler(EulerRot::YXZ, cam.yaw, cam.pitch, 0.0);
    let mut dir = Vec3::ZERO;
    for (k, d) in [
        (KeyCode::KeyW, tf.forward().as_vec3()),
        (KeyCode::KeyS, -tf.forward().as_vec3()),
        (KeyCode::KeyD, tf.right().as_vec3()),
        (KeyCode::KeyA, -tf.right().as_vec3()),
        (KeyCode::KeyE, Vec3::Y),
        (KeyCode::KeyQ, -Vec3::Y),
    ] {
        if keys.pressed(k) {
            dir += d;
        }
    }
    viewpoint.altitude_wu = (pos.0.y / crate::world::WU) as f32;
    let speed = if keys.pressed(KeyCode::ShiftLeft) { 3000.0 } else { 300.0 };
    if dir != Vec3::ZERO {
        pos.0 += (dir.normalize() * speed * time.delta_secs()).as_dvec3();
    }
}
