//! Spawns the player's aircraft on the home runway, plus a temporary text readout of the
//! flight state (replaced by the world-space HUD in M3).

use bevy::math::DVec3;
use bevy::prelude::*;

use crate::camera::{View, ViewState};
use crate::flight::{Aircraft, FlightConstants, Player};
use crate::models::{ModelAssets, ModelLibrary};
use crate::settings::{AppState, MissionSetup, SetupSet};
use crate::world::{Theater, WorldPos, bevy_to_wu, map_to_wu, wu_to_bevy};

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            OnEnter(AppState::Flight),
            spawn_player.in_set(SetupSet::Spawn).run_if(|s: Res<MissionSetup>| !s.freecam),
        )
        .add_systems(
            Update,
            (readout, respawn_own_model, attach_own_model, own_model_visibility)
                .chain()
                .run_if(in_state(AppState::Flight)),
        )
        .add_systems(OnExit(AppState::Flight), despawn_readout);
    }
}

/// Distance from the base reference point to the runway start (original: 1800 wu).
const RUNWAY_OFFSET_WU: f64 = 1800.0;

#[allow(clippy::too_many_arguments)]
fn spawn_player(
    mut commands: Commands,
    theater: Res<Theater>,
    constants: Res<FlightConstants>,
    setup: Res<MissionSetup>,
    mut meshes: ResMut<Assets<Mesh>>,
    palette: Res<crate::palette::Palette>,
    xr_root: Res<crate::camera::XrRoot>,
    mission: Option<Res<crate::mission::ActiveMission>>,
    settings: Res<crate::settings::Settings>,
) {
    let base = setup
        .start_base
        .clone()
        .and_then(|name| {
            theater
                .world
                .objects
                .iter()
                .find(|o| theater.world.name(o.name).eq_ignore_ascii_case(&name))
        })
        .or_else(|| theater.world.objects.iter().find(|o| o.target_flags & 0x100 != 0));
    let (pos, heading) = match setup.start {
        // Carrier take-offs start on the deck (altitude 0x80).
        Some(s) => {
            (wu_to_bevy(s.east_wu, s.north_wu, if s.carrier { 128.0 } else { 0.0 }), s.heading)
        }
        None => {
            let (east, north) = base.map(|o| map_to_wu(o.x, o.y)).unwrap_or((524_288.0, 524_288.0));
            (wu_to_bevy(east + 2.0, north - RUNWAY_OFFSET_WU, 0.0), 0.0)
        }
    };
    let mut pos: DVec3 = pos;
    let mut aircraft_state = Aircraft::new_on_runway(constants.fuel);
    aircraft_state.altitude = (pos.y / crate::world::WU) as f32;
    aircraft_state.orientation = Aircraft::from_euler(heading, 0.0, 0.0);
    if let Some((km, alt, kts)) = start_final() {
        // Test aid (`--start-final km[,ft[,kts]]`): on final approach to the take-off runway,
        // gear and flaps down, `km` before the runway start.
        let (e, n, _) = bevy_to_wu(pos);
        let d = km as f64 * 1000.0 / crate::world::WU;
        let (e, n) = (e - heading.sin() as f64 * d, n - heading.cos() as f64 * d);
        pos = wu_to_bevy(e, n, crate::flight::view_z(alt) as f64);
        let a = &mut aircraft_state;
        a.altitude = alt;
        a.on_ground = false;
        a.has_flown = true;
        a.flaps = true;
        a.velocity = kts * 27.0;
        a.set_thrust = 50.0;
        a.thrust = 50.0;
    }
    if let Some((p, heading, alt, kts)) =
        mission.as_ref().and_then(|m| crate::stage::chase_start(&m.0))
    {
        // Test aid (`--start-chase`): in the air behind a drone, gear up.
        pos = p;
        let a = &mut aircraft_state;
        a.orientation = Aircraft::from_euler(heading, 0.0, 0.0);
        a.altitude = alt;
        a.on_ground = false;
        a.has_flown = true;
        a.gear_down = false;
        a.velocity = kts * 27.0;
        a.set_thrust = 60.0;
        a.thrust = 60.0;
    }
    let aircraft = commands
        .spawn((Player, aircraft_state, WorldPos(pos), Transform::default(), Visibility::default()))
        .id();
    crate::camera::spawn_rig(
        &mut commands,
        aircraft,
        &mut meshes,
        &palette,
        xr_root.0,
        settings.cockpit_eye(),
    );
    // Debug text readout of the flight state (opt-in, overlaps the panel).
    if !std::env::args().any(|a| a == "--readout") {
        return;
    }
    commands.spawn((
        Text::new(""),
        Readout,
        TextFont { font_size: bevy::text::FontSize::Px(16.0), ..default() },
        Node { position_type: PositionType::Absolute, bottom: px(8), left: px(8), ..default() },
    ));
}

