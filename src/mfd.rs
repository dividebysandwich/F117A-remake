//! Multi-function displays. Each MFD draws into its own canvas:
//! 77x60 like the original with the classic cockpit panel, 231x180 (`HIRES`) with the modern one.
//!
//! Left MFD, F3 toggles:
//! - MAP: top-down render of the theater tile models through the 16-colour map remap table,
//!   CPU-rasterised straight into palette indices from the theater model meshes. Classic: north
//!   up, centred ahead of the aircraft, re-rendered on zoom or mode changes and when the aircraft
//!   leaves the display (like the original). Modern: a larger north-up buffer resampled every
//!   frame into a north-up or heading-up view, with the route, airbases and a heading-true
//!   aircraft symbol.
//! - TAC: heading-up tactical display with a 1024-map-unit grid.
//!
//! Right MFD: tracking camera (TC), WAY, WPN, ORD pages.
//! Z/X zoom the map (2..9, 2..10 modern) or the TAC display (0..7); the modern cockpit's bezel
//! buttons (`osb`) select pages and zoom too.
//!
//! With the modern avionics (`MfdState::modern`) the MFDs show the F-16 style formats of
//! `avionics` instead.

use std::collections::HashMap;

use bevy::ecs::system::SystemParam;
use bevy::gltf::{Gltf, GltfMaterial, GltfMesh, GltfNode};
use bevy::math::DVec2;
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;
use stealth_formats::terrain::{GRID_DIM, LEVEL_UNIT};

use crate::canvas::Canvas;
use crate::cockpit::CockpitArt;
use crate::flight::{Aircraft, Player, to_bam};
use crate::font::{Fonts, ink, upper};
use crate::hud::{Stores, Waypoints, map_units};
use crate::input::{Action, ActionEvent};
use crate::models::ModelLibrary;
use crate::palette::{Palette, index_from_name};
use crate::world::{Theater, WU, WorldPos, bevy_to_wu};

pub struct MfdPlugin;

impl Plugin for MfdPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MfdState>()
            .init_resource::<MissionOrders>()
            .init_resource::<TrackCam>()
            .init_resource::<Contacts>()
            .init_resource::<crate::rwr::Emitters>()
            .init_resource::<MissionClock>()
            .init_resource::<MapRaster>()
            .add_message::<MfdCommand>()
            .add_systems(OnEnter(crate::settings::AppState::Flight), reset_mfd)
            .add_systems(
                Update,
                (mfd_keys, (build_map_geometry, render_map).chain())
                    .chain()
                    .after(crate::flight::FlightSystems)
                    .run_if(in_state(crate::settings::AppState::Flight)),
            );
    }
}

/// MFD canvas size: the original screen, and the modern cockpit's (3x).
pub const CLASSIC: (usize, usize) = (77, 60);
pub const HIRES: (usize, usize) = (231, 180);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MfdSide {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LeftMfd {
    #[default]
    Map,
    Tac,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RightMfd {
    /// Tracking camera (TC).
    Camera,
    Way,
    Wpn,
    #[default]
    Ord,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CamDir {
    #[default]
    Ahead,
    Right,
    Rear,
    Left,
}

#[derive(Resource, Debug, Clone)]
pub struct MfdState {
    pub left: LeftMfd,
    pub right: RightMfd,
    /// Map zoom 2 (whole theater) ..= 9.
    pub map_zoom: i32,
    /// TAC zoom 0..=7.
    pub tac_zoom: i32,
    pub cam_dir: CamDir,
    /// F6: FLIR on the camera page.
    pub flir: bool,
    /// Modern map: heading up instead of north up.
    pub map_heading_up: bool,
    /// Modern (high-resolution) displays; classic cockpit = the original 77x60 ones.
    pub hires: bool,
    /// F9: ILS on. The camera page then shows the runway with its PAPI on an approach.
    pub ils: bool,
    /// Right page before ILS was switched on (restored when it goes off).
    pub page_before_ils: Option<RightMfd>,
    /// Tracking camera / targeting pod zoom step (0 = 1X .. 5); None: by range.
    pub cam_zoom: Option<u8>,
    /// Modern avionics: the MFDs show `av`'s formats.
    pub modern: bool,
    pub av: crate::avionics::AvionicsState,
}

impl Default for MfdState {
    fn default() -> Self {
        Self {
            left: LeftMfd::Map,
            right: RightMfd::Ord,
            map_zoom: 2,
            tac_zoom: 1,
            cam_dir: CamDir::Ahead,
            flir: false,
            map_heading_up: false,
            hires: true,
            ils: false,
            page_before_ils: None,
            cam_zoom: None,
            modern: false,
            av: default(),
        }
    }
}

impl MfdState {
    pub fn max_map_zoom(&self) -> i32 {
        if self.hires { 10 } else { 9 }
    }

    /// The tracking camera page is up (classic: the right MFD's camera page; modern: the
    /// targeting pod on either MFD).
    pub fn camera_shown(&self) -> bool {
        if self.modern { self.av.shown(crate::avionics::Format::Tgp).is_some() } else { self.right == RightMfd::Camera }
    }

    /// The moving map is up (classic: the left MFD's map; modern: the MAP format).
    pub fn map_shown(&self) -> bool {
        if self.modern { self.av.shown(crate::avionics::Format::Map).is_some() && !self.av.map_way } else { self.left == LeftMfd::Map }
    }

    /// The tracking camera page is on this MFD.
    pub fn camera_on(&self, side: MfdSide) -> bool {
        if self.modern { self.av.shown(crate::avionics::Format::Tgp) == Some(side) } else { side == MfdSide::Right && self.right == RightMfd::Camera }
    }
}

/// A display command (keys, panel caps and bezel buttons).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MfdCmd {
    LeftPage(LeftMfd),
    RightPage(RightMfd),
    ZoomIn,
    ZoomOut,
    ToggleOrient,
    Flir,
    Cam(CamDir),
    Ils,
    /// Modern avionics: a format on an MFD (swapping with the other MFD if it shows it).
    Format(MfdSide, crate::avionics::Format),
    /// Modern avionics: this MFD's format becomes the sensor of interest.
    Soi(MfdSide),
    /// Modern avionics: the sensor of interest moves to the other MFD.
    SwapSoi,
    HsdRange(i8),
    HsdCentre,
    MapZoom(i8),
    MapWay,
    /// SMS page: the ORD (true) or WPN page.
    Sms(bool),
    HarmFilter,
    /// Tracking camera zoom step in / out, the next step round (auto, 1X..6X).
    CamZoom(i8),
    CamZoomCycle,
    /// Targeting pod back to slaved, zoom by range.
    TgpCz,
}

/// Sent by the cockpit's bezel buttons.
#[derive(Message, Debug, Clone, Copy)]
pub struct MfdCommand(pub MfdCmd);

/// What a bezel button does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OsbFn {
    Cmd(MfdCmd),
    Action(Action),
    /// A readout next to the button (it does nothing).
    Readout,
}

/// Bezel buttons per MFD: 0..5 along the top, 5..10 along the bottom (left to right).
pub const OSB_COUNT: u8 = 10;

/// Label, function and lit state of a bezel button on the current page (None = blank).
pub fn osb(side: MfdSide, slot: u8, s: &MfdState) -> Option<(&'static str, OsbFn, bool)> {
    use OsbFn::*;
    if s.modern {
        return crate::avionics::osb(side, slot, s);
    }
    let cmd = |c| Cmd(c);
    Some(match (side, slot) {
        (MfdSide::Left, 0) => ("MAP", cmd(MfdCmd::LeftPage(LeftMfd::Map)), s.left == LeftMfd::Map),
        (MfdSide::Left, 1) => ("TAC", cmd(MfdCmd::LeftPage(LeftMfd::Tac)), s.left == LeftMfd::Tac),
        (MfdSide::Left, 4) if s.left == LeftMfd::Map => (if s.map_heading_up { "HUP" } else { "NUP" }, cmd(MfdCmd::ToggleOrient), false),
        (MfdSide::Left, 5) => ("ZM-", cmd(MfdCmd::ZoomOut), false),
        (MfdSide::Left, 6) => ("ZM+", cmd(MfdCmd::ZoomIn), false),
        (MfdSide::Right, 0) => ("TC", cmd(MfdCmd::RightPage(RightMfd::Camera)), s.right == RightMfd::Camera),
        (MfdSide::Right, 1) => ("WAY", cmd(MfdCmd::RightPage(RightMfd::Way)), s.right == RightMfd::Way),
        (MfdSide::Right, 2) => ("WPN", cmd(MfdCmd::RightPage(RightMfd::Wpn)), s.right == RightMfd::Wpn),
        (MfdSide::Right, 3) => ("ORD", cmd(MfdCmd::RightPage(RightMfd::Ord)), s.right == RightMfd::Ord),
        (MfdSide::Right, 4) => ("FLIR", cmd(MfdCmd::Flir), s.flir),
        (MfdSide::Right, 5..=8) if s.right == RightMfd::Camera => {
            let (label, dir) = [("AHD", CamDir::Ahead), ("RGT", CamDir::Right), ("REAR", CamDir::Rear), ("LFT", CamDir::Left)][slot as usize - 5];
            (label, cmd(MfdCmd::Cam(dir)), s.cam_dir == dir)
        }
        (MfdSide::Right, 5) if s.right == RightMfd::Way => ("WP-", Action(crate::input::Action::LastWaypoint), false),
        (MfdSide::Right, 6) if s.right == RightMfd::Way => ("WP+", Action(crate::input::Action::NextWaypoint), false),
        (MfdSide::Right, 7) if s.right == RightMfd::Way => ("RST", Action(crate::input::Action::ResetWaypoint), false),
        (MfdSide::Right, 5) if s.right == RightMfd::Wpn => ("SEL", Action(crate::input::Action::SelectWeapon), false),
        (MfdSide::Right, 9) if s.right == RightMfd::Camera => (cam_zoom_label(s.cam_zoom), cmd(MfdCmd::CamZoomCycle), s.cam_zoom.is_some()),
        _ => return None,
    })
}

/// Tracking camera zoom steps (as the camera page shows them: step + 1).
pub const CAM_ZOOM_MAX: u8 = 5;

/// Label of a camera zoom setting.
pub fn cam_zoom_label(z: Option<u8>) -> &'static str {
    z.and_then(|z| ["1X", "2X", "3X", "4X", "5X", "6X"].get(z as usize).copied()).unwrap_or("AUTO")
}

