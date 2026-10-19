//! Original sounds, recorded from the AdLib driver into assets/sounds/.
//!
//! - Effects are requested by id with a priority (`makeSound(id, prio)`); they play when
//!   `prio >= 3 - level` (level 3 = all sounds, 2 = no engine, 1 = firing and explosions only,
//!   0 = none; Alt-V cycles, with an extra step at level 3 that halves the engine first).
//!   Gear, bay doors, the error buzz and speech bypass the level.
//! - The engine drone follows airspeed only (pitch `(0x83 + (knots & ~1)) * 49716 / 2^18` Hz),
//!   is silent at 0 kt and with the throttle closed, and only plays at level 3.
//! - Samples are optional: `sfx_NN_*` / `speech_NN_*` (.flac) register under id NN, and a
//!   requested id with no sample stays silent, so speech can be added later by dropping files in.
//! - Radar warning tones come from `sounds/rwr/` instead of the original sounds 8 and 9.

use std::collections::HashMap;

use bevy::audio::{AudioSink, PlaybackMode, PlaybackSettings, Volume};
use bevy::prelude::*;

use crate::flight::{Aircraft, Player};
use crate::input::{Action, ActionEvent};
use crate::settings::AppState;

pub struct SoundPlugin;

impl Plugin for SoundPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<PlaySound>()
            .init_resource::<SoundLevel>()
            .add_systems(Startup, load_bank)
            .add_systems(
                Update,
                (flight_sounds, play_sounds, engine_drone).chain().run_if(in_state(AppState::Flight)),
            )
            .add_systems(OnExit(AppState::Flight), stop_all);
    }
}

/// Request to play an original sound id. `priority` 255 = direct (bypasses the level).
#[derive(Message, Debug, Clone, Copy)]
pub struct PlaySound {
    pub id: u8,
    pub priority: u8,
}

pub const DIRECT: u8 = 255;

/// Sound ids of the radar warning tones (above the original's ids).
pub fn rwr_id(tone: stealth_sim::RwrTone) -> u8 {
    use stealth_sim::RwrTone::*;
    match tone {
        Track => 100,
        Scan => 101,
        SearchWarning => 102,
        SamLaunch => 103,
        AirLaunch => 104,
    }
}

const RWR_FILES: [(stealth_sim::RwrTone, &str); 5] = {
    use stealth_sim::RwrTone::*;
    [
        (Track, "Track.flac"),
        (Scan, "Scan.flac"),
        (SearchWarning, "SearchWarning.flac"),
        (SamLaunch, "Launch.flac"),
        (AirLaunch, "LaunchWarningUS.flac"),
    ]
};

/// Alt-V sound level (3 = all sounds .. 0 = none); `engine_half` = level 3 with the engine
/// at half volume.
#[derive(Resource)]
pub struct SoundLevel(pub u8, pub bool);

impl Default for SoundLevel {
    fn default() -> Self {
        Self(3, false)
    }
}

/// Engine drone volume at full level.
const ENGINE_VOLUME: f32 = 0.8;

#[derive(Resource, Default)]
struct SoundBank {
    /// Sound id -> variants (speech 43 has two).
    sfx: HashMap<u8, Vec<Handle<AudioSource>>>,
    /// Engine loops by recorded airspeed.
    engine: Vec<(f32, Handle<AudioSource>)>,
}

/// A playing one-shot sound (one instance per id at a time, like the single-voice driver).
#[derive(Component)]
struct OneShot(u8);

#[derive(Component)]
struct EngineDrone {
    sample_knots: f32,
}

fn load_bank(mut commands: Commands, server: Res<AssetServer>) {
    let mut bank = SoundBank::default();
    let Ok(dir) = std::fs::read_dir(crate::asset_path::asset("sounds")) else {
        warn!("assets/sounds missing: no original sounds");
        commands.insert_resource(bank);
        return;
    };
    for e in dir.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Some(stem) = name.strip_suffix(".flac") else { continue };
        let path = format!("sounds/{name}");
        if let Some(k) = stem.strip_prefix("engine_").and_then(|k| k.parse::<f32>().ok()) {
            bank.engine.push((k, server.load(path)));
        } else if let Some(rest) = stem.strip_prefix("sfx_").or_else(|| stem.strip_prefix("speech_")) {
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            if let Ok(id) = digits.parse::<u8>() {
                bank.sfx.entry(id).or_default().push(server.load(path));
            }
        }
    }
    for (tone, file) in RWR_FILES {
        if crate::asset_path::asset(&format!("sounds/rwr/{file}")).exists() {
            bank.sfx.entry(rwr_id(tone)).or_default().push(server.load(format!("sounds/rwr/{file}")));
        }
    }
    bank.engine.sort_by(|a, b| a.0.total_cmp(&b.0));
    commands.insert_resource(bank);
}

fn play_sounds(
    mut commands: Commands,
    mut requests: MessageReader<PlaySound>,
    mut actions: MessageReader<ActionEvent>,
    mut level: ResMut<SoundLevel>,
    bank: Option<Res<SoundBank>>,
    playing: Query<&OneShot>,
) {
    for ActionEvent(a) in actions.read() {
        if *a == Action::Volume {
            *level = match *level {
                SoundLevel(3, false) => SoundLevel(3, true),
                SoundLevel(0, _) => SoundLevel(3, false),
                SoundLevel(l, _) => SoundLevel(l - 1, false),
            };
        }
    }
    let Some(bank) = bank else {
        requests.clear();
        return;
    };
    for r in requests.read() {
        let allowed = r.priority == DIRECT || (level.0 > 0 && r.priority as i32 >= 3 - level.0 as i32);
        if !allowed || playing.iter().any(|p| p.0 == r.id) {
            continue;
        }
        let Some(variants) = bank.sfx.get(&r.id) else { continue };
        let handle = variants[rand::random_range(0..variants.len())].clone();
        debug!("sound {} (priority {})", r.id, r.priority);
        commands.spawn((AudioPlayer(handle), PlaybackSettings::DESPAWN, OneShot(r.id)));
    }
}

