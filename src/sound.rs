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
//! - The cannon (sound 6) holds a loop of its burst while it fires and plays the burst's decay
//!   when it stops (`cannon_loop.flac`, `cannon_tail.flac`, tools/cut_cannon.py).
//! - A fire on board sounds the fire warning siren (`fire_siren.flac`, tools/gen_fire_siren.py)
//!   without a break until the fire is out (or the pilot has ejected).
//! - After ejecting the aircraft falls silent (engine, warnings, radio); the pilot hears the
//!   ejection, explosions and flares, and on the ground the emergency beacon (`beacon.flac`, two
//!   chirps every 3 s at half volume) until the rescue ends.
//! - A helicopter has no engine drone: its rotor (`heli_rotor.flac`) plays at the rotor speed,
//!   louder with the torque and in translational lift (the blade slap), over the turbines
//!   (`heli_turbine.flac`) pitched up with the torque.
//! - Out of the cockpit under the chute and on the ground, the pilot hears the aircraft around
//!   (`jet.flac`, `helicopter.flac`, tools/gen_aircraft_sounds.py), placed by direction with
//!   stereo panning that follows the head, fading with distance, with a Doppler shift.
//! - With Sidewinders selected in the air-to-air HUD mode, the seeker growls (cut from a
//!   recording of the real tones by tools/cut_sidewinder_tones.py): the low growl
//!   (`aim9_growl.flac`) with nothing in its view, the higher one (`aim9_seen.flac`) with the
//!   locked target in the seeker (the LOCK oval), and the steady lock tone (`aim9_tone.flac`)
//!   once the target is in range.

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
                (
                    flight_sounds,
                    play_sounds,
                    cannon,
                    engine_drone,
                    heli_sounds,
                    beacon,
                    fire_siren,
                    sidewinder_tones,
                    objective_sound,
                )
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
    /// The cannon's loop and tail.
    cannon: Option<(Handle<AudioSource>, Handle<AudioSource>)>,
    jet: Option<Handle<AudioSource>>,
    helicopter: Option<Handle<AudioSource>>,
    heli_rotor: Option<Handle<AudioSource>>,
    heli_turbine: Option<Handle<AudioSource>>,
    /// The Sidewinder seeker's loops, by `SeekerTone`.
    aim9: [Option<Handle<AudioSource>>; 3],
}

/// A playing one-shot sound (one instance per id at a time, like the single-voice driver).
#[derive(Component)]
struct OneShot(u8);

#[derive(Component)]
struct Beacon;

/// The cannon's loop, while it fires.
#[derive(Component)]
struct CannonLoop;

/// Sound id of the cannon.
const CANNON: u8 = 6;

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

/// A Sidewinder seeker loop: 0 the search growl, 1 the growl with the target in the seeker,
/// 2 the lock tone.
#[derive(Component)]
struct SeekerTone(usize);

const AIM9_FILES: [&str; 3] = ["aim9_growl.flac", "aim9_seen.flac", "aim9_tone.flac"];

/// Sidewinder seeker volume (the loops keep the recording's relative loudness).
const SEEKER_VOLUME: f32 = 0.3;
/// How long (real seconds) the seeker has to see or lose the target before its sound changes,
/// so the target at the edge of the seeker's view doesn't chop it.
const SEEKER_HOLD: f32 = 0.15;

/// Missile table index of the AIM-9 the player carries.
const AIM9_SPEC: i16 = 23;

/// The flown helicopter's loops: rotor (false) and turbines (true).
#[derive(Component)]
struct HeliLoop(bool);

/// Helicopter loop volumes at full level: the rotor at idle torque and its rise to full torque
/// and in translational lift, the turbines.
const ROTOR_VOLUME: (f32, f32, f32) = (0.45, 0.35, 0.2);
const TURBINE_VOLUME: f32 = 0.35;

/// Sound id of an anti-aircraft shell burst nearby (not an AdLib sound).
pub const AIRBURST: u8 = 110;
/// Sound id of an objective achieved (`sounds/objective_complete.flac`, optional).
pub const OBJECTIVE: u8 = 111;