/// Applies a camera zoom command (`auto`: the zoom step the range gives now).
pub fn cam_zoom(state: &mut MfdState, cmd: MfdCmd, auto: u8) {
    state.cam_zoom = match cmd {
        MfdCmd::CamZoom(d) => Some((state.cam_zoom.unwrap_or(auto) as i32 + d as i32).clamp(0, CAM_ZOOM_MAX as i32) as u8),
        MfdCmd::CamZoomCycle => match state.cam_zoom {
            None => Some(0),
            Some(z) if z < CAM_ZOOM_MAX => Some(z + 1),
            _ => None,
        },
        _ => state.cam_zoom,
    };
}

/// The key action that does the same as a display command (script `press=` lookups).
pub fn cmd_action(cmd: MfdCmd) -> Action {
    match cmd {
        MfdCmd::LeftPage(_) => Action::Maps,
        MfdCmd::RightPage(RightMfd::Camera) | MfdCmd::Cam(CamDir::Ahead) => Action::CamAhead,
        MfdCmd::Cam(CamDir::Right) => Action::CamRight,
        MfdCmd::Cam(CamDir::Rear) => Action::CamRear,
        MfdCmd::Cam(CamDir::Left) => Action::CamLeft,
        MfdCmd::RightPage(RightMfd::Way) => Action::SelectWaypoint,
        MfdCmd::RightPage(RightMfd::Wpn) => Action::Ordnance,
        MfdCmd::RightPage(RightMfd::Ord) => Action::MissionOrders,
        MfdCmd::ZoomIn => Action::MapZoomIn,
        MfdCmd::ZoomOut => Action::MapZoomOut,
        MfdCmd::ToggleOrient => Action::MapOrientation,
        MfdCmd::Flir => Action::Flir,
        MfdCmd::Ils => Action::Ils,
        MfdCmd::Format(_, f) => f.action(),
        MfdCmd::Soi(_) | MfdCmd::SwapSoi => Action::SwapSoi,
        MfdCmd::HsdRange(d) | MfdCmd::MapZoom(d) => {
            if d > 0 {
                Action::MapZoomIn
            } else {
                Action::MapZoomOut
            }
        }
        MfdCmd::HsdCentre | MfdCmd::HarmFilter => Action::Maps,
        MfdCmd::CamZoom(d) => {
            if d > 0 {
                Action::CameraZoomIn
            } else {
                Action::CameraZoomOut
            }
        }
        MfdCmd::CamZoomCycle => Action::CameraZoomIn,
        MfdCmd::TgpCz => Action::CamAhead,
        MfdCmd::MapWay => Action::SelectWaypoint,
        MfdCmd::Sms(true) => Action::MissionOrders,
        MfdCmd::Sms(false) => Action::Ordnance,
    }
}

pub fn apply(state: &mut MfdState, raster: &mut MapRaster, cmd: MfdCmd) {
    if state.modern {
        crate::avionics::apply(state, raster, cmd);
        return;
    }
    match cmd {
        MfdCmd::LeftPage(p) => {
            state.left = p;
            raster.invalidate();
        }
        MfdCmd::RightPage(p) => state.right = p,
        MfdCmd::ZoomIn => match state.left {
            LeftMfd::Map if state.map_zoom < state.max_map_zoom() => {
                state.map_zoom += 1;
                raster.invalidate();
            }
            LeftMfd::Tac if state.tac_zoom < 7 => state.tac_zoom += 1,
            _ => {}
        },
        MfdCmd::ZoomOut => match state.left {
            LeftMfd::Map if state.map_zoom > 2 => {
                state.map_zoom -= 1;
                raster.invalidate();
            }
            LeftMfd::Tac if state.tac_zoom > 0 => state.tac_zoom -= 1,
            _ => {}
        },
        MfdCmd::ToggleOrient => {
            if state.hires {
                state.map_heading_up = !state.map_heading_up;
            }
        }
        MfdCmd::Flir => {
            state.flir = !state.flir;
            state.right = RightMfd::Camera;
        }
        MfdCmd::Cam(d) => {
            state.right = RightMfd::Camera;
            state.cam_dir = d;
        }
        MfdCmd::Ils => {
            state.ils = !state.ils;
            if state.ils {
                state.page_before_ils = Some(state.right);
                state.right = RightMfd::Camera;
            } else if let Some(p) = state.page_before_ils.take() {
                state.right = p;
            }
        }
        // Modern avionics only.
        _ => {}
    }
}

/// Mission orders shown on the ORD page: (colour, text) lines, upper-cased when drawn.
/// Original colours: primary target 17, secondary 41, return-to-base 10. Set from the mission.
#[derive(Resource, Debug, Clone)]
pub struct MissionOrders {
    pub lines: Vec<(u8, String)>,
}

impl Default for MissionOrders {
    fn default() -> Self {
        Self { lines: vec![(10, "RETURN TO BASE".into())] }
    }
}

/// Tracking camera information (right MFD TC page), filled from the combat simulation's
/// target selection. The picture itself is rendered by a second camera (`trackcam.rs`) into a
/// texture shown behind the MFD hole; `live` = that picture is up, so the page background is
/// left see-through.
#[derive(Resource, Debug, Clone, Default)]
pub struct TrackCam {
    pub live: bool,
    /// Target name, two centred lines.
    pub name: [String; 2],
    pub range_km: Option<f32>,
    pub bearing: Option<i32>,
    /// Zoom factor shown as "3X" (0 = hidden).
    pub zoom: u8,
}

/// Other objects for the map and TAC displays (filled by the combat simulation).
#[derive(Resource, Debug, Clone, Default)]
pub struct Contacts {
    pub list: Vec<Contact>,
    /// The simulation's air lock, else its ground lock.
    pub locked: Option<ContactRef>,
    /// Radar coverage arc of the sweep being displayed (MAP display).
    pub radar_arc: Option<MapArc>,
}

/// A radar's coverage on the MAP display: radius `range * (EMV + score) / 100`, a full circle
/// while the site is searching, +-45 degrees towards the player once alerted.
#[derive(Debug, Clone, Copy)]
pub struct MapArc {
    /// Centre in map units.
    pub x: f64,
    pub y: f64,
    /// Radius in map units.
    pub radius: f64,
    /// Start/end bearing (BAM high byte, clockwise from north); None = full circle.
    pub arc: Option<(u8, u8)>,
    pub colour: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContactKind {
    Aircraft,
    /// Attack drone (always shown: early warning datalink).
    Drone,
    Missile,
    GroundRadar,
    Target,
}

#[derive(Debug, Clone, Copy)]
pub struct Contact {
    pub kind: ContactKind,
    /// World units east / north / altitude.
    pub east: f64,
    pub north: f64,
    pub alt: f32,
    /// Binary angle.
    pub heading: u16,
    /// Friendly aircraft.
    pub friendly: bool,
    /// What a lock on it locks (aircraft: flight unit; sites and targets: world object).
    pub target: Option<ContactRef>,
    /// Ground radar: its missile's lock range (km) and RWR symbol.
    pub threat: Option<(f32, [u8; 3])>,
}

/// A lockable simulation object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContactRef {
    /// Flight unit index.
    Unit(usize),
    /// World object index.
    Object(usize),
}

/// Mission clock for the WAY page ("TIME hh:mm:ss"): seconds of the day at take-off (from the
/// mission) and the elapsed mission time (the simulation's mission ticks; None = flight time).
#[derive(Resource, Debug, Clone, Copy)]
pub struct MissionClock {
    pub start_seconds: f32,
    pub elapsed: Option<f32>,
}

impl Default for MissionClock {
    fn default() -> Self {
        Self { start_seconds: 18.0 * 3600.0, elapsed: None }
    }
}

fn reset_mfd(mut state: ResMut<MfdState>, mut raster: ResMut<MapRaster>, settings: Res<crate::settings::Settings>) {
    *state = MfdState { hires: settings.hires_displays(), modern: !settings.classic_avionics, ..default() };
    raster.invalidate();
    raster.geometry_theater.clear();
}

