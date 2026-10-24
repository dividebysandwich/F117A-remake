//! Modern flight model: a point-mass aerodynamic model with a fly-by-wire
//! control system. The airframe constants come per aircraft type (`Aero`); the F-117A's are
//! tuned to the classic model's performance (about 560 kt at sea level, 120 kt clean stall, 7 g).
//!
//! - Forces: thrust along the nose (lapsing with air density), lift from the angle of attack
//!   (linear up to the stall, falling off past it), drag (parasite, induced, gear, flaps, speed
//!   brake, bay doors, transonic rise, the engine's ram drag at low throttle), side force from
//!   sideslip and gravity, on a mass that lightens as fuel burns.
//! - Control laws (rate command, attitude hold): the stick commands pitch rate (squared stick
//!   shaping), at most what gives 7 g pulling or -3 g pushing, and roll rate about the flight
//!   path. Released, the nose stays where it was left (the angle of attack then settles as the
//!   flight path catches up with it); once the path has settled, it is held, also in banks up
//!   to 60 degrees. The angle of attack and load factor limiters only act near the limits.
//!   Turns are coordinated (sideslip held at 0, or set by the rudder). The pitch rate follows its
//!   command like an airframe (second order, slightly underdamped), roll and yaw with short lags.
//! - Flaps extend over a few seconds and add lift and stall margin (a higher angle of attack
//!   limit), so the aircraft flies slower; they don't move the nose by themselves.
//! - Ground: rolling friction, wheel brakes, nosewheel steering, rotation above about 0.8 of the
//!   stall speed, carrier catapult and arresting wire; ground contact, landing and crash rules
//!   are the classic model's.
//!
//! Units: feet (= world units), seconds; `vel` is in the Bevy frame (x east, y up, -z north).

use bevy::prelude::*;

use super::{
    Aircraft, FlightConstants, FlightControls, Theater, WorldPos, bam, bam_diff, bevy_to_wu,
    engine, ground_contact, stick_released, to_bam, wu_to_bevy,
};

const G: f32 = 32.174;
/// Sea level air density (slug/ft^3).
const RHO0: f32 = 0.002_376_9;
/// Seconds the flaps take to extend or retract.
const FLAP_TIME: f32 = 3.0;
/// Dynamic pressure (psf) below which the full stick roll rate fades.
const ROLL_Q_FULL: f32 = 150.0;

/// Airframe constants of one aircraft type.
#[derive(Debug, Clone, Copy)]
pub struct Aero {
    /// Wing loading at the reference weight (lb/ft^2).
    pub wing_loading: f32,
    /// Weight without fuel and the reference weight (full internal fuel), lb.
    pub dry_weight: f32,
    pub ref_weight: f32,
    /// Static thrust to reference weight at full throttle, and its rise per Mach (ram effect).
    pub thrust_ratio: f32,
    pub thrust_mach: f32,
    /// Lift slope (1/rad) and stall angles of attack (rad).
    pub cl_alpha: f32,
    pub alpha_stall: f32,
    pub alpha_stall_neg: f32,
    /// Angle of attack limiter of the control system (rad).
    pub alpha_limit: f32,
    pub alpha_limit_neg: f32,
    /// Lift added by the fully extended flaps, and the stall / limiter angles they add (rad).
    pub cl_flaps: f32,
    pub alpha_flaps: f32,
    pub cd0: f32,
    /// Induced drag factor.
    pub cd_k: f32,
    pub cd_gear: f32,
    pub cd_flaps: f32,
    pub cd_speed_brake: f32,
    pub cd_bay: f32,
    /// Ram and spillage drag of the engine at idle, fading out as the throttle opens (none at
    /// full throttle), so the speed bleeds off with the throttle back.
    pub cd_idle: f32,
    /// Side force per radian of sideslip.
    pub cy_beta: f32,
    /// Transonic and supersonic drag rise.
    pub drag_rise: DragRise,
    /// Load factor limits (g).
    pub n_max: f32,
    pub n_min: f32,
    /// Full stick roll rate (rad/s).
    pub roll_rate: f32,
    /// Full stick pitch rate (rad/s) where the load factor doesn't limit it, pushing a fraction.
    pub pitch_rate_max: f32,
    pub push_rate: f32,
    /// Yaw rate limit (rad/s).
    pub yaw_rate_max: f32,
}