fn load_bank(mut commands: Commands, server: Res<AssetServer>) {
    let mut bank = SoundBank::default();
    let names = crate::asset_path::list("sounds");
    if names.is_empty() {
        warn!("assets/sounds missing: no original sounds");
        commands.insert_resource(bank);
        return;
    }
    for name in names {
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
        if crate::asset_path::exists(&format!("sounds/rwr/{file}")) {
            bank.sfx
                .entry(rwr_id(tone))
                .or_default()
                .push(server.load(format!("sounds/rwr/{file}")));
        }
    }
    let optional = |name: &str| {
        crate::asset_path::exists(&format!("sounds/{name}"))
            .then(|| server.load(format!("sounds/{name}")))
    };
    if let Some(h) = optional("airburst.flac") {
        bank.sfx.entry(AIRBURST).or_default().push(h);
    }
    if let Some(h) = optional("objective_complete.flac") {
        bank.sfx.entry(OBJECTIVE).or_default().push(h);
    }
    bank.beacon = optional("beacon.flac");
    bank.siren = optional("fire_siren.flac");
    bank.cannon = optional("cannon_loop.flac").zip(optional("cannon_tail.flac"));
    bank.jet = optional("jet.flac");
    bank.helicopter = optional("helicopter.flac");
    bank.heli_rotor = optional("heli_rotor.flac");
    bank.heli_turbine = optional("heli_turbine.flac");
    bank.aim9 = AIM9_FILES.map(optional);
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
        // The cannon's loop and tail take its place.
        if r.id == CANNON && bank.cannon.is_some() {
            continue;
        }
        let Some(variants) = bank.sfx.get(&r.id) else { continue };
        let handle = variants[rand::random_range(0..variants.len())].clone();
        debug!("sound {} (priority {})", r.id, r.priority);
        commands.spawn((AudioPlayer(handle), PlaybackSettings::DESPAWN, OneShot(r.id)));
    }
}

/// The cannon: its loop while it fires (held while the flight is paused), then the tail. Ejecting
/// cuts it off.
fn cannon(
    mut commands: Commands,
    bank: Option<Res<SoundBank>>,
    level: Res<SoundLevel>,
    combat: Option<Res<crate::combat::Combat>>,
    virt: Res<Time<Virtual>>,
    playing: Query<(Entity, Option<&AudioSink>), With<CannonLoop>>,
) {
    let Some((looped, tail)) = bank.as_ref().and_then(|b| b.cannon.clone()) else { return };
    let ejected = has_ejected(combat.as_deref());
    let firing = combat
        .as_deref()
        .is_some_and(|c| c.sim.gun_firing() && c.end.is_none() && !c.sim.player_absent())
        && !ejected
        && level.0 > 0;
    if !firing {
        if !playing.is_empty() && !ejected && level.0 > 0 {
            commands.spawn((AudioPlayer(tail), PlaybackSettings::DESPAWN, OneShot(CANNON)));
        }
        for (e, _) in &playing {
            commands.entity(e).despawn();
        }
        return;
    }
    match playing.single() {
        Ok((_, Some(sink))) => {
            if virt.is_paused() != sink.is_paused() {
                if virt.is_paused() { sink.pause() } else { sink.play() }
            }
        }
        Ok(_) => {}
        Err(_) => {
            commands.spawn((AudioPlayer(looped), PlaybackSettings::LOOP, CannonLoop));
        }
    }
}

