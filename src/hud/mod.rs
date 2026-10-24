//! Head-up display state and the per-frame [`HudFrame`] (gathered from the ECS by
//! [`HudParams`]). Two looks draw it, each a pure function of the frame:
//! * [`classic`]: the classic low-resolution HUD in 320x200 screen pixels;
//! * [`modern`]: a high-resolution F-16 style HUD (NAV / AA / AG), drawn as a monochrome
//!   mask that the combiner glass shows in HUD green.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::flight::{Aircraft, Player, to_bam};
use crate::input::{Action, ActionEvent};
use crate::world::{WorldPos, bevy_to_wu};

pub mod classic;
pub mod modern;

pub use classic::draw;

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HudState>()
            .init_resource::<HudMessages>()
            .init_resource::<Stores>()
            .init_resource::<Waypoints>()
            .init_resource::<GunLead>()
            .init_resource::<HudCues>()
            .add_systems(OnEnter(crate::settings::AppState::Flight), reset_hud)
            .add_systems(
                Update,
                (hud_keys, update_messages, advance_waypoints, track_max_g)
                    .after(crate::flight::FlightSystems)
                    .run_if(in_state(crate::settings::AppState::Flight)),
            );
    }
}

/// HUD centre (boresight) in screen pixels.
pub const CX: i32 = 159;
pub const CY: i32 = 52;

/// How much smaller the classic HUD is drawn on the combiner glass than at full 320x200 size
/// (focal length factor) at the modern HUD's `scale`: it fits the F-16's small combiner and the VR
/// symbology size as the modern HUD does.
pub fn classic_zoom(scale: f32) -> f32 {
    scale / modern::SCALE
}

/// F2: NAV / AIR / GND.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HudMode {
    #[default]
    Nav,
    Air,
    Gnd,
}

#[derive(Resource, Debug, Clone, Default)]
pub struct HudState {
    pub mode: HudMode,
    /// V: 0 = tapes, 1 = boxed digits, 2 = boxed digits + heading box.
    pub declutter: u8,
    /// F4: night vision goggles (the world drawn green and brightened, palette.wgsl).
    pub nvg: bool,
    /// Highest load factor of the flight (1/16 g), shown by the modern HUD.
    pub max_gees: i32,
    /// Bombing mode: CCRP (release on the target with the button held) instead of CCIP.
    pub ccrp: bool,
}

/// Main HUD colour and second colour (pitch ladder).
pub const HUD_COLOURS: (u8, u8) = (15, 7);

/// HUD message line (y = 20, centred) and status words. Other systems post messages with
/// [`HudMessages::show`].
#[derive(Resource, Debug, Clone, Default)]
pub struct HudMessages {
    pub text: String,
    /// Seconds the message stays up.
    pub timer: f32,
    /// "ACCEL" (time acceleration active).
    pub accel: bool,
    /// "TRAINING".
    pub training: bool,
}

/// Damage flash request: rows 0..89 are filled with colour 0x0D for one frame in the cockpit
/// views when the aircraft is hit.
#[derive(Resource, Default)]
pub struct DamageFlash(pub u8);

impl HudMessages {
    pub fn show(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.timer = 3.0;
    }
}

/// One weapon bay.
#[derive(Debug, Clone)]
pub struct Bay {
    /// Classic weapon name, e.g. "Incendiary".
    pub name: String,
    pub count: u32,
    /// Index of the weapon's side view picture (WPN page).
    pub picture: usize,
    /// Air-to-air missile (weapon class 7): AIR mode shows the missile envelope.
    pub air_to_air: bool,
    /// Lock range of the weapon's missile (km), 0 for unguided stores.
    pub range_km: f32,
}

/// Weapon stores (mirrored from the combat simulation every frame; defaults to a typical load).
#[derive(Resource, Debug, Clone)]
pub struct Stores {
    pub bays: Vec<Bay>,
    pub selected: usize,
    pub gun_rounds: Option<u32>,
    /// The gun is the selected weapon (no bay is).
    pub gun_selected: bool,
    /// What a turret gun is slaved to ("HMS": the helmet sight, "TGP": the targeting pod);
    /// None for a fixed gun.
    pub gun_slave: Option<&'static str>,
}