/// Drag added near and above the speed of sound.
#[derive(Debug, Clone, Copy)]
pub enum DragRise {
    /// Subsonic airframe: a steep rise from Mach 0.85 (the F-117A's limit).
    Subsonic,
    /// Supersonic airframe: the rise from `start` peaks by `peak_mach` and eases off above it.
    Supersonic { start: f32, peak_mach: f32, peak: f32, supersonic: f32 },
}

impl DragRise {
    fn cd(self, mach: f32) -> f32 {
        match self {
            DragRise::Subsonic => {
                if mach > 0.85 {
                    0.6 * (mach - 0.85).powi(2) + 4.0 * (mach - 0.95).max(0.0).powi(2)
                } else {
                    0.0
                }
            }
            DragRise::Supersonic { start, peak_mach, peak, supersonic } => {
                let smooth = |t: f32| {
                    let t = t.clamp(0.0, 1.0);
                    t * t * (3.0 - 2.0 * t)
                };
                if mach <= peak_mach {
                    peak * smooth((mach - start) / (peak_mach - start))
                } else {
                    peak + (supersonic - peak) * smooth((mach - peak_mach) / 0.5)
                }
            }
        }
    }
}

impl Aero {
    /// F-117A: about 560 kt at sea level, 120 kt clean stall, 7 g.
    pub const F117: Aero = Aero {
        wing_loading: 58.5,
        dry_weight: 37_000.0,
        ref_weight: 47_000.0,
        thrust_ratio: 0.8,
        thrust_mach: 0.0,
        cl_alpha: 3.0,
        alpha_stall: 0.40,
        alpha_stall_neg: -0.28,
        alpha_limit: 0.36,
        alpha_limit_neg: -0.22,
        cl_flaps: 0.45,
        alpha_flaps: 0.05,
        cd0: 0.040,
        cd_k: 0.16,
        cd_gear: 0.025,
        cd_flaps: 0.05,
        cd_speed_brake: 0.12,
        cd_bay: 0.008,
        cd_idle: 0.028,
        cy_beta: 0.8,
        drag_rise: DragRise::Subsonic,
        n_max: 7.0,
        n_min: -3.0,
        roll_rate: 1.75,
        pitch_rate_max: 0.5,
        push_rate: 0.6,
        yaw_rate_max: 0.5,
    };

    /// F-16C: Mach 1.2 at sea level, about Mach 1.9 at altitude, 9 g, a 240 deg/s roll rate.
    pub const F16: Aero = Aero {
        wing_loading: 90.0,
        dry_weight: 19_000.0,
        ref_weight: 29_000.0,
        thrust_ratio: 1.07,
        thrust_mach: 0.1,
        cl_alpha: 4.0,
        alpha_stall: 0.50,
        alpha_stall_neg: -0.30,
        alpha_limit: 0.44,
        alpha_limit_neg: -0.25,
        cl_flaps: 0.35,
        alpha_flaps: 0.04,
        cd0: 0.021,
        cd_k: 0.12,
        cd_gear: 0.02,
        cd_flaps: 0.04,
        cd_speed_brake: 0.10,
        cd_bay: 0.0,
        cd_idle: 0.022,
        cy_beta: 0.8,
        drag_rise: DragRise::Supersonic {
            start: 0.85,
            peak_mach: 1.1,
            peak: 0.036,
            supersonic: 0.0245,
        },
        n_max: 9.0,
        n_min: -3.0,
        roll_rate: 4.2,
        pitch_rate_max: 0.6,
        push_rate: 0.6,
        yaw_rate_max: 0.6,
    };
}

