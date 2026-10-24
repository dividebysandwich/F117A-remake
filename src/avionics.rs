//! Modern avionics (on unless `--classic-avionics`): F-16 style MFD
//! formats on either MFD instead of the classic fixed pages.
//!
//! - Formats: HSD (horizontal situation display: route, threat rings, air and ground contacts),
//!   HARM (the radars painting the aircraft, placed by danger like the RWR), TGP (the tracking
//!   camera), MAP (the moving map in muted colours, or the waypoint table) and SMS (stores: the
//!   WPN and ORD pages); on the F-117A's third MFD also HSI (`draw_hsi_page`). The bottom bezel
//!   buttons pick the format; picking one another MFD shows swaps the two. The top buttons are the format's own, the top right one makes the
//!   format the sensor of interest (SOI). The buttons' functions are drawn on the display next
//!   to them, like the real ones.
//! - SOI: HSD, HARM and the targeting pod can be the sensor of interest: the "sensor/cursor
//!   slew" controls move its cursor, "lock cursor target" locks what is under it (an aircraft or
//!   a ground object on the HSD, a radar on the HARM display).
//! - Targeting pod (TGP): slaved to the locked target; a target key or a display lock slaves it
//!   again, CZ (cursor zero) turns it to the current steerpoint (point track). As the SOI, slewing moves the pod itself, stabilized against the aircraft's motion (area track);
//!   once the slew stops it holds the ground point it looks at (point track), and "lock cursor
//!   target" locks what is there. SP (snowplow) drops the locks and fixes it ahead of the nose.
//!   The HSD shows where the pod looks.

use bevy::math::{DVec2, DVec3};
use bevy::prelude::*;

use crate::canvas::Canvas;
use crate::cockpit::CockpitArt;
use crate::flight::{Aircraft, Player};
use crate::font::{Align, Fonts, stroke_text_at, upper};
use crate::hud::{HudMode, HudState};
use crate::input::{Action, ActionEvent, FlightControls};
use crate::mfd::{
    ContactKind, ContactRef, Contacts, HIRES, MapRaster, MfdCmd, MfdParams, MfdSide, MfdState,
    OsbFn,
};
use crate::rwr::{EmitterView, Emitters};
use crate::world::{WU, WorldPos, bevy_to_wu};

pub struct AvionicsPlugin;

impl Plugin for AvionicsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<LockRequest>()
            .add_message::<PodDesignate>()
            .add_systems(
                Update,
                slew_and_lock
                    .after(crate::input::InputSystemsSet)
                    .after(crate::flight::FlightSystems)
                    .before(crate::combat::CombatSystems)
                    .run_if(in_state(crate::settings::AppState::Flight)),
            )
            .add_systems(
                Update,
                update_pod
                    .after(crate::combat::CombatSystems)
                    .before(crate::trackcam::update_camera)
                    .run_if(in_state(crate::settings::AppState::Flight)),
            );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Format {
    #[default]
    Hsd,
    Harm,
    Tgp,
    Map,
    Sms,
    /// Horizontal situation indicator: compass rose, course and bearing to the steerpoint.
    Hsi,
}

/// The formats on the bottom bezel buttons, left to right.
pub const FORMATS: [Format; 5] = [Format::Hsd, Format::Harm, Format::Tgp, Format::Map, Format::Sms];
/// The third MFD's: the HSI instead of the targeting pod (which belongs on the centre display).
pub const THIRD_FORMATS: [Format; 5] =
    [Format::Hsi, Format::Hsd, Format::Harm, Format::Map, Format::Sms];

/// The formats on an MFD's bottom bezel buttons.
pub fn formats(side: MfdSide) -> [Format; 5] {
    if side == MfdSide::Third { THIRD_FORMATS } else { FORMATS }
}

impl Format {
    pub fn label(self) -> &'static str {
        match self {
            Format::Hsd => "HSD",
            Format::Harm => "HARM",
            Format::Tgp => "TGP",
            Format::Map => "MAP",
            Format::Sms => "SMS",
            Format::Hsi => "HSI",
        }
    }

    /// Formats with a cursor (or a pod) the slew controls move.
    pub fn soi_capable(self) -> bool {
        matches!(self, Format::Hsd | Format::Harm | Format::Tgp)
    }

    /// The key that brings the format up (script `press=` lookups).
    pub fn action(self) -> Action {
        match self {
            Format::Hsd | Format::Harm | Format::Map => Action::Maps,
            Format::Tgp => Action::CamAhead,
            Format::Sms => Action::Ordnance,
            Format::Hsi => Action::SelectWaypoint,
        }
    }
}

/// F3 on the left MFD: HSD -> MAP -> HARM -> HSD.
pub fn next_left_format(f: Format) -> Format {
    match f {
        Format::Hsd => Format::Map,
        Format::Map => Format::Harm,
        _ => Format::Hsd,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HarmFilter {
    #[default]
    All,
    Sam,
    Air,
}

/// HSD ranges (km from the aircraft to the outer ring).
pub const HSD_RANGES: [f32; 6] = [10.0, 20.0, 40.0, 80.0, 160.0, 320.0];
const HSD_RANGE_LABELS: [&str; 6] = ["10", "20", "40", "80", "160", "320"];

#[derive(Debug, Clone)]
pub struct AvionicsState {
    /// Format on the left, the right and the third MFD (never the same twice).
    pub formats: [Format; 3],
    /// The cockpit has the third MFD (the F-117A's); without it its format is never shown.
    pub third: bool,
    /// The sensor of interest (a format on one of the MFDs).
    pub soi: Option<Format>,
    pub hsd_range: usize,
    /// HSD with the aircraft in the middle instead of low (depressed).
    pub hsd_centred: bool,
    /// Cursors in display pixels.
    pub hsd_cursor: Vec2,
    pub harm_cursor: Vec2,
    /// The HSD view the cursors were last placed in: the HSD cursor stays on its ground point
    /// and the HARM cursor turns with the symbols when the aircraft moves or turns.
    cursor_view: Option<HsdView>,
    /// Seconds "lock cursor target" has been held on the HSD or HARM display; past `MARK_HOLD`
    /// a zoomed window around the cursor shows (`zoom_window`).
    pub mark_held: f32,
    pub harm_filter: HarmFilter,
    /// MAP shows the waypoint table.
    pub map_way: bool,
    /// SMS shows the mission orders (else the weapons).
    pub sms_ord: bool,
    /// Right format before ILS was switched on.
    pub before_ils: Option<Format>,
    pub tgp: TgpState,
    /// The helmet sight (with a helmet-mounted display): where the pilot looks from the
    /// cockpit (world direction; None in the outside views).
    pub helmet: Option<Vec3>,
    /// The smoothed helmet sight in the aircraft's frame (head tremor and tracking jitter
    /// filtered out; the airframe's own motion is not delayed).
    helmet_body: Option<Vec3>,
}

/// Where the targeting pod looks.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum TgpMode {
    /// At the locked target (else ahead and down, or the camera key's direction).
    #[default]
    Slaved,
    /// Turned by the slew controls, the aircraft's motion not turning it: the ground point it
    /// looks at (Bevy world position), or above the horizon a point far out on its line of sight.
    Area(DVec3),
    /// Holding a ground point (Bevy world position).
    Point(DVec3),
    /// Snowplow: fixed ahead of the nose (`SP_EL` down), nothing locked, no ground track.
    Snowplow,
}

#[derive(Debug, Clone, Default)]
pub struct TgpState {
    pub mode: TgpMode,
    /// Seconds since the last slew in area track.
    idle: f32,
    /// Seconds the current slew has been held.
    held: f32,
    /// The ground point on the pod's line of sight (Bevy world position), when it is up.
    pub los_ground: Option<DVec3>,
    /// The pod's line of sight (world direction), when it is up: the gunship's turret and
    /// Hellfires follow it.
    pub los: Option<Vec3>,
    /// SP pressed: the simulation is to drop its locks.
    clear: bool,
    /// CZ pressed: the pod is to hold the current steerpoint (`update_pod`, which has the route).
    cz: bool,
}

impl TgpMode {
    fn label(self) -> &'static str {
        match self {
            TgpMode::Slaved => "SLV",
            TgpMode::Area(_) => "AREA",
            TgpMode::Point(_) => "PNT",
            TgpMode::Snowplow => "SP",
        }
    }
}

/// Pod turn rate at full slew and 1X (rad/s); halved per zoom step.
const POD_RATE: f32 = 0.5;
/// Seconds without slewing before an area track becomes a point track.
const POD_SETTLE: f32 = 0.15;
/// Farthest ground point the pod tracks (m).
const POD_RANGE: f64 = 40_000.0;
/// How far out an area track above the horizon puts its point (m): far enough that flying
/// does not turn the line of sight.
const POD_SKY: f64 = 1.0e7;
const POD_AZ_LIMIT: f32 = 2.97;
const POD_EL_LIMITS: (f32, f32) = (-1.75, 0.5);
/// Snowplow elevation (rad): ahead and a little down, as the unlocked slaved pod.
const SP_EL: f32 = -0.15;
/// Slew response by the range to the ground point the pod looks at: linear up to `SLEW_NEAR`
/// (m); by `SLEW_FAR` each axis follows `SLEW_EXPO_FAR` (fine near the centre) and a held slew
/// starts at `SLEW_START_FAR` of its rate, building to the full rate over `SLEW_RAMP` seconds
/// (fine taps of the keys). Full deflection held still turns the pod at the full rate.
const SLEW_NEAR: f32 = 3_000.0;
const SLEW_FAR: f32 = 20_000.0;
const SLEW_EXPO_FAR: f32 = 3.0;
const SLEW_START_FAR: f32 = 0.15;
const SLEW_RAMP: f32 = 0.8;
/// Ground slew away from (and toward) the aircraft against slewing across, at the same range.
const SLEW_AWAY: f32 = 1.1;
/// At a shallow angle the away slew moves the line of sight by only the sine of the depression;
/// it is sped up by that sine to this power (so the line of sight follows it to 1 - this), e.g.
/// about 3x at 10 km from 1000 ft.
const SLEW_AWAY_SHALLOW: f64 = 0.32;

/// Direction from the nose (aircraft frame, -Z forward) by azimuth and elevation.
pub fn pod_dir(az: f32, el: f32) -> Vec3 {
    Vec3::new(az.sin() * el.cos(), el.sin(), -az.cos() * el.cos())
}

/// Azimuth and elevation of a direction in the aircraft frame.
pub fn pod_angles(d: Vec3) -> (f32, f32) {
    let d = d.normalize_or(Vec3::NEG_Z);
    (d.x.atan2(-d.z), d.y.clamp(-1.0, 1.0).asin())
}

/// Where a line of sight meets the ground (the plane at height `ground_y`), within the pod's
/// range.
pub fn ground_hit(origin: DVec3, dir: Vec3, ground_y: f64) -> Option<DVec3> {
    let t = ground_distance(origin, dir, ground_y)?;
    (t <= POD_RANGE).then(|| origin + dir.as_dvec3().normalize_or_zero() * t)
}