impl Default for Stores {
    fn default() -> Self {
        let bay = |name: &str, count, picture, air_to_air| Bay {
            name: name.into(),
            count,
            picture,
            air_to_air,
            range_km: 0.0,
        };
        Self {
            bays: vec![
                bay("Incendiary", 2, 9, false),
                bay("Durandal", 2, 8, false),
                bay("Maverick", 2, 16, false),
                bay("Sidewinder", 4, 0, true),
            ],
            selected: 0,
            gun_rounds: Some(650),
            gun_selected: false,
            gun_slave: None,
        }
    }
}

/// A navigation waypoint in world units.
#[derive(Debug, Clone, Copy)]
pub struct Waypoint {
    pub east: f64,
    pub north: f64,
}

/// INS waypoints (from the mission: 0 midpoint, 1 primary, 2 secondary, 3 landing base). The
/// HUD cue and the autopilot point at `points[current]`; empty = no waypoint.
#[derive(Resource, Debug, Clone, Default)]
pub struct Waypoints {
    pub points: Vec<Waypoint>,
    pub current: usize,
    /// Waypoint the mission logic selected last; Shift-F8 returns to it.
    pub mission_current: usize,
    /// Index of the mission's waypoints 0..3 (midpoint, primary, secondary, landing base) in
    /// a planned route; empty: the points are the mission's own 4.
    pub anchors: Vec<usize>,
}

impl Waypoints {
    /// Point to fly to when the mission logic selects its waypoint `w` (1 primary next after
    /// the secondary, 2 secondary next after the primary, 3 home): with a planned route, the
    /// points after the target just done are flown in order.
    pub fn for_mission_waypoint(&self, w: usize) -> usize {
        let a = |k: usize| self.anchors.get(k).copied().unwrap_or(k);
        let i = match w {
            1 => a(1),
            2 => a(1) + 1,
            3 => a(1).max(a(2)) + 1,
            _ => 0,
        };
        i.min(self.points.len().saturating_sub(1))
    }

    pub fn target(&self) -> Option<Waypoint> {
        self.points.get(self.current).copied()
    }
}

/// Weapon cues over the 3D view, in 320x200 screen pixels; filled by
/// the combat simulation.
#[derive(Resource, Debug, Clone, Default)]
pub struct HudCues {
    /// Tracking box around the locked target.
    pub target_box: Option<TargetBox>,
    /// CCIP bomb impact point (free-fall bombs), screen position.
    pub ccip: Option<(i32, i32)>,
    /// Ranging bar half-length and whether it flashes.
    pub ranging_bar: Option<(i32, bool)>,
    /// Flight-path guide diamond (128 map units past the target at the own altitude).
    pub diamond: Option<(i32, i32)>,
    /// The same cues as directions from the eye (aircraft frame: x right, y up, -z forward),
    /// for the high-resolution HUD; also set when off the HUD.
    pub target_dir: Option<Vec3>,
    pub ccip_dir: Option<Vec3>,
    pub diamond_dir: Option<Vec3>,
    /// Lock valid and target in range (the LOCK oval and its red state).
    pub lock: bool,
    pub in_range: bool,
    /// Gun selected: the gun pipper.
    pub gun: Option<GunCue>,
    /// The helmet-mounted display's cues, in every cockpit view whatever the HUD mode: the
    /// locked target's direction (aircraft frame) and whether it is an aircraft, and a turret
    /// gun's pipper (selected or not).
    pub helmet_lock: Option<(Vec3, bool)>,
    pub turret_gun: Option<GunCue>,
    /// Unguided bomb selected in AG: the CCIP or CCRP solution.
    pub bomb: Option<BombCue>,
    /// A recon target still to photograph is locked.
    pub recon: Option<ReconCue>,
    /// Missile selected with a target locked (AG: air-to-ground, AA: air-to-air): its launch
    /// zone.
    pub launch_zone: Option<LaunchZoneCue>,
}