/// Full rudder sideslip (rad).
const BETA_RUDDER: f32 = 0.1;
/// Lags (s): roll rate, yaw rate, sideslip correction, the limiters' angle of attack correction.
const TAU_ROLL: f32 = 0.12;
const TAU_RATE: f32 = 0.05;
const TAU_BETA: f32 = 0.3;
const TAU_LIMIT: f32 = 0.25;
/// Pitch rate response: natural frequency (rad/s) and damping.
const PITCH_OMEGA: f32 = 5.0;
const PITCH_ZETA: f32 = 0.7;
/// Stick deflection (after shaping) below which the stick counts as released.
const STICK_FREE: f32 = 0.003;
/// Flight path hold: earliest and latest seconds after the release it starts, the path rate
/// (rad/s) below which it may start, and its gain (1/s).
const HOLD_AFTER: f32 = 1.5;
const HOLD_LATEST: f32 = 4.0;
const HOLD_SETTLED: f32 = 0.02;
const HOLD_GAIN: f32 = 0.5;
/// Auto trim: trim (stick) per second per radian of flight path error held, and the share of
/// the roll bias trimmed out per second with the stick released.
const TRIM_PATH_GAIN: f32 = 0.5;
const TRIM_ROLL_GAIN: f32 = 2.0;

/// Ground: rolling friction, wheel brakes (g), nose-up rotation rate (rad/s), tail strike pitch.
const ROLL_FRICTION: f32 = 0.02;
const WHEEL_BRAKES: f32 = 1.0;
const ROTATE_RATE: f32 = 0.15;
const MAX_GROUND_PITCH: f32 = 0.22;
/// Carrier catapult and arresting wire (g).
const CATAPULT: f32 = 2.5;

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct State {
    /// Velocity (ft/s, Bevy frame).
    vel: Vec3,
    /// Roll rate about the flight path, body pitch rate, body yaw rate (right), rad/s.
    rates: Vec3,
    /// `vel` has been set up from the scalar speed.
    init: bool,
    /// Pitch acceleration of the second-order pitch rate response (rad/s^2).
    q_dot: f32,
    /// Flap extension 0..1.
    flap_pos: f32,
    /// Seconds since the stick was released (0 while it is moved).
    released: f32,
    /// Flight path angle held with the stick released (rad).
    hold: Option<f32>,
}

impl State {
    /// Direction of the velocity once the model runs and the aircraft moves.
    pub(super) fn velocity(&self) -> Option<Vec3> {
        (self.init && self.vel.length_squared() > 1.0).then(|| self.vel.normalize())
    }
}

/// (density ratio, speed of sound ft/s) at an altitude in feet.
fn atmosphere(h: f32) -> (f32, f32) {
    let h = h.clamp(0.0, 60000.0);
    if h < 36089.0 {
        let theta = 1.0 - 6.875_6e-6 * h;
        (theta.powf(4.2559), 1116.4 * theta.sqrt())
    } else {
        (0.2971 * (-(h - 36089.0) / 20806.0).exp(), 1116.4 * 0.7519f32.sqrt())
    }
}

fn cl_zero(ae: &Aero, a: &Aircraft) -> f32 {
    ae.cl_flaps * a.modern.flap_pos
}

fn alpha_stall(ae: &Aero, a: &Aircraft) -> f32 {
    let s = ae.alpha_stall + ae.alpha_flaps * a.modern.flap_pos;
    if a.damage.heavy_stall { s - 0.06 } else { s }
}

fn alpha_limit(ae: &Aero, a: &Aircraft) -> f32 {
    let l = ae.alpha_limit + ae.alpha_flaps * a.modern.flap_pos;
    if a.damage.heavy_stall { l - 0.06 } else { l }
}

/// Lift coefficient: linear to the stall, then falling off to 60% of the peak.
fn lift_coefficient(ae: &Aero, a: &Aircraft, alpha: f32) -> f32 {
    let c0 = cl_zero(ae, a);
    let (hi, lo) = (alpha_stall(ae, a), ae.alpha_stall_neg);
    let peak = |s: f32| c0 + ae.cl_alpha * s;
    if alpha > hi {
        let p = peak(hi);
        (p - (alpha - hi) * 2.0).max(0.6 * p)
    } else if alpha < lo {
        let p = peak(lo);
        (p - (alpha - lo) * 2.0).min(0.6 * p)
    } else {
        peak(alpha)
    }
}

/// Load factor that holds the flight path: up to 60 degrees of bank, fading to 0 at 90 degrees,
/// -1 g inverted.
fn neutral_load(gamma: f32, roll: f32) -> f32 {
    let c = roll.cos();
    let f = if c >= 0.5 {
        1.0 / c
    } else if c >= 0.0 {
        4.0 * c
    } else {
        c
    };
    gamma.cos() * f
}

