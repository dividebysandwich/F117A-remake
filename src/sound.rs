//! Sound effects, recorded from the AdLib driver into assets/sounds/.
//!
//! - Effects are requested by id with a priority (`makeSound(id, prio)`); they play when
//!   `prio >= 3 - level` (level 3 = all sounds, 2 = no engine, 1 = firing and explosions only,
//!   0 = none; Alt-V cycles, with an extra step at level 3 that halves the engine first).
//!   Gear, bay doors, the error buzz and speech bypass the level.
//! - The engine drone follows airspeed only (pitch `(0x83 + (knots & ~1)) * 49716 / 2^18` Hz),
//!   is silent at 0 kt and with the throttle closed on the ground (in the air it keeps playing
//!   as wind noise), and only plays at level 3.
//! - Samples are optional: `sfx_NN_*` / `speech_NN_*` (.flac) register under id NN, and a
//!   requested id with no sample stays silent, so speech can be added later by dropping files in.
//! - Radar warning tones come from `sounds/rwr/` instead of sounds 8 and 9.
//! - A fire on board sounds the fire warning siren (`fire_siren.flac`, tools/gen_fire_siren.py)
//!   without a break until the fire is out (or the pilot has ejected).
//! - After ejecting the aircraft falls silent (engine, warnings, radio); the pilot hears the
//!   ejection, explosions and flares, and on the ground the emergency beacon (`beacon.flac`, two
//!   chirps every 3 s at half volume) until the rescue ends.
//! - Out of the cockpit under the chute and on the ground, the pilot hears the aircraft around
//!   (`jet.flac`, `helicopter.flac`, tools/gen_aircraft_sounds.py), placed by direction with
//!   stereo panning that follows the head, fading with distance, with a Doppler shift.

use std::collections::HashMap;

use bevy::audio::{
    AudioSink, PlaybackMode, PlaybackSettings, SpatialAudioSink, SpatialListener, SpatialScale,
    Volume,
};
use bevy::prelude::*;
use stealth_sim::{AircraftKind, fflags};

use crate::camera::EyeCamera;
use crate::combat::{CombatSystems, unit_position};
use crate::flight::{Aircraft, Player};
use crate::input::{Action, ActionEvent};
use crate::settings::AppState;
use crate::world::{FloatingOrigin, WorldPos, wu_to_bevy};

pub struct SoundPlugin;

impl Plugin for SoundPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<PlaySound>()
            .init_resource::<SoundLevel>()
            .add_systems(Startup, load_bank)
            .add_systems(
                Update,
                (flight_sounds, play_sounds, engine_drone, beacon, fire_siren)
                    .chain()
                    .run_if(in_state(AppState::Flight)),
            )
            .add_systems(
                Update,
                distant_aircraft.after(CombatSystems).run_if(in_state(AppState::Flight)),
            )
            .add_systems(OnExit(AppState::Flight), stop_all);
    }
}

/// Request to play a sound id. `priority` 255 = direct (bypasses the level).
#[derive(Message, Debug, Clone, Copy)]
pub struct PlaySound {
    pub id: u8,
    pub priority: u8,
}

pub const DIRECT: u8 = 255;

/// Sound ids of the radar warning tones (above the AdLib sound ids).
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

/// The sounds still heard after ejecting: impact, explosion, the ejection, signal flare.
const EJECTED_SOUNDS: [u8; 4] = [4, 5, 0x14, 0x1C];

/// Fire warning siren volume.
const SIREN_VOLUME: f32 = 0.35;

/// Emergency beacon interval (real seconds) and volume.
const BEACON_INTERVAL: f32 = 3.0;
const BEACON_VOLUME: f32 = 0.5;

/// Aircraft heard by the downed pilot: jets and helicopters up to their range (m), at full
/// volume within the reference distance (m), then falling off with 1 / distance; the nearest
/// `DISTANT_MAX` of them.
const JET_RANGE: f32 = 12_000.0;
const JET_REF: f32 = 500.0;
const JET_VOLUME: f32 = 0.9;
const HELICOPTER_RANGE: f32 = 5_000.0;
const HELICOPTER_REF: f32 = 200.0;
const HELICOPTER_VOLUME: f32 = 0.8;
const DISTANT_MAX: usize = 6;
/// Speed of sound (m/s) for the Doppler shift, and the shift's limits.
const SPEED_OF_SOUND: f32 = 340.0;
const DOPPLER_RANGE: (f32, f32) = (0.75, 1.35);
/// Distance between the listener's ears (m).
const EAR_GAP: f32 = 0.2;