fn mfd_keys(
    mut actions: MessageReader<ActionEvent>,
    mut commands: MessageReader<MfdCommand>,
    mut state: ResMut<MfdState>,
    mut raster: ResMut<MapRaster>,
    settings: Res<crate::settings::Settings>,
    cam: Res<TrackCam>,
) {
    let auto_zoom = cam.zoom.saturating_sub(1);
    if state.hires != settings.hires_displays() || state.modern == settings.classic_avionics {
        state.hires = settings.hires_displays();
        state.modern = !settings.classic_avionics;
        state.map_zoom = state.map_zoom.min(state.max_map_zoom());
        raster.invalidate();
    }
    for ActionEvent(a) in actions.read() {
        let cmd = match a {
            Action::Maps if state.modern => MfdCmd::Format(MfdSide::Left, crate::avionics::next_left_format(state.av.formats[0])),
            Action::SwapSoi => MfdCmd::SwapSoi,
            Action::CameraZoomIn => MfdCmd::CamZoom(1),
            Action::CameraZoomOut => MfdCmd::CamZoom(-1),
            // Z / X zoom the targeting pod while it is the sensor of interest.
            Action::Zoom if state.modern && state.av.soi_format() == Some(crate::avionics::Format::Tgp) => MfdCmd::CamZoom(1),
            Action::Unzoom if state.modern && state.av.soi_format() == Some(crate::avionics::Format::Tgp) => MfdCmd::CamZoom(-1),
            Action::Maps => MfdCmd::LeftPage(if state.left == LeftMfd::Map { LeftMfd::Tac } else { LeftMfd::Map }),
            Action::Zoom | Action::MapZoomIn => MfdCmd::ZoomIn,
            Action::Unzoom | Action::MapZoomOut => MfdCmd::ZoomOut,
            Action::MapOrientation => MfdCmd::ToggleOrient,
            Action::Ordnance => MfdCmd::RightPage(RightMfd::Wpn),
            Action::MissionOrders => MfdCmd::RightPage(RightMfd::Ord),
            Action::SelectWaypoint | Action::ChangeWaypoint | Action::LastWaypoint | Action::NextWaypoint => MfdCmd::RightPage(RightMfd::Way),
            Action::Flir => MfdCmd::Flir,
            Action::CamAhead | Action::NearestTarget => MfdCmd::Cam(CamDir::Ahead),
            Action::CamRear => MfdCmd::Cam(CamDir::Rear),
            Action::CamRight => MfdCmd::Cam(CamDir::Right),
            Action::CamLeft => MfdCmd::Cam(CamDir::Left),
            Action::Ils => MfdCmd::Ils,
            _ => continue,
        };
        apply_with_zoom(&mut state, &mut raster, cmd, auto_zoom);
    }
    for MfdCommand(cmd) in commands.read() {
        apply_with_zoom(&mut state, &mut raster, *cmd, auto_zoom);
    }
}

fn apply_with_zoom(state: &mut MfdState, raster: &mut MapRaster, cmd: MfdCmd, auto_zoom: u8) {
    if matches!(cmd, MfdCmd::CamZoom(_) | MfdCmd::CamZoomCycle) {
        cam_zoom(state, cmd, auto_zoom);
    } else {
        apply(state, raster, cmd);
    }
}

// ------------------------------------------------------------------------------------------
// Map: geometry and rasteriser

/// Flat geometry of one theater model for the map, in model units (x east, y north).
#[derive(Default, Clone)]
pub(crate) struct MapModel {
    attr: u8,
    /// Triangles in painter's order (by height).
    tris: Vec<([Vec2; 3], u8)>,
    lines: Vec<(Vec2, Vec2, u8)>,
}

/// The rendered map and the view it was rendered for. Classic: the MFD interior (77x60) and
/// its centre. Modern: a north-up buffer larger than the display's diagonal, resampled into
/// the display every frame.
#[derive(Resource)]
pub struct MapRaster {
    pub canvas: Canvas,
    /// Map centre in map units.
    pub centre: (i32, i32),
    pub zoom: i32,
    pub valid: bool,
    buffer: Canvas,
    buf_view: MapView,
    buf_valid: bool,
    models: HashMap<u8, MapModel>,
    geometry_theater: String,
}

/// Side of the modern map buffer (pixels).
const BUFFER: usize = 384;
/// The buffer is re-rendered when the display comes this close to its edge (pixels).
const BUFFER_MARGIN: f32 = 8.0;

impl Default for MapRaster {
    fn default() -> Self {
        Self {
            canvas: Canvas::new(CLASSIC.0, CLASSIC.1),
            centre: (0, 0),
            zoom: 2,
            valid: false,
            buffer: Canvas::new(BUFFER, BUFFER),
            buf_view: MapView::default(),
            buf_valid: false,
            models: HashMap::new(),
            geometry_theater: String::new(),
        }
    }
}

impl MapRaster {
    pub fn invalidate(&mut self) {
        self.valid = false;
        self.buf_valid = false;
    }
}

/// A modern map view: map units (x east, y south) to display pixels, square pixels, rotated so
/// that `rot` (radians clockwise from north) points up.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MapView {
    pub centre: DVec2,
    /// Map units per pixel.
    pub scale: f64,
    pub rot: f64,
    pub size: Vec2,
}

impl MapView {
    pub fn to_screen(self, x: f64, y: f64) -> Vec2 {
        let (dx, dy) = ((x - self.centre.x) / self.scale, (y - self.centre.y) / self.scale);
        let (s, c) = self.rot.sin_cos();
        Vec2::new((dx * c + dy * s) as f32 + self.size.x / 2.0, (-dx * s + dy * c) as f32 + self.size.y / 2.0)
    }

    pub fn to_map(self, p: Vec2) -> DVec2 {
        let (sx, sy) = ((p.x - self.size.x / 2.0) as f64, (p.y - self.size.y / 2.0) as f64);
        let (s, c) = self.rot.sin_cos();
        self.centre + DVec2::new(sx * c - sy * s, sx * s + sy * c) * self.scale
    }

    /// Screen direction of a map direction (x east, y south).
    pub fn dir(self, d: DVec2) -> Vec2 {
        let (s, c) = self.rot.sin_cos();
        Vec2::new((d.x * c + d.y * s) as f32, (-d.x * s + d.y * c) as f32)
    }
}

/// Map units per pixel of the modern map at a zoom level (same ground as the original's
/// display at each level, over three times the pixels).
fn hires_scale(zoom: i32) -> f64 {
    (1i32 << (10 - zoom)) as f64 / 3.0
}

/// The modern map view for an aircraft at map position `p` with heading `h` (radians clockwise
/// from north). North up: centred ahead of the aircraft, clamped to the theater. Heading up:
/// the aircraft sits at 70 % of the height.
pub fn hires_view(p: DVec2, h: f64, zoom: i32, heading_up: bool, size: Vec2) -> MapView {
    let scale = hires_scale(zoom);
    let ahead = DVec2::new(h.sin(), -h.cos());
    if heading_up {
        return MapView { centre: p + ahead * (0.2 * size.y as f64 * scale), scale, rot: h, size };
    }
    let mut centre = p + ahead * (0.2 * size.y.min(size.x) as f64 * scale);
    let half = DVec2::new(size.x as f64, size.y as f64) / 2.0 * scale;
    for (c, hf) in [(&mut centre.x, half.x), (&mut centre.y, half.y)] {
        if hf < 16384.0 {
            *c = c.clamp(hf, 32768.0 - hf);
        }
    }
    MapView { centre, scale, rot: 0.0, size }
}

/// Parses `_s<slot>v<value>` conditions of a node name.
fn conditions(name: &str) -> Vec<(usize, i16)> {
    let mut out = Vec::new();
    for part in name.split("_s").skip(1) {
        let Some((slot, rest)) = part.split_once('v') else { continue };
        let value: String = rest.chars().take_while(|c| *c == '-' || c.is_ascii_digit()).collect();
        if let (Ok(s), Ok(v)) = (slot.parse::<usize>(), value.parse::<i16>()) {
            out.push((s, v));
        }
    }
    out
}

struct GeometrySource<'a> {
    nodes: &'a Assets<GltfNode>,
    meshes: &'a Assets<GltfMesh>,
    mesh_data: &'a Assets<Mesh>,
    material_names: HashMap<AssetId<GltfMaterial>, String>,
}

impl GeometrySource<'_> {
    /// Collects the most detailed level of a model (intact, lights off).
    fn collect(&self, handle: &Handle<GltfNode>, parent: Mat4, out: &mut Vec<(Vec<Vec3>, u8)>, lines: &mut Vec<(Vec3, Vec3, u8)>) {
        let Some(node) = self.nodes.get(handle) else { return };
        let name = node.name.as_str();
        if name.contains("_dots_") || conditions(name).iter().any(|&(_, v)| v != 0) {
            return;
        }
        // Only level 0 below the model root.
        if let Some(i) = name.rfind("_lod") {
            let digits: String = name[i + 4..].chars().take_while(char::is_ascii_digit).collect();
            if digits.parse::<u32>().is_ok_and(|k| k != 0) {
                return;
            }
        }
        let t = node.transform;
        let m = parent * Mat4::from_scale_rotation_translation(t.scale, t.rotation, t.translation);
        if let Some(mesh) = node.mesh.as_ref().and_then(|h| self.meshes.get(h)) {
            let node_colour = index_from_name(name);
            for prim in &mesh.primitives {
                let colour = node_colour
                    .or_else(|| {
                        prim.material.as_ref().and_then(|h| self.material_names.get(&h.id())).and_then(|n| index_from_name(n))
                    })
                    .unwrap_or(15);
                let Some(data) = self.mesh_data.get(&prim.mesh) else { continue };
                let Some(VertexAttributeValues::Float32x3(pos)) = data.attribute(Mesh::ATTRIBUTE_POSITION) else {
                    continue;
                };
                let pos: Vec<Vec3> = pos.iter().map(|&p| m.transform_point3(Vec3::from(p))).collect();
                let idx: Vec<usize> = match data.indices() {
                    Some(Indices::U16(v)) => v.iter().map(|&i| i as usize).collect(),
                    Some(Indices::U32(v)) => v.iter().map(|&i| i as usize).collect(),
                    None => (0..pos.len()).collect(),
                };
                match data.primitive_topology() {
                    PrimitiveTopology::TriangleList => {
                        for t in idx.chunks_exact(3) {
                            out.push((vec![pos[t[0]], pos[t[1]], pos[t[2]]], colour));
                        }
                    }
                    PrimitiveTopology::LineList => {
                        for l in idx.chunks_exact(2) {
                            lines.push((pos[l[0]], pos[l[1]], colour));
                        }
                    }
                    _ => {}
                }
            }
        }
        for c in &node.children {
            self.collect(c, m, out, lines);
        }
    }
}