/// Distance along a line of sight to the ground (the plane at height `ground_y`), at any
/// range; None when it does not meet it.
fn ground_distance(origin: DVec3, dir: Vec3, ground_y: f64) -> Option<f64> {
    let d = dir.as_dvec3().normalize_or_zero();
    if d.y > -1e-4 {
        return None;
    }
    let t = (ground_y - origin.y) / d.y;
    (t > 0.0).then_some(t)
}

/// The slew's share of the full pod rate per axis, with the ground point `range` (m) away
/// (None: the pod looks above the horizon, slewing coarsely) and the slew held `held` seconds.
fn slew_gain(slew: Vec2, range: Option<f64>, held: f32) -> Vec2 {
    let far =
        range.map_or(0.0, |r| ((r as f32 - SLEW_NEAR) / (SLEW_FAR - SLEW_NEAR)).clamp(0.0, 1.0));
    let expo = 1.0 + (SLEW_EXPO_FAR - 1.0) * far;
    let start = 1.0 + (SLEW_START_FAR - 1.0) * far;
    let ramp = start + (1.0 - start) * (held / SLEW_RAMP).min(1.0);
    let curve = |v: f32| v.signum() * v.abs().min(1.0).powf(expo);
    Vec2::new(curve(slew.x), curve(slew.y)) * ramp
}

/// One step of the pod: slewing (`slew`, with the pod the SOI) moves it from where it looks
/// (`looking`, world direction) in area track, stabilized against the aircraft's motion: on the
/// ground it moves the ground point, right and away from the aircraft, at a pace by its range (so
/// at a shallow angle the line of sight rises and falls finely); above the horizon it turns the
/// line of sight. `POD_SETTLE` seconds after the slew stops it holds the ground point it looks
/// at. Returns the ground point on its line of sight.
#[allow(clippy::too_many_arguments)]
pub fn pod_step(
    tgp: &mut TgpState,
    slew: Vec2,
    dt: f32,
    zoom: u8,
    orientation: Quat,
    origin: DVec3,
    looking: Option<Vec3>,
    ground_y: f64,
) -> Option<DVec3> {
    let to_world = |az: f32, el: f32| orientation * pod_dir(az, el);
    // An area track's ground point (as it is when on the ground, not rounded through its
    // direction).
    let area_ground = |p: DVec3| {
        if p.y == ground_y {
            Some(p).filter(|p| (p - origin).length() <= POD_RANGE)
        } else {
            ground_hit(origin, (p - origin).as_vec3(), ground_y)
        }
    };
    if slew.length() > 0.05 {
        let dir = match tgp.mode {
            TgpMode::Area(p) | TgpMode::Point(p) => (p - origin).as_vec3(),
            TgpMode::Slaved => looking.unwrap_or_else(|| to_world(0.0, SP_EL)),
            TgpMode::Snowplow => to_world(0.0, SP_EL),
        }
        .normalize_or(orientation * Vec3::NEG_Z);
        let range = ground_distance(origin, dir, ground_y);
        let slew = slew_gain(slew, range, tgp.held);
        tgp.held += dt;
        let rate = POD_RATE / (1u32 << zoom.min(6)) as f32 * dt;
        let on_ground = range.filter(|&r| r <= POD_RANGE).map(|r| origin + dir.as_dvec3() * r);
        let moved = match on_ground {
            Some(p) => {
                let flat = DVec3::new(dir.x as f64, 0.0, dir.z as f64);
                let away = flat.try_normalize().unwrap_or_else(|| {
                    let nose = (orientation * Vec3::NEG_Z).as_dvec3();
                    DVec3::new(nose.x, 0.0, nose.z).normalize_or(DVec3::NEG_Z)
                });
                let right = away.cross(DVec3::Y);
                let r = (p - origin).length();
                let shallow = (-dir.y as f64).max(1e-3).powf(-SLEW_AWAY_SHALLOW);
                let away = away * SLEW_AWAY as f64 * shallow;
                p + (right * slew.x as f64 + away * slew.y as f64) * r * rate as f64
            }
            // Above the horizon (or past the pod's range): turned by heading and elevation, as
            // its picture is upright.
            None => {
                let heading = dir.x.atan2(-dir.z) + slew.x * rate;
                let el = dir.y.clamp(-1.0, 1.0).asin() + slew.y * rate;
                origin + pod_dir(heading, el.clamp(-1.55, 1.55)).as_dvec3() * POD_SKY
            }
        };
        // Within the pod's gimbal limits and range.
        let (az, el) = pod_angles(orientation.inverse() * (moved - origin).as_vec3());
        let (caz, cel) =
            (az.clamp(-POD_AZ_LIMIT, POD_AZ_LIMIT), el.clamp(POD_EL_LIMITS.0, POD_EL_LIMITS.1));
        let ground = area_ground(moved).is_some_and(|p| p == moved);
        tgp.mode = TgpMode::Area(if (caz, cel) == (az, el) && (ground || moved.y > ground_y) {
            moved
        } else {
            let dir = to_world(caz, cel);
            ground_hit(origin, dir, ground_y).unwrap_or(origin + dir.as_dvec3() * POD_SKY)
        });
        tgp.idle = 0.0;
    } else {
        tgp.held = 0.0;
        if let TgpMode::Area(p) = tgp.mode {
            tgp.idle += dt;
            if tgp.idle >= POD_SETTLE
                && let Some(p) = area_ground(p)
            {
                tgp.mode = TgpMode::Point(p);
            }
        }
    }
    match tgp.mode {
        TgpMode::Point(p) => Some(p),
        TgpMode::Area(p) => area_ground(p),
        TgpMode::Snowplow => ground_hit(origin, to_world(0.0, SP_EL), ground_y),
        TgpMode::Slaved => looking.and_then(|d| ground_hit(origin, d, ground_y)),
    }
}

/// The pod's line of sight in area or point track (None: slaved, the tracking camera's own
/// rule), from `origin`.
pub fn pod_direction(tgp: &TgpState, orientation: Quat, origin: DVec3) -> Option<Vec3> {
    match tgp.mode {
        TgpMode::Slaved => None,
        TgpMode::Snowplow => Some(orientation * pod_dir(0.0, SP_EL)),
        TgpMode::Area(p) | TgpMode::Point(p) => (p - origin).as_vec3().try_normalize(),
    }
}

impl Default for AvionicsState {
    fn default() -> Self {
        let centre = Vec2::new(HIRES.0 as f32 / 2.0, HIRES.1 as f32 / 2.0);
        Self {
            formats: [Format::Hsd, Format::Sms, Format::Hsi],
            third: false,
            soi: Some(Format::Hsd),
            hsd_range: 2,
            hsd_centred: false,
            hsd_cursor: centre - Vec2::Y * 30.0,
            harm_cursor: centre,
            cursor_view: None,
            mark_held: 0.0,
            harm_filter: HarmFilter::All,
            map_way: false,
            sms_ord: true,
            before_ils: None,
            tgp: default(),
            helmet: None,
            helmet_body: None,
        }
    }
}

/// Seconds "lock cursor target" is held before the zoomed window shows.
const MARK_HOLD: f32 = 0.3;

impl AvionicsState {
    /// The zoomed window around the SOI's cursor is up.
    pub fn zoom_window(&self) -> bool {
        self.mark_held >= MARK_HOLD
    }
}

fn index(side: MfdSide) -> usize {
    match side {
        MfdSide::Left => 0,
        MfdSide::Right => 1,
        MfdSide::Third => 2,
    }
}

impl AvionicsState {
    pub fn format(&self, side: MfdSide) -> Format {
        self.formats[index(side)]
    }

    /// The cockpit's MFDs, left to right on the F-16's panel (the F-117A's third is right of
    /// its centre display, the right MFD).
    pub fn sides(&self) -> &'static [MfdSide] {
        if self.third {
            &[MfdSide::Left, MfdSide::Right, MfdSide::Third]
        } else {
            &[MfdSide::Left, MfdSide::Right]
        }
    }

    /// The MFD showing a format.
    pub fn shown(&self, f: Format) -> Option<MfdSide> {
        self.sides().iter().copied().find(|&s| self.format(s) == f)
    }

    /// The cockpit has the third MFD or not (its format: the HSI, or one not on another MFD).
    pub fn set_third(&mut self, third: bool) {
        self.third = third;
        let [l, r, _] = self.formats;
        self.formats[2] = if l != Format::Hsi && r != Format::Hsi {
            Format::Hsi
        } else {
            THIRD_FORMATS.into_iter().find(|&f| f != l && f != r).unwrap_or(Format::Map)
        };
        self.fix_soi();
    }

    /// The sensor of interest if it is on an MFD.
    pub fn soi_format(&self) -> Option<Format> {
        self.soi.filter(|&f| self.shown(f).is_some())
    }

    /// Puts a format on an MFD; the MFD showing the new one gets this one's format.
    pub fn select(&mut self, side: MfdSide, f: Format) {
        let i = index(side);
        match self.shown(f) {
            Some(o) => self.formats.swap(i, index(o)),
            None => self.formats[i] = f,
        }
        self.fix_soi();
    }

    /// Shows a format, on the right MFD unless it is up already.
    pub fn show(&mut self, f: Format) {
        if self.shown(f).is_none() {
            self.select(MfdSide::Right, f);
        }
    }

    /// The SOI stays on a displayed format that can be one.
    fn fix_soi(&mut self) {
        if self.soi_format().is_none() {
            self.soi = self.sides().iter().map(|&s| self.format(s)).find(|f| f.soi_capable());
        }
    }
}