/// Dynamic launch zone of the selected missile against the locked target
/// (`stealth_sim::dlz`): the ground ranges (km) from which a missile fired now reaches it.
/// Drawn F-16 style: a line from the shortest range up to the longest, the no-escape part a
/// box on it, and the target range caret beside it (above the line out of range, the line
/// scaled down to make room).
#[derive(Debug, Clone, Copy)]
pub struct LaunchZoneCue {
    pub range_km: f32,
    /// Shortest and longest range; None when it reaches the target from none.
    pub zone_km: Option<(f32, f32)>,
    /// Air-to-air: the longest range a target turning tail at the launch cannot escape from.
    pub no_escape_km: Option<f32>,
    /// In the zone (AG: with the lock): fire.
    pub shoot: bool,
    /// Inside the shortest range.
    pub too_close: bool,
    /// The zone is against the locked target (its box), not a pod point slewed away from it.
    pub at_lock: bool,
}

/// How close the camera has to get to the locked recon target: a frame counts within
/// `max_km`, which shrinks the farther the nose points off the target (None: too far off
/// to count at any range). One frame in view is enough.
#[derive(Debug, Clone, Copy)]
pub struct ReconCue {
    pub range_km: f32,
    pub max_km: Option<f32>,
    pub in_view: bool,
    pub needs: stealth_sim::PhotoNeeds,
}

impl ReconCue {
    /// The HUD's recon line; `page` names the camera page.
    pub fn text(&self, page: &str) -> String {
        use stealth_sim::PhotoNeeds;
        let step = match self.needs {
            PhotoNeeds::Camera => Some("SELECT CAMERA".to_string()),
            PhotoNeeds::BayOpen => Some("OPEN BAY".to_string()),
            PhotoNeeds::CameraPage => Some(format!("{page} PAGE")),
            PhotoNeeds::Nothing => None,
        };
        let state = if self.in_view {
            if step.is_none() {
                "TARGET IN VIEW - SHOOT".to_string()
            } else {
                "TARGET IN VIEW".to_string()
            }
        } else {
            // Rounded apart, so that a range still too far never reads as the maximum.
            let range = (self.range_km * 10.0).ceil() / 10.0;
            match self.max_km {
                Some(max) => format!("{range:.1} KM / MAX {:.1}", (max * 10.0).floor() / 10.0),
                None => format!("{range:.1} KM / AIM AT TGT"),
            }
        };
        let state = if self.in_view { state } else { format!("PHOTO {state}") };
        match step {
            Some(s) => format!("{state}  {s}"),
            None => state,
        }
    }
}

/// The bombsight's solution for the selected free-fall or retarded bomb.
#[derive(Debug, Clone, Copy)]
pub struct BombCue {
    pub ccrp: bool,
    /// Seconds a bomb released now falls.
    pub fall_secs: f32,
    /// Slant range (m) to the impact point (CCIP) or the target (CCRP; None without one).
    pub range_m: Option<f32>,
    /// CCRP: seconds to the release point (negative past it); None without a target.
    pub release_secs: Option<f32>,
}

/// Where the target has to be for the rounds fired now to hit it: compensates the time of
/// flight to the target's range for its motion relative to the rounds and for their drop.
#[derive(Debug, Clone, Copy)]
pub struct GunCue {
    /// Direction from the eye (aircraft frame: x right, y up, -z forward).
    pub dir: Vec3,
    /// The pipper on the 320x200 screen (classic HUD).
    pub screen: Option<(i32, i32)>,
    /// Range to the locked target (m); None: no target, the pipper is for 1000 m.
    pub range_m: Option<f32>,
    /// Locked target within the gun's range.
    pub in_range: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct TargetBox {
    pub x: i32,
    pub y: i32,
    /// Half width (half height = size - size/4).
    pub size: i32,
    /// 0 = HUD colour, 6 = wrong weapon, 0x29 = in range.
    pub colour: u8,
    /// LOCK oval shown; `red` = in range (the oval contracts).
    pub oval: bool,
    pub red: bool,
}

/// AIR-mode gun sight lead (pixels << 4). STUB: 0 = boresight.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct GunLead {
    pub x: i32,
    pub y: i32,
}