/// Flat map geometry of a theater's models (`<id>_NNN` nodes of its loaded glTF file).
pub(crate) fn map_models(id: &str, g: &Gltf, nodes: &Assets<GltfNode>, meshes: &Assets<GltfMesh>, mesh_data: &Assets<Mesh>) -> HashMap<u8, MapModel> {
    let src = GeometrySource {
        nodes,
        meshes,
        mesh_data,
        material_names: g.named_materials.iter().map(|(n, h)| (h.id(), n.to_string())).collect(),
    };
    let mut models = HashMap::new();
    for shape in 0..=255u8 {
        let name = format!("{}_{:03}", id, shape);
        let Some(handle) = g.named_nodes.get(name.as_str()) else { continue };
        let attr = nodes
            .get(handle)
            .and_then(|n| n.extras.as_ref())
            .and_then(|x| serde_json::from_str::<serde_json::Value>(&x.value).ok())
            .and_then(|v| v["attr"].as_u64())
            .unwrap_or(0) as u8;
        let (mut tris, mut lines) = (Vec::new(), Vec::new());
        src.collect(handle, Mat4::IDENTITY, &mut tris, &mut lines);
        // Painter's order: coplanar primitives were lifted along their normal by list position.
        tris.sort_by(|a, b| {
            let h = |t: &Vec<Vec3>| t.iter().map(|p| p.y).sum::<f32>();
            h(&a.0).total_cmp(&h(&b.0))
        });
        let flat = |p: Vec3| Vec2::new(p.x, -p.z);
        models.insert(
            shape,
            MapModel {
                attr,
                tris: tris.into_iter().map(|(p, c)| ([flat(p[0]), flat(p[1]), flat(p[2])], c)).collect(),
                lines: lines.into_iter().map(|(a, b, c)| (flat(a), flat(b), c)).collect(),
            },
        );
    }
    models
}

/// Extracts the map geometry of the theater models once they are loaded.
#[allow(clippy::too_many_arguments)]
fn build_map_geometry(
    theater: Option<Res<Theater>>,
    library: Res<ModelLibrary>,
    gltfs: Res<Assets<Gltf>>,
    nodes: Res<Assets<GltfNode>>,
    meshes: Res<Assets<GltfMesh>>,
    mesh_data: Res<Assets<Mesh>>,
    mut raster: ResMut<MapRaster>,
    terrain: Res<crate::terrain::RefinedTerrain>,
) {
    let Some(theater) = theater else { return };
    if raster.geometry_theater == theater.id || !terrain.ready(&theater.id) {
        return;
    }
    let Some(g) = library.files.get(&theater.id).and_then(|h| gltfs.get(h)) else { return };
    let models = map_models(&theater.id, g, &nodes, &meshes, &mesh_data);
    if models.is_empty() {
        return;
    }
    raster.models = models;
    raster.geometry_theater = theater.id.clone();
    raster.valid = false;
}

/// Classic map projection: map units -> MFD pixels (the original
/// centre (112, 139) on the 320x200 screen is (38, 29) in the display).
pub fn map_to_screen(centre: (i32, i32), zoom: i32, x: f64, y: f64) -> Vec2 {
    let s = (1i32 << (10 - zoom)) as f64;
    Vec2::new((38.0 + (x - centre.0 as f64) / s) as f32, (29.0 + (y - centre.1 as f64) * 0.75 / s) as f32)
}

/// Map centre for an aircraft position: ahead along the heading, clamped to the theater.
fn map_centre(x: i32, y: i32, heading: u16, zoom: i32) -> (i32, i32) {
    let r = (0x8000u32 >> zoom) as f32;
    let a = heading as f32 / 65536.0 * std::f32::consts::TAU;
    let lo_x = 0x4D << (9 - zoom);
    let lo_y = (60 << (9 - zoom)) / 3 * 4;
    let cx = (x + (a.sin() * r) as i32).clamp(lo_x, 0x7FFF - lo_x);
    let cy = (y - (a.cos() * r) as i32).clamp(lo_y, 0x7FFF - lo_y);
    (cx, cy)
}

fn render_map(
    q: Query<(&Aircraft, &WorldPos), With<Player>>,
    state: Res<MfdState>,
    theater: Option<Res<Theater>>,
    palette: Option<Res<Palette>>,
    mut raster: ResMut<MapRaster>,
) {
    let (Some(theater), Some(palette)) = (theater, palette) else { return };
    let Ok((a, pos)) = q.single() else { return };
    if !state.map_shown() || raster.models.is_empty() {
        return;
    }
    let (x, y) = map_units(pos);
    let (h, _, _) = a.euler();
    let zoom = state.map_zoom;
    let map16 = palette.remaps.iter().find(|t| t.name == "map16").map(|t| t.entries.clone()).unwrap_or_default();
    let remap = |c: u8| map16.get(c as usize).map_or(c, |w| (*w & 0xFF) as u8);
    let raster = raster.as_mut();

    if state.hires {
        let (bx, by) = bevy_to_map(pos);
        let view = hires_view(DVec2::new(bx, by), h as f64, zoom, state.map_heading_up, Vec2::new(HIRES.0 as f32, HIRES.1 as f32));
        if raster.buf_valid && raster.buf_view.scale == view.scale {
            // Re-render when the display gets near the buffer's edge.
            let corners = [Vec2::ZERO, Vec2::new(view.size.x, 0.0), view.size, Vec2::new(0.0, view.size.y)];
            let lim = BUFFER as f32 - BUFFER_MARGIN;
            let inside = corners.iter().all(|&c| {
                let m = view.to_map(c);
                let b = raster.buf_view.to_screen(m.x, m.y);
                b.x >= BUFFER_MARGIN && b.y >= BUFFER_MARGIN && b.x <= lim && b.y <= lim
            });
            if inside {
                return;
            }
        }
        let bv = MapView { centre: view.centre, scale: view.scale, rot: 0.0, size: Vec2::splat(BUFFER as f32) };
        raster.buf_view = bv;
        raster.buf_valid = true;
        let half = bv.size.x as f64 / 2.0 * bv.scale * 32.0;
        let visible = (bv.centre.x * 32.0, (0x8000 as f64 - bv.centre.y) * 32.0, half, half);
        let to_px = |east: f64, north: f64| bv.to_screen(east / 32.0, 0x8000 as f64 - north / 32.0);
        rasterise_map(&mut raster.buffer, &raster.models, &theater, &to_px, visible, zoom, &remap);
        return;
    }

    if raster.valid {
        // Re-render when the aircraft leaves the display.
        let p = map_to_screen(raster.centre, raster.zoom, x as f64, y as f64);
        if p.x >= 0.0 && p.x <= (CLASSIC.0 - 1) as f32 && p.y >= 0.0 && p.y <= (CLASSIC.1 - 1) as f32 {
            return;
        }
    }
    let centre = map_centre(x, y, to_bam(h) as i32 as u16, zoom);
    raster.centre = centre;
    raster.zoom = zoom;
    raster.valid = true;
    // Visible world rectangle (with margin).
    let s = (1i32 << (10 - zoom)) as f64 * 32.0;
    let visible = (centre.0 as f64 * 32.0, (0x8000 - centre.1) as f64 * 32.0, 40.0 * s, 32.0 * s / 0.75);
    let to_px = |east: f64, north: f64| map_to_screen(centre, zoom, east / 32.0, 0x8000 as f64 - north / 32.0);
    rasterise_map(&mut raster.canvas, &raster.models, &theater, &to_px, visible, zoom, &remap);
}

/// Exact map units (x east, y south) of a position.
fn bevy_to_map(pos: &WorldPos) -> (f64, f64) {
    let (e, n, _) = bevy_to_wu(pos.0);
    (e / 32.0, 0x8000 as f64 - n / 32.0)
}

/// Renders the theater top-down: LOD4, LOD3, LOD2 tiles (a level only if
/// its tiles are larger than 16 units on screen at the original scale), then LOD4 and LOD3
/// again for the models flagged 0x80 (drawn on top). Only ground-level objects (z = 0) are
/// drawn. `to_px` maps world units (east, north) to canvas pixels; `visible` is the world
/// rectangle to cover (centre east, centre north, half width, half height).
pub(crate) fn rasterise_map(
    c: &mut Canvas,
    models: &HashMap<u8, MapModel>,
    theater: &Theater,
    to_px: &dyn Fn(f64, f64) -> Vec2,
    visible: (f64, f64, f64, f64),
    zoom: i32,
    remap: &dyn Fn(u8) -> u8,
) {
    c.clear(6);
    let (cx, cn, half_w, half_h) = visible;
    for (level, flagged) in [(4usize, false), (3, false), (2, false), (4, true), (3, true)] {
        if (4 - level as i32) * 2 + (9 - zoom) >= 8 {
            continue;
        }
        let unit = LEVEL_UNIT[level];
        let tile = 4096.0 * unit;
        let lo = if level == 4 { -2 } else { 0 };
        let range = |a: f64, b: f64| ((a / tile).floor() as i32 - 1, (b / tile).floor() as i32 + 1);
        let (c0, c1) = range(cx - half_w, cx + half_w);
        let (r0, r1) = range(cn - half_h, cn + half_h);
        for row in r0.max(lo)..=r1.min(GRID_DIM[level] + lo - 1) {
            for col in c0.max(lo)..=c1.min(GRID_DIM[level] + lo - 1) {
                let tile_type = theater.grid.tile(level, col, row) as usize;
                let Some(objects) = theater.tiles.tiles.get(level).and_then(|t| t.get(tile_type)) else { continue };
                let (tx, ty) = ((col as f64 * 4096.0 + 2048.0) * unit, (row as f64 * 4096.0 + 2048.0) * unit);
                for o in objects.iter().filter(|o| o.z == 0) {
                    let Some(m) = models.get(&o.shape) else { continue };
                    if level >= 3 && (m.attr & 0x80 != 0) != flagged {
                        continue;
                    }
                    let place = |p: Vec2| to_px(tx + (o.x as f64 + p.x as f64) * unit, ty + (o.y as f64 + p.y as f64) * unit);
                    for (t, colour) in &m.tris {
                        fill_triangle(c, [place(t[0]), place(t[1]), place(t[2])], remap(*colour));
                    }
                    for (a, b, colour) in &m.lines {
                        let (a, b) = (place(*a), place(*b));
                        c.line(a.x.floor() as i32, a.y.floor() as i32, b.x.floor() as i32, b.y.floor() as i32, remap(*colour));
                    }
                }
            }
        }
    }
}

