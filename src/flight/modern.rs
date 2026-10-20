//! Modern flight model (not in the original): a point-mass aerodynamic model of the F-117A with
//! a fly-by-wire control system, tuned to the classic model's performance (about 560 kt at sea
//! level, 120 kt clean stall, 7 g).
//!
//! - Forces: thrust along the nose (lapsing with air density), lift from the angle of attack
//!   (linear up to the stall, falling off past it), drag (parasite, induced, gear, flaps, speed
//!   brake, bay doors, transonic rise), side force from sideslip and gravity, on a mass that
//!   lightens as fuel burns.
//! - Control laws: the stick commands load factor (neutral holds the flight path, also in banks
//!   up to 60 degrees; full pull 7 g, full push -3 g, squared stick shaping) through an angle of attack limiter just below
//!   the stall, and roll rate about the flight path; turns are coordinated (sideslip held at 0,
//!   or set by the rudder). The body rates follow their commands with short lags.
//! - Ground: rolling friction, wheel brakes, nosewheel steering, rotation above about 0.8 of the
//!   stall speed, carrier catapult and arresting wire; ground contact, landing and crash rules
//!   are the classic model's.
//!
//! Units: feet (= world units), seconds; `vel` is in the Bevy frame (x east, y up, -z north).

use bevy::prelude::*;

use super::{Aircraft, FlightConstants, FlightControls, Theater, WorldPos, bam, bam_diff, bevy_to_wu, engine, ground_contact, to_bam, wu_to_bevy};

const G: f32 = 32.174;
/// Sea level air density (slug/ft^3).
const RHO0: f32 = 0.002_376_9;
/// Wing loading at the reference weight (lb/ft^2).
const WING_LOADING: f32 = 58.5;
/// Weight without fuel and the reference weight (full internal fuel), lb.
const DRY_WEIGHT: f32 = 37_000.0;
const REF_WEIGHT: f32 = 47_000.0;
/// Static thrust to reference weight at full throttle.
const THRUST_RATIO: f32 = 0.8;

/// Lift slope (1/rad) and stall angles of attack (rad).
const CL_ALPHA: f32 = 3.0;
const ALPHA_STALL: f32 = 0.40;
const ALPHA_STALL_NEG: f32 = -0.28;
/// Angle of attack limiter of the control system (rad).
const ALPHA_LIMIT: f32 = 0.36;
const ALPHA_LIMIT_NEG: f32 = -0.22;
/// Lift added by the flaps.
const CL_FLAPS: f32 = 0.35;

const CD0: f32 = 0.040;
/// Induced drag factor.
const CD_K: f32 = 0.16;
const CD_GEAR: f32 = 0.025;
const CD_FLAPS: f32 = 0.04;
const CD_SPEED_BRAKE: f32 = 0.12;
const CD_BAY: f32 = 0.008;
/// Side force per radian of sideslip.
const CY_BETA: f32 = 0.8;

/// Load factor limits (g).
const N_MAX: f32 = 7.0;
const N_MIN: f32 = -3.0;
/// Full stick roll rate (rad/s) and the dynamic pressure (psf) below which it fades.
const ROLL_RATE: f32 = 1.75;
const ROLL_Q_FULL: f32 = 150.0;
/// Body pitch / yaw rate limit (rad/s).
const PITCH_RATE_MAX: f32 = 0.5;
/// Full rudder sideslip (rad).
const BETA_RUDDER: f32 = 0.1;
/// Lags (s): roll rate, pitch and yaw rate, angle of attack and sideslip corrections.
const TAU_ROLL: f32 = 0.12;
const TAU_RATE: f32 = 0.05;
const TAU_ALPHA: f32 = 0.15;
const TAU_BETA: f32 = 0.3;

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

fn cl_zero(a: &Aircraft) -> f32 {
    if a.flaps { CL_FLAPS } else { 0.0 }
}

fn alpha_stall(a: &Aircraft) -> f32 {
    if a.damage.heavy_stall { ALPHA_STALL - 0.06 } else { ALPHA_STALL }
}

fn alpha_limit(a: &Aircraft) -> f32 {
    if a.damage.heavy_stall { ALPHA_LIMIT - 0.06 } else { ALPHA_LIMIT }
}