/// Sounds the original triggers from the flight step and frame update.
#[allow(clippy::too_many_arguments)]
fn flight_sounds(
    mut out: MessageWriter<PlaySound>,
    mut actions: MessageReader<ActionEvent>,
    player: Query<(&Aircraft, &crate::world::WorldPos), With<Player>>,
    mission: Option<Res<crate::mission::ActiveMission>>,
    mut prev: Local<Option<(bool, f32)>>,
    mut said_takeoff: Local<bool>,
    mut said_land: Local<bool>,
    mut been_away: Local<bool>,
) {
    let Ok((a, pos)) = player.single() else { return };
    for ActionEvent(act) in actions.read() {
        match act {
            Action::Gear if !a.on_ground => {
                out.write(PlaySound { id: 19, priority: DIRECT });
            }
            Action::ThrottleIdle => {
                out.write(PlaySound { id: 12, priority: 0 });
            }
            _ => {}
        }
    }
    let (was_on_ground, prev_thrust) = prev.unwrap_or((a.on_ground, a.thrust));
    // Spool up when the throttle is raised from zero actual thrust.
    if prev_thrust == 0.0 && a.set_thrust > 0.0 {
        out.write(PlaySound { id: 11, priority: 2 });
    }
    // Touchdown in a landing corridor.
    if a.on_ground && !was_on_ground && a.in_landing_corridor {
        out.write(PlaySound { id: 10, priority: 2 });
    }
    // Stall horn while stalled (the original repeats it every frame).
    if !a.on_ground && a.velocity < a.stall_speed {
        out.write(PlaySound { id: 14, priority: 1 });
    }
    // "Cleared for takeoff" about 2 s into the mission; "Cleared to land" once on approach.
    if a.time < 1.0 {
        *said_takeoff = false;
        *said_land = false;
        *been_away = false;
    }
    if !*said_takeoff && a.time > 2.0 {
        *said_takeoff = true;
        out.write(PlaySound { id: 44, priority: DIRECT });
    }
    if let Some(m) = mission
        && !*said_land
        && a.has_flown
        && !a.on_ground
    {
        let base = &m.0.landing.position;
        let (be, bn) = crate::world::map_to_wu(base.x as u16, base.y as u16);
        let (e, n, _) = crate::world::bevy_to_wu(pos.0);
        // On approach: back within range 5000 of the landing base, descending with the gear
        // down (the original also waits for the AOA indexer to show on-speed).
        let d = (e - be).hypot(n - bn);
        if d > 10_000.0 {
            *been_away = true;
        }
        if *been_away && d < 5000.0 && a.climb_rate < 0.0 && a.gear_down {
            *said_land = true;
            out.write(PlaySound { id: 45, priority: DIRECT });
        }
    }
    *prev = Some((a.on_ground, a.thrust));
}

fn engine_drone(
    mut commands: Commands,
    bank: Option<Res<SoundBank>>,
    level: Res<SoundLevel>,
    player: Query<&Aircraft, With<Player>>,
    mut drone: Query<(Entity, &EngineDrone, Option<&mut AudioSink>)>,
) {
    let Some(bank) = bank else { return };
    let Ok(a) = player.single() else { return };
    let knots = a.knots().min(768.0);
    let on = level.0 == 3 && a.set_thrust > 0.0 && knots >= 2.0 && a.ended.is_none();
    if !on || bank.engine.is_empty() {
        for (e, _, _) in &drone {
            commands.entity(e).despawn();
        }
        return;
    }
    // Nearest recording at or below the airspeed, pitched to the exact original frequency.
    let (sample_knots, handle) = bank
        .engine
        .iter()
        .rev()
        .find(|(k, _)| *k <= knots)
        .unwrap_or(&bank.engine[0])
        .clone();
    let freq = |k: f32| 0x83 as f32 + ((k as u32) & !1) as f32;
    let speed = freq(knots) / freq(sample_knots);
    let volume = if level.1 { ENGINE_VOLUME * 0.5 } else { ENGINE_VOLUME };
    match drone.single_mut() {
        Ok((e, d, sink)) if d.sample_knots != sample_knots => {
            let _ = sink;
            commands.entity(e).despawn();
            spawn_drone(&mut commands, handle, sample_knots, speed, volume);
        }
        Ok((_, _, Some(mut sink))) => {
            sink.set_speed(speed);
            sink.set_volume(Volume::Linear(volume));
        }
        Ok(_) => {}
        Err(_) => spawn_drone(&mut commands, handle, sample_knots, speed, volume),
    }
}

fn spawn_drone(commands: &mut Commands, handle: Handle<AudioSource>, sample_knots: f32, speed: f32, volume: f32) {
    commands.spawn((
        AudioPlayer(handle),
        PlaybackSettings { mode: PlaybackMode::Loop, volume: Volume::Linear(volume), speed, ..default() },
        EngineDrone { sample_knots },
    ));
}

fn stop_all(mut commands: Commands, q: Query<Entity, Or<(With<OneShot>, With<EngineDrone>)>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}