/// Applies a display command with the modern avionics.
pub fn apply(state: &mut MfdState, raster: &mut MapRaster, cmd: MfdCmd) {
    let av = &mut state.av;
    match cmd {
        MfdCmd::Format(side, f) => {
            av.select(side, f);
            if f == Format::Map {
                raster.invalidate();
            }
        }
        MfdCmd::Soi(side) => {
            let f = av.format(side);
            if f.soi_capable() {
                av.soi = Some(f);
            }
        }
        // To the next MFD round (left, right, third) with a format that can be the SOI.
        MfdCmd::SwapSoi => {
            let side = av.soi_format().and_then(|f| av.shown(f)).unwrap_or(MfdSide::Right);
            let sides = av.sides();
            let at = sides.iter().position(|&s| s == side).unwrap_or(0);
            let next = (1..sides.len())
                .map(|k| av.format(sides[(at + k) % sides.len()]))
                .find(|f| f.soi_capable());
            if let Some(f) = next {
                av.soi = Some(f);
            }
        }
        MfdCmd::HsdRange(d) => {
            av.hsd_range =
                (av.hsd_range as i32 + d as i32).clamp(0, HSD_RANGES.len() as i32 - 1) as usize
        }
        MfdCmd::HsdCentre => av.hsd_centred = !av.hsd_centred,
        MfdCmd::MapZoom(d) => {
            state.map_zoom = (state.map_zoom + d as i32).clamp(2, state.max_map_zoom());
            raster.invalidate();
        }
        // Z/X: the HSD's range when it is the SOI, else the map's zoom.
        MfdCmd::ZoomIn | MfdCmd::ZoomOut => {
            let d = if cmd == MfdCmd::ZoomIn { 1 } else { -1 };
            if av.soi_format() == Some(Format::Hsd) || av.shown(Format::Map).is_none() {
                av.hsd_range =
                    (av.hsd_range as i32 - d).clamp(0, HSD_RANGES.len() as i32 - 1) as usize;
            } else {
                state.map_zoom = (state.map_zoom + d).clamp(2, state.max_map_zoom());
                raster.invalidate();
            }
        }
        MfdCmd::ToggleOrient => state.map_heading_up = !state.map_heading_up,
        MfdCmd::MapWay => {
            av.map_way = !av.map_way;
            raster.invalidate();
        }
        MfdCmd::Sms(ord) => {
            av.sms_ord = ord;
            av.show(Format::Sms);
        }
        MfdCmd::HarmFilter => {
            av.harm_filter = match av.harm_filter {
                HarmFilter::All => HarmFilter::Sam,
                HarmFilter::Sam => HarmFilter::Air,
                HarmFilter::Air => HarmFilter::All,
            }
        }
        MfdCmd::Flir => {
            state.flir = !state.flir;
            av.show(Format::Tgp);
        }
        MfdCmd::Cam(d) => {
            state.cam_dir = d;
            av.tgp.mode = TgpMode::Slaved;
            av.show(Format::Tgp);
        }
        // Snowplow, with the pod the SOI.
        MfdCmd::TgpSp => {
            if av.soi_format() == Some(Format::Tgp) {
                av.tgp.mode = TgpMode::Snowplow;
                av.tgp.clear = true;
            }
        }
        // A target key or a display lock: the pod follows the lock again.
        MfdCmd::TgpSlave => av.tgp.mode = TgpMode::Slaved,
        MfdCmd::TgpCz => av.tgp.cz = true,
        // Applied before (`mfd::cam_zoom`).
        MfdCmd::CamZoom(_) | MfdCmd::CamZoomCycle => {}
        MfdCmd::Ils => {
            state.ils = !state.ils;
            if state.ils {
                av.before_ils = Some(av.format(MfdSide::Right));
                av.select(MfdSide::Right, Format::Tgp);
            } else if let Some(f) = av.before_ils.take() {
                av.select(MfdSide::Right, f);
            }
        }
        // The classic pages' keys and caps.
        MfdCmd::LeftPage(crate::mfd::LeftMfd::Map) => av.select(MfdSide::Left, Format::Map),
        MfdCmd::LeftPage(crate::mfd::LeftMfd::Tac) => av.select(MfdSide::Left, Format::Hsd),
        MfdCmd::RightPage(p) => match p {
            crate::mfd::RightMfd::Camera => av.show(Format::Tgp),
            crate::mfd::RightMfd::Way => {
                av.map_way = true;
                av.show(Format::Map);
            }
            crate::mfd::RightMfd::Wpn => {
                av.sms_ord = false;
                av.show(Format::Sms);
            }
            crate::mfd::RightMfd::Ord => {
                av.sms_ord = true;
                av.show(Format::Sms);
            }
        },
    }
}

/// Label, function and lit state of a bezel button with the modern avionics: the formats along
/// the bottom, the format's own functions along the top (SOI top right).
pub fn osb(side: MfdSide, slot: u8, s: &MfdState) -> Option<(&'static str, OsbFn, bool)> {
    use crate::input::Action as A;
    use OsbFn::*;
    let av = &s.av;
    let f = av.format(side);
    if slot >= 5 {
        let g = formats(side)[slot as usize - 5];
        return Some((g.label(), Cmd(MfdCmd::Format(side, g)), f == g));
    }
    let soi = ("SOI", Cmd(MfdCmd::Soi(side)), av.soi_format() == Some(f));
    Some(match (f, slot) {
        (Format::Hsd, 0) => {
            (if av.hsd_centred { "CEN" } else { "DEP" }, Cmd(MfdCmd::HsdCentre), false)
        }
        (Format::Hsd, 1) => ("RNG-", Cmd(MfdCmd::HsdRange(-1)), false),
        (Format::Hsd, 2) => (HSD_RANGE_LABELS[av.hsd_range], Readout, false),
        (Format::Hsd, 3) => ("RNG+", Cmd(MfdCmd::HsdRange(1)), false),
        (Format::Harm, 0) => (
            match av.harm_filter {
                HarmFilter::All => "ALL",
                HarmFilter::Sam => "SAM",
                HarmFilter::Air => "AIR",
            },
            Cmd(MfdCmd::HarmFilter),
            false,
        ),
        (Format::Hsd | Format::Harm | Format::Tgp, 4) => soi,
        (Format::Tgp, 0) => ("FLIR", Cmd(MfdCmd::Flir), s.flir),
        (Format::Tgp, 1) => (
            crate::mfd::cam_zoom_label(s.cam_zoom),
            Cmd(MfdCmd::CamZoomCycle),
            s.cam_zoom.is_some(),
        ),
        (Format::Tgp, 2) => ("SP", Cmd(MfdCmd::TgpSp), av.tgp.mode == TgpMode::Snowplow),
        // CZ: the pod to the current steerpoint, not in A-A.
        (Format::Tgp, 3) if !s.air => ("CZ", Cmd(MfdCmd::TgpCz), false),
        (Format::Map, 0) if av.map_way => ("WP-", Action(A::LastWaypoint), false),
        (Format::Map, 1) if av.map_way => ("WP+", Action(A::NextWaypoint), false),
        (Format::Map, 2) if av.map_way => ("RST", Action(A::ResetWaypoint), false),
        (Format::Map, 0) => {
            (if s.map_heading_up { "HUP" } else { "NUP" }, Cmd(MfdCmd::ToggleOrient), false)
        }
        (Format::Map, 1) => ("ZM-", Cmd(MfdCmd::MapZoom(-1)), false),
        (Format::Map, 2) => ("ZM+", Cmd(MfdCmd::MapZoom(1)), false),
        (Format::Map, 3) => ("WAY", Cmd(MfdCmd::MapWay), av.map_way),
        (Format::Sms, 0) => ("WPN", Cmd(MfdCmd::Sms(false)), !av.sms_ord),
        (Format::Sms, 1) => ("ORD", Cmd(MfdCmd::Sms(true)), av.sms_ord),
        (Format::Sms, 2) if !av.sms_ord => ("SEL", Action(A::SelectWeapon), false),
        (Format::Sms, 3) if !av.sms_ord => {
            (crate::mfd::bomb_mode_label(s.ccrp), Action(A::BombMode), false)
        }
        (Format::Hsi, 0) => ("WP-", Action(A::LastWaypoint), false),
        (Format::Hsi, 1) => ("WP+", Action(A::NextWaypoint), false),
        _ => return None,
    })
}

// ------------------------------------------------------------------------------------------
// Drawing

/// Height of the bezel label bands at the top and bottom of the display.
const BAND: i32 = 16;
/// Colours (palette 0..15, the same at every time of day).
const BLACK: u8 = 5;
const DARK: u8 = 8;
const GREY: u8 = 7;
const NAVY: u8 = 2;
const GREEN: u8 = 10;
const CYAN: u8 = 11;
const RED: u8 = 13;
const WHITE: u8 = 15;
const YELLOW: u8 = 17;

/// Draws an MFD with the modern avionics.
pub fn draw(p: &MfdParams, side: MfdSide, c: &mut Canvas, art: &CockpitArt, fonts: &Fonts) {
    let s = &p.state;
    let av = &s.av;
    let f = av.format(side);
    let ac = p.aircraft.single().ok();
    let soi = av.soi_format() == Some(f);
    match f {
        Format::Hsd => draw_hsd(p, c, ac, soi),
        Format::Harm => draw_harm(p, c, soi),
        Format::Hsi => draw_hsi_page(p, c, fonts),
        _ => {
            // The classic pages, between the label bands.
            let mut page = Canvas::scaled(c.w, c.h - 2 * BAND as usize, c.scale);
            match f {
                Format::Tgp => p.draw_camera_hires(&mut page, fonts),
                Format::Map if av.map_way => p.draw_way_hires(&mut page, fonts, ac),
                Format::Map => p.draw_map_hires(&mut page, art, fonts, ac, true),
                _ if av.sms_ord => p.draw_ord_hires(&mut page, fonts),
                _ => p.draw_wpn_hires(&mut page, art, fonts),
            }
            // See-through around the pod picture, black elsewhere.
            c.clear(if f == Format::Tgp && p.cam.live { 0 } else { BLACK });
            c.copy_rect(&page, 0, 0, page.w as i32, page.h as i32, 0, BAND);
            if f == Format::Tgp {
                // Crosshair over the picture's centre, the pod's mode bottom left of the page.
                let col = p.camera_text_colour();
                fonts.text_scaled(
                    c,
                    6,
                    c.h as i32 - BAND - 42,
                    av.tgp.mode.label().as_bytes(),
                    col,
                    2,
                );
                let o = Vec2::new(c.w as f32 / 2.0, c.h as f32 / 2.0);
                for d in [Vec2::X, Vec2::NEG_X, Vec2::Y, Vec2::NEG_Y] {
                    c.stroke(o + d * 6.0, o + d * 22.0, 1.3, col);
                }
            }
        }
    }
    draw_labels(c, fonts, side, s);
    if soi {
        c.frame(Vec2::splat(1.0), Vec2::new(c.w as f32 - 2.0, c.h as f32 - 2.0), 2.0, WHITE);
    }
}

/// The bezel button labels next to their buttons: lit ones inverted.
fn draw_labels(c: &mut Canvas, fonts: &Fonts, side: MfdSide, s: &MfdState) {
    for slot in 0..crate::mfd::OSB_COUNT {
        let Some((label, _, lit)) = osb(side, slot, s) else { continue };
        let col = (slot % 5) as i32;
        let text = upper(label);
        let w = 2 * fonts.width(&text) - 2;
        let x = (c.w as i32 * (2 * col + 1)) / 10 - w / 2;
        let y = if slot < 5 { 3 } else { c.h as i32 - 13 };
        if lit {
            c.fill_rect(x - 2, y - 2, w + 4, 14, GREEN);
        }
        fonts.text_scaled(c, x, y, &text, if lit { BLACK } else { GREEN }, 2);
    }
}

/// HSI course deviation at full scale (km off the course, two dots).
const HSI_FULL_SCALE_KM: f64 = 5.0;

