//! `--screenshot <file.png> [--shot-frames N | --shot-time S]`: save a screenshot after N frames
//! (or once the aircraft's flight time reaches S seconds) and exit. Used for automated visual
//! checks. A `Screenshot` step in a `--script` takes it at that moment instead.

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};

use crate::flight::{Aircraft, Player};

pub struct AutoShotPlugin {
    pub path: String,
    pub frames: u32,
    pub time: Option<f32>,
}

/// Set by the test script (`Screenshot` step) to take the screenshot now.
#[derive(Resource, Default)]
pub struct ShotRequest(pub bool);

#[derive(Resource)]
struct AutoShot {
    path: String,
    frames: u32,
    time: Option<f32>,
    taken: Option<u32>,
}

impl Plugin for AutoShotPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(AutoShot { path: self.path.clone(), frames: self.frames, time: self.time, taken: None })
            .init_resource::<ShotRequest>()
            .add_systems(Update, shoot);
    }
}

fn shoot(
    mut commands: Commands,
    mut shot: ResMut<AutoShot>,
    mut frame: Local<u32>,
    aircraft: Query<&Aircraft, With<Player>>,
    request: Res<ShotRequest>,
    mut exit: MessageWriter<AppExit>,
) {
    *frame += 1;
    let due = request.0
        || match shot.time {
            Some(t) => aircraft.single().is_ok_and(|a| a.time >= t),
            None => *frame >= shot.frames,
        };
    match shot.taken {
        None if due => {
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(shot.path.clone()));
            shot.taken = Some(*frame);
        }
        Some(t) if *frame > t + 10 => {
            exit.write(AppExit::Success);
        }
        _ => {}
    }
}

/// Parses `--screenshot` / `--shot-frames` / `--shot-time` from the command line.
pub fn from_args() -> Option<AutoShotPlugin> {
    let args: Vec<String> = std::env::args().collect();
    let get = |f: &str| args.iter().position(|a| a == f).and_then(|i| args.get(i + 1)).cloned();
    let path = get("--screenshot")?;
    let frames = get("--shot-frames").and_then(|s| s.parse().ok()).unwrap_or(90);
    let time = get("--shot-time").and_then(|s| s.parse().ok());
    Some(AutoShotPlugin { path, frames, time })
}
