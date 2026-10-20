//! Test automation: `--script "0:ThrottleMax,14:pitch=0.5,18:pitch=0,22:Gear"` replays actions
//! and stick inputs at the given seconds; `--log-flight` prints the flight state every second
//! (and the combat simulation's events).
//!
//! Conditional steps fire once when their condition first holds (after the time steps before
//! them in the list): `wp<1.5:FireWeapon` = within 1.5 km of the current INS waypoint
//! (`wp1<1.5`: of waypoint 1, the primary target),
//! `alt<300:pitch=0.3` / `alt>8000:Autopilot` = below / above an altitude in feet,
//! `msl<2:ViewChase` / `air<2:HudMode` = an enemy missile / an airborne aircraft within 2 km. The step `Screenshot` takes the
//! `--screenshot` picture at that moment. `yaw=0.5` sets the rudder, `throttle=0.8` holds an
//! absolute throttle, `trigger=1` / `trigger=0` holds / releases the fire button and `press=Gear` pushes the 3D-cockpit control bound to that action.

use bevy::prelude::*;

use crate::flight::{Aircraft, FlightSystems, Player};
use crate::input::{ActionEvent, FlightControls};

pub struct ScriptPlugin;

impl Plugin for ScriptPlugin {
    fn build(&self, app: &mut App) {
        let args: Vec<String> = std::env::args().collect();
        let get = |f: &str| args.iter().position(|a| a == f).and_then(|i| args.get(i + 1)).cloned();
        let (steps, conditional) = get("--script").map(|s| parse(&s)).unwrap_or_default();
        app.insert_resource(Script { steps, conditional, next: 0, axes: Axes::default() })
            .add_systems(Update, run_script.before(FlightSystems).run_if(in_state(crate::settings::AppState::Flight)));
        if args.iter().any(|a| a == "--log-flight") {
            app.add_systems(Update, log_flight.after(FlightSystems));
        }
    }
}

enum Step {
    Action(String),
    Roll(f32),
    Pitch(f32),
    Yaw(f32),
    /// Absolute throttle 0..1 (like the VR throttle lever).
    Throttle(f32),
    /// Sensor/cursor slew right and up (`slewx=`, `slewy=`).
    SlewX(f32),
    SlewY(f32),
    /// Presses a 3D-cockpit control bound to this action (`press=Gear`).
    Press(String),
    /// Holds (true) or releases the fire button (`trigger=1`, `trigger=0`).
    Trigger(bool),
}

#[derive(Clone, Copy)]
enum Cond {
    /// Distance to the current waypoint (None) or waypoint n below this many km.
    WaypointBelow(Option<usize>, f64),
    /// Altitude below / above this many feet.
    AltBelow(f32),
    AltAbove(f32),
    /// An enemy missile within this many km.
    MissileBelow(f64),
    /// An airborne aircraft within this many km.
    AircraftBelow(f64),
}

#[derive(Resource)]
struct Script {
    steps: Vec<(f32, Step)>,
    /// (earliest time, condition, step, fired)
    conditional: Vec<(f32, Cond, Step, bool)>,
    next: usize,
    axes: Axes,
}

/// Continuous controls held by the script.
#[derive(Default)]
struct Axes {
    roll: f32,
    pitch: f32,
    yaw: f32,
    throttle: Option<f32>,
    slew: Vec2,
    trigger: bool,
}

fn parse_step(what: &str) -> Option<Step> {
    Some(if let Some(x) = what.strip_prefix("roll=") {
        Step::Roll(x.parse().ok()?)
    } else if let Some(x) = what.strip_prefix("pitch=") {
        Step::Pitch(x.parse().ok()?)
    } else if let Some(x) = what.strip_prefix("yaw=") {
        Step::Yaw(x.parse().ok()?)
    } else if let Some(x) = what.strip_prefix("slewx=") {
        Step::SlewX(x.parse().ok()?)
    } else if let Some(x) = what.strip_prefix("slewy=") {
        Step::SlewY(x.parse().ok()?)
    } else if let Some(x) = what.strip_prefix("throttle=") {
        Step::Throttle(x.parse().ok()?)
    } else if let Some(x) = what.strip_prefix("trigger=") {
        Step::Trigger(x.parse::<u8>().ok()? != 0)
    } else if let Some(x) = what.strip_prefix("press=") {
        Step::Press(x.to_string())
    } else {
        Step::Action(what.to_string())
    })
}