/// The HSI format (and the third MFD's page with the classic pages): a heading-up compass
/// rose with the digital heading over it; the course arrow along the leg to the steerpoint
/// (from the previous one; to the first one, from the aircraft) with its deviation bar, two
/// dots either side (`HSI_FULL_SCALE_KM`) and the TO / FROM arrow; the bearing pointer to the
/// steerpoint (cyan); the steerpoint, distance and ETA left, bearing, course and ground speed
/// right.
pub fn draw_hsi_page(p: &MfdParams, c: &mut Canvas, fonts: &Fonts) {
    c.clear(BLACK);
    let Ok((a, pos)) = p.aircraft.single() else { return };
    let heading = a.euler().0;
    let (e, n, _) = bevy_to_wu(pos.0);
    let own = DVec2::new(e, n);
    let wps = &p.waypoints;
    let steer = wps.target().map(|w| DVec2::new(w.east, w.north));
    // Compass bearing (radians clockwise from north) of a world vector (east, north).
    let compass = |v: DVec2| v.x.atan2(v.y) as f32;
    let bearing = steer.map(|w| compass(w - own));
    let from = match wps.current {
        0 => None,
        i => wps.points.get(i - 1).map(|w| DVec2::new(w.east, w.north)),
    };
    let course_dir = steer.zip(from).map(|(w, f)| w - f).filter(|d| d.length() > 1.0);
    let course = course_dir.map(compass).or(bearing);

    let o = Vec2::new(c.w as f32 / 2.0, c.h as f32 / 2.0 + 10.0);
    let r = c.h as f32 / 2.0 - BAND as f32 - 14.0;
    // Up the display = the nose.
    let dir = |b: f32| Vec2::new((b - heading).sin(), -(b - heading).cos());
    // The rose: a tick every 5 degrees, longer every 10, labelled every 30.
    for k in 0..72 {
        let b = (k as f32 * 5.0).to_radians();
        let len = if k % 2 == 0 { 7.0 } else { 4.0 };
        c.stroke(o + dir(b) * r, o + dir(b) * (r - len), 1.2, WHITE);
        if k % 6 == 0 {
            let label = match k * 5 {
                0 => "N".to_string(),
                90 => "E".to_string(),
                180 => "S".to_string(),
                270 => "W".to_string(),
                d => (d / 10).to_string(),
            };
            let q = o + dir(b) * (r - 16.0);
            stroke_text_at(c, q.x, q.y, &label, 10.0, 1.3, WHITE, Align::Centre);
        }
    }
    // Fixed index marks every 45 degrees outside the rose, the lubber line at the top.
    for k in 1..8 {
        let d =
            Vec2::from_angle(k as f32 * std::f32::consts::FRAC_PI_4 - std::f32::consts::FRAC_PI_2);
        c.stroke(o + d * (r + 2.0), o + d * (r + 7.0), 1.2, GREY);
    }
    c.triangle(
        [o - Vec2::Y * (r + 1.0), o + Vec2::new(-5.0, -r - 9.0), o + Vec2::new(5.0, -r - 9.0)],
        WHITE,
    );
    // Digital heading in a box over the rose.
    let hdg = format!("{:03}", (heading.to_degrees().rem_euclid(360.0).round() as i32) % 360);
    let w = 2 * fonts.width(hdg.as_bytes()) - 2;
    let top = (o.y - r) as i32 - 24;
    c.frame(
        Vec2::new(o.x - (w / 2 + 4) as f32, top as f32),
        Vec2::new(o.x + (w / 2 + 4) as f32, top as f32 + 14.0),
        1.2,
        WHITE,
    );
    fonts.text_scaled(c, o.x as i32 - w / 2, top + 3, hdg.as_bytes(), WHITE, 2);

    // Course arrow with its deviation bar and dots, the TO / FROM arrow.
    if let (Some(crs), Some(w)) = (course, steer) {
        let d = dir(crs);
        let side = Vec2::new(-d.y, d.x);
        let unit =
            course_dir.map_or(DVec2::new(crs.sin() as f64, crs.cos() as f64), |v| v.normalize());
        // Off the course to its right (km): the course is then left of the aircraft.
        let off_km = (own - w).dot(DVec2::new(unit.y, -unit.x)) * WU / 1000.0;
        let dot = r * 0.17;
        for k in [-2.0, -1.0, 1.0, 2.0] {
            c.ring(o + side * dot * k, 2.0, 1.0, GREY);
        }
        let head = o + d * (r - 20.0);
        c.stroke(o + d * (r * 0.45), head, 1.8, GREEN);
        c.triangle([head + d * 9.0, head + side * 5.0, head - side * 5.0], GREEN);
        c.stroke(o - d * (r * 0.45), o - d * (r - 18.0), 1.8, GREEN);
        let dev = (-off_km / HSI_FULL_SCALE_KM).clamp(-1.0, 1.0) as f32 * 2.0 * dot;
        c.stroke(o + side * dev - d * (r * 0.4), o + side * dev + d * (r * 0.4), 1.8, GREEN);
        let to = (w - own).dot(unit) > 0.0;
        let t = if to { d } else { -d };
        let tip = o + t * (r * 0.32) + side * (r * 0.22);
        c.triangle([tip, tip - t * 8.0 + side * 4.0, tip - t * 8.0 - side * 4.0], GREEN);
    }
    // Bearing pointer to the steerpoint: head at the rose, tail opposite.
    if let Some(b) = bearing {
        let d = dir(b);
        let side = Vec2::new(-d.y, d.x);
        let head = o + d * (r - 1.0);
        c.stroke(o + d * (r - 26.0), head - d * 6.0, 1.6, CYAN);
        c.triangle([head, head - d * 10.0 + side * 5.0, head - d * 10.0 - side * 5.0], CYAN);
        c.stroke(o - d * (r - 26.0), o - d * (r - 2.0), 1.6, CYAN);
    }
    // Aircraft symbol, fixed in the middle.
    c.stroke(o - Vec2::X * 9.0, o + Vec2::X * 9.0, 1.8, WHITE);
    c.stroke(o - Vec2::Y * 6.0, o + Vec2::Y * 9.0, 1.8, WHITE);
    c.stroke(o + Vec2::new(-4.0, 8.0), o + Vec2::new(4.0, 8.0), 1.8, WHITE);

    // Readouts: a caption over each value, left and right of the rose.
    let km = steer.map(|w| ((w - own).length() * WU / 1000.0) as f32);
    let t = p.clock.start_seconds + p.clock.elapsed.unwrap_or(a.time);
    let ms = a.knots() * 0.5144;
    let deg = |b: Option<f32>| {
        b.map_or("---".to_string(), |b| {
            format!("{:03}", (b.to_degrees().rem_euclid(360.0).round() as i32) % 360)
        })
    };
    let left = [
        ("STPT", if steer.is_some() { format!("{}", wps.current + 1) } else { "-".to_string() }),
        (
            "DIS",
            km.map_or("---".to_string(), |k| {
                if k < 100.0 { format!("{k:.1}") } else { format!("{k:.0}") }
            }),
        ),
        (
            "ETA",
            km.filter(|_| ms > 1.0).map_or("--:--".to_string(), |k| {
                let (h, m, _) = crate::mfd::hms(t + k * 1000.0 / ms);
                format!("{h:02}:{m:02}")
            }),
        ),
    ];
    let right = [("BRG", deg(bearing)), ("CRS", deg(course)), ("GS", format!("{:.0}", a.knots()))];
    for (row, ((lc, lv), (rc, rv))) in left.iter().zip(&right).enumerate() {
        let y = BAND + 8 + row as i32 * 46;
        for (x, caption, value, align_right) in [(4, lc, lv, false), (c.w as i32 - 4, rc, rv, true)]
        {
            for (dy, text, colour) in [(0, caption.to_string(), GREY), (12, value.clone(), GREEN)] {
                let text = upper(&text);
                let w = 2 * fonts.width(&text) - 2;
                fonts.text_scaled(c, if align_right { x - w } else { x }, y + dy, &text, colour, 2);
            }
        }
    }
}

/// The SOI's cursor: two short vertical lines.
fn draw_cursor(c: &mut Canvas, p: Vec2) {
    for dx in [-3.0, 3.0] {
        c.stroke(p + Vec2::new(dx, -6.0), p + Vec2::new(dx, 6.0), 1.5, WHITE);
    }
}

/// The HSD's projection: heading up, the aircraft low in the display (or centred).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HsdView {
    /// Aircraft position (wu east, north), heading (radians clockwise from north).
    pub own: DVec2,
    pub heading: f64,
    /// Aircraft on the display and display pixels per world unit.
    pub origin: Vec2,
    pub scale: f64,
    /// Outer ring radius (pixels).
    pub radius: f32,
}

impl HsdView {
    pub fn new(own: DVec2, heading: f64, range_km: f32, centred: bool, size: Vec2) -> Self {
        let (origin, radius) = if centred {
            let o = Vec2::new(size.x / 2.0, size.y / 2.0);
            (o, (size.y / 2.0 - BAND as f32 - 6.0).min(size.x / 2.0 - 6.0))
        } else {
            let o = Vec2::new(size.x / 2.0, size.y * 0.72);
            (o, o.y - BAND as f32 - 6.0)
        };
        let scale = radius as f64 / (range_km as f64 * 1000.0 / WU);
        Self { own, heading, origin, scale, radius }
    }

    pub fn to_screen(self, east: f64, north: f64) -> Vec2 {
        let (de, dn) = (east - self.own.x, north - self.own.y);
        let (s, c) = self.heading.sin_cos();
        let right = de * c - dn * s;
        let ahead = de * s + dn * c;
        self.origin + Vec2::new((right * self.scale) as f32, (-ahead * self.scale) as f32)
    }

    /// This view magnified `zoom` times around display point `cursor`, which lands on `at`.
    pub fn zoomed(self, cursor: Vec2, at: Vec2, zoom: f32) -> Self {
        Self {
            origin: at + (self.origin - cursor) * zoom,
            scale: self.scale * zoom as f64,
            radius: self.radius * zoom,
            ..self
        }
    }

    pub fn to_world(self, p: Vec2) -> DVec2 {
        let d = (p - self.origin).as_dvec2() / self.scale;
        let (right, ahead) = (d.x, -d.y);
        let (s, c) = self.heading.sin_cos();
        self.own + DVec2::new(right * c + ahead * s, -right * s + ahead * c)
    }

    /// Direction on the display of a heading (radians clockwise from north).
    pub fn dir(self, h: f64) -> Vec2 {
        let a = h - self.heading;
        Vec2::new(a.sin() as f32, -a.cos() as f32)
    }
}

/// The HSD view for the aircraft (None without one).
pub fn hsd_view(av: &AvionicsState, ac: Option<(&Aircraft, &WorldPos)>) -> Option<HsdView> {
    let (a, pos) = ac?;
    let (e, n, _) = bevy_to_wu(pos.0);
    let size = Vec2::new(HIRES.0 as f32, HIRES.1 as f32);
    Some(HsdView::new(
        DVec2::new(e, n),
        a.euler().0 as f64,
        HSD_RANGES[av.hsd_range],
        av.hsd_centred,
        size,
    ))
}

/// The HARM display's scope: centre and radius.
fn harm_scope() -> (Vec2, f32) {
    let (w, h) = (HIRES.0 as f32, HIRES.1 as f32);
    (Vec2::new(w / 2.0, h / 2.0), h / 2.0 - BAND as f32 - 4.0)
}

fn draw_hsd(p: &MfdParams, c: &mut Canvas, ac: Option<(&Aircraft, &WorldPos)>, soi: bool) {
    c.clear(BLACK);
    let av = &p.state.av;
    let Some(view) = hsd_view(av, ac) else { return };
    hsd_picture(p, c, view);
    if soi {
        draw_cursor(c, av.hsd_cursor);
        if av.zoom_window() {
            draw_zoom_window(c, av.hsd_cursor, |c, at| {
                hsd_picture(p, c, view.zoomed(av.hsd_cursor, at, ZOOM))
            });
        }
    }
}