fn reset_hud(mut hud: ResMut<HudState>, mut msg: ResMut<HudMessages>) {
    *hud = HudState::default();
    *msg = HudMessages::default();
}

fn hud_keys(
    mut actions: MessageReader<ActionEvent>,
    mut hud: ResMut<HudState>,
    mut wps: ResMut<Waypoints>,
    stores: Res<Stores>,
    mut msg: ResMut<HudMessages>,
) {
    let n = wps.points.len();
    for ActionEvent(a) in actions.read() {
        match a {
            Action::HudMode => {
                hud.mode = match hud.mode {
                    HudMode::Nav => HudMode::Air,
                    HudMode::Air => HudMode::Gnd,
                    HudMode::Gnd => HudMode::Nav,
                }
            }
            Action::NearestTarget => {
                // A turret gun is aimed at the ground as much as at aircraft.
                let air = (stores.gun_selected && stores.gun_slave.is_none())
                    || stores.bays.get(stores.selected).is_some_and(|b| b.air_to_air);
                hud.mode = if air { HudMode::Air } else { HudMode::Gnd };
            }
            Action::BombMode => {
                hud.ccrp = !hud.ccrp;
                msg.show(if hud.ccrp { "CCRP" } else { "CCIP" });
            }
            Action::HudDeclutter => hud.declutter = (hud.declutter + 1) % 3,
            Action::Nvg => hud.nvg = !hud.nvg,
            // INS: PgUp / PgDn step through the waypoints, F8 changes to the next one, Shift-F8
            // returns to the mission's current waypoint (F7 shows the WAY page, see mfd.rs).
            // Weapon selection (Space) is done by the combat simulation.
            Action::LastWaypoint if n > 0 => wps.current = (wps.current + n - 1) % n,
            Action::NextWaypoint | Action::ChangeWaypoint if n > 0 => {
                wps.current = (wps.current + 1) % n
            }
            Action::ResetWaypoint if n > 0 => wps.current = wps.mission_current.min(n - 1),
            _ => {}
        }
    }
}

fn track_max_g(q: Query<&Aircraft, With<Player>>, mut hud: ResMut<HudState>) {
    if let Ok(a) = q.single()
        && !a.on_ground
        && a.gees as i32 > hud.max_gees
    {
        hud.max_gees = a.gees as i32;
    }
}

fn update_messages(time: Res<Time>, mut msg: ResMut<HudMessages>) {
    if msg.timer > 0.0 {
        msg.timer = (msg.timer - time.delta_secs()).max(0.0);
    }
}

/// Map units (wu >> 5, y growing southward) of a world position.
pub fn map_units(pos: &WorldPos) -> (i32, i32) {
    let (e, n, _) = bevy_to_wu(pos.0);
    ((e / 32.0) as i32, 0x8000 - (n / 32.0) as i32)
}

/// Bearing (binary angle) from a position to a waypoint.
fn waypoint_bearing(pos: &WorldPos, wp: Waypoint) -> u16 {
    let (x, y) = map_units(pos);
    let dx = (wp.east / 32.0) as i32 - x;
    let dy_north = y - (0x8000 - (wp.north / 32.0) as i32);
    to_bam((dx as f32).atan2(dy_north as f32)) as i32 as u16
}