pub(super) fn step(
    a: &mut Aircraft,
    pos: &mut WorldPos,
    tf: &mut Transform,
    dt: f32,
    controls: &FlightControls,
    k: &FlightConstants,
    theater: Option<&Theater>,
) {
    let ae = &k.aero;
    a.time += dt;
    if a.ended.is_some() {
        return;
    }
    if !a.modern.init {
        a.modern.init = true;
        a.modern.flap_pos = if a.flaps { 1.0 } else { 0.0 };
        let speed = a.velocity / 16.0;
        a.modern.vel = a.orientation * Vec3::NEG_Z * speed;
        if a.on_ground {
            a.modern.vel.y = 0.0;
        } else {
            // Airborne start: trimmed, the nose raised above the flight path by the 1 g angle of
            // attack, and the path held.
            let (sigma, _) = atmosphere(a.altitude);
            let per_cl = 0.5 * RHO0 * sigma * speed * speed
                / (ae.wing_loading * (ae.dry_weight + a.fuel) / ae.ref_weight);
            if per_cl > 1e-3 {
                let (_, _, roll) = a.euler();
                let n0 =
                    neutral_load(a.modern.vel.normalize_or_zero().y.clamp(-1.0, 1.0).asin(), roll)
                        .max(0.0);
                let alpha =
                    ((n0 / per_cl - cl_zero(ae, a)) / ae.cl_alpha).clamp(0.0, alpha_limit(ae, a));
                a.orientation = (a.orientation * Quat::from_rotation_x(alpha)).normalize();
            }
            a.modern.hold = Some(a.modern.vel.normalize_or_zero().y.clamp(-1.0, 1.0).asin());
            a.modern.released = HOLD_LATEST;
        }
    }
    let (heading, pitch, roll) = a.euler();
    let air = !a.on_ground;
    let flaps = if a.flaps { 1.0 } else { 0.0 };
    let f = &mut a.modern.flap_pos;
    *f = if *f < flaps {
        (*f + dt / FLAP_TIME).min(flaps)
    } else {
        (*f - dt / FLAP_TIME).max(flaps)
    };

    // --- controls
    let roll_stick = controls.roll.clamp(-1.0, 1.0);
    let pitch_stick = controls.pitch.clamp(-1.0, 1.0);
    let rudder = controls.yaw.clamp(-1.0, 1.0);
    if roll_stick.abs() > 0.06 || pitch_stick.abs() > 0.06 || a.damage.autopilot_failed {
        a.autopilot_alt = 0.0;
    }
    // Squared stick shaping: fine control around the centre, full authority at the stops.
    let mut roll_stick = roll_stick * roll_stick.abs();
    let mut pitch_stick = pitch_stick * pitch_stick.abs();

    // Turbulence (the classic model's strength, in stick units): low level, gear down, damage.
    let vz = a.view_z();
    let mut turb =
        if k.landing > 0 { ((500.0 - vz) * a.knots() / 8192.0).floor().max(0.0) } else { 0.0 };
    if a.gear_down {
        turb += ((a.knots() - 200.0) / 16.0).clamp(0.0, 32.0);
    }
    if a.flaps {
        turb = (turb / 2.0).floor();
    }
    turb += a.damage.stick_jitter;
    if turb > 0.0 && air {
        roll_stick += (rand::random_range(0.0..turb) - turb / 2.0) / 126.0;
        pitch_stick += (rand::random_range(0.0..turb) - turb / 2.0) / 2.0 / 42.0;
    }

    engine(a, controls, k, dt);

    // --- air data
    let mut vel = a.modern.vel;
    let mut rates = a.modern.rates;
    let speed = vel.length();
    let vhat = if speed > 1.0 { vel / speed } else { a.orientation * Vec3::NEG_Z };
    let (sigma, sound) = atmosphere(a.altitude);
    let qbar = 0.5 * RHO0 * sigma * speed * speed;
    let mass = (ae.dry_weight + a.fuel) / ae.ref_weight;
    let fwd = a.orientation * Vec3::NEG_Z;
    let up = a.orientation * Vec3::Y;
    let right = a.orientation * Vec3::X;
    let vb = a.orientation.inverse() * vel;
    let (alpha, beta) =
        if speed > 1.0 { ((-vb.y).atan2(-vb.z), vb.x.atan2(-vb.z)) } else { (pitch, 0.0) };
    let gamma = vhat.y.clamp(-1.0, 1.0).asin();
    let mach = speed / sound;

    // --- aerodynamic and thrust accelerations (g per unit of q * CL / wing loading)
    let per_cl = qbar / (ae.wing_loading * mass);
    let cl = lift_coefficient(ae, a, alpha);
    let mut cd = ae.cd0 + ae.cd_k * cl * cl;
    if a.gear_down {
        cd += ae.cd_gear;
    }
    cd += ae.cd_flaps * a.modern.flap_pos;
    if a.brakes && air {
        cd += ae.cd_speed_brake;
    }
    if a.bay_open {
        cd += ae.cd_bay;
    }
    if alpha > alpha_stall(ae, a) {
        cd += (alpha - alpha_stall(ae, a)) * 1.5;
    }
    cd += ae.drag_rise.cd(mach);
    cd += ae.cd_idle * (1.0 - a.thrust / 100.0).clamp(0.0, 1.0).powi(2);
    let thrust = ae.thrust_ratio * (1.0 + ae.thrust_mach * mach.min(2.0)) * a.thrust / 100.0
        * sigma.powf(0.6)
        / mass;
    let lift_dir = (up - vhat * up.dot(vhat)).normalize_or(up);
    let side_dir = (right - vhat * right.dot(vhat)).normalize_or(right);
    let n_lift = per_cl * cl;
    let mut accel = (fwd * thrust + lift_dir * n_lift
        - vhat * per_cl * cd
        - side_dir * per_cl * ae.cy_beta * beta)
        * G;
    accel.y -= G;

    // Stall speed at the current load (velocity units) for the warnings.
    let cl_avail = cl_zero(ae, a) + ae.cl_alpha * alpha_limit(ae, a);
    let load = (a.gees / 16.0).max(1.0);
    a.stall_speed = (2.0 * load * ae.wing_loading * mass / (RHO0 * sigma * cl_avail)).sqrt() * 16.0;

    if air {
        // --- control laws
        let roll_b = to_bam(roll);
        // Rates that keep the nose with the turning flight path (the angle of attack constant).
        let a_perp = accel - vhat * accel.dot(vhat);
        let path_rate = if speed > 1.0 { vhat.cross(a_perp) / speed } else { Vec3::ZERO };
        let q_path = path_rate.dot(right);
        let n0 = neutral_load(gamma, roll);
        let v = speed.max(50.0);
        // Turn compensation: the body pitch rate of a level turn at this bank (nose held up in
        // banks to 60 degrees, fading out towards 90).
        let q_turn = (n0 - gamma.cos() * roll.cos()).max(0.0) * G / v;
        // Pilot: pitch rate, at most what gives the load factor limits.
        let q_stick = if pitch_stick >= 0.0 {
            pitch_stick * ae.pitch_rate_max.min((ae.n_max - n0).max(0.0) * G / v)
        } else {
            pitch_stick * (ae.pitch_rate_max * ae.push_rate).min((n0 - ae.n_min).max(0.0) * G / v)
        };
        let free = pitch_stick.abs() < STICK_FREE && !a.autopilot();
        let st = &mut a.modern;
        st.released = if free { st.released + dt } else { 0.0 };
        // Flight path hold once the path has caught up with the nose (wings within 60 degrees).
        let gamma_rate = path_rate.dot(right) * roll.cos();
        if !free || roll.cos() < 0.5 {
            st.hold = None;
        } else if st.hold.is_none()
            && st.released > HOLD_AFTER
            && (gamma_rate.abs() < HOLD_SETTLED || st.released > HOLD_LATEST)
        {
            st.hold = Some(gamma);
        }
        let mut q_cmd =
            q_turn + q_stick + st.hold.map_or(0.0, |g0| (g0 - gamma) * HOLD_GAIN * roll.cos());
        let mut p_cmd = roll_stick * ae.roll_rate * (qbar / ROLL_Q_FULL).clamp(0.25, 1.0);
        // Damaged control surfaces: out of trim or flapping, whatever the fly-by-wire wants;
        // the trim adds to them.
        let (roll_fault, pitch_fault) = a.damage.control_bias(a.time);
        let (roll_fault, pitch_fault) = (roll_fault + a.stick_trim.x, pitch_fault + a.stick_trim.y);
        // Auto trim: the held flight path's error is trimmed out (what the path hold alone
        // leaves against a pitch bias), and with the stick released the roll bias.
        if a.auto_trim && !a.autopilot() {
            let pitch_trim = a.modern.hold.map_or(0.0, |g0| (g0 - gamma) * TRIM_PATH_GAIN);
            let roll_free = stick_released(controls).0;
            let roll_trim = if roll_free { -roll_fault * TRIM_ROLL_GAIN } else { 0.0 };
            a.move_trim(Vec2::new(roll_trim, pitch_trim) * dt);
        }
        if a.autopilot() {
            // Bank toward the waypoint bearing (at most 30 degrees), hold the altitude.
            let e = bam(bam_diff(a.waypoint_bearing, to_bam(heading)));
            let bank = (e * 1.5).clamp(-0.52, 0.52);
            p_cmd = ((bank - roll) * 2.0).clamp(-0.5, 0.5);
            let gamma_cmd = ((a.autopilot_alt - vz) * 0.0005).clamp(-0.17, 0.17);
            q_cmd = q_turn + ((gamma_cmd - gamma) * 0.8).clamp(-0.5 * G / v, 2.0 * G / v);
        }
        // Crash protection: wings level and pull when the ground is less than 3 s away.
        if k.landing == 0
            && !a.gear_down
            && vel.y < 0.0
            && a.altitude - a.ground_altitude + vel.y * 3.0 < 150.0
            && roll_b.abs() < 24576.0
        {
            p_cmd = (-roll * 3.0).clamp(-ae.roll_rate, ae.roll_rate);
            q_cmd = q_cmd.max(q_path + (4.0 - n0).max(0.0) * G / v);
            a.modern.hold = None;
        }
        q_cmd += pitch_fault * ae.pitch_rate_max;
        // Limiters: the angle of attack stays within the limiter angles and the angles that give
        // the load factor limits (corrected towards them near the limits only).
        let lo_n = if per_cl > 1e-3 {
            (ae.n_min / per_cl - cl_zero(ae, a)) / ae.cl_alpha
        } else {
            ae.alpha_limit_neg
        };
        let hi_n = if per_cl > 1e-3 {
            (ae.n_max / per_cl - cl_zero(ae, a)) / ae.cl_alpha
        } else {
            alpha_limit(ae, a)
        };
        let hi = alpha_limit(ae, a).min(hi_n);
        let lo = ae.alpha_limit_neg.max(lo_n).min(hi);
        q_cmd = q_cmd.min(q_path + (hi - alpha) / TAU_LIMIT).max(q_path + (lo - alpha) / TAU_LIMIT);
        // Past the stall the nose drops.
        if alpha > alpha_stall(ae, a) {
            q_cmd -= (alpha - alpha_stall(ae, a)) * 4.0;
        }
        p_cmd += roll_fault * ae.roll_rate * (qbar / ROLL_Q_FULL).clamp(0.25, 1.0);
        let r_cmd = (-path_rate.dot(up) + (beta + rudder * BETA_RUDDER) / TAU_BETA)
            .clamp(-ae.yaw_rate_max, ae.yaw_rate_max);
        // Pitch rate: second-order response to its command.
        let st = &mut a.modern;
        st.q_dot += (PITCH_OMEGA * PITCH_OMEGA * (q_cmd - rates.y)
            - 2.0 * PITCH_ZETA * PITCH_OMEGA * st.q_dot)
            * dt;
        rates.y += st.q_dot * dt;
        let r = &mut rates;
        r.x += (p_cmd - r.x) * (dt / TAU_ROLL).min(1.0);
        r.z += (r_cmd - r.z) * (dt / TAU_RATE).min(1.0);
        let omega = vhat * r.x + right * r.y - up * r.z;
        a.orientation = (Quat::from_scaled_axis(omega * dt) * a.orientation).normalize();
        vel += accel * dt;
        a.gees = ((accel + Vec3::Y * G).dot(up) / G * 16.0).clamp(-k.max_gees, k.max_gees);
        a.roll_input = r.x / ae.roll_rate * 126.0;
        a.pitch_input = pitch_stick * 42.0;
    } else {
        // --- ground roll along the nose, lift unloading the wheels
        let flat = Vec3::new(heading.sin(), 0.0, -heading.cos());
        let mut along = vel.dot(flat);
        let weight_on_wheels = (1.0 - n_lift * pitch.cos()).max(0.0);
        let mut decel = ROLL_FRICTION * weight_on_wheels;
        if a.brakes {
            decel += WHEEL_BRAKES * weight_on_wheels;
        }
        // Carrier deck: the catapult launches at full throttle, the wire stops a landing.
        if a.ground_altitude > 0.0 {
            if !a.has_flown && a.thrust >= 90.0 {
                decel -= CATAPULT;
            } else if a.has_flown && a.thrust <= 50.0 {
                decel += CATAPULT;
            }
        }
        let drive = thrust * pitch.cos() - per_cl * cd;
        along += (drive * G - decel * G * along.signum()) * dt;
        if along < 0.5 && drive <= decel {
            along = along.max(0.0) * 0.5;
        }
        along = along.max(0.0);
        // Rotation above 0.8 of the 1 g stall speed, nose down again when released.
        let vs = a.stall_speed / 16.0 / load.sqrt();
        let mut new_pitch = pitch;
        let pull = controls.pitch.clamp(-1.0, 1.0);
        if along > 0.8 * vs && pull > 0.05 {
            new_pitch += pull * ROTATE_RATE * dt;
        } else {
            new_pitch -= ROTATE_RATE * 0.7 * dt;
        }
        let new_pitch = new_pitch.clamp(0.0, MAX_GROUND_PITCH);
        // Nosewheel steering by the rudder, else the stick; less authority with speed.
        let steer = if rudder != 0.0 { rudder } else { roll_stick };
        let yaw_rate = steer * (0.6 * (1.0 - along / 250.0)).clamp(0.06, 0.6);
        a.orientation = Aircraft::from_euler(heading + yaw_rate * dt, new_pitch, 0.0);
        // Lift off once the lift exceeds the weight.
        let climb = ((n_lift * new_pitch.cos() - 1.0) * G).max(0.0) * dt;
        vel = flat * along + Vec3::Y * climb;
        rates = Vec3::ZERO;
        a.modern.q_dot = 0.0;
        a.modern.released = 0.0;
        a.modern.hold = None;
        a.gees = 16.0;
        a.roll_input = 0.0;
        a.pitch_input = 0.0;
    }

    // --- integration
    let prev_alt = a.altitude;
    a.altitude += vel.y * dt;
    a.climb_rate = vel.y;
    let (mut east, mut north, _) = bevy_to_wu(pos.0);
    east += (vel.x * dt) as f64;
    north += (-vel.z * dt) as f64;
    let (_, _, roll_now) = a.euler();
    ground_contact(a, k, false, roll_now, prev_alt, east, north, theater, dt);
    if a.on_ground {
        vel.y = 0.0;
        if prev_alt > a.ground_altitude {
            // Touchdown: wings level, the nose stays up until the speed drops.
            let (h, p, _) = a.euler();
            a.orientation = Aircraft::from_euler(h, p.clamp(0.0, MAX_GROUND_PITCH), 0.0);
            let flat = Vec3::new(h.sin(), 0.0, -h.cos());
            vel = flat * vel.dot(flat).max(0.0);
        }
    }
    a.modern.vel = vel;
    a.modern.rates = rates;
    let speed = vel.length();
    a.velocity = speed * 16.0;
    // Angle of attack as the flight path marker offset, the classic trim and lift.
    let vb = a.orientation.inverse() * vel;
    let alpha = if speed > 1.0 && !a.on_ground { (-vb.y).atan2(-vb.z) } else { 0.0 };
    a.fpm = to_bam(alpha);
    a.trim = a.fpm * roll_now.cos();
    a.lift = a.fpm + 512.0;
    pos.0 = wu_to_bevy(east, north, a.view_z() as f64);
    tf.rotation = a.orientation;
}