/// `--start-final km[,alt ft[,knots]]` (defaults 3 km, 500 ft, 170 kt).
fn start_final() -> Option<(f32, f32, f32)> {
    let args: Vec<String> = std::env::args().collect();
    let v = args.iter().position(|a| a == "--start-final").and_then(|i| args.get(i + 1))?;
    let mut it = v.split(',').map(|x| x.parse::<f32>().ok());
    let km = it.next().flatten().unwrap_or(3.0);
    let alt = it.next().flatten().unwrap_or(500.0);
    let kts = it.next().flatten().unwrap_or(170.0);
    Some((km, alt, kts))
}

#[derive(Component)]
struct Readout;

fn despawn_readout(mut commands: Commands, q: Query<Entity, With<Readout>>) {
    for e in &q {
        commands.entity(e).try_despawn();
    }
}

fn readout(
    q: Query<(&Aircraft, &WorldPos), With<Player>>,
    mut text: Query<&mut Text, With<Readout>>,
) {
    let Ok(mut text) = text.single_mut() else { return };
    let Ok((a, pos)) = q.single() else { return };
    let (h, p, r) = a.euler();
    let (e, n, _) = bevy_to_wu(pos.0);
    text.0 = format!(
        "KTS {:4.0}  ALT {:5.0}  HDG {:03.0}  PITCH {:+5.1}  ROLL {:+6.1}  G {:.1}  THR {:3.0}/{:3.0}  FUEL {:5.0}  STALL {:3.0}\n\
         gear {} flaps {} brakes {} bay {} ap {} ground {} {}  pos {:.0},{:.0}",
        a.knots(),
        a.altitude,
        h.to_degrees(),
        p.to_degrees(),
        r.to_degrees(),
        a.gees / 16.0,
        a.thrust,
        a.set_thrust,
        a.fuel,
        a.stall_speed / 27.0,
        a.gear_down as u8,
        a.flaps as u8,
        a.brakes as u8,
        a.bay_open as u8,
        a.autopilot() as u8,
        a.on_ground as u8,
        a.ended.map(|e| format!("{e:?}")).unwrap_or_default(),
        e,
        n
    );
}

/// Aircraft models (STFLT.3DX) use 1/8 world unit per model unit (F-117 model: 288 units long
/// = 17.6 m). ASSUMED from model sizes; not confirmed.
pub const AIRCRAFT_MODEL_SCALE: f64 = crate::world::WU / 8.0;

#[derive(Component)]
pub struct OwnModel;

/// The own model as seen from the cockpit (`AircraftSpec::cockpit_view`).
#[derive(Component)]
pub struct OwnModelInterior;

/// The own model's lights as seen from the cockpit (inside `OwnModelInterior`), so they show
/// over the shoulder. Aircraft without a cockpit view show none: they would hang in the air.
#[derive(Component)]
pub struct OwnLights;