/// Fills a triangle (pixel centres inside, top-left fill rule).
fn fill_triangle(c: &mut Canvas, v: [Vec2; 3], colour: u8) {
    let area = (v[1] - v[0]).perp_dot(v[2] - v[0]);
    if area.abs() < 1e-6 {
        return;
    }
    let v = if area < 0.0 { [v[0], v[2], v[1]] } else { v };
    let min = v[0].min(v[1]).min(v[2]);
    let max = v[0].max(v[1]).max(v[2]);
    let (x0, x1) = ((min.x.floor() as i32).max(0), (max.x.ceil() as i32).min(c.w as i32 - 1));
    let (y0, y1) = ((min.y.floor() as i32).max(0), (max.y.ceil() as i32).min(c.h as i32 - 1));
    for y in y0..=y1 {
        for x in x0..=x1 {
            let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
            let inside = (0..3).all(|i| {
                let (a, b) = (v[i], v[(i + 1) % 3]);
                let e = (b - a).perp_dot(p - a);
                let top_left = (a.y == b.y && b.x < a.x) || b.y < a.y;
                e > 0.0 || (e == 0.0 && top_left)
            });
            if inside {
                c.set(x, y, colour);
            }
        }
    }
}

// ------------------------------------------------------------------------------------------
// Drawing

#[derive(SystemParam)]
pub struct MfdParams<'w, 's> {
    pub aircraft: Query<'w, 's, (&'static Aircraft, &'static WorldPos), With<Player>>,
    pub state: Res<'w, MfdState>,
    pub raster: Res<'w, MapRaster>,
    pub orders: Res<'w, MissionOrders>,
    pub cam: Res<'w, TrackCam>,
    pub contacts: Res<'w, Contacts>,
    pub clock: Res<'w, MissionClock>,
    pub stores: Res<'w, Stores>,
    pub waypoints: Res<'w, Waypoints>,
    pub theater: Option<Res<'w, Theater>>,
    pub time: Res<'w, Time>,
    pub palette: Option<Res<'w, Palette>>,
    pub emitters: Res<'w, crate::rwr::Emitters>,
}

/// MFD text helper: x = 4*col, y = 6*row in the display.
fn mfd_text(c: &mut Canvas, fonts: &Fonts, col: i32, row: i32, s: &str, colour: u8) {
    fonts.text(c, 4 * col, 6 * row, &upper(s), colour);
}

/// Modern display text: the flight font at twice the size (8 px advance, 10 px high).
fn text2(c: &mut Canvas, fonts: &Fonts, x: i32, y: i32, s: &str, colour: u8) -> i32 {
    fonts.text_scaled(c, x, y, &upper(s), colour, 2)
}

pub(crate) fn hms(seconds: f32) -> (u32, u32, u32) {
    let s = seconds.max(0.0) as u32 % 86400;
    (s / 3600, s / 60 % 60, s % 60)
}

/// Pixels of a modern TAC display per original one.
const K: i32 = 3;

impl MfdParams<'_, '_> {
    /// Whether the right display should let the tracking camera picture through (index 0).
    pub fn camera_see_through(&self, side: MfdSide) -> bool {
        self.cam.live && self.state.camera_on(side)
    }

    pub fn draw_left(&self, c: &mut Canvas, art: &CockpitArt, fonts: &Fonts) {
        let ac = self.aircraft.single().ok();
        c.reset_clip();
        if self.state.modern {
            crate::avionics::draw(self, MfdSide::Left, c, art, fonts);
            return;
        }
        match (self.state.left, self.state.hires) {
            (LeftMfd::Map, false) => self.draw_map(c, ac),
            (LeftMfd::Tac, false) => self.draw_tac(c, art, ac),
            (LeftMfd::Map, true) => self.draw_map_hires(c, art, fonts, ac, false),
            (LeftMfd::Tac, true) => self.draw_tac_hires(c, art, fonts, ac),
        }
    }

    pub fn draw_right(&self, c: &mut Canvas, art: &CockpitArt, fonts: &Fonts) {
        let ac = self.aircraft.single().ok();
        c.reset_clip();
        if self.state.modern {
            crate::avionics::draw(self, MfdSide::Right, c, art, fonts);
            return;
        }
        match (self.state.right, self.state.hires) {
            (RightMfd::Camera, false) => self.draw_camera_page(c, fonts),
            (RightMfd::Way, false) => self.draw_way(c, fonts, ac),
            (RightMfd::Wpn, false) => self.draw_wpn(c, art, fonts),
            (RightMfd::Ord, false) => self.draw_ord(c, fonts),
            (RightMfd::Camera, true) => self.draw_camera_hires(c, fonts),
            (RightMfd::Way, true) => self.draw_way_hires(c, fonts, ac),
            (RightMfd::Wpn, true) => self.draw_wpn_hires(c, art, fonts),
            (RightMfd::Ord, true) => self.draw_ord_hires(c, fonts),
        }
    }

    // --- Classic (original 77x60) pages -------------------------------------------------------

    fn draw_map(&self, c: &mut Canvas, ac: Option<(&Aircraft, &WorldPos)>) {
        if !self.raster.valid {
            c.clear(6);
            return;
        }
        c.copy_rect(&self.raster.canvas, 0, 0, 77, 60, 0, 0);
        let (centre, zoom) = (self.raster.centre, self.raster.zoom);
        let blink = crate::hud::blink(&self.time);
        for k in &self.contacts.list {
            let p = map_to_screen(centre, zoom, k.east / 32.0, 0x8000 as f64 - k.north / 32.0);
            let (x, y) = (p.x as i32, p.y as i32);
            match k.kind {
                ContactKind::Aircraft => c.fill_rect(x, y, 2, 2, 41),
                ContactKind::Drone => c.fill_rect(x, y, 2, 1, 13),
                ContactKind::Missile => c.set(x, y, 17),
                ContactKind::GroundRadar => c.fill_rect(x, y, 2, 2, 0),
                ContactKind::Target if blink => c.fill_rect(x - 1, y - 1, 3, 3, 15),
                ContactKind::Target => {}
            }
        }
        // Radar coverage arc of the current sweep.
        if let Some(arc) = &self.contacts.radar_arc {
            let o = map_to_screen(centre, zoom, arc.x, arc.y);
            let r = arc.radius / (1i32 << (10 - zoom)) as f64;
            let (a0, a1) = arc_range(arc);
            let steps = ((a1 - a0) * r / 16.0).clamp(16.0, 512.0) as i32;
            let mut prev: Option<(i32, i32)> = None;
            for i in 0..=steps {
                let t = (a0 + (a1 - a0) * i as f64 / steps as f64) / 256.0 * std::f64::consts::TAU;
                let p = (o.x as f64 + r * t.sin(), o.y as f64 - r * 0.75 * t.cos());
                let p = (p.0 as i32, p.1 as i32);
                if let Some(q) = prev {
                    c.line(q.0, q.1, p.0, p.1, arc.colour);
                }
                prev = Some(p);
            }
        }
        // Own aircraft: white chevron.
        if let Some((_, pos)) = ac {
            let (x, y) = map_units(pos);
            let p = map_to_screen(centre, zoom, x as f64, y as f64);
            let (x, y) = (p.x as i32 - 1, p.y as i32);
            c.set(x, y - 2, 15);
            c.set(x, y - 1, 15);
            c.set(x - 1, y, 15);
            c.set(x + 1, y, 15);
        }
    }

    /// TAC display.
    fn draw_tac(&self, c: &mut Canvas, art: &CockpitArt, ac: Option<(&Aircraft, &WorldPos)>) {
        self.tac(c, art, ac, 1);
    }