#[derive(Resource, Default)]
struct SoundBank {
    /// Sound id -> variants (speech 43 has two).
    sfx: HashMap<u8, Vec<Handle<AudioSource>>>,
    /// Engine loops by recorded airspeed.
    engine: Vec<(f32, Handle<AudioSource>)>,
    beacon: Option<Handle<AudioSource>>,
    siren: Option<Handle<AudioSource>>,
    jet: Option<Handle<AudioSource>>,
    helicopter: Option<Handle<AudioSource>>,
}

/// A playing one-shot sound (one instance per id at a time, like the single-voice driver).
#[derive(Component)]
struct OneShot(u8);

#[derive(Component)]
struct Beacon;

/// The fire warning siren's loop.
#[derive(Component)]
struct FireSiren;

/// The loop of another aircraft heard by the downed pilot.
#[derive(Component)]
struct DistantAircraft {
    unit: usize,
    helicopter: bool,
    /// Distance (m) last frame, and the current Doppler factor.
    dist: f32,
    doppler: f32,
}

#[derive(Component)]
struct EngineDrone {
    sample_knots: f32,
}

/// Sound id of an anti-aircraft shell burst nearby (not an AdLib sound).
pub const AIRBURST: u8 = 110;

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
        } else if let Some(rest) =
            stem.strip_prefix("sfx_").or_else(|| stem.strip_prefix("speech_"))
        {
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            if let Ok(id) = digits.parse::<u8>() {
                bank.sfx.entry(id).or_default().push(server.load(path));
            }
        }
    }
    for (tone, file) in RWR_FILES {
        if crate::asset_path::asset(&format!("sounds/rwr/{file}")).exists() {
            bank.sfx
                .entry(rwr_id(tone))
                .or_default()
                .push(server.load(format!("sounds/rwr/{file}")));
        }
    }
    let optional = |name: &str| {
        crate::asset_path::asset(&format!("sounds/{name}"))
            .exists()
            .then(|| server.load(format!("sounds/{name}")))
    };
    if let Some(h) = optional("airburst.flac") {
        bank.sfx.entry(AIRBURST).or_default().push(h);
    }
    bank.beacon = optional("beacon.flac");
    bank.siren = optional("fire_siren.flac");
    bank.jet = optional("jet.flac");
    bank.helicopter = optional("helicopter.flac");
    bank.engine.sort_by(|a, b| a.0.total_cmp(&b.0));
    commands.insert_resource(bank);
}

/// The pilot has ejected.
fn has_ejected(combat: Option<&crate::combat::Combat>) -> bool {
    combat.is_some_and(|c| c.sim.chute().is_some())
}

#[allow(clippy::too_many_arguments)]
fn play_sounds(
    mut commands: Commands,
    mut requests: MessageReader<PlaySound>,
    mut actions: MessageReader<ActionEvent>,
    mut level: ResMut<SoundLevel>,
    bank: Option<Res<SoundBank>>,
    combat: Option<Res<crate::combat::Combat>>,
    playing: Query<(Entity, &OneShot)>,
    mut silenced: Local<bool>,
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
    // Ejecting cuts off the aircraft's sounds at once.
    let ejected = has_ejected(combat.as_deref());
    let silence = ejected && !*silenced;
    *silenced = ejected;
    let survives = |id: u8| !ejected || EJECTED_SOUNDS.contains(&id);
    if silence {
        for (e, _) in playing.iter().filter(|(_, p)| !survives(p.0)) {
            commands.entity(e).despawn();
        }
    }
    let Some(bank) = bank else {
        requests.clear();
        return;
    };
    for r in requests.read() {
        let allowed =
            r.priority == DIRECT || (level.0 > 0 && r.priority as i32 >= 3 - level.0 as i32);
        if !allowed || !survives(r.id) || playing.iter().any(|(_, p)| p.0 == r.id && survives(p.0))
        {
            continue;
        }
        let Some(variants) = bank.sfx.get(&r.id) else { continue };
        let handle = variants[rand::random_range(0..variants.len())].clone();
        debug!("sound {} (priority {})", r.id, r.priority);
        commands.spawn((AudioPlayer(handle), PlaybackSettings::DESPAWN, OneShot(r.id)));
    }
}

