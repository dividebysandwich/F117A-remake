//! Staged shots for screenshots (manual, release notes):
//!
//! - `--start-chase k,metres,knots`: start in the air, `metres` behind drone `k` of the
//!   mission's wave, at its height and on its heading.
//! - `--stage-cam mode[,distance,height]`: a camera placed relative to the chased drone `k`
//!   and the player: `front` (ahead of the drone looking back), `behind` (behind the player
//!   looking past him at the drone), `side`, `high`, `missile` (behind the player's missile,
//!   else `behind`), `drone` (past the drone, looking back over it at the player).

use bevy::math::DVec3;
use bevy::prelude::*;

use stealth_mission::Mission;

use crate::camera::{EyeCamera, HeadAnchor, RigSystems, View, ViewState};
use crate::combat::{Combat, unit_position};
use crate::flight::{Aircraft, Player};
use crate::settings::AppState;
use crate::world::{WU, WorldPos, wu_to_bevy};

pub struct StagePlugin;

impl Plugin for StagePlugin {
    fn build(&self, app: &mut App) {
        if cam_arg().is_some() {
            app.add_systems(
                Update,
                stage_camera.after(RigSystems).run_if(in_state(AppState::Flight)),
            );
        }
    }
}

fn arg(f: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter().position(|a| a == f).and_then(|i| args.get(i + 1)).cloned()
}

/// `--start-chase`: (drone k, metres behind, knots).
fn chase_arg() -> Option<(usize, f64, f32)> {
    let v = arg("--start-chase")?;
    let mut it = v.split(',');
    let k = it.next()?.parse().ok()?;
    let m = it.next().and_then(|s| s.parse().ok()).unwrap_or(300.0);
    let kts = it.next().and_then(|s| s.parse().ok()).unwrap_or(250.0);
    Some((k, m, kts))
}

fn cam_arg() -> Option<(String, f32, f32)> {
    let v = arg("--stage-cam")?;
    let mut it = v.split(',');
    let mode = it.next()?.to_string();
    let d = it.next().and_then(|s| s.parse().ok()).unwrap_or(25.0);
    let h = it.next().and_then(|s| s.parse().ok()).unwrap_or(3.0);
    Some((mode, d, h))
}

/// Flight unit of the chased drone.
fn chased_unit(m: &Mission) -> Option<usize> {
    let w = m.drones.as_ref()?;
    let k = chase_arg().map_or(0, |c| c.0).min(w.count.saturating_sub(1));
    Some(w.first_unit + k)
}

/// `--start-chase`: start position (Bevy), heading (radians), altitude (ft) and speed (knots).
pub fn chase_start(m: &Mission) -> Option<(DVec3, f32, f32, f32)> {
    let (_, metres, kts) = chase_arg()?;
    let u = m.world.flights.get(chased_unit(m)?)?;
    let h = u.heading as u16 as f64 / 65536.0 * std::f64::consts::TAU;
    let d = metres / WU;
    let (east, north) = (
        (u.x as u16 as f64) * 32.0 - h.sin() * d,
        (0x8000 as f64 - u.y as u16 as f64) * 32.0 - h.cos() * d,
    );
    let alt = u.alt as f64;
    Some((wu_to_bevy(east, north, alt), h as f32, alt as f32, kts))
}

#[allow(clippy::type_complexity)]
fn stage_camera(
    combat: Option<Res<Combat>>,
    mission: Option<Res<crate::mission::ActiveMission>>,
    mut view: ResMut<ViewState>,
    aircraft: Query<(&Aircraft, &WorldPos), With<Player>>,
    mut head: Query<&mut Transform, (With<HeadAnchor>, Without<EyeCamera>)>,
    mut cam: Query<&mut Transform, With<EyeCamera>>,
) {
    let (Some(combat), Some(mission), Some((mode, dist, height))) = (combat, mission, cam_arg())
    else {
        return;
    };
    let (Ok((a, pos)), Ok(mut head), Ok(mut cam)) =
        (aircraft.single(), head.single_mut(), cam.single_mut())
    else {
        return;
    };
    if view.view != View::Chase {
        // External view: own model shown, cockpit hidden.
        view.view = View::Chase;
    }
    let sim = &combat.sim;
    let Some(i) = chased_unit(&mission.0) else { return };
    let Some(u) = sim.flights().get(i) else { return };
    let (e, n, z) = unit_position(combat.prev_flights.get(i), u, combat.alpha);
    let drone = wu_to_bevy(e, n, z);
    let me = pos.0;
    let up = DVec3::Y;
    let mut ax = drone - me;
    ax.y = 0.0;
    let ax = ax.try_normalize().unwrap_or(DVec3::NEG_Z);
    let side = ax.cross(up);
    let (d, h) = (dist as f64, height as f64);
    let mid = (drone + me) / 2.0;
    let missile =
        sim.active_projectiles().into_iter().find(|(s, _, _, _)| *s >= 8).map(|(_, m, _, _)| {
            crate::sim_render::map_point(m.x as f64 + 0.5, m.y as f64 + 0.5, m.alt as f64)
        });
    let (eye, target) = match mode.as_str() {
        "front" => (drone + ax * d + up * h + side * (d * 0.15), drone.lerp(me, 0.45)),
        "side" => (mid + side * d + up * h, mid),
        "high" => (mid + up * d - ax * (d * 0.4) + side * (d * 0.2), mid),
        "drone" => (drone + ax * (d * 0.4) + side * (d * 0.15) + up * (h * 0.4), me),
        "missile" => match missile {
            Some(m) => {
                let dir = (drone - m).try_normalize().unwrap_or(ax);
                (m - dir * d + up * h + dir.cross(up) * (d * 0.3), m.lerp(drone, 0.25))
            }
            None => (me - ax * d + up * h, drone),
        },
        _ => (me - ax * d + up * h + side * (d * 0.1), me.lerp(drone, 0.6)),
    };
    let world_rot =
        Transform::from_translation(eye.as_vec3()).looking_at(target.as_vec3(), Vec3::Y).rotation;
    let local = a.orientation.inverse();
    *head =
        Transform::from_translation(local * (eye - me).as_vec3()).with_rotation(local * world_rot);
    *cam = Transform::IDENTITY;
}