    /// The TAC display at `k` display pixels per original pixel.
    fn tac(&self, c: &mut Canvas, art: &CockpitArt, ac: Option<(&Aircraft, &WorldPos)>, k: i32) {
        c.clear(0);
        let Some((a, pos)) = ac else { return };
        let (ax, ay) = map_units(pos);
        let (h, _, _) = a.euler();
        let (sh, ch) = h.sin_cos();
        let zoom = self.state.tac_zoom;
        let shift = 7 - zoom;
        let kf = k as f32;
        // Heading-up projection (own aircraft at (38, 42) of the original display);
        // also returns whether an icon is inside the display.
        let proj = |x: i32, y: i32| -> (i32, i32, bool) {
            let (dx, dy) = if k == 1 {
                (((x - ax) >> shift) as f32, ((ay - y) >> shift) as f32)
            } else {
                ((x - ax) as f32 / (1 << shift) as f32, (ay - y) as f32 / (1 << shift) as f32)
            };
            let sx = ((ch * dx - sh * dy) * kf) as i32 + 38 * k;
            let sy = 42 * k - ((sh * dx + ch * dy) * kf) as i32;
            (sx, sy, (4 * k..=73 * k).contains(&sx) && (3 * k..=56 * k).contains(&sy))
        };
        // Grid every 1024 map units, colour 8.
        let n = if zoom < 2 { (1 << (2 - zoom)) + 1 } else { 1 };
        let (bx, by) = (ax & !0x3FF, ay & !0x3FF);
        for j in (1 - n)..=n {
            let x = bx + (j << 10);
            let (p, q) = (proj(x, by + 0x1400), proj(x, by - 0x1000));
            c.line(p.0, p.1, q.0, q.1, 8);
            let y = by + (j << 10);
            let (p, q) = (proj(bx + 0x1400, y), proj(bx - 0x1000, y));
            c.line(p.0, p.1, q.0, q.1, 8);
        }
        let s = if k == 1 { 1 } else { 2 };
        let icon = |c: &mut Canvas, kind: i32, x: i32, y: i32| {
            c.blit_scaled(&art.sprites, kind * 8 + 1, 65, 7, 7, x - 3 * s - s / 2, y - 3 * s - s / 2, s, Some(0), None);
        };
        // Aircraft: a triangle pointing along `rel` (radians clockwise from display up).
        let plane = |c: &mut Canvas, rel: f32, x: i32, y: i32, colour: u8| {
            let centre = Vec2::new(x as f32, y as f32) + if s == 1 { Vec2::splat(0.5) } else { Vec2::ZERO };
            let (sr, cr) = rel.sin_cos();
            let (fwd, side) = (Vec2::new(sr, -cr) * s as f32, Vec2::new(cr, sr) * s as f32);
            c.triangle([centre + fwd * 4.0, centre - fwd * 3.0 - side * 2.3, centre - fwd * 3.0 + side * 2.3], colour);
        };
        // Attack drone: a small open delta.
        let drone = |c: &mut Canvas, rel: f32, x: i32, y: i32, colour: u8| {
            let centre = Vec2::new(x as f32, y as f32) + if s == 1 { Vec2::splat(0.5) } else { Vec2::ZERO };
            let (sr, cr) = rel.sin_cos();
            let (fwd, side) = (Vec2::new(sr, -cr) * s as f32, Vec2::new(cr, sr) * s as f32);
            let p = [centre + fwd * 3.0, centre - fwd * 2.0 - side * 2.5, centre - fwd * 2.0 + side * 2.5];
            for (a, b) in [(p[0], p[1]), (p[1], p[2]), (p[2], p[0])] {
                c.line(a.x as i32, a.y as i32, b.x as i32, b.y as i32, colour);
            }
        };
        // Airbases (runway in a ring).
        if let Some(t) = &self.theater {
            for o in t.world.objects.iter().filter(|o| o.target_flags & 0x100 != 0) {
                let (x, y, vis) = proj(o.x as i32, o.y as i32);
                if vis {
                    tac_icon(c, TacIcon::Airfield, x, y, k > 1);
                }
            }
        }
        // Other aircraft by relative heading and altitude band; missiles as short lines.
        for ct in &self.contacts.list {
            let mx = (ct.east / 32.0) as i32;
            let my = 0x8000 - (ct.north / 32.0) as i32;
            let (x, y, vis) = proj(mx, my);
            if !vis {
                continue;
            }
            match ct.kind {
                ContactKind::Aircraft => {
                    let rel = ct.heading.wrapping_sub(to_bam(h) as i32 as u16) as f32 / 65536.0 * std::f32::consts::TAU;
                    // Friendly blue; hostile red level with us, dark red below, yellow above.
                    let dalt = ct.alt - a.altitude;
                    let colour = if ct.friendly {
                        9
                    } else if dalt < -1000.0 {
                        40
                    } else if dalt > 1000.0 {
                        17
                    } else {
                        41
                    };
                    plane(c, rel, x, y, colour);
                }
                ContactKind::Drone => {
                    let rel = ct.heading.wrapping_sub(to_bam(h) as i32 as u16) as f32 / 65536.0 * std::f32::consts::TAU;
                    drone(c, rel, x, y, 13);
                }
                ContactKind::Missile => {
                    let rel = (ct.heading as f32 / 65536.0) * std::f32::consts::TAU - h;
                    let len = (zoom + 1) as f32 * kf;
                    c.line(x, y, x + (rel.sin() * len) as i32, y - (rel.cos() * len) as i32, 41);
                }
                ContactKind::GroundRadar => tac_icon(c, TacIcon::Radar, x, y, k > 1),
                ContactKind::Target => icon(c, AIRBASE_ICON, x, y),
            }
        }
        // Own aircraft, blue.
        plane(c, 0.0, 38 * k, 42 * k, 9);
    }

    /// Tracking camera page. With the camera picture live the background stays
    /// see-through (index 0) and the 3D picture shows behind the MFD; otherwise a placeholder
    /// (the level "ahead" view's sky bands and ground).
    fn draw_camera_page(&self, c: &mut Canvas, fonts: &Fonts) {
        self.camera_background(c, 1);
        let text = self.camera_text_colour();
        for (k, line) in self.cam.name.iter().enumerate() {
            let s = upper(line);
            fonts.text(c, 39 - 2 * s.len() as i32, 2 + 6 * k as i32, &s, text);
        }
        if let Some(r) = self.cam.range_km {
            fonts.text(c, 12, 45, format!("RANGE {r:.1} KM").as_bytes(), text);
        }
        if let Some(b) = self.cam.bearing {
            fonts.text(c, 16, 51, format!("BRG {b}").as_bytes(), text);
        }
        if self.cam.zoom > 0 {
            fonts.text(c, 67, 53, format!("{}X", self.cam.zoom).as_bytes(), text);
        }
    }

    fn camera_background(&self, c: &mut Canvas, k: i32) {
        const SKY: [(i32, u8); 11] =
            [(3, 138), (6, 137), (6, 136), (3, 135), (3, 134), (3, 133), (1, 132), (2, 131), (1, 129), (1, 128), (31, 238)];
        if self.cam.live {
            c.clear(0);
            return;
        }
        let flir = self.state.flir;
        let mut y = 0;
        for (rows, colour) in SKY {
            // FLIR: black and white, dark sky over grey ground (the grey ramp 0xE0..0xEF).
            let colour = if !flir { colour } else if colour == 238 { 0xEB } else { 0xEF };
            c.fill_rect(0, y, c.w as i32, rows * k, colour);
            y += rows * k;
        }
    }

    /// Black text with FLIR (colour 5: black in every flight palette, 0 is see-through).
    pub(crate) fn camera_text_colour(&self) -> u8 {
        if self.state.flir { 5 } else { crate::hud::HUD_COLOURS.0 }
    }

    /// Clock time shown on the WAY page.
    fn way_time(&self, ac: Option<(&Aircraft, &WorldPos)>) -> f32 {
        self.clock.start_seconds + self.clock.elapsed.unwrap_or_else(|| ac.map_or(0.0, |(a, _)| a.time))
    }

    /// Waypoint legs from the aircraft: (km, minutes, ETA seconds) at the current speed (at
    /// least 360 kt), ETA accumulated.
    fn legs(&self, t: f32, a: &Aircraft, pos: &WorldPos) -> Vec<(f64, f64, f64)> {
        let (mut e, mut n, _) = bevy_to_wu(pos.0);
        let kmh = (a.knots().max(360.0) * 1.852) as f64;
        let mut eta = t as f64;
        let mut out = Vec::new();
        for wp in self.waypoints.points.iter().take(4) {
            let km = ((wp.east - e).hypot(wp.north - n) * WU / 1000.0).round();
            let minutes = (km / kmh * 60.0).round();
            eta += minutes * 60.0;
            out.push((km, minutes, eta));
            (e, n) = (wp.east, wp.north);
        }
        out
    }

    fn way_colour(&self, i: usize) -> u8 {
        if i == self.waypoints.current { 15 } else if i % 2 == 1 { 3 } else { 11 }
    }

    /// WAY page.
    fn draw_way(&self, c: &mut Canvas, fonts: &Fonts, ac: Option<(&Aircraft, &WorldPos)>) {
        c.clear(1);
        let t = self.way_time(ac);
        let (hh, mm, ss) = hms(t);
        mfd_text(c, fonts, 2, 0, &format!("TIME  {hh:02}:{mm:02}:{ss:02}"), 15);
        if let Some((a, pos)) = ac {
            for (i, (km, minutes, eta)) in self.legs(t, a, pos).into_iter().enumerate() {
                let (eh, em, _) = hms(eta as f32);
                let colour = self.way_colour(i);
                mfd_text(c, fonts, 1, 1 + 2 * i as i32, &format!("WAYPOINT {}-{} KM", i + 1, km as i64), colour);
                mfd_text(c, fonts, 1, 2 + 2 * i as i32, &format!("{} MIN. ETA {eh:02}:{em:02}", minutes as i64), colour);
            }
            // Fuel bar (y 54..57): remaining fuel in green.
            let w = ((a.fuel / FULL_FUEL).clamp(0.0, 1.0) * 75.0).round() as i32;
            c.fill_rect(1, 54, 75, 4, 0);
            c.fill_rect(1, 54, w, 4, 47);
        }
    }

