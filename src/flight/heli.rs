//! Helicopter flight model (UH-60A): the main rotor's thrust along the mast, set by the
//! collective (the throttle controls), tilted by the cyclic (the stick); the pedals (rudder)
//! against the rotor's torque.
//!
//! - Thrust: linear in the collective, times rotor speed squared, with translational lift
//!   (more thrust once the rotor flies into clean air, 12..30 kt) and ground effect (within about
//!   one rotor diameter of the ground). The inflow damps the speed along the mast: climbs and
//!   descents, and at speed the disc tilted forward loses thrust (more collective to hold the
//!   altitude), tilted back gains it.
//! - Rotation: the cyclic and pedals command angular accelerations; a stability augmentation
//!   damps the rates but holds no attitude (released, the helicopter stays where it was left
//!   and drifts). Flapback pitches the nose up with forward speed and rolls away from sideways
//!   drift; the tail fin weathervanes the nose into the wind with speed, so turns coordinate.
//! - Speed: past about 140 kt the retreating blade stalls: thrust falls away (full collective no
//!   longer holds the altitude), the nose pitches up and rolls left.
//! - Torque: grows with the collective (less in translational lift); its reaction yaws the nose
//!   right (the rotor turns anticlockwise seen from above) beyond the tail rotor's rigging, so
//!   raising the collective needs left pedal. Past the torque limit the rotor speed droops. Out
//!   of fuel, the rotor keeps turning only in a descent with the collective down.
//! - Ground: wheels anywhere flat; touching down too hard, too fast or tilted is a crash (none
//!   with the "no crashes" landing skill). A helicopter needs no runway: the base it lands at is
//!   its whole airfield (`landing_corridor`).
//!
//! Units: feet (= world units), seconds; `vel` is in the Bevy frame (x east, y up, -z north),
//! `rates` in the body frame (x pitch up, y yaw left, z roll left; rad/s).

use bevy::prelude::*;

use super::{
    Aircraft, FlightConstants, FlightControls, FlightEnd, Theater, WorldPos, bevy_to_wu,
    ground_contact, stick_released, wu_to_bevy,
};

const G: f32 = 32.174;
/// ft/s per knot (`Aircraft::velocity` is knots * 27 = ft/s * 16).
const KNOT: f32 = 27.0 / 16.0;
/// Rotor revolutions per second drawn at 100% rotor speed (the AI helicopters' rate).
const DRAWN_REVS: f32 = 0.94;

/// Rotor and airframe constants.
#[derive(Debug, Clone, Copy)]
pub struct Rotor {
    /// Thrust to weight at flat pitch and its rise to full collective (out of ground effect,
    /// no wind, 100% rotor speed): hovers at about 0.58 collective.
    pub thrust_flat: f32,
    pub thrust_slope: f32,
    /// Translational lift: thrust gained, between these airspeeds (knots).
    pub etl_gain: f32,
    pub etl_knots: (f32, f32),
    /// Ground effect: thrust gained on the ground, fading out by this wheel height (wu, about
    /// one rotor diameter as drawn).
    pub ge_gain: f32,
    pub ge_height: f32,
    /// Damping of the speed along the mast (1/s): the climb and descent rates, and the thrust
    /// lost pitching forward at speed.
    pub heave_damping: f32,
    /// Angular accelerations at full cyclic, full pedal (rad/s^2).
    pub pitch_accel: f32,
    pub roll_accel: f32,
    pub pedal_accel: f32,
    /// Rate damping of the stability augmentation (1/s): the full cyclic rates are
    /// accel / damping.
    pub sas: Vec3,
    /// Flapback: nose-up per ft/s forward, roll away per ft/s sideways (rad/s^2).
    pub flapback: f32,
    pub dihedral: f32,
    /// Retreating blade stall: between these airspeeds (knots) the blade going back (on the
    /// left) stalls, losing this fraction of the thrust at the upper one, pitching the nose up
    /// and rolling left (rad/s^2 there). Past the lower one full collective no longer holds the
    /// altitude: the maximum level speed.
    pub stall_knots: (f32, f32),
    pub stall_loss: f32,
    pub stall_pitch: f32,
    pub stall_roll: f32,
    /// Torque reaction: yaw (nose right) per torque fraction beyond the tail rotor's rigging.
    pub torque_yaw: f32,
    pub torque_rigging: f32,
    /// Torque at flat pitch and its rise to full collective; the torque limit (fractions).
    pub torque_flat: f32,
    pub torque_slope: f32,
    pub torque_limit: f32,
    /// Tail fin: yaw towards sideways airflow per ft/s, at speed (rad/s^2).
    pub weathervane: f32,
    /// Drag: linear (1/s) and quadratic (1/ft) along the body axes (x side, z fore-aft).
    pub drag_linear: f32,
    pub drag: Vec3,
    /// Rotor speed response time (s).
    pub rotor_lag: f32,
    /// Fuel burnt at 100% torque (lb/s).
    pub fuel_burn: f32,
    /// Touchdown limits (easy landing skill; realistic takes 3/4): sink rate (ft/s), pitch or
    /// bank (rad), ground speed (kt).
    pub max_sink: f32,
    pub max_tilt: f32,
    pub max_knots: f32,
}

