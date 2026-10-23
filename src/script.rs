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
//! absolute throttle (`collective=0.6`: a helicopter's collective), `trigger=1` / `trigger=0` holds / releases the fire button (`gun=1` / `gun=0` the gun trigger), `mark=1` / `mark=0` the lock cursor
//! target button, `lockpilot=1` locks another player's aircraft (pilot id 1, as a display
//! lock does), `press=Gear` pushes the 3D-cockpit control bound to that action and `osb=r2`
//! pushes an MFD bezel button (`l` / `r` / `t`: the left, right or third MFD; 0..4 along the
//! top, 5..9 along the bottom). In a coop game `say=Hello there` sends a chat message and
//! `type=Hello` opens the chat field with that text (no commas in either).

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
            .add_systems(
                Update,
                run_script
                    .before(FlightSystems)
                    .run_if(in_state(crate::settings::AppState::Flight)),
            );
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
    /// Holds or releases the gun trigger (`gun=1`, `gun=0`).
    Gun(bool),
    /// Holds or releases "lock cursor target" (`mark=1`, `mark=0`).
    Mark(bool),
    /// Locks another player's aircraft by pilot id (`lockpilot=1`).
    LockPilot(u8),
    /// Pushes an MFD bezel button (`osb=r2`).
    Osb(crate::mfd::MfdSide, u8),
    /// Sends a chat message (`say=`), opens the chat field with this text (`type=`).
    Say(String),
    Type(String),
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
    gun: bool,
    mark: bool,
    lock_pilot: Option<u8>,
    osb: Option<(crate::mfd::MfdSide, u8)>,
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
    } else if let Some(x) = what.strip_prefix("throttle=").or(what.strip_prefix("collective=")) {
        Step::Throttle(x.parse().ok()?)
    } else if let Some(x) = what.strip_prefix("trigger=") {
        Step::Trigger(x.parse::<u8>().ok()? != 0)
    } else if let Some(x) = what.strip_prefix("gun=") {
        Step::Gun(x.parse::<u8>().ok()? != 0)
    } else if let Some(x) = what.strip_prefix("mark=") {
        Step::Mark(x.parse::<u8>().ok()? != 0)
    } else if let Some(x) = what.strip_prefix("lockpilot=") {
        Step::LockPilot(x.parse().ok()?)
    } else if let Some(x) = what.strip_prefix("osb=") {
        let side = match x.get(..1)? {
            "l" => crate::mfd::MfdSide::Left,
            "r" => crate::mfd::MfdSide::Right,
            "t" => crate::mfd::MfdSide::Third,
            _ => return None,
        };
        Step::Osb(side, x[1..].parse().ok()?)
    } else if let Some(x) = what.strip_prefix("say=") {
        Step::Say(x.to_string())
    } else if let Some(x) = what.strip_prefix("type=") {
        Step::Type(x.to_string())
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
    notices: &mut crate::player_notices::PlayerNotices,
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
        Step::Gun(t) => axes.gun = *t,
        Step::Mark(t) => axes.mark = *t,
        Step::LockPilot(k) => axes.lock_pilot = Some(*k),
        Step::Osb(side, slot) => axes.osb = Some((*side, *slot)),
        Step::Say(text) => notices.outbox.push(text.clone()),
        Step::Type(text) => notices.typing = Some(text.clone()),
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
    mut locks: MessageWriter<crate::avionics::LockRequest>,
    mut hud: ResMut<crate::hud::HudState>,
    mfd: Res<crate::mfd::MfdState>,
    mut mfd_cmds: MessageWriter<crate::mfd::MfdCommand>,
    mut notices: ResMut<crate::player_notices::PlayerNotices>,
) {
    if script.steps.is_empty() && script.conditional.is_empty() {
        return;
    }
    let Ok((a, pos)) = aircraft.single() else { return };
    let script = &mut *script;
    while script.next < script.steps.len() && script.steps[script.next].0 <= a.time {
        let i = script.next;
        script.next += 1;
        apply(
            &script.steps[i].1,
            &mut script.axes,
            &mut actions,
            &mut presses,
            &mut shot,
            &mut notices,
        );
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
                        && (u.wx as f64 - e).hypot((0x100000 - u.wy) as f64 - n) * crate::world::WU
                            / 1000.0
                            < km
                })
            }),
        };
        if hit {
            *fired = true;
            println!("script t={:.1}: condition met, {}", a.time, step_name(step));
            apply(step, &mut script.axes, &mut actions, &mut presses, &mut shot, &mut notices);
        }
    }
    if let Some(k) = script.axes.lock_pilot.take() {
        hud.mode = crate::hud::HudMode::Air;
        locks.write(crate::avionics::LockRequest(crate::mfd::ContactRef::Pilot(k)));
    }
    if let Some((side, slot)) = script.axes.osb.take() {
        match crate::mfd::osb(side, slot, &mfd).map(|o| o.1) {
            Some(crate::mfd::OsbFn::Cmd(c)) => {
                mfd_cmds.write(crate::mfd::MfdCommand(c));
            }
            Some(crate::mfd::OsbFn::Action(a)) => {
                actions.write(ActionEvent(a));
            }
            _ => warn!("script: no function on bezel button {side:?} {slot}"),
        }
    }
    controls.roll = script.axes.roll;
    controls.pitch = script.axes.pitch;
    controls.yaw = script.axes.yaw;
    controls.slew = script.axes.slew;
    controls.fire |= script.axes.trigger;
    controls.gun |= script.axes.gun;
    controls.mark |= script.axes.mark;
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
        Step::Gun(t) => format!("gun={}", *t as u8),
        Step::Mark(t) => format!("mark={}", *t as u8),
        Step::LockPilot(k) => format!("lockpilot={k}"),
        Step::Osb(side, slot) => format!("osb={side:?} {slot}"),
        Step::Say(t) => format!("say={t}"),
        Step::Type(t) => format!("type={t}"),
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