    /// WPN page: backdrop (frame and weapon side views) with counts and names.
    fn draw_wpn(&self, c: &mut Canvas, art: &CockpitArt, fonts: &Fonts) {
        c.clear(0);
        for (i, bay) in self.stores.bays.iter().enumerate().take(4) {
            if let Some((sx, sy)) = ordnance_picture(bay.picture) {
                c.copy_rect(&art.ordnance, sx, sy, 77, 15, 0, 15 * i as i32);
            }
        }
        c.rect(0, 0, 77, 60, 47);
        for y in [14, 29, 44] {
            c.hline(0, 76, y, 47);
        }
        for (i, bay) in self.stores.bays.iter().enumerate().take(4) {
            let y = 2 + 15 * i as i32;
            fonts.text(c, 3, y + 6, format!("0{}", bay.count).as_bytes(), 47);
            fonts.text(c, 3, y, &upper(&bay.name), 47);
        }
    }

    /// ORD page.
    fn draw_ord(&self, c: &mut Canvas, fonts: &Fonts) {
        c.clear(0);
        for (row, (colour, line)) in self.orders.lines.iter().enumerate().take(10) {
            // Colour-change byte + text, as the original order strings.
            let mut s = vec![ink(*colour)];
            s.extend(upper(line));
            fonts.text(c, 4, 6 * row as i32, &s, 15);
        }
    }

    // --- Modern (231x180) pages ---------------------------------------------------------------

    /// The modern map; `modern`: the modern avionics' MAP format (muted terrain, no orientation
    /// tag: it is on a bezel button).
    pub(crate) fn draw_map_hires(&self, c: &mut Canvas, art: &CockpitArt, fonts: &Fonts, ac: Option<(&Aircraft, &WorldPos)>, modern: bool) {
        let r = &self.raster;
        let Some((a, pos)) = ac.filter(|_| r.buf_valid) else {
            c.clear(6);
            return;
        };
        let (px, py) = bevy_to_map(pos);
        let (h, _, _) = a.euler();
        let view = hires_view(DVec2::new(px, py), h as f64, self.state.map_zoom, self.state.map_heading_up, Vec2::new(c.w as f32, c.h as f32));
        // Resample the north-up buffer (nearest pixel); the modern format mutes the terrain.
        let tones = if modern { self.modern_tones() } else { std::array::from_fn(|i| i as u8) };
        let (w, ht) = (c.w, c.h);
        for y in 0..ht {
            for x in 0..w {
                let m = view.to_map(Vec2::new(x as f32 + 0.5, y as f32 + 0.5));
                let b = r.buf_view.to_screen(m.x, m.y);
                let (bx, by) = (b.x.floor() as i32, b.y.floor() as i32);
                let v = if bx >= 0 && by >= 0 && (bx as usize) < BUFFER && (by as usize) < BUFFER { r.buffer.px[by as usize * BUFFER + bx as usize] } else { 6 };
                c.px[y * w + x] = tones[v as usize];
            }
        }
        let to = |e: f64, n: f64| view.to_screen(e / 32.0, 0x8000 as f64 - n / 32.0);

        // Radar coverage arc of the current sweep.
        if let Some(arc) = &self.contacts.radar_arc {
            let (a0, a1) = arc_range(arc);
            let steps = ((a1 - a0) * arc.radius / view.scale / 8.0).clamp(24.0, 512.0) as i32;
            let mut prev: Option<Vec2> = None;
            for i in 0..=steps {
                let t = (a0 + (a1 - a0) * i as f64 / steps as f64) / 256.0 * std::f64::consts::TAU;
                let p = view.to_screen(arc.x + arc.radius * t.sin(), arc.y - arc.radius * t.cos());
                if let Some(q) = prev {
                    c.line(q.x as i32, q.y as i32, p.x as i32, p.y as i32, arc.colour);
                }
                prev = Some(p);
            }
        }
        // Route: aircraft -> current waypoint -> the rest; waypoints ringed, the current one larger.
        let wps: Vec<Vec2> = self.waypoints.points.iter().map(|w| to(w.east, w.north)).collect();
        let me = view.to_screen(px, py);
        if let Some(&first) = wps.get(self.waypoints.current) {
            c.line(me.x as i32, me.y as i32, first.x as i32, first.y as i32, 11);
            for pair in wps[self.waypoints.current..].windows(2) {
                c.line(pair[0].x as i32, pair[0].y as i32, pair[1].x as i32, pair[1].y as i32, 11);
            }
        }
        for (i, p) in wps.iter().enumerate() {
            let current = i == self.waypoints.current;
            c.circle(p.x, p.y, if current { 5.0 } else { 3.0 }, if current { 15 } else { 11 });
        }
        // Airbases.
        if let Some(t) = &self.theater {
            for o in t.world.objects.iter().filter(|o| o.target_flags & 0x100 != 0) {
                let p = view.to_screen(o.x as f64, o.y as f64);
                c.blit_scaled(&art.sprites, AIRBASE_ICON * 8 + 1, 65, 7, 7, p.x as i32 - 7, p.y as i32 - 7, 2, Some(0), None);
            }
        }
        let blink = crate::hud::blink(&self.time);
        for k in &self.contacts.list {
            let p = to(k.east, k.north);
            let (x, y) = (p.x as i32, p.y as i32);
            match k.kind {
                ContactKind::Aircraft => c.fill_rect(x - 2, y - 2, 4, 4, 41),
                ContactKind::Drone => {
                    c.line(x - 2, y - 2, x + 2, y + 2, 13);
                    c.line(x - 2, y + 2, x + 2, y - 2, 13);
                }
                ContactKind::Missile => c.fill_rect(x - 1, y - 1, 2, 2, 17),
                ContactKind::GroundRadar => c.fill_rect(x - 2, y - 2, 4, 4, 0),
                ContactKind::Target if blink => c.rect(x - 4, y - 4, 9, 9, 15),
                ContactKind::Target => {}
            }
        }
        // Own aircraft: a triangle along the true heading.
        let d = view.dir(DVec2::new((h as f64).sin(), -(h as f64).cos()));
        let side = Vec2::new(-d.y, d.x);
        let tri = [me + d * 7.0, me - d * 4.0 + side * 5.0, me - d * 4.0 - side * 5.0];
        fill_triangle(c, tri, 15);
        for i in 0..3 {
            let (p, q) = (tri[i], tri[(i + 1) % 3]);
            c.line(p.x as i32, p.y as i32, q.x as i32, q.y as i32, 0);
        }

        // North arrow (heading up), orientation tag, scale bar.
        let w = c.w as i32;
        let ht = c.h as i32;
        if self.state.map_heading_up {
            let n = view.dir(DVec2::new(0.0, -1.0));
            let o = Vec2::new(w as f32 - 16.0, 18.0);
            let (p, q) = (o - n * 8.0, o + n * 8.0);
            c.line(p.x as i32, p.y as i32, q.x as i32, q.y as i32, 15);
            let tip = [q, q - n * 4.0 + Vec2::new(-n.y, n.x) * 3.0, q - n * 4.0 - Vec2::new(-n.y, n.x) * 3.0];
            fill_triangle(c, tip, 15);
            let l = o - n * 14.0;
            fonts.text_scaled(c, l.x as i32 - 3, l.y as i32 - 5, b"N", 15, 2);
        }
        if !modern {
            text2(c, fonts, 10, 6, if self.state.map_heading_up { "HUP" } else { "NUP" }, 15);
        }
        let px_per_km = 1000.0 / (WU * 32.0 * view.scale);
        let km = [0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0].into_iter().rev().find(|k| k * px_per_km <= 60.0).unwrap_or(0.5);
        let len = (km * px_per_km) as i32;
        let (x0, y0) = (10, ht - 6);
        c.hline(x0, x0 + len, y0, 15);
        c.line(x0, y0 - 3, x0, y0, 15);
        c.line(x0 + len, y0 - 3, x0 + len, y0, 15);
        let label = if km < 1.0 { format!("{km} KM") } else { format!("{} KM", km as i32) };
        text2(c, fonts, x0, y0 - 15, &label, 15);
    }

    /// Map colours of the modern MAP format: water dark blue, vegetation dark green, the rest in
    /// greys by brightness (judged in the theater's day palette; the result uses the colours
    /// 0..15, the same at every time of day).
    fn modern_tones(&self) -> [u8; 256] {
        let mut t: [u8; 256] = std::array::from_fn(|i| i as u8);
        let Some(p) = &self.palette else { return t };
        for (i, &[r, g, b]) in p.theater.day.colors.iter().enumerate().take(256) {
            let (r, g, b) = (r as i32, g as i32, b as i32);
            let lum = (r + g + b) / 3;
            t[i] = if b > r + 6 && b > g + 4 {
                2
            } else if g > r + 4 && g > b + 2 {
                4
            } else if lum < 16 {
                8
            } else if lum < 34 {
                14
            } else {
                7
            };
        }
        t[6] = 5;
        t
    }

    fn draw_tac_hires(&self, c: &mut Canvas, art: &CockpitArt, fonts: &Fonts, ac: Option<(&Aircraft, &WorldPos)>) {
        self.tac(c, art, ac, K);
        // Grid spacing readout.
        let km = 1024.0 * 32.0 * WU / 1000.0;
        text2(c, fonts, 10, 6, &format!("TAC {}", self.state.tac_zoom), 8);
        text2(c, fonts, 10, c.h as i32 - 16, &format!("GRID {km:.0} KM"), 8);
    }