impl Rotor {
    pub const UH60: Rotor = Rotor {
        thrust_flat: 0.38,
        thrust_slope: 1.07,
        etl_gain: 0.15,
        etl_knots: (12.0, 30.0),
        ge_gain: 0.12,
        ge_height: 34.0,
        heave_damping: 0.3,
        pitch_accel: 2.2,
        roll_accel: 5.0,
        pedal_accel: 2.4,
        sas: Vec3::new(4.0, 2.2, 5.0),
        flapback: 0.002,
        dihedral: 0.004,
        stall_knots: (140.0, 190.0),
        stall_loss: 0.35,
        stall_pitch: 1.2,
        stall_roll: 1.5,
        torque_yaw: 1.2,
        torque_rigging: 0.55,
        torque_flat: 0.12,
        torque_slope: 0.88,
        torque_limit: 1.05,
        weathervane: 0.025,
        drag_linear: 0.01,
        drag: Vec3::new(4.0e-4, 0.0, 6.0e-5),
        rotor_lag: 1.5,
        fuel_burn: 0.35,
        max_sink: 15.0,
        max_tilt: 0.26,
        max_knots: 45.0,
    };

    /// AH-1Z: lighter and more powerful for its size, quicker on the cyclic and pedals, a
    /// slimmer fuselage and a stiffer four-blade rotor that stalls later (the level speed tops
    /// out near 180 kt).
    pub const AH1Z: Rotor = Rotor {
        thrust_flat: 0.40,
        thrust_slope: 1.12,
        ge_height: 30.0,
        pitch_accel: 3.0,
        roll_accel: 7.0,
        pedal_accel: 3.0,
        stall_knots: (170.0, 215.0),
        stall_loss: 0.4,
        torque_yaw: 1.0,
        drag: Vec3::new(3.0e-4, 0.0, 4.5e-5),
        rotor_lag: 1.2,
        fuel_burn: 0.3,
        ..Rotor::UH60
    };