/// Everything on the HSD but the cursor, through `view`.
fn hsd_picture(p: &MfdParams, c: &mut Canvas, view: HsdView) {
    let av = &p.state.av;
    let o = view.origin;
    // Range rings (the inner ones dashed) and the compass rose on the outer one.
    for k in 1..3 {
        let r = view.radius * k as f32 / 3.0;
        let steps = 36;
        for i in (0..steps).step_by(2) {
            let a0 = i as f32 / steps as f32 * std::f32::consts::TAU;
            let a1 = (i + 1) as f32 / steps as f32 * std::f32::consts::TAU;
            c.arc(o, r, a0, a1, 1.0, DARK);
        }
    }
    c.ring(o, view.radius, 1.0, GREY);
    for k in 0..36 {
        let h = (k as f64 * 10.0).to_radians();
        let d = view.dir(h);
        let len = if k % 3 == 0 { 7.0 } else { 4.0 };
        c.stroke(o + d * view.radius, o + d * (view.radius - len), 1.0, GREY);
        if k % 9 == 0 {
            let label = ["N", "E", "S", "W"][k / 9];
            let q = o + d * (view.radius - 14.0);
            stroke_text_at(c, q.x, q.y, label, 9.0, 1.2, GREY, Align::Centre);
        }
    }
    // Route: the steerpoints as small squares, the current one ringed.
    let wps: Vec<Vec2> =
        p.waypoints.points.iter().map(|w| view.to_screen(w.east, w.north)).collect();
    for pair in wps.windows(2) {
        c.stroke(pair[0], pair[1], 1.0, WHITE);
    }
    for (i, &q) in wps.iter().enumerate() {
        c.frame(q - Vec2::splat(2.5), q + Vec2::splat(2.5), 1.0, WHITE);
        if i == p.waypoints.current {
            c.ring(q, 6.0, 1.2, WHITE);
        }
    }
    // Threat rings, ground targets, aircraft, missiles.
    let px_per_km = (view.scale * 1000.0 / WU) as f32;
    for k in &p.contacts.list {
        let q = view.to_screen(k.east, k.north);
        match k.kind {
            ContactKind::GroundRadar => {
                let colour = if k.friendly { GREEN } else { RED };
                if let Some(t) = k.threat {
                    if t.ring {
                        let ring = if k.friendly { GREEN } else { YELLOW };
                        threat_ring(c, q, t.km * px_per_km, t.doppler, ring);
                    }
                    let text = String::from_utf8_lossy(&t.label).trim_end_matches('\0').to_string();
                    stroke_text_at(c, q.x, q.y, &text, 9.0, 1.3, colour, Align::Centre);
                } else {
                    c.fill_rect(q.x as i32 - 2, q.y as i32 - 2, 4, 4, colour);
                }
            }
            ContactKind::Ship => {
                // A hull with its superstructure.
                let colour = if k.friendly { GREEN } else { RED };
                c.polyline(
                    &[
                        q + Vec2::new(-7.0, -1.0),
                        q + Vec2::new(7.0, -1.0),
                        q + Vec2::new(4.0, 3.0),
                        q + Vec2::new(-4.0, 3.0),
                        q + Vec2::new(-7.0, -1.0),
                    ],
                    1.3,
                    colour,
                );
                c.polyline(
                    &[
                        q + Vec2::new(-2.0, -1.0),
                        q + Vec2::new(-2.0, -4.0),
                        q + Vec2::new(2.0, -4.0),
                        q + Vec2::new(2.0, -1.0),
                    ],
                    1.3,
                    colour,
                );
                if let Some(t) = k.threat {
                    threat_ring(c, q, t.km * px_per_km, t.doppler, YELLOW);
                }
            }
            ContactKind::Target => {
                c.polyline(
                    &[
                        q + Vec2::new(0.0, -5.0),
                        q + Vec2::new(5.0, 0.0),
                        q + Vec2::new(0.0, 5.0),
                        q + Vec2::new(-5.0, 0.0),
                        q + Vec2::new(0.0, -5.0),
                    ],
                    1.3,
                    CYAN,
                );
            }
            ContactKind::Aircraft => {
                let d = view.dir(k.heading as f64 / 65536.0 * std::f64::consts::TAU);
                if k.friendly {
                    c.ring(q, 4.0, 1.3, GREEN);
                    c.stroke(q + d * 4.0, q + d * 9.0, 1.3, GREEN);
                } else {
                    let side = Vec2::new(-d.y, d.x);
                    c.polyline(
                        &[
                            q + d * 5.0,
                            q - d * 3.0 + side * 4.0,
                            q - d * 3.0 - side * 4.0,
                            q + d * 5.0,
                        ],
                        1.3,
                        RED,
                    );
                    c.stroke(q + d * 5.0, q + d * 10.0, 1.3, RED);
                }
            }
            ContactKind::Drone => {
                c.stroke(q - Vec2::splat(3.0), q + Vec2::splat(3.0), 1.2, RED);
                c.stroke(q + Vec2::new(-3.0, 3.0), q + Vec2::new(3.0, -3.0), 1.2, RED);
            }
            ContactKind::Missile => {
                let d = view.dir(k.heading as f64 / 65536.0 * std::f64::consts::TAU);
                c.stroke(q - d * 3.0, q + d * 3.0, 1.5, if k.friendly { GREEN } else { RED });
            }
        }
        if k.target.is_some() && k.target == p.contacts.locked {
            c.frame(q - Vec2::splat(8.0), q + Vec2::splat(8.0), 1.3, WHITE);
        }
    }
    // Where the targeting pod looks: a diamond on its ground point, a dashed line to it.
    if let Some(g) = av.tgp.los_ground.filter(|_| av.shown(Format::Tgp).is_some()) {
        let (e, n, _) = bevy_to_wu(g);
        let q = view.to_screen(e, n);
        c.dashed(o, q, 4.0, 3.0, 1.0, GREEN);
        c.polyline(
            &[
                q + Vec2::new(0.0, -6.0),
                q + Vec2::new(6.0, 0.0),
                q + Vec2::new(0.0, 6.0),
                q + Vec2::new(-6.0, 0.0),
                q + Vec2::new(0.0, -6.0),
            ],
            1.5,
            GREEN,
        );
    }
    // Own aircraft.
    c.stroke(o + Vec2::new(0.0, -6.0), o + Vec2::new(0.0, 6.0), 1.6, CYAN);
    c.stroke(o + Vec2::new(-6.0, -1.0), o + Vec2::new(6.0, -1.0), 1.6, CYAN);
    c.stroke(o + Vec2::new(-3.0, 5.0), o + Vec2::new(3.0, 5.0), 1.6, CYAN);
}

/// A threat ring: solid around a Doppler radar, dotted around a pulse one.
fn threat_ring(c: &mut Canvas, centre: Vec2, r: f32, doppler: bool, colour: u8) {
    if doppler {
        c.ring(centre, r, 1.0, colour);
        return;
    }
    // Dots about 2 px long, 5 px apart.
    let n = ((r * std::f32::consts::TAU / 5.0) as i32).clamp(12, 720);
    let step = std::f32::consts::TAU / n as f32;
    let dot = (2.0 / r.max(1.0)).min(step * 0.5);
    for i in 0..n {
        let a = i as f32 * step;
        c.arc(centre, r, a, a + dot, 1.0, colour);
    }
}

/// HARM display: the radars painting the aircraft (filtered), placed by danger like the RWR.
fn draw_harm(p: &MfdParams, c: &mut Canvas, soi: bool) {
    c.clear(BLACK);
    let av = &p.state.av;
    let (centre, r) = harm_scope();
    harm_picture(p, c, centre, r);
    if p.emitters.list.iter().all(|e| !harm_shows(av.harm_filter, e)) {
        stroke_text_at(
            c,
            centre.x,
            centre.y + r * 0.5,
            "NO EMITTERS",
            8.0,
            1.0,
            NAVY.max(DARK),
            Align::Centre,
        );
    }
    if soi {
        draw_cursor(c, av.harm_cursor);
        if av.zoom_window() {
            let cursor = av.harm_cursor;
            draw_zoom_window(c, cursor, |c, at| {
                harm_picture(p, c, at + (centre - cursor) * ZOOM, r * ZOOM)
            });
        }
    }
}

/// The HARM scope and its symbols, centred at `centre` with radius `r`.
fn harm_picture(p: &MfdParams, c: &mut Canvas, centre: Vec2, r: f32) {
    let av = &p.state.av;
    c.ring(centre, r, 1.0, GREY);
    for f in [0.33, 0.66] {
        c.ring(centre, r * f, 1.0, DARK);
    }
    for k in 0..12 {
        let a = k as f32 * std::f32::consts::TAU / 12.0;
        let d = Vec2::new(a.sin(), -a.cos());
        c.stroke(centre + d * r, centre + d * (r - 5.0), 1.0, GREY);
    }
    c.stroke(centre - Vec2::X * 4.0, centre + Vec2::X * 4.0, 1.0, GREY);
    c.stroke(centre - Vec2::Y * 4.0, centre + Vec2::Y * 4.0, 1.0, GREY);
    let locked = p.contacts.locked.and_then(emitter_source);
    let filter = av.harm_filter;
    crate::rwr::draw_symbols(
        c,
        &p.emitters.list,
        centre,
        r,
        crate::hud::blink(&p.time),
        locked,
        |e| harm_shows(filter, e),
    );
}

/// Magnification of the zoomed window and its size (display pixels).
const ZOOM: f32 = 4.0;
const ZOOM_WINDOW: Vec2 = Vec2::new(104.0, 80.0);

/// The zoomed window shown while "lock cursor target" is held: in the corner away from the
/// cursor, `draw` paints the page magnified around the cursor, which sits in the middle.
fn draw_zoom_window(c: &mut Canvas, cursor: Vec2, draw: impl FnOnce(&mut Canvas, Vec2)) {
    let size = Vec2::new(c.w as f32, c.h as f32);
    let x = if cursor.x > size.x / 2.0 { 3.0 } else { size.x - 3.0 - ZOOM_WINDOW.x };
    let y = if cursor.y > size.y / 2.0 {
        BAND as f32 + 3.0
    } else {
        size.y - BAND as f32 - 3.0 - ZOOM_WINDOW.y
    };
    let (a, b) = (Vec2::new(x, y), Vec2::new(x, y) + ZOOM_WINDOW);
    c.fill_rect(a.x as i32, a.y as i32, ZOOM_WINDOW.x as i32, ZOOM_WINDOW.y as i32, BLACK);
    c.set_clip(a.x as i32 + 1, a.y as i32 + 1, b.x as i32 - 2, b.y as i32 - 2);
    let at = (a + b) / 2.0;
    draw(c, at);
    draw_cursor(c, at);
    c.reset_clip();
    c.frame(a, b - Vec2::ONE, 1.5, WHITE);
    stroke_text_at(c, a.x + 5.0, b.y - 10.0, "X4", 9.0, 1.3, WHITE, Align::Left);
}

/// The HARM display shows the radars heard lately ([`EmitterView::heard`]) that pass its filter.
fn harm_shows(filter: HarmFilter, e: &EmitterView) -> bool {
    e.heard
        && match filter {
            HarmFilter::All => true,
            HarmFilter::Sam => !e.air,
            HarmFilter::Air => e.air,
        }
}