fn attach_own_model(
    mut commands: Commands,
    mut library: ResMut<ModelLibrary>,
    server: Res<AssetServer>,
    assets: ModelAssets,
    settings: Res<crate::settings::Settings>,
    player: Query<Entity, With<Player>>,
    existing: Query<(), With<OwnModel>>,
) {
    if !existing.is_empty() {
        return;
    }
    let Ok(player) = player.single() else { return };
    let spec = settings.aircraft.spec();
    let file = library.load(&server, "stflt");
    let transform = Transform::from_xyz(0.0, spec.model_lift, 0.0)
        .with_scale(Vec3::splat(AIRCRAFT_MODEL_SCALE as f32));
    let Some(model) = assets.spawn(
        &mut commands,
        &file,
        spec.model,
        crate::palette_material::LEVEL_AIRCRAFT,
        true,
    ) else {
        return;
    };
    commands.entity(model).insert((OwnModel, transform, Visibility::Hidden));
    commands.entity(player).add_child(model);
    let Some(view) = &spec.cockpit_view else { return };
    // The cut box in model units.
    let to_model =
        |p: Vec3| (spec.eye + p - Vec3::Y * spec.model_lift) / AIRCRAFT_MODEL_SCALE as f32;
    let (lo, hi) = (to_model(view.cut.0), to_model(view.cut.1));
    let inside = |v: Vec3| v.cmpge(lo).all() && v.cmple(hi).all();
    let interior = commands.spawn((OwnModelInterior, transform, Visibility::Hidden)).id();
    commands.entity(player).add_child(interior);
    if let Some(lights) = assets.spawn_lights(
        &mut commands,
        &file,
        spec.model,
        crate::palette_material::LEVEL_AIRCRAFT,
        true,
    ) {
        commands.entity(lights).insert((OwnLights, Transform::default(), Visibility::Inherited));
        commands.entity(interior).add_child(lights);
    }
    let tag = bevy::mesh::MeshTag(crate::palette_material::mesh_tag(
        crate::palette_material::LEVEL_AIRCRAFT,
        0,
        true,
    ));
    for (colour, tris) in assets.node_triangles(&file, &format!("{}_lod0", spec.model)) {
        if view.canopy_colours.contains(&colour) {
            continue;
        }
        let kept: Vec<[Vec3; 3]> = tris
            .into_iter()
            .filter(|t| !t.iter().any(|&v| inside(v)) && !inside((t[0] + t[1] + t[2]) / 3.0))
            .collect();
        if kept.is_empty() {
            continue;
        }
        let mesh = crate::models::triangle_mesh(&kept);
        let child = commands
            .spawn((
                MeshMaterial3d(assets.palette.material(colour)),
                tag.clone(),
                Transform::default(),
                Visibility::Inherited,
            ))
            .id();
        commands.entity(interior).add_child(child);
        commands.queue(move |world: &mut World| {
            let h = world.resource_mut::<Assets<Mesh>>().add(mesh);
            if let Ok(mut e) = world.get_entity_mut(child) {
                e.insert(Mesh3d(h));
            }
        });
    }
}

/// A campaign sortie in another aircraft than the last one: its model (and shadow) instead.
fn respawn_own_model(
    mut commands: Commands,
    settings: Res<crate::settings::Settings>,
    mut flown: Local<Option<crate::aircraft::AircraftKind>>,
    models: Query<
        Entity,
        Or<(With<OwnModel>, With<OwnModelInterior>, With<crate::sim_render::OwnShadow>)>,
    >,
) {
    let now = settings.aircraft;
    if flown.replace(now).is_some_and(|was| was != now) {
        for e in &models {
            commands.entity(e).try_despawn();
        }
    }
}

/// The own model shows in the external views; from the cockpit, its cockpit view if the
/// aircraft has one.
fn own_model_visibility(
    view: Res<ViewState>,
    hide: Option<Res<crate::camera::HideOwnAircraft>>,
    mut q: Query<&mut Visibility, (With<OwnModel>, Without<OwnModelInterior>)>,
    mut interior: Query<&mut Visibility, (With<OwnModelInterior>, Without<OwnModel>)>,
) {
    let external =
        !matches!(view.view, View::Cockpit | View::LookLeft | View::LookRight | View::LookRear);
    let hidden = hide.is_some_and(|h| h.0);
    let vis = |on: bool| if on && !hidden { Visibility::Inherited } else { Visibility::Hidden };
    for mut v in &mut q {
        v.set_if_neq(vis(external));
    }
    for mut v in &mut interior {
        v.set_if_neq(vis(!external));
    }
}