#[allow(clippy::type_complexity)]
fn parse(s: &str) -> (Vec<(f32, Step)>, Vec<(f32, Cond, Step, bool)>) {
    let mut v = Vec::new();
    let mut conds = Vec::new();
    let mut last_t = 0.0f32;
    for item in s.split(',') {
        let Some((t, what)) = item.trim().split_once(':') else { continue };
        let Some(step) = parse_step(what) else {
            warn!("script: bad step {item}");
            continue;
        };
        if let Ok(t) = t.parse::<f32>() {
            last_t = t;
            v.push((t, step));
        } else if let Some((n, km)) = t.strip_prefix("wp").and_then(|x| x.split_once('<')) {
            let (Ok(km), n) = (km.parse(), n.parse().ok()) else {
                warn!("script: bad condition {t}");
                continue;
            };
            conds.push((last_t, Cond::WaypointBelow(n, km), step, false));
        } else if let Some(ft) = t.strip_prefix("alt<").and_then(|x| x.parse().ok()) {
            conds.push((last_t, Cond::AltBelow(ft), step, false));
        } else if let Some(km) = t.strip_prefix("air<").and_then(|x| x.parse().ok()) {
            conds.push((last_t, Cond::AircraftBelow(km), step, false));
        } else if let Some(km) = t.strip_prefix("msl<").and_then(|x| x.parse().ok()) {
            conds.push((last_t, Cond::MissileBelow(km), step, false));
        } else if let Some(ft) = t.strip_prefix("alt>").and_then(|x| x.parse().ok()) {
            conds.push((last_t, Cond::AltAbove(ft), step, false));
        } else {
            warn!("script: bad time {t}");
        }
    }
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    (v, conds)
}