fn emitter_source(r: ContactRef) -> Option<stealth_sim::EmitterSource> {
    match r {
        ContactRef::Unit(i) => Some(stealth_sim::EmitterSource::Aircraft(i)),
        ContactRef::Object(i) => Some(stealth_sim::EmitterSource::Site(i)),
        ContactRef::Pilot(_) | ContactRef::Point(..) => None,
    }
}

// ------------------------------------------------------------------------------------------
// Cursor slew and lock

/// A lock on a simulation object picked on a display (`combat` passes it to the simulation).
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct LockRequest(pub ContactRef);

/// The targeting pod locks what is at this map point (`combat` passes it to the simulation).
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct PodDesignate(pub i32, pub i32);

/// The targeting pod: slewed as the SOI, tracking; "lock cursor target" locks the ground point
/// it holds (then it follows the lock).
#[allow(clippy::too_many_arguments)]
fn update_pod(
    time: Res<Time>,
    controls: Res<FlightControls>,
    mut actions: MessageReader<ActionEvent>,
    mut state: ResMut<MfdState>,
    info: Res<crate::mfd::TrackCam>,
    mut hud: ResMut<HudState>,
    aircraft: Query<(&Aircraft, &WorldPos), With<Player>>,
    camera: Query<&Transform, With<crate::trackcam::TrackCamera>>,
    eyes: Query<&GlobalTransform, (With<Camera3d>, Without<crate::camera::OffscreenCamera>)>,
    view: Res<crate::camera::ViewState>,
    settings: Res<crate::settings::Settings>,
    mut combat: Option<ResMut<crate::combat::Combat>>,
    mut designate: MessageWriter<PodDesignate>,
    waypoints: Res<crate::hud::Waypoints>,
) {
    let lock = actions.read().any(|a| a.0 == Action::LockCursor);
    let Ok((a, pos)) = aircraft.single() else { return };
    // The helmet sight, with a helmet-mounted display: the gunship's slaved pod follows it
    // whether its page is up or not (the page shows what it looks at), the Sidewinder seeker
    // too.
    let helmet = settings.aircraft.spec().hmd != crate::aircraft::Hmd::None;
    let sight = helmet_sight(&eyes, view.view).filter(|_| helmet);
    let body = sight.map(|d| {
        let d = a.orientation.inverse() * d;
        state.av.helmet_body.map_or(d, |s| smooth_sight(s, d, time.delta_secs()))
    });
    state.av.helmet_body = body;
    state.av.helmet = body.map(|d| a.orientation * d);
    // The gunship's pod is always up (its turret and Hellfires follow it).
    let turret = settings.aircraft.spec().turret;
    if !state.modern && !turret {
        return;
    }
    // Snowplow: nothing stays locked (nor is acquired until the next target key).
    if std::mem::take(&mut state.av.tgp.clear)
        && let Some(combat) = &mut combat
    {
        combat.clear_target();
    }
    let shown = state.camera_shown();
    let looking = if turret {
        slaved_sight(combat.as_deref(), pos.0)
            .or(state.av.helmet)
            .or_else(|| Some(a.orientation * pod_dir(0.0, -0.05)))
    } else {
        camera.single().ok().filter(|_| shown && info.live).map(|t| t.forward().as_vec3())
    };
    let ground_y = crate::world::wu_to_bevy(0.0, 0.0, a.ground_altitude as f64).y;
    // CZ: point track on the current steerpoint, the pod the SOI to slew on from there.
    if std::mem::take(&mut state.av.tgp.cz)
        && let Some(wp) = waypoints.target()
    {
        let p = crate::world::wu_to_bevy(wp.east, wp.north, a.ground_altitude as f64);
        state.av.tgp.mode = TgpMode::Point(p);
        state.av.tgp.idle = 0.0;
        if shown {
            state.av.soi = Some(Format::Tgp);
        }
    }
    let soi = state.av.soi_format() == Some(Format::Tgp);
    let zoom = state.cam_zoom.unwrap_or(info.zoom.saturating_sub(1));
    let slew = if soi && shown { controls.slew } else { Vec2::ZERO };
    let tgp = &mut state.av.tgp;
    let los = pod_step(tgp, slew, time.delta_secs(), zoom, a.orientation, pos.0, looking, ground_y);
    tgp.los_ground = los.filter(|_| shown || turret);
    tgp.los = pod_direction(tgp, a.orientation, pos.0).or(looking).filter(|_| shown || turret);
    if lock && soi && tgp.mode != TgpMode::Slaved {
        match los {
            Some(p) => {
                let (e, n, _) = bevy_to_wu(p);
                designate.write(PodDesignate(
                    (e / 32.0).round() as i32,
                    (0x8000 as f64 - n / 32.0).round() as i32,
                ));
                hud.mode = HudMode::Gnd;
                tgp.mode = TgpMode::Slaved;
            }
            None => tgp.mode = TgpMode::Slaved,
        }
    }
}

/// Where the gunship's slaved pod looks: at the lock (else the helmet sight).
fn slaved_sight(combat: Option<&crate::combat::Combat>, eye: DVec3) -> Option<Vec3> {
    // As the tracking camera: the ground lock, else the aircraft or pilot locked.
    combat.and_then(|c| {
        let (sim, t) = (&c.sim, c.sim.targeting());
        let at = if let Some(o) = t.ground.and_then(|g| sim.objects().get(g)) {
            let (e, n) = crate::combat::map_wu(o.x, o.y);
            crate::world::wu_to_bevy(e + 16.0, n - 16.0, 0.0)
        } else if let Some((i, u)) = t.air.and_then(|i| sim.flights().get(i).map(|u| (i, *u))) {
            let (e, n, z) = crate::combat::unit_position(c.prev_flights.get(i), &u, c.alpha);
            crate::world::wu_to_bevy(e, n, z)
        } else {
            let ((e, n, z), _, _) = t.pilot.and_then(|k| crate::combat::remote_motion(c, k))?;
            crate::world::wu_to_bevy(e, n, z)
        };
        (at - eye).as_vec3().try_normalize()
    })
}

/// Where the pilot looks from the cockpit (the helmet sight: the eye camera, the headset in
/// VR). None in the outside views.
fn helmet_sight(
    eyes: &Query<&GlobalTransform, (With<Camera3d>, Without<crate::camera::OffscreenCamera>)>,
    view: crate::camera::View,
) -> Option<Vec3> {
    let cockpit = matches!(
        view,
        crate::camera::View::Cockpit
            | crate::camera::View::LookLeft
            | crate::camera::View::LookRight
            | crate::camera::View::LookRear
    );
    let sum: Vec3 = eyes.iter().map(|t| t.forward().as_vec3()).sum();
    sum.try_normalize().filter(|_| cockpit)
}

/// Helmet sight smoothing: the time constant (seconds) while the head is nearly still, and
/// the angle (radians) behind the head at which it is halved (a deliberate turn follows fast).
const HELMET_LAG: f32 = 0.12;
const HELMET_FOLLOW: f32 = 0.03;

/// Moves the smoothed sight `s` toward the head's direction `d` (both unit vectors).
fn smooth_sight(s: Vec3, d: Vec3, dt: f32) -> Vec3 {
    let angle = s.angle_between(d);
    let tau = HELMET_LAG / (1.0 + angle / HELMET_FOLLOW);
    let k = 1.0 - (-dt / tau).exp();
    s.slerp(d, k).try_normalize().unwrap_or(d)
}

/// Cursor speed at full slew (display pixels per second).
const SLEW_SPEED: f32 = 110.0;
/// A lock picks the nearest symbol within this many pixels of the cursor.
const PICK_RADIUS: f32 = 14.0;

/// Moves the SOI's cursor with the slew controls; "lock cursor target" locks the nearest
/// symbol under it, switching the HUD to the matching mode.
#[allow(clippy::too_many_arguments)]
fn slew_and_lock(
    time: Res<Time>,
    controls: Res<FlightControls>,
    mut actions: MessageReader<ActionEvent>,
    mut state: ResMut<MfdState>,
    contacts: Res<Contacts>,
    emitters: Res<Emitters>,
    mut hud: ResMut<HudState>,
    aircraft: Query<(&Aircraft, &WorldPos), With<Player>>,
    mut locks: MessageWriter<LockRequest>,
) {
    let lock = actions.read().any(|a| a.0 == Action::LockCursor);
    if !state.modern {
        return;
    }
    let size = Vec2::new(HIRES.0 as f32, HIRES.1 as f32);
    let lo = Vec2::new(4.0, BAND as f32 + 4.0);
    let hi = size - lo;
    let view = hsd_view(&state.av, aircraft.single().ok());
    follow_view(&mut state.av, view, lo, hi);
    let f = state.av.soi_format().filter(|f| matches!(f, Format::Hsd | Format::Harm));
    let av = &mut state.av;
    // Holding the lock shows the zoomed window and slews finely; letting go locks again, now
    // picking only what is right under the cursor in the window.
    let zoomed = av.zoom_window();
    av.mark_held =
        if controls.mark && f.is_some() { av.mark_held + time.delta_secs() } else { 0.0 };
    let release = zoomed && av.mark_held == 0.0;
    let Some(f) = f else { return };
    let fine = if zoomed { ZOOM } else { 1.0 };
    let delta =
        Vec2::new(controls.slew.x, -controls.slew.y) * SLEW_SPEED / fine * time.delta_secs();
    let cursor = match f {
        Format::Hsd => &mut av.hsd_cursor,
        _ => &mut av.harm_cursor,
    };
    if delta != Vec2::ZERO {
        *cursor = (*cursor + delta).clamp(lo, hi);
    }
    if !lock && !release {
        return;
    }
    let cursor = *cursor;
    let radius = if release { PICK_RADIUS / ZOOM } else { PICK_RADIUS };
    let picked = match f {
        Format::Hsd => view.and_then(|view| pick_contact(&contacts, &view, cursor, radius)),
        _ => {
            let (centre, r) = harm_scope();
            let filter = av.harm_filter;
            nearest(
                emitters
                    .list
                    .iter()
                    .filter(|e| harm_shows(filter, e))
                    .map(|e| (crate::rwr::scope_point(e, centre, r), emitter_ref(e))),
                cursor,
                radius,
            )
        }
    };
    if let Some(target) = picked {
        av.tgp.mode = TgpMode::Slaved;
        hud.mode = match target {
            ContactRef::Unit(_) | ContactRef::Pilot(_) => HudMode::Air,
            ContactRef::Object(_) | ContactRef::Point(..) => HudMode::Gnd,
        };
        locks.write(LockRequest(target));
    }
}

/// Keeps the cursors on what they point at as the view changes: the HSD cursor on its ground
/// point (through turns, flight and range changes), the HARM cursor at its bearing (the HARM
/// display is heading up, so it turns against the heading like the emitter symbols).
fn follow_view(av: &mut AvionicsState, view: Option<HsdView>, lo: Vec2, hi: Vec2) {
    let Some(view) = view else {
        av.cursor_view = None;
        return;
    };
    if let Some(prev) = av.cursor_view.filter(|prev| *prev != view) {
        let g = prev.to_world(av.hsd_cursor);
        av.hsd_cursor = view.to_screen(g.x, g.y).clamp(lo, hi);
        let (centre, _) = harm_scope();
        let turn = (prev.heading - view.heading) as f32;
        let (s, c) = turn.sin_cos();
        let d = av.harm_cursor - centre;
        av.harm_cursor = (centre + Vec2::new(d.x * c - d.y * s, d.x * s + d.y * c)).clamp(lo, hi);
    }
    av.cursor_view = Some(view);
}