/// Sounds triggered from the flight step and frame update.
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
    // Stall horn while stalled (repeated every frame).
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
        // down.
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
    combat: Option<Res<crate::combat::Combat>>,
    mut drone: Query<(Entity, &EngineDrone, Option<&mut AudioSink>)>,
) {
    let Some(bank) = bank else { return };
    let Ok(a) = player.single() else { return };
    let knots = a.knots().min(768.0);
    let on = level.0 == 3
        && (a.set_thrust > 0.0 || !a.on_ground)
        && knots >= 2.0
        && a.ended.is_none()
        && !has_ejected(combat.as_deref());
    if !on || bank.engine.is_empty() {
        for (e, _, _) in &drone {
            commands.entity(e).despawn();
        }
        return;
    }
    // Nearest recording at or below the airspeed, pitched to the exact frequency.
    let (sample_knots, handle) =
        bank.engine.iter().rev().find(|(k, _)| *k <= knots).unwrap_or(&bank.engine[0]).clone();
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

fn spawn_drone(
    commands: &mut Commands,
    handle: Handle<AudioSource>,
    sample_knots: f32,
    speed: f32,
    volume: f32,
) {
    commands.spawn((
        AudioPlayer(handle),
        PlaybackSettings {
            mode: PlaybackMode::Loop,
            volume: Volume::Linear(volume),
            speed,
            ..default()
        },
        EngineDrone { sample_knots },
    ));
}

/// The emergency beacon of the pilot on the ground waiting for the rescue: the first chirps on
/// landing, then every `BEACON_INTERVAL` (real time, held while the flight is paused).
fn beacon(
    mut commands: Commands,
    bank: Option<Res<SoundBank>>,
    level: Res<SoundLevel>,
    combat: Option<Res<crate::combat::Combat>>,
    real: Res<Time<Real>>,
    virt: Res<Time<Virtual>>,
    mut wait: Local<f32>,
) {
    let Some(handle) = bank.as_ref().and_then(|b| b.beacon.clone()) else { return };
    let waiting = combat.is_some_and(|c| c.sim.csar().is_some() && c.end.is_none());
    if !waiting {
        *wait = 0.0;
        return;
    }
    if virt.is_paused() {
        return;
    }
    *wait -= real.delta_secs();
    if *wait <= 0.0 {
        *wait = BEACON_INTERVAL;
        if level.0 > 0 {
            commands.spawn((
                AudioPlayer(handle),
                PlaybackSettings::DESPAWN.with_volume(Volume::Linear(BEACON_VOLUME)),
                Beacon,
            ));
        }
    }
}

/// The fire warning: a siren sounding without a break while the aircraft is on fire (held while
/// the flight is paused).
fn fire_siren(
    mut commands: Commands,
    bank: Option<Res<SoundBank>>,
    level: Res<SoundLevel>,
    combat: Option<Res<crate::combat::Combat>>,
    player: Query<&Aircraft, With<Player>>,
    virt: Res<Time<Virtual>>,
    siren: Query<(Entity, Option<&AudioSink>), With<FireSiren>>,
) {
    // Out of the aircraft: ejected, or (campaign) back at the board with the damage of the
    // sortie still on record until the next one.
    let burning = combat.as_deref().is_some_and(|c| {
        c.sim.damage_effects().bits & stealth_sim::dmg::FIRE != 0
            && c.end.is_none()
            && !c.sim.player_absent()
    }) && !has_ejected(combat.as_deref())
        && player.single().is_ok_and(|a| a.ended.is_none())
        && level.0 > 0;
    let Some(handle) = bank.as_ref().and_then(|b| b.siren.clone()).filter(|_| burning) else {
        for (e, _) in &siren {
            commands.entity(e).despawn();
        }
        return;
    };
    match siren.single() {
        Ok((_, Some(sink))) => {
            if virt.is_paused() != sink.is_paused() {
                if virt.is_paused() { sink.pause() } else { sink.play() }
            }
        }
        Ok(_) => {}
        Err(_) => {
            commands.spawn((
                AudioPlayer(handle),
                PlaybackSettings {
                    mode: PlaybackMode::Loop,
                    volume: Volume::Linear(SIREN_VOLUME),
                    ..default()
                },
                FireSiren,
            ));
        }
    }
}

/// The aircraft around the pilot, out of the cockpit (under the chute and on the ground): each
/// one's loop plays from its position, panned by its direction from the eye (the ears follow the
/// head) and shifted by its closing speed; out of range or beyond the nearest few, it stops.
#[allow(clippy::too_many_arguments)]
fn distant_aircraft(
    mut commands: Commands,
    bank: Option<Res<SoundBank>>,
    level: Res<SoundLevel>,
    combat: Option<Res<crate::combat::Combat>>,
    origin: Res<FloatingOrigin>,
    virt: Res<Time<Virtual>>,
    eye: Query<(Entity, &GlobalTransform, Has<SpatialListener>), With<EyeCamera>>,
    mut playing: Query<(
        Entity,
        &mut DistantAircraft,
        &mut WorldPos,
        Option<&mut SpatialAudioSink>,
    )>,
) {
    let heard = level.0 == 3
        && has_ejected(combat.as_deref())
        && combat.as_ref().is_some_and(|c| c.end.is_none());
    let (Some(bank), Some(combat), Ok((cam, eye_tf, listening))) =
        (bank.filter(|_| heard), combat, eye.single())
    else {
        for (e, ..) in &playing {
            commands.entity(e).despawn();
        }
        return;
    };
    if !listening {
        // rodio's panning plays an emitter on the right louder in the left channel, so the ears
        // are swapped to come out right.
        commands.entity(cam).insert(SpatialListener {
            left_ear_offset: Vec3::X * EAR_GAP / 2.0,
            right_ear_offset: Vec3::X * EAR_GAP / -2.0,
        });
    }
    let ear = origin.origin + eye_tf.translation().as_dvec3();
    let sim = &combat.sim;
    // The nearest jets and helicopters in range: (unit, helicopter, position, distance).
    let mut heard: Vec<(usize, bool, bevy::math::DVec3, f32)> = sim
        .active_aircraft()
        .into_iter()
        .filter(|(_, u, _)| {
            u.flags & fflags::DESTROYED == 0
                && (u.wx != 0 || u.wy != 0)
                && (u.flags & fflags::AIRBORNE != 0 || u.speed > 0)
        })
        .filter_map(|(i, u, _)| {
            let helicopter = match sim.aircraft_kind(u.spec) {
                AircraftKind::Plane => false,
                AircraftKind::Helicopter => true,
                AircraftKind::Drone => return None,
            };
            let (east, north, alt) = unit_position(combat.prev_flights.get(i), &u, combat.alpha);
            let p = wu_to_bevy(east, north, alt);
            let d = (p - ear).length() as f32;
            let range = if helicopter { HELICOPTER_RANGE } else { JET_RANGE };
            (d < range).then_some((i, helicopter, p, d))
        })
        .collect();
    heard.sort_by(|a, b| a.3.total_cmp(&b.3));
    heard.truncate(DISTANT_MAX);

    let dt = virt.delta_secs();
    let mut have = Vec::new();
    for (e, mut d, mut pos, sink) in &mut playing {
        let Some(&(_, helicopter, p, dist)) =
            heard.iter().find(|h| h.0 == d.unit && h.1 == d.helicopter)
        else {
            commands.entity(e).despawn();
            continue;
        };
        have.push(d.unit);
        pos.0 = p;
        if dt > 0.0 {
            let away = (dist - d.dist) / dt;
            let target = (SPEED_OF_SOUND / (SPEED_OF_SOUND + away).max(1.0))
                .clamp(DOPPLER_RANGE.0, DOPPLER_RANGE.1);
            d.doppler += (target - d.doppler) * (dt * 4.0).min(1.0);
        }
        d.dist = dist;
        let Some(mut sink) = sink else { continue };
        if virt.is_paused() {
            sink.pause();
            continue;
        }
        sink.play();
        sink.set_volume(Volume::Linear(distant_volume(helicopter, dist)));
        sink.set_speed(unit_pitch(d.unit) * d.doppler);
    }
    for &(unit, helicopter, p, dist) in heard.iter().filter(|h| !have.contains(&h.0)) {
        let handle = if helicopter { bank.helicopter.clone() } else { bank.jet.clone() };
        let Some(handle) = handle else { continue };
        let range = if helicopter { HELICOPTER_RANGE } else { JET_RANGE };
        commands.spawn((
            AudioPlayer(handle),
            PlaybackSettings {
                mode: PlaybackMode::Loop,
                volume: Volume::Linear(distant_volume(helicopter, dist)),
                speed: unit_pitch(unit),
                spatial: true,
                // Positions shrunk to within rodio's unit distance: it only pans, the volume
                // falls off here.
                spatial_scale: Some(SpatialScale::new(1.0 / range)),
                ..default()
            },
            WorldPos(p),
            Transform::from_translation((p - origin.origin).as_vec3()),
            DistantAircraft { unit, helicopter, dist, doppler: 1.0 },
        ));
    }
}

/// Volume of an aircraft heard `dist` metres away, fading out over the last quarter of the range.
fn distant_volume(helicopter: bool, dist: f32) -> f32 {
    let (range, reference, volume) = if helicopter {
        (HELICOPTER_RANGE, HELICOPTER_REF, HELICOPTER_VOLUME)
    } else {
        (JET_RANGE, JET_REF, JET_VOLUME)
    };
    let fade = ((range - dist) / (0.25 * range)).clamp(0.0, 1.0);
    volume * (reference / dist.max(1.0)).min(1.0) * fade
}

/// A slightly different pitch per aircraft, so that several don't sound alike.
fn unit_pitch(unit: usize) -> f32 {
    0.92 + 0.16 * (unit as f32 * 0.618).fract()
}

type FlightSound =
    Or<(With<OneShot>, With<EngineDrone>, With<Beacon>, With<FireSiren>, With<DistantAircraft>)>;

fn stop_all(mut commands: Commands, q: Query<Entity, FlightSound>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}