/// An objective just achieved (ticked in the mission orders): its sound, if there is one.
fn objective_sound(
    orders: Res<crate::mfd::MissionOrders>,
    mut was: Local<[bool; 2]>,
    mut sounds: MessageWriter<PlaySound>,
) {
    if (0..2).any(|k| orders.done[k] && !was[k]) {
        sounds.write(PlaySound { id: OBJECTIVE, priority: 3 });
    }
    *was = orders.done;
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
    settings: Res<crate::settings::Settings>,
) {
    let Ok((a, pos)) = player.single() else { return };
    let helicopter = settings.aircraft.spec().helicopter;
    for ActionEvent(act) in actions.read() {
        match act {
            Action::Gear if !a.on_ground => {
                out.write(PlaySound { id: 19, priority: DIRECT });
            }
            Action::ThrottleIdle if !helicopter => {
                out.write(PlaySound { id: 12, priority: 0 });
            }
            _ => {}
        }
    }
    let (was_on_ground, prev_thrust) = prev.unwrap_or((a.on_ground, a.thrust));
    // Spool up when the throttle is raised from zero actual thrust (a helicopter's turbines
    // keep running).
    if prev_thrust == 0.0 && a.set_thrust > 0.0 && !helicopter {
        out.write(PlaySound { id: 11, priority: 2 });
    }
    // Touchdown in a landing corridor (a helicopter's anywhere).
    if a.on_ground && !was_on_ground && (a.in_landing_corridor || helicopter) {
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
    settings: Res<crate::settings::Settings>,
    mut drone: Query<(Entity, &EngineDrone, Option<&mut AudioSink>)>,
) {
    let Some(bank) = bank else { return };
    let Ok(a) = player.single() else { return };
    let knots = a.knots().min(768.0);
    let on = level.0 == 3
        && !settings.aircraft.spec().helicopter
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

/// The flown helicopter's rotor and turbines (instead of the engine drone), while the rotor
/// turns.
fn heli_sounds(
    mut commands: Commands,
    bank: Option<Res<SoundBank>>,
    level: Res<SoundLevel>,
    player: Query<&Aircraft, With<Player>>,
    combat: Option<Res<crate::combat::Combat>>,
    settings: Res<crate::settings::Settings>,
    mut loops: Query<(Entity, &HeliLoop, Option<&mut AudioSink>)>,
) {
    let Some(bank) = bank else { return };
    let Ok(a) = player.single() else { return };
    let nr = a.rotor_speed();
    let on = level.0 == 3
        && settings.aircraft.spec().helicopter
        && nr > 0.05
        && a.ended.is_none()
        && !has_ejected(combat.as_deref());
    if !on {
        for (e, ..) in &loops {
            commands.entity(e).despawn();
        }
        return;
    }
    let half = if level.1 { 0.5 } else { 1.0 };
    let torque = a.torque().clamp(0.0, 1.2);
    let rotor = (
        nr.max(0.3),
        half * nr
            * (ROTOR_VOLUME.0 + ROTOR_VOLUME.1 * torque + ROTOR_VOLUME.2 * a.translational_lift()),
    );
    // The turbines run down with the fuel.
    let turbine = (0.85 + 0.2 * torque, if a.fuel > 0.0 { half * TURBINE_VOLUME } else { 0.0 });
    let mut have = [false; 2];
    for (_, l, sink) in &mut loops {
        have[l.0 as usize] = true;
        let (speed, volume) = if l.0 { turbine } else { rotor };
        if let Some(mut sink) = sink {
            sink.set_speed(speed);
            sink.set_volume(Volume::Linear(volume));
        }
    }
    for (turbines, handle) in [(false, &bank.heli_rotor), (true, &bank.heli_turbine)] {
        let Some(handle) = handle.clone().filter(|_| !have[turbines as usize]) else { continue };
        let (speed, volume) = if turbines { turbine } else { rotor };
        commands.spawn((
            AudioPlayer(handle),
            PlaybackSettings {
                mode: PlaybackMode::Loop,
                volume: Volume::Linear(volume),
                speed,
                ..default()
            },
            HeliLoop(turbines),
        ));
    }
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

/// The Sidewinder seeker's growl and lock tone, while Sidewinders are selected in the air-to-air
/// HUD mode (held while the flight is paused).
#[allow(clippy::too_many_arguments)]
fn sidewinder_tones(
    mut commands: Commands,
    bank: Option<Res<SoundBank>>,
    level: Res<SoundLevel>,
    combat: Option<Res<crate::combat::Combat>>,
    hud: Res<crate::hud::HudState>,
    player: Query<&Aircraft, With<Player>>,
    real: Res<Time<Real>>,
    virt: Res<Time<Virtual>>,
    mut loops: Query<(Entity, &SeekerTone, Option<&mut AudioSink>)>,
    mut held: Local<((bool, bool), f32)>,
) {
    // None: silent; Some(false): growl; Some(true): lock tone.
    let state = combat.as_deref().and_then(|c| {
        let sim = &c.sim;
        let (groups, cur) = sim.weapon_groups();
        let (w, rounds) = *groups.get(cur?)?;
        let aim9 = sim.weapon_types().get(w.max(0) as usize).is_some_and(|t| t.spec == AIM9_SPEC);
        let on = aim9
            && rounds > 0
            && !sim.gun_selected()
            && hud.mode == crate::hud::HudMode::Air
            && c.end.is_none()
            && !sim.player_absent()
            && !has_ejected(Some(c))
            && player.single().is_ok_and(|a| a.ended.is_none())
            && level.0 > 0;
        let t = sim.targeting();
        on.then_some((t.lock_valid, t.lock_valid && t.in_range))
    });
    let Some(now) = state else {
        *held = default();
        for (e, ..) in &loops {
            commands.entity(e).despawn();
        }
        return;
    };
    // (seen, locked) as heard: follows the seeker once a change has lasted SEEKER_HOLD.
    if now == held.0 {
        held.1 = 0.0;
    } else if !virt.is_paused() {
        held.1 += real.delta_secs();
        if held.1 >= SEEKER_HOLD {
            *held = (now, 0.0);
        }
    }
    let (seen, locked) = held.0;
    let tone = if locked { 2 } else { seen as usize };
    let mut have = false;
    for (e, l, sink) in &mut loops {
        if l.0 != tone {
            commands.entity(e).despawn();
            continue;
        }
        have = true;
        if let Some(sink) = sink
            && virt.is_paused() != sink.is_paused()
        {
            if virt.is_paused() { sink.pause() } else { sink.play() }
        }
    }
    if have {
        return;
    }
    if let Some(handle) = bank.as_ref().and_then(|b| b.aim9[tone].clone()) {
        debug!("sidewinder {}", AIM9_FILES[tone]);
        commands.spawn((
            AudioPlayer(handle),
            PlaybackSettings {
                mode: PlaybackMode::Loop,
                volume: Volume::Linear(SEEKER_VOLUME),
                ..default()
            },
            SeekerTone(tone),
        ));
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

type FlightSound = Or<(
    With<OneShot>,
    With<EngineDrone>,
    With<HeliLoop>,
    With<Beacon>,
    With<FireSiren>,
    With<CannonLoop>,
    With<DistantAircraft>,
)>;

fn stop_all(mut commands: Commands, q: Query<Entity, FlightSound>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}