/// "Waypoint n reached" when within 0x200 map units; the autopilot steers to
/// the current waypoint.
fn advance_waypoints(
    mut q: Query<(&WorldPos, &mut Aircraft), With<Player>>,
    mut wps: ResMut<Waypoints>,
    mut msg: ResMut<HudMessages>,
) {
    let Ok((pos, mut a)) = q.single_mut() else { return };
    let Some(wp) = wps.target() else { return };
    let (x, y) = map_units(pos);
    let (dx, dy) = ((wp.east / 32.0) as i32 - x, (0x8000 - (wp.north / 32.0) as i32) - y);
    if ((dx * dx + dy * dy) as f64).sqrt() < 512.0 && wps.current + 1 < wps.points.len() {
        wps.current += 1;
        msg.show(format!("Waypoint {} reached", wps.current));
    }
    if let Some(wp) = wps.target() {
        a.waypoint_bearing = waypoint_bearing(pos, wp) as f32;
    }
}

/// Everything the HUD shows, in the sim's units.
#[derive(Debug, Clone, Default)]
pub struct HudFrame {
    /// Modern HUD: canvas pixels per unit of the tangent plane (`modern`).
    pub scale: f32,
    pub state: HudState,
    /// Airspeed, knots.
    pub knots: i32,
    /// Altitude in feet.
    pub alt: i32,
    /// Heading, pitch, roll in 16-bit binary angles; roll > 0 = right.
    pub heading: u16,
    pub pitch: i16,
    pub roll: i16,
    /// 1/16 g
    pub gees: i32,
    /// Stall speed, knots.
    pub stall_knots: i32,
    /// Climb rate, wu/s.
    pub climb: i32,
    /// Flight path marker offset << 6 (= cos(roll) * (lift - 0x200)).
    pub fpm: i32,
    pub gear_down: bool,
    /// Night mission: stall/VVI bar colour.
    pub night_mission: bool,
    /// Frame parity used for blinking.
    pub blink: bool,
    /// Bearing to the current waypoint (binary angle).
    pub waypoint_bearing: Option<u16>,
    /// Selected weapon: (count, name).
    pub weapon: Option<(u32, String)>,
    pub weapon_air_to_air: bool,
    pub gun_rounds: Option<u32>,
    pub gun_selected: bool,
    /// A turret gun's slave ("HMS" / "TGP", `Stores::gun_slave`).
    pub gun_slave: Option<&'static str>,
    pub message: Option<String>,
    pub accel: bool,
    pub training: bool,
    /// Tracking camera selected on the right MFD: camera sight cross.
    pub camera_sight: bool,
    pub gun_lead: (i32, i32),
    pub cues: HudCues,
    /// Aircraft attitude and direction of flight (Bevy frame).
    pub orientation: Quat,
    pub path_dir: Vec3,
    pub mach: f32,
    /// Angle of attack, degrees.
    pub aoa: f32,
    /// Height above the ground, feet.
    pub radar_alt: f32,
    /// Current waypoint: (number, km, seconds to go at the current speed) and its direction
    /// from the cockpit eye (Bevy frame, not normalised).
    pub steerpoint: Option<(usize, f32, f32)>,
    pub steerpoint_dir: Option<Vec3>,
    /// Where the targeting pod looks on the ground, from the cockpit eye (Bevy frame, not
    /// normalised), while it is up.
    pub tgp_dir: Option<Vec3>,
    /// Lock range of the selected weapon (km), range to the locked target (km).
    pub weapon_range_km: Option<f32>,
    pub target_range_km: Option<f32>,
    /// ILS on and a runway end ahead: the deviations from its approach (degrees): localizer
    /// (positive = right of the centreline) and glideslope (positive = above the glide path).
    pub ils: Option<(f32, f32)>,
    /// A helicopter's readouts (`modern`: torque and rotor speed instead of g and Mach, a hover
    /// drift vector instead of the flight path marker at low speed).
    pub heli: Option<HeliHud>,
}

/// A helicopter's HUD readouts.
#[derive(Debug, Clone, Copy, Default)]
pub struct HeliHud {
    /// Engine torque and rotor speed, percent.
    pub torque: f32,
    pub rotor: f32,
    /// Ground speed over the ground in the aircraft's heading frame (knots, x right, y ahead).
    pub drift: Vec2,
}