    pub(crate) fn draw_camera_hires(&self, c: &mut Canvas, fonts: &Fonts) {
        self.camera_background(c, K);
        let text = self.camera_text_colour();
        let w = c.w as i32;
        for (k, line) in self.cam.name.iter().enumerate() {
            let s = upper(line);
            fonts.text_scaled(c, (w - 2 * fonts.width(&s)) / 2, 4 + 12 * k as i32, &s, text, 2);
        }
        if let Some(r) = self.cam.range_km {
            text2(c, fonts, 6, c.h as i32 - 28, &format!("RANGE {r:.1} KM"), text);
        }
        if let Some(b) = self.cam.bearing {
            text2(c, fonts, 6, c.h as i32 - 14, &format!("BRG {b}"), text);
        }
        if self.cam.zoom > 0 {
            let s = format!("{}X", self.cam.zoom);
            text2(c, fonts, w - 6 - 2 * fonts.width(s.as_bytes()), c.h as i32 - 14, &s, text);
            if self.state.cam_zoom.is_none() {
                text2(c, fonts, w - 6 - 2 * fonts.width(b"AUTO"), c.h as i32 - 28, "AUTO", text);
            }
        }
    }

    pub(crate) fn draw_way_hires(&self, c: &mut Canvas, fonts: &Fonts, ac: Option<(&Aircraft, &WorldPos)>) {
        c.clear(1);
        let t = self.way_time(ac);
        let (hh, mm, ss) = hms(t);
        text2(c, fonts, 6, 4, &format!("TIME  {hh:02}:{mm:02}:{ss:02}"), 15);
        text2(c, fonts, 6, 26, "WP    KM   MIN   ETA", 7);
        c.hline(4, c.w as i32 - 5, 38, 7);
        let Some((a, pos)) = ac else { return };
        for (i, (km, minutes, eta)) in self.legs(t, a, pos).into_iter().enumerate() {
            let (eh, em, _) = hms(eta as f32);
            let y = 44 + 16 * i as i32;
            if i == self.waypoints.current {
                c.fill_rect(2, y - 3, c.w as i32 - 4, 16, 8);
            }
            text2(c, fonts, 6, y, &format!("{}  {:>5} {:>5}  {eh:02}:{em:02}", i + 1, km as i64, minutes as i64), self.way_colour(i));
        }
        // Fuel bar: remaining fuel in green.
        text2(c, fonts, 6, 156, "FUEL", 15);
        let (x0, w) = (46, c.w as i32 - 52);
        let fill = ((a.fuel / FULL_FUEL).clamp(0.0, 1.0) * w as f32).round() as i32;
        c.fill_rect(x0, 155, w, 12, 0);
        c.fill_rect(x0, 155, fill, 12, 47);
        c.rect(x0 - 1, 154, w + 2, 14, 7);
    }

    pub(crate) fn draw_wpn_hires(&self, c: &mut Canvas, art: &CockpitArt, fonts: &Fonts) {
        c.clear(0);
        let (w, ht) = (c.w as i32, c.h as i32);
        let rows = self.stores.bays.len().clamp(4, 6);
        let row = ht / rows as i32;
        for (i, bay) in self.stores.bays.iter().enumerate().take(rows) {
            let y = row * i as i32;
            if i == self.stores.selected && !self.stores.gun_selected {
                c.fill_rect(1, y + 1, w - 2, row - 1, 8);
            }
            if let Some((sx, sy)) = ordnance_picture(bay.picture) {
                // The side view without the picture's frame lines.
                c.blit_scaled(&art.ordnance, sx + 1, sy + 1, 75, 13, w - 152, y + (row - 26) / 2, 2, Some(0), None);
            }
            text2(c, fonts, 6, y + 6, &bay.name, 47);
            text2(c, fonts, 6, y + 22, &format!("X{}", bay.count), 47);
        }
        c.rect(0, 0, w, ht, 47);
        for i in 1..rows as i32 {
            c.hline(0, w - 1, row * i, 47);
        }
    }

    pub(crate) fn draw_ord_hires(&self, c: &mut Canvas, fonts: &Fonts) {
        c.clear(0);
        for (row, (colour, line)) in self.orders.lines.iter().enumerate().take(13) {
            let mut s = vec![ink(*colour)];
            s.extend(upper(line));
            fonts.text_scaled(c, 6, 6 + 13 * row as i32, &s, 15, 2);
        }
    }
}

/// Fuel load shown as a full bar (lbs).
pub(crate) const FULL_FUEL: f32 = 10000.0;

/// Start/end bearing of a radar arc in BAM high-byte units (end > start).
fn arc_range(arc: &MapArc) -> (f64, f64) {
    arc.arc.map_or((0.0, 256.0), |(a, b)| {
        let (a, b) = (a as f64, b as f64);
        (a, if b < a { b + 256.0 } else { b })
    })
}

/// Weapon side views in `images/ordnance.idx`: picture `p` is the 77x15 box (frame included)
/// at (p % 4 * 80, p / 4 * 16).
fn ordnance_picture(p: usize) -> Option<(i32, i32)> {
    (p < 19).then(|| ((p % 4) as i32 * 80, (p / 4) as i32 * 16))
}

/// Map icons in the sprite atlas, at (8 * icon + 1, 65), 7x7.
const AIRBASE_ICON: i32 = 5;

#[derive(Clone, Copy)]
enum TacIcon {
    Airfield,
    Radar,
}

/// Classic TAC icons, 7x7 around the middle pixel: `g` light green, `w` white, `r` red,
/// `p` light red, `.` clear.
const AIRFIELD_PIXELS: [&[u8; 7]; 7] = [b"..ggg..", b".g.w.g.", b"g.www.g", b"g..w..g", b"g.www.g", b".g.w.g.", b"..ggg.."];
const RADAR_PIXELS: [&[u8; 7]; 7] = [b".p...p.", b"p.rrr.p", b"p..r..p", b".p.r.p.", b"...r...", b"..rrr..", b"......."];

/// A TAC icon centred on (x, y): pixel art on the classic display, drawn at the modern
/// display's resolution otherwise.
fn tac_icon(c: &mut Canvas, icon: TacIcon, x: i32, y: i32, hires: bool) {
    if !hires {
        let rows = match icon {
            TacIcon::Airfield => AIRFIELD_PIXELS,
            TacIcon::Radar => RADAR_PIXELS,
        };
        for (j, row) in rows.iter().enumerate() {
            for (i, &p) in row.iter().enumerate() {
                let colour = match p {
                    b'g' => 10,
                    b'w' => 15,
                    b'r' => 41,
                    b'p' => 13,
                    _ => continue,
                };
                c.set(x - 3 + i as i32, y - 3 + j as i32, colour);
            }
        }
        return;
    }
    // Pixels whose centres are within `r0..r1` of (cx, cy), between angles `a0..a1` (radians
    // counter-clockwise from screen right).
    let ring = |c: &mut Canvas, cx: f32, cy: f32, r0: f32, r1: f32, a0: f32, a1: f32, colour: u8| {
        let n = r1.ceil() as i32 + 1;
        for j in -n..=n {
            for i in -n..=n {
                let (px, py) = (cx.floor() as i32 + i, cy.floor() as i32 + j);
                let d = Vec2::new(px as f32 + 0.5 - cx, cy - (py as f32 + 0.5));
                let t = d.y.atan2(d.x);
                if (r0..=r1).contains(&d.length()) && (a0..=a1).contains(&t) {
                    c.set(px, py, colour);
                }
            }
        }
    };
    let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
    let pi = std::f32::consts::PI;
    match icon {
        TacIcon::Airfield => {
            // Ring around a runway with threshold bars and a dashed centre line.
            ring(c, fx, fy, 8.6, 10.2, -pi, pi, 10);
            c.fill_rect(x - 2, y - 6, 5, 13, 7);
            c.fill_rect(x - 3, y - 6, 7, 1, 15);
            c.fill_rect(x - 3, y + 6, 7, 1, 15);
            for d in [-3, 1] {
                c.fill_rect(x, y + d, 1, 2, 15);
            }
        }
        TacIcon::Radar => {
            // Dish on a tapered mast, waves rising from the feed.
            ring(c, fx, fy - 1.0, 0.0, 5.8, -pi, 0.0, 41);
            c.fill_rect(x - 6, y - 1, 13, 1, 13);
            c.fill_rect(x, y - 4, 1, 3, 41);
            c.line(x - 1, y + 4, x - 4, y + 9, 41);
            c.line(x + 1, y + 4, x + 4, y + 9, 41);
            c.hline(x - 5, x + 5, y + 9, 41);
            for r in [3.5, 6.5] {
                ring(c, fx, fy - 4.0, r, r + 1.2, 0.5, pi - 0.5, 13);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_view_round_trip() {
        let v = MapView { centre: DVec2::new(1000.0, 2000.0), scale: 2.5, rot: 0.7, size: Vec2::new(231.0, 180.0) };
        for p in [Vec2::ZERO, Vec2::new(30.0, 170.0), Vec2::new(231.0, 2.0)] {
            let m = v.to_map(p);
            assert!(v.to_screen(m.x, m.y).distance(p) < 1e-3);
        }
    }

    #[test]
    fn map_view_orientation() {
        let size = Vec2::new(231.0, 180.0);
        let p = DVec2::new(10000.0, 10000.0);
        // North up: north (map y decreasing) is up on the screen.
        let v = hires_view(p, 1.0, 6, false, size);
        let me = v.to_screen(p.x, p.y);
        assert!(v.to_screen(p.x, p.y - 100.0).y < me.y);
        // Heading up (heading east): a point ahead is straight above the aircraft.
        let v = hires_view(p, std::f64::consts::FRAC_PI_2, 6, true, size);
        let me = v.to_screen(p.x, p.y);
        let ahead = v.to_screen(p.x + 100.0, p.y);
        assert!(ahead.y < me.y && (ahead.x - me.x).abs() < 1e-3);
        assert!((me.y - 0.7 * size.y).abs() < 1.0);
    }
}