/// Lift coefficient: linear to the stall, then falling off to 60% of the peak.
fn lift_coefficient(a: &Aircraft, alpha: f32) -> f32 {
    let c0 = cl_zero(a);
    let (hi, lo) = (alpha_stall(a), ALPHA_STALL_NEG);
    let peak = |s: f32| c0 + CL_ALPHA * s;
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

/// Load factor the stick asks for with the stick neutral: holds the flight path up to 60
/// degrees of bank, fading to 0 at 90 degrees, -1 g inverted.
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

pub(super) fn step(a: &mut Aircraft, pos: &mut WorldPos, tf: &mut Transform, dt: f32, controls: &FlightControls, k: &FlightConstants, theater: Option<&Theater>) {
    a.time += dt;
    if a.ended.is_some() {
        return;
    }
    if !a.modern.init {
        a.modern.init = true;
        a.modern.vel = a.orientation * Vec3::NEG_Z * (a.velocity / 16.0);
        if a.on_ground {
            a.modern.vel.y = 0.0;
        }
    }
    let (heading, pitch, roll) = a.euler();
    let air = !a.on_ground;

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
    let mut turb = if k.landing > 0 { ((500.0 - vz) * a.knots() / 8192.0).floor().max(0.0) } else { 0.0 };
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
    let mass = (DRY_WEIGHT + a.fuel) / REF_WEIGHT;
    let fwd = a.orientation * Vec3::NEG_Z;
    let up = a.orientation * Vec3::Y;
    let right = a.orientation * Vec3::X;
    let vb = a.orientation.inverse() * vel;
    let (alpha, beta) = if speed > 1.0 { ((-vb.y).atan2(-vb.z), vb.x.atan2(-vb.z)) } else { (pitch, 0.0) };
    let gamma = vhat.y.clamp(-1.0, 1.0).asin();
    let mach = speed / sound;

    // --- aerodynamic and thrust accelerations (g per unit of q * CL / wing loading)
    let per_cl = qbar / (WING_LOADING * mass);
    let cl = lift_coefficient(a, alpha);
    let mut cd = CD0 + CD_K * cl * cl;
    if a.gear_down {
        cd += CD_GEAR;
    }
    if a.flaps {
        cd += CD_FLAPS;
    }
    if a.brakes && air {
        cd += CD_SPEED_BRAKE;
    }
    if a.bay_open {
        cd += CD_BAY;
    }
    if alpha > alpha_stall(a) {
        cd += (alpha - alpha_stall(a)) * 1.5;
    }
    if mach > 0.85 {
        cd += 0.6 * (mach - 0.85).powi(2) + 4.0 * (mach - 0.95).max(0.0).powi(2);
    }
    let thrust = THRUST_RATIO * a.thrust / 100.0 * sigma.powf(0.6) / mass;
    let lift_dir = (up - vhat * up.dot(vhat)).normalize_or(up);
    let side_dir = (right - vhat * right.dot(vhat)).normalize_or(right);
    let n_lift = per_cl * cl;
    let mut accel = (fwd * thrust + lift_dir * n_lift - vhat * per_cl * cd - side_dir * per_cl * CY_BETA * beta) * G;
    accel.y -= G;

    // Stall speed at the current load (velocity units) for the warnings.
    let cl_avail = cl_zero(a) + CL_ALPHA * alpha_limit(a);
    let load = (a.gees / 16.0).max(1.0);
    a.stall_speed = (2.0 * load * WING_LOADING * mass / (RHO0 * sigma * cl_avail)).sqrt() * 16.0;

    if air {
        // --- control laws
        let roll_b = to_bam(roll);
        let mut n_cmd = {
            let n0 = neutral_load(gamma, roll);
            if pitch_stick >= 0.0 { n0 + pitch_stick * (N_MAX - n0) } else { n0 + pitch_stick * (n0 - N_MIN) }
        };
        let mut p_cmd = roll_stick * ROLL_RATE * (qbar / ROLL_Q_FULL).clamp(0.25, 1.0);
        if a.autopilot() {
            // Bank toward the waypoint bearing (at most 30 degrees), hold the altitude.
            let e = bam(bam_diff(a.waypoint_bearing, to_bam(heading)));
            let bank = (e * 1.5).clamp(-0.52, 0.52);
            p_cmd = ((bank - roll) * 2.0).clamp(-0.5, 0.5);
            let gamma_cmd = ((a.autopilot_alt - vz) * 0.0005).clamp(-0.17, 0.17);
            n_cmd = (neutral_load(gamma, roll) + (gamma_cmd - gamma) * speed / G * 0.8).clamp(-0.5, 3.0);
        }
        // Crash protection: wings level and pull when the ground is less than 3 s away.
        if k.landing == 0 && !a.gear_down && vel.y < 0.0 && a.altitude - a.ground_altitude + vel.y * 3.0 < 150.0 && roll_b.abs() < 24576.0 {
            p_cmd = (-roll * 3.0).clamp(-ROLL_RATE, ROLL_RATE);
            n_cmd = n_cmd.max(4.0);
        }
        let n_cmd = n_cmd.clamp(N_MIN, N_MAX);
        let alpha_cmd = if per_cl > 1e-3 {
            ((n_cmd / per_cl - cl_zero(a)) / CL_ALPHA).clamp(ALPHA_LIMIT_NEG, alpha_limit(a))
        } else {
            0.0
        };
        // Rates that keep the nose with the turning flight path, plus the corrections.
        let a_perp = accel - vhat * accel.dot(vhat);
        let path_rate = if speed > 1.0 { vhat.cross(a_perp) / speed } else { Vec3::ZERO };
        let mut q_cmd = path_rate.dot(right) + (alpha_cmd - alpha) / TAU_ALPHA;
        // Past the stall the nose drops.
        if alpha > alpha_stall(a) {
            q_cmd -= (alpha - alpha_stall(a)) * 4.0;
        }
        let q_cmd = q_cmd.clamp(-PITCH_RATE_MAX, PITCH_RATE_MAX);
        let r_cmd = (-path_rate.dot(up) + (beta + rudder * BETA_RUDDER) / TAU_BETA).clamp(-PITCH_RATE_MAX, PITCH_RATE_MAX);
        let r = &mut rates;
        r.x += (p_cmd - r.x) * (dt / TAU_ROLL).min(1.0);
        r.y += (q_cmd - r.y) * (dt / TAU_RATE).min(1.0);
        r.z += (r_cmd - r.z) * (dt / TAU_RATE).min(1.0);
        let omega = vhat * r.x + right * r.y - up * r.z;
        a.orientation = (Quat::from_scaled_axis(omega * dt) * a.orientation).normalize();
        vel += accel * dt;
        a.gees = ((accel + Vec3::Y * G).dot(up) / G * 16.0).clamp(-128.0, 128.0);
        a.roll_input = r.x / ROLL_RATE * 126.0;
        a.pitch_input = (n_cmd - 1.0) * 6.0;
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
    ground_contact(a, k, roll_now, prev_alt, east, north, theater, dt);
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