#[derive(SystemParam)]
pub struct HudParams<'w, 's> {
    pub aircraft: Query<'w, 's, (&'static Aircraft, &'static WorldPos), With<Player>>,
    pub state: Res<'w, HudState>,
    pub messages: Res<'w, HudMessages>,
    pub stores: Res<'w, Stores>,
    pub waypoints: Res<'w, Waypoints>,
    pub lead: Res<'w, GunLead>,
    pub cues: Res<'w, HudCues>,
    pub tod: Res<'w, crate::palette::TimeOfDay>,
    pub time: Res<'w, Time>,
    pub cam: Option<Res<'w, crate::mfd::TrackCam>>,
    pub settings: Res<'w, crate::settings::Settings>,
    pub mfd: Option<Res<'w, crate::mfd::MfdState>>,
    pub papi: Option<Res<'w, crate::scenery::PapiSites>>,
}

/// (temperature ratio, speed of sound in ft/s) at an altitude in feet (standard atmosphere).
fn speed_of_sound(h: f32) -> (f32, f32) {
    let theta = 1.0 - 6.875_6e-6 * h.clamp(0.0, 36089.0);
    (theta, 1116.4 * theta.sqrt())
}

/// Blink frame parity, flipping 8 times a second.
pub fn blink(time: &Time) -> bool {
    (time.elapsed_secs() * 8.0) as u32 & 1 == 1
}

impl HudParams<'_, '_> {
    pub fn frame(&self, camera_sight: bool) -> Option<HudFrame> {
        let (a, pos) = self.aircraft.single().ok()?;
        let (h, p, r) = a.euler();
        // Avionics damage also takes out the INS waypoint cue.
        let bearing = self
            .waypoints
            .target()
            .filter(|_| !a.damage.autopilot_failed)
            .map(|wp| waypoint_bearing(pos, wp));
        let bay = self.stores.bays.get(self.stores.selected).filter(|_| !self.stores.gun_selected);
        let knots = a.knots();
        let (_, sound) = speed_of_sound(a.altitude);
        let path = a.path_dir();
        let body = a.orientation.inverse() * path;
        let eye = pos.0 + (a.orientation * self.settings.cockpit_eye()).as_dvec3();
        let steer = self.waypoints.target().filter(|_| !a.damage.autopilot_failed).map(|wp| {
            let d = crate::world::wu_to_bevy(wp.east, wp.north, a.ground_altitude as f64) - eye;
            let km = (d.x.hypot(d.z) / 1000.0) as f32;
            let ms = knots * 0.5144;
            (
                (self.waypoints.current, km, if ms > 1.0 { km * 1000.0 / ms } else { 0.0 }),
                d.as_vec3(),
            )
        });
        let ils =
            self.mfd.as_ref().filter(|m| m.ils).zip(self.papi.as_ref()).and_then(|(_, papi)| {
                let eye = pos.0 + (a.orientation * self.settings.cockpit_eye()).as_dvec3();
                let (_, along, lateral) =
                    crate::scenery::approach_to(&papi.0, bevy::math::DVec2::new(eye.x, -eye.z), h)?;
                let loc = lateral.atan2(along).to_degrees();
                let gs = eye.y.max(0.0).atan2(along).to_degrees() - crate::scenery::GLIDE_PATH_DEG;
                Some((loc as f32, gs as f32))
            });
        Some(HudFrame {
            ils,
            scale: self.settings.hud_scale(),
            state: self.state.clone(),
            knots: a.knots() as i32,
            alt: a.altitude.clamp(0.0, 65535.0) as i32,
            heading: to_bam(h) as i32 as u16,
            pitch: to_bam(p) as i32 as i16,
            roll: to_bam(r) as i32 as i16,
            gees: a.gees as i32,
            stall_knots: (a.stall_speed / 27.0) as i32,
            climb: a.climb_rate as i32,
            fpm: a.fpm as i32,
            gear_down: a.gear_down,
            night_mission: self.tod.night_mission,
            blink: blink(&self.time),
            waypoint_bearing: bearing,
            weapon: bay.map(|b| (b.count, b.name.clone())),
            weapon_air_to_air: bay.is_some_and(|b| b.air_to_air),
            gun_rounds: self.stores.gun_rounds,
            gun_selected: self.stores.gun_selected,
            gun_slave: self.stores.gun_slave,
            message: (self.messages.timer > 0.0).then(|| self.messages.text.clone()),
            accel: self.messages.accel,
            training: self.messages.training,
            camera_sight,
            gun_lead: self
                .cues
                .gun
                .and_then(|g| g.screen)
                .map_or((self.lead.x, self.lead.y), |(x, y)| ((x - 160) << 4, (y - 52) << 4)),
            cues: self.cues.clone(),
            orientation: a.orientation,
            path_dir: path,
            mach: knots * 1.6878 / sound,
            aoa: (-body.y).atan2(-body.z).to_degrees(),
            radar_alt: (a.altitude - a.ground_altitude).max(0.0),
            steerpoint: steer.map(|s| s.0),
            steerpoint_dir: steer.map(|s| s.1),
            tgp_dir: self
                .mfd
                .as_ref()
                .and_then(|m| m.av.tgp.los_ground)
                .map(|p| (p - eye).as_vec3()),
            weapon_range_km: bay.filter(|b| b.range_km > 0.0).map(|b| b.range_km),
            target_range_km: self.cam.as_ref().and_then(|c| c.range_km),
            heli: self.settings.aircraft.spec().helicopter.then(|| {
                let v = path * knots;
                let ahead = Vec3::new(h.sin(), 0.0, -h.cos());
                let right = Vec3::new(h.cos(), 0.0, h.sin());
                HeliHud {
                    torque: a.torque() * 100.0,
                    rotor: a.rotor_speed() * 100.0,
                    drift: Vec2::new(v.dot(right), v.dot(ahead)),
                }
            }),
        })
    }
}