    /// Collective that hovers out of ground effect, in still air.
    pub fn hover_collective(&self) -> f32 {
        (1.0 - self.thrust_flat) / self.thrust_slope
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct State {
    /// Velocity (ft/s, Bevy frame).
    vel: Vec3,
    /// Body rates: x pitch up, y yaw left, z roll left (rad/s).
    rates: Vec3,
    /// Rotor speed (1 = 100%).
    pub(super) nr: f32,
    /// Torque (1 = 100%).
    pub(super) torque: f32,
    /// Main rotor angle (rad), as drawn.
    pub(super) angle: f32,
    /// Translational lift 0..1, for the blade slap.
    pub(super) etl: f32,
    /// Auto trim: the pitch and bank (rad) the cyclic was released at.
    trim_hold: (Option<f32>, Option<f32>),
    init: bool,
}

impl State {
    pub(super) fn turning() -> Self {
        Self { nr: 1.0, ..default() }
    }

    pub(super) fn velocity(&self) -> Option<Vec3> {
        (self.init && self.vel.length_squared() > 1.0).then(|| self.vel.normalize())
    }
}

fn smoothstep(lo: f32, hi: f32, x: f32) -> f32 {
    let t = ((x - lo) / (hi - lo)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The collective (`set_thrust`, 0..100): an analog lever sets it, the held collective keys or
/// triggers move it.
fn collective(a: &mut Aircraft, controls: &FlightControls, dt: f32) -> f32 {
    if let Some(t) = controls.throttle {
        a.set_thrust = t.clamp(0.0, 1.0) * 100.0;
    } else {
        a.set_thrust =
            (a.set_thrust + controls.collective_rate * COLLECTIVE_RATE * dt).clamp(0.0, 100.0);
    }
    a.set_thrust / 100.0
}

/// Auto trim: trim (stick) per second per rad/s of pitch or roll rate, and per radian off the
/// attitude the cyclic was released at.
const TRIM_GAIN: f32 = 2.5;
const TRIM_HOLD_GAIN: f32 = 2.0;

/// Collective travel per second with the keys held (percent).
pub const COLLECTIVE_RATE: f32 = 40.0;

#[allow(clippy::too_many_arguments)]
pub(super) fn step(
    a: &mut Aircraft,
    pos: &mut WorldPos,
    tf: &mut Transform,
    dt: f32,
    controls: &FlightControls,
    k: &FlightConstants,
    r: &Rotor,
    theater: Option<&Theater>,
) {
    a.time += dt;
    if a.ended.is_some() {
        return;
    }
    if !a.heli.init {
        a.heli.init = true;
        a.heli.vel = a.orientation * Vec3::NEG_Z * (a.velocity / 16.0);
        if a.on_ground {
            a.heli.vel = Vec3::ZERO;
        } else {
            // Airborne start: the collective that holds the altitude.
            a.set_thrust = (r.hover_collective() * 100.0).round();
        }
    }
    let c = collective(a, controls, dt);

    // --- controls (damaged controls add their bias, jitter while flying)
    // Auto trim: with the cyclic released, the trim takes out the pitch and roll rates, so the
    // attitude stays where it was left (against flapback at speed).
    let (roll_free, pitch_free) = stick_released(controls);
    if a.auto_trim && !a.on_ground {
        let (_, p, b) = a.euler();
        let rates = a.heli.rates;
        let hold = &mut a.heli.trim_hold;
        let (p0, b0) = (*hold.0.get_or_insert(p), *hold.1.get_or_insert(b));
        let d = Vec2::new(
            if roll_free { TRIM_GAIN * rates.z - TRIM_HOLD_GAIN * (b - b0) } else { 0.0 },
            if pitch_free { -TRIM_GAIN * rates.x - TRIM_HOLD_GAIN * (p - p0) } else { 0.0 },
        );
        a.move_trim(d * dt);
    }
    if !roll_free || !a.auto_trim || a.on_ground {
        a.heli.trim_hold.1 = None;
    }
    if !pitch_free || !a.auto_trim || a.on_ground {
        a.heli.trim_hold.0 = None;
    }
    let (bias_roll, bias_pitch) = a.damage.control_bias(a.time);
    let mut roll = (controls.roll + bias_roll + a.stick_trim.x).clamp(-1.0, 1.0);
    let mut pitch = (controls.pitch + bias_pitch + a.stick_trim.y).clamp(-1.0, 1.0);
    let pedal = controls.yaw.clamp(-1.0, 1.0);
    if a.damage.stick_jitter > 0.0 && !a.on_ground {
        let j = a.damage.stick_jitter / 126.0;
        roll += rand::random_range(-j..j);
        pitch += rand::random_range(-j..j);
    }

    // --- air data
    let mut vel = a.heli.vel;
    let mut rates = a.heli.rates;
    let inv = a.orientation.inverse();
    let vb = inv * vel;
    let horizontal = Vec3::new(vel.x, 0.0, vel.z).length() / KNOT;
    let etl = smoothstep(r.etl_knots.0, r.etl_knots.1, horizontal);
    let height = (a.altitude - a.ground_altitude).max(0.0);
    let ge = 1.0 - (height / r.ge_height).clamp(0.0, 1.0);
    let ge = 1.0 + r.ge_gain * ge * ge;

    // --- engines and rotor speed
    let torque_need = (r.torque_flat + r.torque_slope * c) * (1.0 - 0.18 * etl)
        + 0.25 * smoothstep(110.0, 170.0, horizontal);
    let limit = a.damage.throttle_cap.map_or(r.torque_limit, |cap| r.torque_limit.min(cap / 100.0));
    let nr_target = if a.fuel <= 0.0 {
        // Engines out: the airflow up through a descending rotor keeps it turning, the
        // collective's pitch slows it.
        let up_flow = (-vb.y / 40.0).clamp(0.0, 1.2);
        (0.25 + 0.9 * up_flow - 0.6 * c).clamp(0.0, 1.05)
    } else if torque_need > limit {
        (1.0 - (torque_need - limit) * 1.5).max(0.6)
    } else {
        1.0
    };
    let st = &mut a.heli;
    st.nr += (nr_target - st.nr) * (dt / r.rotor_lag).min(1.0);
    st.torque = if a.fuel <= 0.0 { 0.0 } else { torque_need.min(limit) };
    st.etl = etl;
    let nr2 = st.nr * st.nr;
    st.angle = (st.angle + st.nr * DRAWN_REVS * std::f32::consts::TAU * dt)
        .rem_euclid(std::f32::consts::TAU);
    let torque = st.torque;

    // --- forces (accelerations, ft/s^2)
    let stall = smoothstep(r.stall_knots.0, r.stall_knots.1, vel.length() / KNOT);
    let thrust = (r.thrust_flat + r.thrust_slope * c).max(0.0)
        * nr2
        * (1.0 + r.etl_gain * etl)
        * ge
        * (1.0 - r.stall_loss * stall);
    let up = a.orientation * Vec3::Y;
    let mut accel_body = Vec3::Y * thrust * G;
    // Drag along the body's side and fore-aft axes.
    accel_body.x -= r.drag_linear * vb.x + r.drag.x * vb.x * vb.x.abs();
    accel_body.z -= r.drag_linear * vb.z + r.drag.z * vb.z * vb.z.abs();
    // The inflow: air flowing down through the disc costs thrust, up through it adds some. It
    // damps climbs and descents, and at speed the disc tilted forward meets the air from above:
    // pitching forward sinks unless the collective comes up (a flare lifts).
    accel_body.y -= r.heave_damping * vb.y;
    let mut accel = a.orientation * accel_body - Vec3::Y * G;

    // --- rotation
    let fin = smoothstep(15.0, 60.0, horizontal);
    let mut rate_dot = Vec3::new(
        r.pitch_accel * pitch * nr2 + r.flapback * (-vb.z) + r.stall_pitch * stall * nr2
            - r.sas.x * rates.x,
        -r.pedal_accel * pedal * nr2 - r.torque_yaw * (torque - r.torque_rigging) * nr2
            + r.weathervane * fin * (-vb.x)
            - r.sas.y * rates.y,
        -r.roll_accel * roll * nr2 + r.dihedral * vb.x + r.stall_roll * stall * nr2
            - r.sas.z * rates.z,
    );

    // --- ground: wheels on flat ground hold it level and stop it sliding
    if a.on_ground {
        let (_, p, b) = a.euler();
        // Weight on the wheels: what the rotor does not lift.
        let wow = (1.0 - thrust * up.y).clamp(0.0, 1.0);
        rate_dot.x += wow * (-60.0 * p - 12.0 * rates.x);
        rate_dot.z += wow * (60.0 * b - 12.0 * rates.z);
        // The wheels hold the heading against the torque; the pedals still turn it slowly.
        rate_dot.y = rate_dot.y * (1.0 - 0.8 * wow) - wow * 20.0 * rates.y;
        let flat = Vec3::new(vel.x, 0.0, vel.z);
        let fwd = (a.orientation * Vec3::NEG_Z).with_y(0.0).normalize_or_zero();
        let side = Vec3::new(-fwd.z, 0.0, fwd.x);
        let along = flat.dot(fwd);
        let across = flat.dot(side);
        let roll_friction = if a.brakes { 0.5 } else { 0.03 } * G * wow;
        let side_friction = 0.8 * G * wow;
        let stop = |v: f32, f: f32| if v.abs() <= f * dt { -v / dt } else { -f * v.signum() };
        accel += fwd * stop(along, roll_friction) + side * stop(across, side_friction);
        if accel.y < 0.0 && vel.y <= 0.0 {
            accel.y = 0.0;
        }
    }

    rates += rate_dot * dt;
    a.orientation = (a.orientation * Quat::from_scaled_axis(rates * dt)).normalize();
    vel += accel * dt;
    let (_, _, roll_now) = a.euler();
    a.gees = ((accel + Vec3::Y * G).dot(up) / G * 16.0).clamp(-64.0, 64.0);
    a.roll_input = roll * 126.0;
    a.pitch_input = pitch * 42.0;

    // --- fuel
    if a.fuel > 0.0 {
        let leak = if c > 0.0 { a.damage.fuel_leak / 2.0 } else { 0.0 };
        a.fuel = (a.fuel - (r.fuel_burn * torque + leak) * dt).max(0.0);
    }

    // --- integration and the ground
    let prev_alt = a.altitude;
    let was_on_ground = a.on_ground;
    a.altitude += vel.y * dt;
    a.climb_rate = vel.y;
    let (mut east, mut north, _) = bevy_to_wu(pos.0);
    east += (vel.x * dt) as f64;
    north += (-vel.z * dt) as f64;
    // Touchdown: judged before the ground contact rules see it.
    if a.altitude <= a.ground_altitude && prev_alt > a.ground_altitude && !was_on_ground {
        touchdown(a, k, r, vel, roll_now);
    }
    ground_contact(a, k, true, roll_now, prev_alt, east, north, theater, dt);
    if a.on_ground && vel.y < 0.0 {
        vel.y = 0.0;
    }
    a.heli.vel = vel;
    a.heli.rates = rates;
    let speed = vel.length();
    a.velocity = speed * 16.0;
    a.thrust = torque * 100.0;
    a.stall_speed = 0.0;
    a.fpm = 0.0;
    a.trim = 0.0;
    a.lift = 512.0;
    pos.0 = wu_to_bevy(east, north, a.view_z() as f64);
    tf.rotation = a.orientation;
}

/// A hard, fast or tilted touchdown wrecks the helicopter (not with the "no crashes" skill).
fn touchdown(a: &mut Aircraft, k: &FlightConstants, r: &Rotor, vel: Vec3, roll: f32) {
    if k.landing == 0 && !a.damage.fragile {
        return;
    }
    let f = if k.landing >= 2 { 0.75 } else { 1.0 };
    let sink = -vel.y;
    let knots = Vec3::new(vel.x, 0.0, vel.z).length() / KNOT;
    let (_, pitch, _) = a.euler();
    let tilt = pitch.abs().max(roll.abs());
    if sink > r.max_sink * f || tilt > r.max_tilt * f || knots > r.max_knots * f {
        a.ended = Some(FlightEnd::Crash);
        info!(
            "helicopter crash: sink {sink:.0} ft/s, tilt {:.0} deg, {knots:.0} kt",
            tilt.to_degrees()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Flies the UH-60A for `secs` (no theater: flat ground at 0, no bases).
    fn fly(
        a: Aircraft,
        secs: f32,
        landing: u8,
        ctl: impl FnMut(&Aircraft, &mut FlightControls),
    ) -> (Aircraft, Vec3) {
        fly_as(FlightConstants::uh60(), a, secs, landing, ctl)
    }

    fn fly_as(
        mut k: FlightConstants,
        mut a: Aircraft,
        secs: f32,
        landing: u8,
        mut ctl: impl FnMut(&Aircraft, &mut FlightControls),
    ) -> (Aircraft, Vec3) {
        k.landing = landing;
        let r = k.heli.unwrap();
        let mut pos = WorldPos(wu_to_bevy(0.0, 0.0, a.view_z() as f64));
        let mut tf = Transform::default();
        let mut c = FlightControls::default();
        let dt = 1.0 / 120.0;
        for _ in 0..(secs / dt) as usize {
            ctl(&a, &mut c);
            step(&mut a, &mut pos, &mut tf, dt, &c, &k, &r, None);
        }
        let (e, n, _) = bevy_to_wu(pos.0);
        let alt = a.altitude;
        (a, Vec3::new(e as f32, alt, n as f32))
    }

    fn hovering(alt: f32) -> Aircraft {
        let mut a = Aircraft::new_on_runway(2400.0);
        a.heli = State::turning();
        a.altitude = alt;
        a.on_ground = false;
        a.has_flown = true;
        a
    }

    /// Holds the heading with the pedals (a pilot's feet), the wings level with the cyclic.
    fn pilot(a: &Aircraft, c: &mut FlightControls, heading: f32) {
        let (h, p, b) = a.euler();
        let mut dh = h - heading;
        if dh > std::f32::consts::PI {
            dh -= std::f32::consts::TAU;
        }
        let rates = a.heli.rates;
        c.yaw = (-6.0 * dh + 1.0 * rates.y).clamp(-1.0, 1.0);
        c.roll = (-2.0 * b + 0.5 * rates.z).clamp(-1.0, 1.0);
        c.pitch = (-2.0 * p - 0.5 * rates.x).clamp(-1.0, 1.0);
    }

    #[test]
    fn hovers_at_the_hover_collective() {
        let hc = Rotor::UH60.hover_collective();
        let (a, p) = fly(hovering(200.0), 10.0, 1, |a, c| {
            c.throttle = Some(hc);
            pilot(a, c, 0.0);
        });
        println!(
            "hover: collective {hc:.2} alt {:.0} climb {:.1} drift {:.0}",
            a.altitude,
            a.climb_rate,
            p.xz().length()
        );
        assert!((0.5..0.66).contains(&hc));
        assert!(a.climb_rate.abs() < 2.0, "climb {}", a.climb_rate);
        assert!(a.knots() < 10.0, "{} kt", a.knots());
        assert!(a.heli.torque > 0.5 && a.heli.torque < 0.9, "torque {}", a.heli.torque);
    }

    #[test]
    fn ground_effect_and_translational_lift() {
        // Climb after 3 s at the hover collective: in ground effect (5 wu up) and out of it.
        let hc = Rotor::UH60.hover_collective();
        let climb = |alt: f32| {
            fly(hovering(alt), 3.0, 0, |a, c| {
                c.throttle = Some(hc);
                pilot(a, c, 0.0);
            })
            .0
            .climb_rate
        };
        let (low, high) = (climb(5.0), climb(300.0));
        println!("climb at the hover collective: 5 wu {low:.1}, 300 wu {high:.1}");
        assert!(low > high + 1.0);
        // At 40 kt the same collective climbs.
        let mut a0 = hovering(300.0);
        a0.velocity = 40.0 * 27.0;
        let (a, _) = fly(a0, 2.0, 1, |a, c| {
            c.throttle = Some(hc);
            pilot(a, c, 0.0);
        });
        println!("at 40 kt: climb {:.1}", a.climb_rate);
        assert!(a.climb_rate > high + 2.0);
    }

    #[test]
    fn torque_yaws_the_nose_right() {
        let hc = Rotor::UH60.hover_collective();
        let (a, _) = fly(hovering(300.0), 3.0, 1, |_, c| c.throttle = Some(hc.min(1.0) + 0.2));
        let (h, ..) = a.euler();
        println!("3 s without pedals, collective up: heading {:.0}", h.to_degrees());
        assert!(h.to_degrees() > 10.0 && h.to_degrees() < 180.0);
        // The pilot's feet hold it.
        let (a, _) = fly(hovering(300.0), 5.0, 1, |a, c| {
            c.throttle = Some(hc + 0.2);
            pilot(a, c, 0.0);
        });
        let (h, ..) = a.euler();
        let h = if h > std::f32::consts::PI { h - std::f32::consts::TAU } else { h };
        assert!(h.to_degrees().abs() < 5.0, "heading {}", h.to_degrees());
    }

    #[test]
    fn rates_damped_but_no_attitude_hold() {
        let hc = Rotor::UH60.hover_collective();
        let mut t = 0.0;
        let (a, _) = fly(hovering(500.0), 3.0, 1, |_, c| {
            t += 1.0 / 120.0;
            c.throttle = Some(hc);
            c.roll = if t < 0.5 { 0.6 } else { 0.0 };
        });
        let (_, _, roll) = a.euler();
        println!(
            "roll after a cyclic pulse {:.0} deg, rate {:.3}",
            roll.to_degrees(),
            a.heli.rates.z
        );
        assert!(a.heli.rates.z.abs() < 0.05);
        assert!(roll.to_degrees() > 5.0, "the bank stays");
    }

    #[test]
    fn forward_flight_speed() {
        // Nose down 10 degrees, the altitude held with the collective.
        let (a, _) = fly(hovering(500.0), 60.0, 1, |a, c| {
            pilot(a, c, 0.0);
            let (_, p, _) = a.euler();
            c.pitch = (-8.0 * (p + 0.17) - 1.0 * a.heli.rates.x).clamp(-1.0, 1.0);
            c.throttle =
                Some((0.6 + (500.0 - a.altitude) * 0.01 - a.climb_rate * 0.02).clamp(0.0, 1.0));
        });
        println!(
            "10 deg nose down: {:.0} kt alt {:.0} pitch {:.1} torque {:.2}",
            a.knots(),
            a.altitude,
            a.euler().1.to_degrees(),
            a.heli.torque
        );
        assert!((100.0..190.0).contains(&a.knots()), "{} kt", a.knots());
    }

    /// Holds a pitch attitude with the cyclic, the altitude with the collective if `hold`.
    fn cruise(a: Aircraft, secs: f32, pitch: f32, hold: Option<f32>, c0: f32) -> (Aircraft, f32) {
        let mut sum = 0.0;
        let mut n = 0.0;
        let (a, _) = fly(a, secs, 1, |a, c| {
            pilot(a, c, 0.0);
            let (_, p, _) = a.euler();
            c.pitch = (-8.0 * (p - pitch) - 1.0 * a.heli.rates.x).clamp(-1.0, 1.0);
            c.throttle = Some(match hold {
                Some(alt) => (c0 + (alt - a.altitude) * 0.01 - a.climb_rate * 0.03).clamp(0.0, 1.0),
                None => c0,
            });
            sum += c.throttle.unwrap();
            n += 1.0;
        });
        (a, sum / n)
    }

    #[test]
    fn pitching_forward_at_speed_takes_collective() {
        // Trimmed at about 80 kt, the altitude held.
        let mut a0 = hovering(1000.0);
        a0.velocity = 80.0 * 27.0;
        let (a, _) = cruise(a0, 30.0, -0.08, Some(1000.0), 0.6);
        let c0 = a.set_thrust / 100.0;
        let (a, trim) = cruise(a, 5.0, -0.08, Some(1000.0), c0);
        // Nose 8 degrees further down, the collective left: it sinks.
        let (sunk, _) = cruise(a.clone(), 4.0, -0.22, None, trim);
        // Held level: more collective.
        let (_, more) = cruise(a.clone(), 10.0, -0.22, Some(a.altitude), trim);
        println!(
            "{:.0} kt collective {trim:.2}; nose down: climb {:.1} ft/s, to hold {more:.2}",
            a.knots(),
            sunk.climb_rate
        );
        assert!(sunk.climb_rate < -8.0, "climb {}", sunk.climb_rate);
        assert!(more > trim + 0.04, "collective {trim} -> {more}");
    }

    /// Cyclic released in cruise: without the auto trim flapback raises the nose, with it the
    /// attitude stays where it was left.
    #[test]
    fn auto_trim_holds_the_attitude() {
        let mut a0 = hovering(2000.0);
        a0.velocity = 120.0 * 27.0;
        let (a, c0) = cruise(a0, 20.0, -0.12, Some(2000.0), 0.6);
        let start = a.euler().1;
        let released = |auto: bool| {
            let mut a = a.clone();
            a.auto_trim = auto;
            let mut highest = f32::MIN;
            let (a, _) = fly(a, 10.0, 1, |a, c| {
                c.pitch = 0.0;
                c.roll = 0.0;
                c.throttle = Some(c0);
                highest = highest.max(a.euler().1);
            });
            (highest, a)
        };
        let ((free, _), (trimmed, a)) = (released(false), released(true));
        println!(
            "{:.0} kt, pitch {:.1}: released, highest {:.1}; auto trim {:.1} (trim {:.2})",
            a.knots(),
            start.to_degrees(),
            free.to_degrees(),
            trimmed.to_degrees(),
            a.stick_trim.y
        );
        assert!(free > start + 0.2, "the nose rose without trim");
        assert!(trimmed < start + 0.06, "{} -> {}", start.to_degrees(), trimmed.to_degrees());
        assert!((a.euler().1 - start).abs() < 0.015, "pitch {}", a.euler().1.to_degrees());
        assert!(a.stick_trim.y < -0.05, "trimmed nose down: {}", a.stick_trim.y);
    }

    #[test]
    fn maximum_level_speed() {
        // Full collective, the nose held down: climbs at 150 kt, past 160 kt the retreating
        // blade stalls and it sinks.
        let full = |deg: f32| {
            let mut a0 = hovering(3000.0);
            a0.velocity = 100.0 * 27.0;
            cruise(a0, 60.0, -deg.to_radians(), None, 1.0).0
        };
        let (fast, faster) = (full(13.0), full(17.0));
        println!(
            "full collective: {:.0} kt climb {:.1}, {:.0} kt climb {:.1}",
            fast.knots(),
            fast.climb_rate,
            faster.knots(),
            faster.climb_rate
        );
        assert!(fast.knots() > 145.0 && fast.climb_rate > 5.0);
        assert!(faster.knots() > 160.0 && faster.climb_rate < -10.0);
    }

    #[test]
    fn lands_anywhere_flat() {
        let hc = Rotor::UH60.hover_collective();
        // Settles onto the ground at 4 ft/s, collective down on contact: stays on its wheels,
        // the flight goes on.
        let (a, _) = fly(hovering(20.0), 20.0, 1, |a, c| {
            let hold = hc - 0.04 + (-4.0 - a.climb_rate) * 0.05;
            c.throttle = Some(if a.on_ground { 0.0 } else { hold.clamp(0.0, 1.0) });
            pilot(a, c, 0.0);
        });
        assert!(a.on_ground && a.ended.is_none(), "{:?} ground {}", a.ended, a.on_ground);
        // Collective down on the ground: it stays there, level.
        let (a, _) = fly(a, 5.0, 1, |_, c| c.throttle = Some(0.0));
        let (_, p, b) = a.euler();
        assert!(a.on_ground && a.ended.is_none());
        assert!(p.abs() < 0.02 && b.abs() < 0.02, "pitch {p} roll {b}");
        // Lifts off again.
        let (a, _) = fly(a, 4.0, 1, |a, c| {
            c.throttle = Some(hc + 0.1);
            pilot(a, c, 0.0);
        });
        assert!(!a.on_ground && a.altitude > 5.0, "alt {}", a.altitude);
    }

    #[test]
    fn hard_landings_crash() {
        let cases = [
            ("sink", Vec3::new(0.0, -25.0, 0.0), 0.0),
            ("fast", Vec3::new(0.0, -2.0, -100.0), 0.0),
            ("bank", Vec3::new(0.0, -3.0, 0.0), 0.5),
        ];
        for (name, v, bank) in cases {
            for landing in [0, 1] {
                let mut a0 = hovering(0.5);
                a0.orientation = Aircraft::from_euler(0.0, 0.0, bank);
                a0.heli.vel = v;
                a0.heli.init = true;
                let (a, _) = fly(a0, 0.5, landing, |_, c| c.throttle = Some(0.3));
                let crashed = a.ended == Some(FlightEnd::Crash);
                assert_eq!(crashed, landing != 0, "{name} landing skill {landing}");
            }
        }
    }

    #[test]
    fn collective_keys_and_lever() {
        let mut a = hovering(500.0);
        a.set_thrust = 0.0;
        a.heli.init = true;
        let (a, _) = fly(a, 1.0, 1, |_, c| c.collective_rate = 1.0);
        assert!((a.set_thrust - COLLECTIVE_RATE).abs() < 1.0, "{}", a.set_thrust);
        let (a, _) = fly(a, 0.1, 1, |_, c| c.throttle = Some(0.7));
        assert!((a.set_thrust - 70.0).abs() < 0.01);
    }

    #[test]
    fn ah1z_hovers_and_outruns_the_uh60() {
        let r = Rotor::AH1Z;
        let hc = r.hover_collective();
        let (a, _) = fly_as(FlightConstants::ah1z(), hovering(200.0), 10.0, 1, |a, c| {
            c.throttle = Some(hc);
            pilot(a, c, 0.0);
        });
        assert!((0.45..0.6).contains(&hc), "hover collective {hc}");
        assert!(a.climb_rate.abs() < 2.0, "climb {}", a.climb_rate);
        // Full collective, the nose held down: level speed between the UH-60's and 200 kt.
        let full = |deg: f32| {
            let mut a0 = hovering(3000.0);
            a0.velocity = 120.0 * 27.0;
            fly_as(FlightConstants::ah1z(), a0, 60.0, 1, |a, c| {
                pilot(a, c, 0.0);
                let (_, p, _) = a.euler();
                c.pitch = (-8.0 * (p + deg.to_radians()) - 1.0 * a.heli.rates.x).clamp(-1.0, 1.0);
                c.throttle = Some(1.0);
            })
            .0
        };
        let (fast, faster) = (full(14.0), full(19.0));
        println!(
            "AH-1Z full collective: {:.0} kt climb {:.1}, {:.0} kt climb {:.1}",
            fast.knots(),
            fast.climb_rate,
            faster.knots(),
            faster.climb_rate
        );
        assert!(fast.knots() > 160.0 && fast.climb_rate > 0.0);
        assert!(faster.knots() < 230.0 && faster.climb_rate < 0.0);
    }
}