fn apply(
    step: &Step,
    axes: &mut Axes,
    actions: &mut MessageWriter<ActionEvent>,
    presses: &mut MessageWriter<crate::cockpit3d::PressControl>,
    shot: &mut Option<ResMut<crate::autoshot::ShotRequest>>,
) {
    match step {
        Step::Action(name) if name == "Screenshot" => {
            if let Some(s) = shot {
                s.0 = true;
            }
        }
        Step::Roll(r) => axes.roll = *r,
        Step::Pitch(p) => axes.pitch = *p,
        Step::Yaw(y) => axes.yaw = *y,
        Step::Throttle(t) => axes.throttle = Some(*t),
        Step::SlewX(x) => axes.slew.x = *x,
        Step::SlewY(y) => axes.slew.y = *y,
        Step::Trigger(t) => axes.trigger = *t,
        Step::Press(name) => {
            if let Some(act) = crate::input::action_by_name(name) {
                presses.write(crate::cockpit3d::PressControl(act));
            } else {
                warn!("script: unknown action {name}");
            }
        }
        Step::Action(name) => {
            if let Some(act) = crate::input::action_by_name(name) {
                actions.write(ActionEvent(act));
            } else {
                warn!("script: unknown action {name}");
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_script(
    mut script: ResMut<Script>,
    aircraft: Query<(&Aircraft, &crate::world::WorldPos), With<Player>>,
    waypoints: Res<crate::hud::Waypoints>,
    combat: Option<Res<crate::combat::Combat>>,
    mut shot: Option<ResMut<crate::autoshot::ShotRequest>>,
    mut controls: ResMut<FlightControls>,
    mut actions: MessageWriter<ActionEvent>,
    mut presses: MessageWriter<crate::cockpit3d::PressControl>,
) {
    if script.steps.is_empty() && script.conditional.is_empty() {
        return;
    }
    let Ok((a, pos)) = aircraft.single() else { return };
    let script = &mut *script;
    while script.next < script.steps.len() && script.steps[script.next].0 <= a.time {
        let i = script.next;
        script.next += 1;
        apply(&script.steps[i].1, &mut script.axes, &mut actions, &mut presses, &mut shot);
    }
    let (e, n, _) = crate::world::bevy_to_wu(pos.0);
    let wp_km = |i: Option<usize>| {
        i.map_or(waypoints.target(), |i| waypoints.points.get(i).copied())
            .map(|w| (w.east - e).hypot(w.north - n) * crate::world::WU / 1000.0)
    };
    for (t, cond, step, fired) in script.conditional.iter_mut() {
        if *fired || a.time < *t {
            continue;
        }
        let hit = match *cond {
            Cond::WaypointBelow(i, km) => wp_km(i).is_some_and(|d| d < km),
            Cond::AltBelow(ft) => a.altitude < ft,
            Cond::AltAbove(ft) => a.altitude > ft,
            Cond::MissileBelow(km) => combat.as_ref().is_some_and(|c| {
                c.sim.projectiles()[..8].iter().any(|m| {
                    let (me, mn) = crate::combat::map_wu(m.x, m.y);
                    m.ttl != 0 && (me - e).hypot(mn - n) * crate::world::WU / 1000.0 < km
                })
            }),
            Cond::AircraftBelow(km) => combat.as_ref().is_some_and(|c| {
                c.sim.active_aircraft().iter().any(|(_, u, _)| {
                    u.flags & stealth_sim::fflags::AIRBORNE != 0
                        && (u.wx as f64 - e).hypot((0x100000 - u.wy) as f64 - n) * crate::world::WU / 1000.0 < km
                })
            }),
        };
        if hit {
            *fired = true;
            println!("script t={:.1}: condition met, {}", a.time, step_name(step));
            apply(step, &mut script.axes, &mut actions, &mut presses, &mut shot);
        }
    }
    controls.roll = script.axes.roll;
    controls.pitch = script.axes.pitch;
    controls.yaw = script.axes.yaw;
    controls.slew = script.axes.slew;
    controls.fire |= script.axes.trigger;
    if script.axes.throttle.is_some() {
        controls.throttle = script.axes.throttle;
    }
}

fn step_name(s: &Step) -> String {
    match s {
        Step::Action(a) => a.clone(),
        Step::Roll(r) => format!("roll={r}"),
        Step::Pitch(p) => format!("pitch={p}"),
        Step::Yaw(y) => format!("yaw={y}"),
        Step::Throttle(t) => format!("throttle={t}"),
        Step::SlewX(x) => format!("slewx={x}"),
        Step::SlewY(y) => format!("slewy={y}"),
        Step::Press(a) => format!("press={a}"),
        Step::Trigger(t) => format!("trigger={}", *t as u8),
    }
}

fn log_flight(q: Query<(&Aircraft, &crate::world::WorldPos), With<Player>>, mut last: Local<i32>) {
    let Ok((a, pos)) = q.single() else { return };
    let t = a.time.floor() as i32;
    if t != *last {
        *last = t;
        let (h, p, r) = a.euler();
        println!(
            "t={t:3} kts={:5.1} alt={:6.0} hdg={:5.1} pitch={:+5.1} roll={:+6.1} g={:.2} thr={:3.0} climb={:+6.0} stall={:5.1} ground={} map={:?} ended={:?}",
            a.knots(),
            a.altitude,
            h.to_degrees(),
            p.to_degrees(),
            r.to_degrees(),
            a.gees / 16.0,
            a.thrust,
            a.climb_rate,
            a.stall_speed / 27.0,
            a.on_ground as u8,
            crate::hud::map_units(pos),
            a.ended
        );
    }
}