#[cfg(test)]
mod route_tests {
    use super::*;

    fn wps(n: usize, anchors: Vec<usize>) -> Waypoints {
        Waypoints {
            points: vec![Waypoint { east: 0.0, north: 0.0 }; n],
            current: 0,
            mission_current: 0,
            anchors,
        }
    }

    #[test]
    fn objectives_follow_the_planned_route() {
        // The mission's own 4 points: the objective waypoints as they are.
        let w = wps(4, vec![]);
        assert_eq!([1, 2, 3].map(|k| w.for_mission_waypoint(k)), [1, 2, 3]);
        let w = wps(4, vec![0, 1, 2, 3]);
        assert_eq!([1, 2, 3].map(|k| w.for_mission_waypoint(k)), [1, 2, 3]);
        // N1 P N2 S N3 L: after the primary the planned N2, after both targets N3.
        let w = wps(6, vec![0, 1, 3, 5]);
        assert_eq!([1, 2, 3].map(|k| w.for_mission_waypoint(k)), [1, 2, 4]);
    }
}

#[cfg(test)]
mod recon_tests {
    use super::*;
    use stealth_sim::PhotoNeeds;

    #[test]
    fn recon_line_names_the_range_to_get_to() {
        let cue = |range_km, max_km, in_view, needs| ReconCue { range_km, max_km, in_view, needs };
        let far = cue(5.71, Some(5.69), false, PhotoNeeds::Nothing);
        assert_eq!(far.text("TGP"), "PHOTO 5.8 KM / MAX 5.6");
        let off = cue(3.0, None, false, PhotoNeeds::BayOpen);
        assert_eq!(off.text("TGP"), "PHOTO 3.0 KM / AIM AT TGT  OPEN BAY");
        let near = cue(2.0, Some(4.0), true, PhotoNeeds::CameraPage);
        assert_eq!(near.text("CAMERA"), "TARGET IN VIEW  CAMERA PAGE");
        let ready = cue(2.0, Some(4.0), true, PhotoNeeds::Nothing);
        assert_eq!(ready.text("TGP"), "TARGET IN VIEW - SHOOT");
    }
}