fn emitter_ref(e: &EmitterView) -> ContactRef {
    match e.source {
        stealth_sim::EmitterSource::Site(i) => ContactRef::Object(i),
        stealth_sim::EmitterSource::Aircraft(i) => ContactRef::Unit(i),
        // A Shilka: whatever lies where it stands.
        stealth_sim::EmitterSource::Gun(_) => {
            let (east, north) = (e.east, e.north);
            ContactRef::Point((east / 32.0) as i32, (0x8000 as f64 - north / 32.0) as i32)
        }
    }
}

/// The lockable contact nearest the cursor on the HSD: a hostile one if any is in reach,
/// else a friendly one.
pub fn pick_contact(
    contacts: &Contacts,
    view: &HsdView,
    cursor: Vec2,
    radius: f32,
) -> Option<ContactRef> {
    let pick = |friendly: bool| {
        nearest(
            contacts
                .list
                .iter()
                .filter(|k| k.friendly == friendly)
                .filter_map(|k| Some((view.to_screen(k.east, k.north), k.target?))),
            cursor,
            radius,
        )
    };
    pick(false).or_else(|| pick(true))
}

fn nearest(
    candidates: impl Iterator<Item = (Vec2, ContactRef)>,
    cursor: Vec2,
    radius: f32,
) -> Option<ContactRef> {
    candidates
        .map(|(q, t)| (q.distance(cursor), t))
        .filter(|(d, _)| *d <= radius)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, t)| t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mfd::Contact;

    #[test]
    fn selecting_the_other_mfds_format_swaps() {
        let mut av = AvionicsState::default();
        assert_eq!(av.formats[..2], [Format::Hsd, Format::Sms]);
        av.select(MfdSide::Right, Format::Hsd);
        assert_eq!(av.formats[..2], [Format::Sms, Format::Hsd]);
        av.select(MfdSide::Left, Format::Harm);
        assert_eq!(av.formats[..2], [Format::Harm, Format::Hsd]);
        // The SOI stays with its format.
        assert_eq!(av.soi_format(), Some(Format::Hsd));
        av.select(MfdSide::Right, Format::Map);
        assert_eq!(
            av.soi_format(),
            Some(Format::Harm),
            "SOI moves to the remaining capable format"
        );
        av.select(MfdSide::Left, Format::Tgp);
        assert_eq!(av.soi_format(), Some(Format::Tgp));
        av.select(MfdSide::Left, Format::Sms);
        assert_eq!(av.soi_format(), None);
    }

    /// With the F-117A's third MFD: it starts on the HSI, swaps with either other MFD, and
    /// without it its format counts as shown nowhere.
    #[test]
    fn three_mfds() {
        let mut av = AvionicsState::default();
        assert_eq!(av.shown(Format::Hsi), None);
        av.set_third(true);
        assert_eq!(av.formats, [Format::Hsd, Format::Sms, Format::Hsi]);
        av.select(MfdSide::Third, Format::Hsd);
        assert_eq!(av.formats, [Format::Hsi, Format::Sms, Format::Hsd]);
        assert_eq!(av.soi_format(), Some(Format::Hsd), "the SOI goes with its format");
        av.select(MfdSide::Right, Format::Hsd);
        assert_eq!(av.formats, [Format::Hsi, Format::Hsd, Format::Sms]);
        av.select(MfdSide::Left, Format::Tgp);
        assert_eq!(av.formats, [Format::Tgp, Format::Hsd, Format::Sms]);
        // Removed and back, it shows the HSI again.
        av.select(MfdSide::Third, Format::Hsi);
        av.set_third(false);
        av.set_third(true);
        assert_eq!(av.format(MfdSide::Third), Format::Hsi);
        // A third MFD whose HSI went to another MFD takes a format not shown elsewhere.
        av.select(MfdSide::Left, Format::Hsi);
        av.set_third(false);
        av.set_third(true);
        let [l, r, t] = av.formats;
        assert!(t != l && t != r, "{:?}", av.formats);
    }

    /// The sensor of interest goes round the three MFDs' capable formats.
    #[test]
    fn soi_round_three_mfds() {
        let mut s = MfdState { modern: true, ..default() };
        let mut raster = MapRaster::default();
        s.av.set_third(true);
        s.av.select(MfdSide::Right, Format::Harm);
        s.av.select(MfdSide::Third, Format::Tgp);
        assert_eq!(s.av.soi_format(), Some(Format::Hsd));
        apply(&mut s, &mut raster, MfdCmd::SwapSoi);
        assert_eq!(s.av.soi_format(), Some(Format::Harm));
        apply(&mut s, &mut raster, MfdCmd::SwapSoi);
        assert_eq!(s.av.soi_format(), Some(Format::Tgp));
        apply(&mut s, &mut raster, MfdCmd::SwapSoi);
        assert_eq!(s.av.soi_format(), Some(Format::Hsd));
    }

    #[test]
    fn soi_commands() {
        let mut s = MfdState { modern: true, ..default() };
        let mut raster = MapRaster::default();
        s.av.select(MfdSide::Right, Format::Harm);
        apply(&mut s, &mut raster, MfdCmd::Soi(MfdSide::Right));
        assert_eq!(s.av.soi_format(), Some(Format::Harm));
        apply(&mut s, &mut raster, MfdCmd::SwapSoi);
        assert_eq!(s.av.soi_format(), Some(Format::Hsd));
        // Not on a format without a cursor.
        s.av.select(MfdSide::Right, Format::Sms);
        apply(&mut s, &mut raster, MfdCmd::Soi(MfdSide::Right));
        assert_eq!(s.av.soi_format(), Some(Format::Hsd));
    }

    #[test]
    fn every_button_on_every_format() {
        for side in [MfdSide::Left, MfdSide::Third] {
            for f in formats(side) {
                for (way, ord) in [(false, false), (true, true)] {
                    let mut s = MfdState { modern: true, ..default() };
                    s.av.set_third(true);
                    s.av.select(side, f);
                    s.av.map_way = way;
                    s.av.sms_ord = ord;
                    for slot in 0..crate::mfd::OSB_COUNT {
                        if let Some((label, _, _)) = osb(side, slot, &s) {
                            assert!(label.len() <= 5, "{f:?} {slot}: {label}");
                        }
                    }
                    assert!(osb(side, 5, &s).is_some());
                }
            }
        }
        // The third MFD's bottom row: the HSI first.
        let s = MfdState { modern: true, ..default() };
        assert_eq!(osb(MfdSide::Third, 5, &s).map(|o| o.0), Some("HSI"));
    }

    /// The ground point of an area track.
    fn area_point(tgp: &TgpState) -> DVec3 {
        let TgpMode::Area(p) = tgp.mode else { panic!("{:?}", tgp.mode) };
        p
    }

    #[test]
    fn pod_area_then_point_track() {
        let origin = DVec3::new(0.0, 1000.0, 0.0);
        let mut tgp = TgpState::default();
        let ahead_down = Vec3::new(0.0, -0.5, -1.0).normalize();
        let start = ground_hit(origin, ahead_down, 0.0).unwrap();
        let r = (start - origin).length();
        // Slewing right from where the slaved pod looks: the ground point moves right.
        let p0 = pod_step(&mut tgp, Vec2::X, 0.5, 0, Quat::IDENTITY, origin, Some(ahead_down), 0.0);
        let p = area_point(&tgp);
        assert!((p.x - r * 0.25).abs() < 1e-3 && (p.z - start.z).abs() < 1e-3, "{p}");
        assert_eq!(p0, Some(p));
        // Zoomed in, slower.
        let mut z = tgp.clone();
        pod_step(&mut z, Vec2::X, 0.5, 2, Quat::IDENTITY, origin, None, 0.0);
        let r2 = (p - origin).length();
        let moved = (area_point(&z) - p).length();
        assert!((moved - r2 * 0.0625).abs() < 1e-3, "{moved}");
        // Released: point track on the ground point after a moment.
        pod_step(&mut tgp, Vec2::ZERO, 0.1, 0, Quat::IDENTITY, origin, None, 0.0);
        assert!(matches!(tgp.mode, TgpMode::Area(_)));
        let p = pod_step(&mut tgp, Vec2::ZERO, 0.1, 0, Quat::IDENTITY, origin, None, 0.0);
        let TgpMode::Point(q) = tgp.mode else { panic!("{:?}", tgp.mode) };
        assert_eq!(p, Some(q));
        // It stays on that point as the aircraft moves.
        assert!(pod_direction(&tgp, Quat::IDENTITY, origin + DVec3::Z * 500.0).unwrap().z < -0.0);
    }

    #[test]
    fn pod_slew_ignores_aircraft_motion() {
        // Slewing right while the aircraft pitches and rolls: the ground point moves as it would
        // in level flight.
        let slew = |pitch: f32, roll: f32| {
            let mut origin = DVec3::new(0.0, 1000.0, 0.0);
            let start = DVec3::new(0.0, 0.0, -5000.0);
            let mut tgp = TgpState { mode: TgpMode::Point(start), ..default() };
            for i in 0..20 {
                let t = i as f32 * 0.05;
                let attitude = Quat::from_rotation_x(pitch * t) * Quat::from_rotation_z(roll * t);
                pod_step(&mut tgp, Vec2::X, 0.05, 0, attitude, origin, None, 0.0);
                origin += DVec3::new(0.0, 0.0, -12.0);
            }
            area_point(&tgp)
        };
        let level = slew(0.0, 0.0);
        assert!(level.x > 1000.0, "{level}");
        let rolled = slew(0.3, -0.8);
        assert!((rolled - level).length() < 0.01, "{rolled} {level}");
        // Above the horizon it holds its direction while the aircraft turns.
        let origin = DVec3::new(0.0, 1000.0, 0.0);
        let sky = origin + pod_dir(0.0, 0.2).as_dvec3() * POD_SKY;
        let mut tgp = TgpState { mode: TgpMode::Area(sky), ..default() };
        pod_step(&mut tgp, Vec2::Y, 0.2, 0, Quat::IDENTITY, origin, None, 0.0);
        let d = pod_direction(&tgp, Quat::IDENTITY, origin).unwrap();
        assert!(d.y > 0.0, "{d}");
        let pitched = Quat::from_rotation_x(0.2);
        pod_step(&mut tgp, Vec2::ZERO, 0.5, 0, pitched, origin, None, 0.0);
        assert!((pod_direction(&tgp, pitched, origin).unwrap() - d).length() < 1e-4);
    }

    #[test]
    fn pod_slew_fine_at_shallow_angles() {
        // 25 km out from 1000 m: up and down move the line of sight far less than left and right
        // (the ground point moves alike either way).
        let origin = DVec3::new(0.0, 1000.0, 0.0);
        let start = DVec3::new(0.0, 0.0, -25_000.0);
        let angles =
            |tgp: &TgpState| pod_angles(pod_direction(tgp, Quat::IDENTITY, origin).unwrap());
        let (az0, el0) = pod_angles((start - origin).as_vec3());
        let mut right = TgpState { mode: TgpMode::Point(start), ..default() };
        pod_step(&mut right, Vec2::X, 0.1, 0, Quat::IDENTITY, origin, None, 0.0);
        let mut up = TgpState { mode: TgpMode::Point(start), ..default() };
        pod_step(&mut up, Vec2::Y, 0.1, 0, Quat::IDENTITY, origin, None, 0.0);
        let daz = angles(&right).0 - az0;
        let del = angles(&up).1 - el0;
        assert!(daz > 0.0 && del > 0.0 && del < daz / 5.0, "{daz} {del}");
        // Still on the ground (farther out), not above the horizon.
        assert!(area_point(&up).z < start.z && area_point(&up).y.abs() < 1e-6);
    }

    #[test]
    fn pod_slew_finer_far_out() {
        // Up close: linear, at once.
        assert_eq!(slew_gain(Vec2::new(0.5, -1.0), Some(1000.0), 0.0), Vec2::new(0.5, -1.0));
        assert_eq!(slew_gain(Vec2::X, None, 0.0), Vec2::X);
        // Far out: a half deflection much finer, a held slew building up to the full rate.
        let far = Some(30_000.0);
        let half = slew_gain(Vec2::new(0.5, 0.0), far, SLEW_RAMP).x;
        assert!((half - 0.125).abs() < 1e-5, "{half}");
        let tap = slew_gain(Vec2::NEG_Y, far, 0.0).y;
        assert!((tap + SLEW_START_FAR).abs() < 1e-5, "{tap}");
        assert_eq!(slew_gain(Vec2::NEG_Y, far, SLEW_RAMP), Vec2::NEG_Y);
        // In between, in between.
        let mid = slew_gain(Vec2::new(0.5, 0.0), Some(11_500.0), SLEW_RAMP).x;
        assert!(mid < 0.5 && mid > 0.125, "{mid}");
        // A slew held far out ramps up; letting go starts over (moves relative to the range).
        let origin = DVec3::new(0.0, 1000.0, 0.0);
        let mut tgp =
            TgpState { mode: TgpMode::Area(DVec3::new(0.0, 0.0, -25_000.0)), ..default() };
        let step = |tgp: &mut TgpState, s: Vec2| {
            let before = area_point(tgp);
            pod_step(tgp, s, 0.1, 0, Quat::IDENTITY, origin, None, 0.0);
            (area_point(tgp) - before).length() / (before - origin).length()
        };
        let first = step(&mut tgp, Vec2::X);
        let later = (0..10).map(|_| step(&mut tgp, Vec2::X)).last().unwrap();
        assert!(later > 4.0 * first, "{first} {later}");
        assert!((later - POD_RATE as f64 * 0.1).abs() < 1e-4, "{later}");
        step(&mut tgp, Vec2::ZERO);
        assert!((step(&mut tgp, Vec2::X) - first).abs() < 1e-4);
    }

    #[test]
    fn camera_zoom_steps() {
        use crate::mfd::{CAM_ZOOM_MAX, cam_zoom, cam_zoom_label};
        let mut s = MfdState::default();
        // From the range's zoom.
        cam_zoom(&mut s, MfdCmd::CamZoom(1), 2);
        assert_eq!(s.cam_zoom, Some(3));
        for _ in 0..9 {
            cam_zoom(&mut s, MfdCmd::CamZoom(1), 2);
        }
        assert_eq!(s.cam_zoom, Some(CAM_ZOOM_MAX));
        cam_zoom(&mut s, MfdCmd::CamZoomCycle, 0);
        assert_eq!((s.cam_zoom, cam_zoom_label(s.cam_zoom)), (None, "AUTO"));
        cam_zoom(&mut s, MfdCmd::CamZoomCycle, 4);
        assert_eq!(cam_zoom_label(s.cam_zoom), "1X");
        cam_zoom(&mut s, MfdCmd::CamZoom(-1), 4);
        assert_eq!(s.cam_zoom, Some(0));
    }

    #[test]
    fn pod_snowplow_and_slave() {
        let mut s = MfdState { modern: true, ..default() };
        let raster = &mut MapRaster::default();
        s.av.tgp.mode = TgpMode::Point(DVec3::new(100.0, 0.0, -2000.0));
        // SP only with the pod the SOI.
        apply(&mut s, raster, MfdCmd::TgpSp);
        assert!(matches!(s.av.tgp.mode, TgpMode::Point(_)) && !s.av.tgp.clear);
        s.av.select(MfdSide::Right, Format::Tgp);
        apply(&mut s, raster, MfdCmd::Soi(MfdSide::Right));
        apply(&mut s, raster, MfdCmd::TgpSp);
        assert_eq!((s.av.tgp.mode, s.av.tgp.clear), (TgpMode::Snowplow, true));
        assert_eq!(osb(MfdSide::Right, 2, &s).map(|o| (o.0, o.2)), Some(("SP", true)));
        // Fixed ahead of the nose, turning with the aircraft, without holding the ground.
        let origin = DVec3::new(0.0, 1000.0, 0.0);
        let turned = Quat::from_rotation_y(-0.5);
        let p = pod_step(&mut s.av.tgp, Vec2::ZERO, 1.0, 0, turned, origin, None, 0.0);
        assert_eq!(s.av.tgp.mode, TgpMode::Snowplow);
        let d = pod_direction(&s.av.tgp, turned, origin).unwrap();
        assert!((d - turned * pod_dir(0.0, SP_EL)).length() < 1e-5);
        assert!(p.is_some_and(|p| p.x > 0.0));
        // A slew starts from there.
        pod_step(&mut s.av.tgp, Vec2::Y, 0.5, 0, turned, origin, None, 0.0);
        let d = pod_direction(&s.av.tgp, turned, origin).unwrap();
        let (az, el) = pod_angles(turned.inverse() * d);
        // (Farther out on the ground, finer at the shallow angle.)
        assert!(az.abs() < 1e-4 && el > SP_EL && el < SP_EL + 0.1, "{az} {el}");
        // A target key slaves it again; CZ (to the steerpoint), but not in A-A.
        apply(&mut s, raster, MfdCmd::TgpSlave);
        assert_eq!(s.av.tgp.mode, TgpMode::Slaved);
        let cz = osb(MfdSide::Right, 3, &s).map(|o| (o.0, o.1));
        assert_eq!(cz, Some(("CZ", OsbFn::Cmd(MfdCmd::TgpCz))));
        apply(&mut s, raster, MfdCmd::TgpCz);
        assert!(s.av.tgp.cz);
        s.air = true;
        assert_eq!(osb(MfdSide::Right, 3, &s), None);
    }

    #[test]
    fn ground_hits() {
        let o = DVec3::new(10.0, 100.0, 0.0);
        assert_eq!(ground_hit(o, Vec3::NEG_Y, 0.0), Some(DVec3::new(10.0, 0.0, 0.0)));
        assert_eq!(ground_hit(o, Vec3::Y, 0.0), None);
        assert_eq!(ground_hit(o, Vec3::new(1.0, -1e-5, 0.0), 0.0), None);
        let (az, el) = pod_angles(pod_dir(0.7, -0.3));
        assert!((az - 0.7).abs() < 1e-5 && (el + 0.3).abs() < 1e-5);
    }

    #[test]
    fn hsd_projection_round_trips() {
        let v = HsdView::new(DVec2::new(1000.0, 2000.0), 0.7, 40.0, false, Vec2::new(231.0, 180.0));
        let ahead = DVec2::new(1000.0 + 0.7f64.sin() * 5000.0, 2000.0 + 0.7f64.cos() * 5000.0);
        let p = v.to_screen(ahead.x, ahead.y);
        assert!((p.x - v.origin.x).abs() < 1e-3 && p.y < v.origin.y, "ahead is up: {p}");
        let back = v.to_world(Vec2::new(150.0, 40.0));
        assert!((v.to_screen(back.x, back.y) - Vec2::new(150.0, 40.0)).length() < 1e-3);
    }

    #[test]
    fn cursors_follow_a_turn() {
        let size = Vec2::new(HIRES.0 as f32, HIRES.1 as f32);
        let (lo, hi) = (Vec2::splat(-1e4), Vec2::splat(1e4));
        let mut av = AvionicsState::default();
        let before = HsdView::new(DVec2::new(1000.0, 2000.0), 0.0, 40.0, false, size);
        follow_view(&mut av, Some(before), lo, hi);
        let ground = before.to_world(av.hsd_cursor);
        let (centre, r) = harm_scope();
        av.harm_cursor = centre - Vec2::Y * r * 0.5;
        // A right turn of 90 degrees: the HSD cursor stays on its ground point, the HARM cursor
        // (dead ahead before) moves to the left, like an emitter ahead does.
        let after = HsdView::new(
            DVec2::new(1100.0, 2050.0),
            std::f64::consts::FRAC_PI_2,
            40.0,
            false,
            size,
        );
        follow_view(&mut av, Some(after), lo, hi);
        assert!((av.hsd_cursor - after.to_screen(ground.x, ground.y)).length() < 1e-2);
        assert!(
            (av.harm_cursor - (centre - Vec2::X * r * 0.5)).length() < 1e-3,
            "{}",
            av.harm_cursor
        );
    }

    #[test]
    fn lock_picks_the_nearest_lockable() {
        let v = HsdView::new(DVec2::ZERO, 0.0, 20.0, true, Vec2::new(231.0, 180.0));
        let at = |e: f64, n: f64, target| Contact {
            kind: ContactKind::Aircraft,
            east: e,
            north: n,
            alt: 0.0,
            heading: 0,
            friendly: false,
            target,
            threat: None,
        };
        let contacts = Contacts {
            list: vec![
                at(0.0, 1000.0, Some(ContactRef::Unit(3))),
                at(0.0, 1200.0, Some(ContactRef::Object(7))),
                at(0.0, 1050.0, None),
            ],
            ..default()
        };
        let p = v.to_screen(0.0, 1040.0);
        assert_eq!(pick_contact(&contacts, &v, p, PICK_RADIUS), Some(ContactRef::Unit(3)));
        assert_eq!(pick_contact(&contacts, &v, p + Vec2::X * 40.0, PICK_RADIUS), None);
    }

    #[test]
    fn lock_prefers_the_enemy_over_a_friendly() {
        let v = HsdView::new(DVec2::ZERO, 0.0, 20.0, true, Vec2::new(231.0, 180.0));
        let at = |n: f64, friendly, unit| Contact {
            kind: ContactKind::Aircraft,
            east: 0.0,
            north: n,
            alt: 0.0,
            heading: 0,
            friendly,
            target: Some(ContactRef::Unit(unit)),
            threat: None,
        };
        let contacts =
            Contacts { list: vec![at(1000.0, true, 1), at(1100.0, false, 2)], ..default() };
        let p = v.to_screen(0.0, 1000.0);
        assert_eq!(pick_contact(&contacts, &v, p, PICK_RADIUS), Some(ContactRef::Unit(2)));
        // A friendly alone is lockable.
        let contacts = Contacts { list: vec![at(1000.0, true, 1)], ..default() };
        assert_eq!(pick_contact(&contacts, &v, p, PICK_RADIUS), Some(ContactRef::Unit(1)));
    }
}
