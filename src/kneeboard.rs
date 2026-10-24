//! Kneeboard: the briefing, the route, a map and the targets (recon photos and the target area,
//! `recon.rs`) on a board strapped to the
//! pilot's left thigh in the 3D cockpit (look down; in VR touch a tab with a fingertip, or click
//! one on the desktop). On the desktop K also shows it large over the view and turns its pages
//! (after the last page it goes away); in VR the action shows or hides the board, and squeezing
//! the grip on it picks it up: it floats wherever it is let go.
//!
//! The KEYS page is the key guide (`key_guide.rs`), scrolled with the mouse wheel over the board
//! or, in VR, the thumbstick of the hand carrying it or with a fingertip over it.

use std::sync::Arc;

use bevy::input::mouse::AccumulatedMouseScroll;
use bevy::light::NotShadowCaster;
use bevy::math::DVec2;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;

use stealth_sim::oflags;

use crate::camera::EyeCamera;
use crate::canvas::{Canvas, CanvasTexture, Picture};
use crate::cockpit3d::{CockpitRoot, Hands};
use crate::flight::Player;
use crate::font::{Align, stroke_text_at, stroke_width};
use crate::hotas::InputConfig;
use crate::input::{Action, ActionEvent};
use crate::key_guide;
use crate::mission::{ActiveMission, PlannedRoute, RouteKind, RoutePoint, clean_text};
use crate::recon::{PHOTO_H, PHOTO_W, ReconPhotos};
use crate::settings::{AppState, Settings};
use crate::world::WorldPos;

pub struct KneeboardPlugin;

impl Plugin for KneeboardPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Kneeboard>()
            .add_systems(OnEnter(AppState::Flight), |mut kb: ResMut<Kneeboard>| {
                *kb = Kneeboard::default();
            })
            .add_systems(
                Update,
                (toggle, spawn, carry, interact, prepare_maps, draw)
                    .chain()
                    .run_if(in_state(AppState::Flight)),
            );
    }
}

/// Board size (metres) and where it lies in the cockpit frame (the eye at the origin): on top of
/// the left thigh, from the ejection handle at the front of the seat between the knees (inboard
/// enough to clear the F-16's throttle grip over its whole travel), in the F-16's cockpit model:
/// both scale with the flown one (`AircraftSpec::cockpit_model_scale`).
const SIZE: Vec2 = Vec2::new(0.15, 0.20);
const FROM_EJECT: Vec3 = Vec3::new(-0.12, 0.11, 0.06);
/// Canvas pixels.
const W: usize = 768;
const H: usize = 1024;
/// Tab row height and the page area below it.
const TABS: f32 = 84.0;
const MAP_TOP: usize = 96;
const MAP_W: usize = W - 32;
const MAP_H: usize = H - MAP_TOP - 80;
/// Palette colours (the low 16, which the time of day leaves alone).
const BG: u8 = 5;
const WHITE: u8 = 15;
const GREY: u8 = 7;
const CYAN: u8 = 11;
const GREEN: u8 = 10;
const RED: u8 = 13;
const BLUE: u8 = 9;
const TAB_BG: u8 = 8;
/// Cruise speed for the leg times (knots), as in the planner.
const CRUISE_KTS: f64 = 480.0;
/// A VR fingertip touches the board from this far in front of it to this far behind it.
const TOUCH_FRONT: f32 = 0.04;
const TOUCH_BEHIND: f32 = 0.06;
/// A VR grip picks the board up from this far beyond its edges and in front of or behind it.
const GRAB_MARGIN: f32 = 0.04;
const GRAB_DEPTH: f32 = 0.08;
/// A VR fingertip up to this far in front of the board lets its thumbstick scroll the key guide.
const SCROLL_REACH: f32 = 0.15;
/// The key guide's area on the KEYS page.
const GUIDE: [f32; 4] = [16.0, TABS + 12.0, W as f32 - 16.0, H as f32 - 52.0];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Page {
    #[default]
    Brief,
    Route,
    Map,
    Target,
    Keys,
}

impl Page {
    const ALL: [Page; 5] = [Page::Brief, Page::Route, Page::Map, Page::Target, Page::Keys];

    fn name(self) -> &'static str {
        match self {
            Page::Brief => "BRIEF",
            Page::Route => "ROUTE",
            Page::Map => "MAP",
            Page::Target => "TARGET",
            Page::Keys => "KEYS",
        }
    }
}

/// A map picture and how it maps map units to its pixels.
struct MapPicture {
    canvas: Canvas,
    centre: DVec2,
    /// Map units per pixel.
    scale: f64,
}

impl MapPicture {
    fn to_px(&self, x: f64, y: f64) -> Vec2 {
        Vec2::new(
            ((x - self.centre.x) / self.scale) as f32 + self.canvas.w as f32 / 2.0,
            ((y - self.centre.y) / self.scale) as f32 + self.canvas.h as f32 / 2.0,
        )
    }
}

#[derive(Resource, Default)]
pub struct Kneeboard {
    page: Page,
    /// The board on the thigh is hidden (VR: the action).
    hidden: bool,
    /// Desktop: shown large over the view.
    overlay: bool,
    /// Something changed: redraw now.
    dirty: bool,
    /// Redraw timer (the own-ship marker and the clock move).
    timer: f32,
    route_map: Option<MapPicture>,
    target_map: Option<MapPicture>,
    /// Tab each fingertip was on last frame.
    touched: [Option<usize>; 2],
    /// VR: where the board was let go (cockpit frame); on the thigh until then.
    placed: Option<Transform>,
    /// VR: the hand carrying the board and the board in that hand's grip frame.
    held: Option<(usize, Transform)>,
    /// The key guide scrolled down this far (canvas pixels).
    keys_scroll: f32,
}

impl Kneeboard {
    /// Desktop: the board is shown large over the view.
    pub fn overlay_shown(&self) -> bool {
        self.overlay
    }

    /// A new sortie (campaign): the maps are drawn again for its route.
    pub fn new_mission(&mut self) {
        self.route_map = None;
        self.target_map = None;
        self.dirty = true;
    }
}

#[derive(Component)]
struct Board;

#[derive(Component)]
struct Overlay;

fn toggle(
    mut actions: MessageReader<ActionEvent>,
    mut kb: ResMut<Kneeboard>,
    settings: Res<Settings>,
) {
    for _ in actions.read().filter(|a| a.0 == Action::Kneeboard) {
        if settings.vr {
            kb.hidden = !kb.hidden;
        } else if !kb.overlay {
            kb.overlay = true;
        } else {
            let i = Page::ALL.iter().position(|&p| p == kb.page).unwrap_or(0);
            if i + 1 == Page::ALL.len() {
                kb.overlay = false;
                kb.page = Page::Brief;
            } else {
                kb.page = Page::ALL[i + 1];
            }
        }
        kb.dirty = true;
    }
}

/// The board in the cockpit (spawned again when the cockpit is rebuilt) and, on the desktop,
/// the overlay showing the same picture.
#[allow(clippy::too_many_arguments)]
fn spawn(
    mut commands: Commands,
    roots: Query<Entity, With<CockpitRoot>>,
    boards: Query<&CanvasTexture, With<Board>>,
    overlays: Query<(), With<Overlay>>,
    settings: Res<Settings>,
    mut kb: ResMut<Kneeboard>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let Ok(root) = roots.single() else { return };
    let image = match boards.iter().next() {
        Some(tex) => tex.image.clone(),
        None => {
            let tex = CanvasTexture::mipmapped(Canvas::new(W, H), &mut images, false);
            let image = tex.image.clone();
            let material = materials.add(StandardMaterial {
                base_color_texture: Some(image.clone()),
                unlit: true,
                fog_enabled: false,
                ..default()
            });
            let board = commands
                .spawn((
                    Board,
                    crate::cockpit3d::Solid,
                    crate::cockpit3d::Tip::new(
                        "Kneeboard",
                        "Maps, mission notes and the key guide",
                        &[crate::input::Action::Kneeboard],
                    ),
                    tex,
                    Mesh3d(meshes.add(Rectangle::new(SIZE.x, SIZE.y))),
                    MeshMaterial3d(material),
                    Transform::default(),
                    NotShadowCaster,
                ))
                .id();
            commands.entity(root).add_child(board);
            kb.dirty = true;
            image
        }
    };
    if !settings.vr && overlays.is_empty() {
        let holder = commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    width: percent(100),
                    height: percent(100),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                GlobalZIndex(9),
                DespawnOnExit(AppState::Flight),
            ))
            .id();
        let overlay = commands
            .spawn((
                Overlay,
                ImageNode::new(image),
                Node { height: vh(90), aspect_ratio: Some(W as f32 / H as f32), ..default() },
                RelativeCursorPosition::default(),
                Visibility::Hidden,
            ))
            .id();
        commands.entity(holder).add_child(overlay);
    }
}

/// Whether a hand's grip is on the board (in the board's local frame).
fn within_reach(local: Vec3) -> bool {
    local.x.abs() < SIZE.x / 2.0 + GRAB_MARGIN
        && local.y.abs() < SIZE.y / 2.0 + GRAB_MARGIN
        && local.z.abs() < GRAB_DEPTH
}

/// The board on the thigh of the cockpit model's seat, facing the eye, its top towards the knee;
/// in VR a free hand squeezing its grip on the board carries it, and it stays where let go.
fn carry(
    settings: Res<Settings>,
    anchors: Res<crate::cockpit3d::levers::CockpitAnchors>,
    mut kb: ResMut<Kneeboard>,
    mut hands: ResMut<Hands>,
    mut boards: Query<&mut Transform, With<Board>>,
    hide: Option<Res<crate::camera::HideOwnAircraft>>,
) {
    let Ok(mut t) = boards.single_mut() else { return };
    // The cockpit is not there (VR, the campaign's Ops screen): the board is let go and stays.
    if hide.is_some_and(|h| h.0) {
        if kb.held.take().is_some() {
            kb.placed = Some(*t);
        }
        for h in hands.hands.iter_mut() {
            h.carrying = false;
        }
        return;
    }
    let scale = settings.aircraft.spec().cockpit_model_scale();
    let at = anchors.eject + FROM_EJECT * scale;
    let rest = kb.placed.unwrap_or(
        Transform::from_translation(at).looking_to(at, Vec3::NEG_Z).with_scale(Vec3::splat(scale)),
    );
    if let Some((i, _)) = kb.held {
        let h = &mut hands.hands[i];
        if kb.hidden || !h.active || h.squeeze < 0.4 {
            kb.held = None;
            kb.placed = Some(*t);
            h.haptic = h.haptic.max(0.2);
        }
    } else if !kb.hidden {
        let local_from_cockpit = rest.compute_affine().inverse();
        if let Some((i, h)) = hands.hands.iter_mut().enumerate().find(|(_, h)| {
            h.active
                && !h.lever
                && h.squeeze > 0.6
                && within_reach(local_from_cockpit.transform_point3(h.grip.translation))
        }) {
            let in_grip = h.grip.to_matrix().inverse() * rest.to_matrix();
            kb.held = Some((i, Transform::from_matrix(in_grip)));
            h.haptic = h.haptic.max(0.4);
        }
    }
    for (i, h) in hands.hands.iter_mut().enumerate() {
        h.carrying = kb.held.is_some_and(|(k, _)| k == i);
    }
    let tf = match kb.held {
        Some((i, in_grip)) => hands.hands[i].grip * in_grip,
        None => kb.placed.unwrap_or(rest),
    };
    t.set_if_neq(tf);
}

/// Canvas pixel of a point in the board's local frame (None off the board).
fn to_canvas(local: Vec3) -> Option<Vec2> {
    let u = local.x / SIZE.x + 0.5;
    let v = 0.5 - local.y / SIZE.y;
    ((0.0..1.0).contains(&u) && (0.0..1.0).contains(&v))
        .then(|| Vec2::new(u * W as f32, v * H as f32))
}

/// Tab under a canvas pixel.
fn tab_at(p: Vec2) -> Option<usize> {
    let n = Page::ALL.len();
    (p.y >= 0.0 && p.y < TABS).then(|| ((p.x / W as f32 * n as f32) as usize).min(n - 1))
}

/// Tabs: the mouse on the overlay or the board, VR fingertips on the board. The key guide
/// scrolls with the wheel over it (not while looking around: that zooms) and the VR thumbsticks.
#[allow(clippy::too_many_arguments)]
fn interact(
    mut kb: ResMut<Kneeboard>,
    settings: Res<Settings>,
    mouse: Res<ButtonInput<MouseButton>>,
    wheel: Res<AccumulatedMouseScroll>,
    time: Res<Time<Real>>,
    mut hands: ResMut<Hands>,
    windows: Query<&Window>,
    cams: Query<(&Camera, &GlobalTransform), With<EyeCamera>>,
    mut boards: Query<(&GlobalTransform, &Transform, &mut Visibility), With<Board>>,
    mut overlays: Query<
        (&RelativeCursorPosition, &mut Visibility),
        (With<Overlay>, Without<Board>),
    >,
    menu: Res<crate::flight_menu::FlightMenu>,
) {
    let mut pick: Option<usize> = None;
    // The canvas pixel under the mouse, on the overlay or on the board in the cockpit.
    let mut pointed: Option<Vec2> = None;
    let on_keys = kb.page == Page::Keys;
    for hand in hands.hands.iter_mut() {
        hand.scrolling = false;
    }
    if let Ok((rel, mut vis)) = overlays.single_mut() {
        vis.set_if_neq(if kb.overlay { Visibility::Inherited } else { Visibility::Hidden });
        if kb.overlay
            && rel.cursor_over()
            && let Some(n) = rel.normalized
        {
            pointed = Some(Vec2::new((n.x + 0.5) * W as f32, (n.y + 0.5) * H as f32));
        }
    }
    let Ok((gtf, tf, mut vis)) = boards.single_mut() else { return };
    vis.set_if_neq(if kb.hidden { Visibility::Hidden } else { Visibility::Inherited });
    let mut scroll = 0.0;
    if !kb.hidden && !menu.open {
        let inv = gtf.affine().inverse();
        if !settings.vr
            && !kb.overlay
            && let (Ok(window), Ok((camera, cam_tf))) = (windows.single(), cams.single())
            && let Some(cursor) = window.cursor_position()
            && let Ok(ray) = camera.viewport_to_world(cam_tf, cursor)
        {
            let o = inv.transform_point3(ray.origin);
            let d = inv.transform_vector3(*ray.direction);
            if d.z.abs() > 1e-6 {
                let t = -o.z / d.z;
                pointed = (t > 0.0).then(|| to_canvas(o + d * t)).flatten();
            }
        }
        if pointed.is_some() && mouse.just_pressed(MouseButton::Left) {
            pick = pointed.and_then(tab_at);
        }
        if on_keys && pointed.is_some() && !mouse.pressed(MouseButton::Right) {
            scroll -= key_guide::wheel_notches(&wheel) * key_guide::WHEEL_STEP;
        }
        // A fingertip turns to a page when it reaches its tab (fingertips are in the cockpit
        // frame, the board's parent).
        let local_from_cockpit = tf.compute_affine().inverse();
        for (k, hand) in hands.hands.iter_mut().enumerate().take(2) {
            let l = local_from_cockpit.transform_point3(hand.finger);
            let tab = (hand.active && !hand.carrying && l.z < TOUCH_FRONT && l.z > -TOUCH_BEHIND)
                .then(|| to_canvas(l))
                .flatten()
                .and_then(tab_at);
            if tab.is_some() && tab != kb.touched[k] {
                hand.haptic = hand.haptic.max(0.3);
                pick = tab;
            }
            kb.touched[k] = tab;
        }
        // The thumbstick of the hand carrying the board or with its fingertip over it.
        for hand in hands.hands.iter_mut() {
            let l = local_from_cockpit.transform_point3(hand.finger);
            let over = to_canvas(l).is_some() && l.z < SCROLL_REACH && l.z > -TOUCH_BEHIND;
            hand.scrolling = on_keys && hand.active && (hand.carrying || over);
            if hand.scrolling && hand.thumb.y.abs() > 0.15 {
                scroll -= hand.thumb.y * key_guide::SCROLL_SPEED * time.delta_secs();
            }
        }
    }
    if scroll != 0.0 {
        // Kept within the list when it is drawn.
        kb.keys_scroll = (kb.keys_scroll + scroll).max(0.0);
        kb.dirty = true;
    }
    if let Some(i) = pick
        && kb.page != Page::ALL[i]
    {
        kb.page = Page::ALL[i];
        kb.dirty = true;
    }
}

/// The map pictures, once the theater's map geometry is there.
fn prepare_maps(
    mut kb: ResMut<Kneeboard>,
    mission: Option<Res<ActiveMission>>,
    route: Option<Res<PlannedRoute>>,
    raster: Res<crate::mfd::MapRaster>,
    theater: Option<Res<crate::world::Theater>>,
    palette: Option<Res<crate::palette::Palette>>,
) {
    if kb.route_map.is_some() {
        return;
    }
    let (Some(m), Some(theater), Some(palette)) = (mission, theater, palette) else { return };
    let models = raster.models();
    if models.is_empty() {
        return;
    }
    let m = &m.0;
    let map16 = palette
        .remaps
        .iter()
        .find(|t| t.name == "map16")
        .map(|t| t.entries.clone())
        .unwrap_or_default();
    let remap = |c: u8| map16.get(c as usize).map_or(c, |w| (*w & 0xFF) as u8);
    let render = |centre: DVec2, scale: f64, w: usize, h: usize, zoom: i32| {
        let mut pic = MapPicture { canvas: Canvas::new(w, h), centre, scale };
        let half = (w as f64 / 2.0 * scale * 32.0, h as f64 / 2.0 * scale * 32.0);
        let visible = (centre.x * 32.0, (0x8000 as f64 - centre.y) * 32.0, half.0, half.1);
        let px = |east: f64, north: f64| pic.to_px(east / 32.0, 0x8000 as f64 - north / 32.0);
        let mut canvas = Canvas::new(w, h);
        crate::mfd::rasterise_map(&mut canvas, models, &theater, &px, visible, zoom, &remap);
        pic.canvas = canvas;
        pic
    };
    // The route with its base and the targets, with a margin.
    let pts = route_points(m, route.as_deref());
    let (mut lo, mut hi) = (DVec2::splat(f64::MAX), DVec2::splat(f64::MIN));
    for p in &pts {
        lo = lo.min(DVec2::new(p.x as f64, p.y as f64));
        hi = hi.max(DVec2::new(p.x as f64, p.y as f64));
    }
    let scale = ((hi.x - lo.x) / MAP_W as f64).max((hi.y - lo.y) / MAP_H as f64).max(4.0) * 1.15;
    kb.route_map = Some(render((lo + hi) / 2.0, scale, MAP_W, MAP_H, 5));
    // The primary target's surroundings, 12 km across.
    let t = m.waypoints[1];
    let scale = 12.0 * 64.0 / MAP_W as f64;
    kb.target_map = Some(render(DVec2::new(t.x as f64, t.y as f64), scale, MAP_W, MAP_W, 7));
    kb.dirty = true;
}

/// The takeoff base and the route flown.
fn route_points(m: &stealth_mission::Mission, route: Option<&PlannedRoute>) -> Vec<RoutePoint> {
    let route = route.filter(|r| r.fits(m)).cloned().unwrap_or_else(|| PlannedRoute::of(m));
    let base =
        RoutePoint { x: m.takeoff.position.x, y: m.takeoff.position.y, kind: RouteKind::Nav };
    std::iter::once(base).chain(route.points).collect()
}

/// Words of `text` in lines no wider than `width` pixels at height `h`.
pub fn wrap(text: &str, width: f32, h: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let next = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
        if !line.is_empty() && stroke_width(&next, h) > width {
            lines.push(std::mem::replace(&mut line, word.to_string()));
        } else {
            line = next;
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// What a target is (its shape's name, else the mission type).
fn target_name(t: &stealth_mission::Target) -> String {
    let name = clean_text(&t.shape_name).trim().to_ascii_uppercase();
    if name.is_empty() { t.type_name.to_ascii_uppercase() } else { name }
}

/// `text` cut short to fit `width` pixels at height `h`.
fn fit(text: &str, width: f32, h: f32) -> String {
    let mut s = text.to_string();
    while !s.is_empty() && stroke_width(&s, h) > width {
        s.pop();
    }
    s
}

/// Text cursor down the page.
struct Writer<'a> {
    c: &'a mut Canvas,
    y: f32,
}

impl Writer<'_> {
    const LEFT: f32 = 28.0;

    fn heading(&mut self, s: &str) {
        self.y += 10.0;
        stroke_text_at(self.c, Self::LEFT, self.y, s, 20.0, 2.6, CYAN, Align::Left);
        self.y += 30.0;
    }

    fn para(&mut self, s: &str, col: u8) {
        let s = clean_text(s).to_ascii_uppercase();
        for l in wrap(&s, W as f32 - 2.0 * Self::LEFT, 17.0) {
            if self.y > H as f32 - 24.0 {
                return;
            }
            stroke_text_at(self.c, Self::LEFT, self.y, &l, 17.0, 2.2, col, Align::Left);
            self.y += 26.0;
        }
    }

    /// An objective's paragraph, ticked in the margin once achieved.
    fn objective(&mut self, s: &str, col: u8, done: bool) {
        if done && self.y <= H as f32 - 24.0 {
            let y = self.y;
            self.c.polyline(
                &[Vec2::new(8.0, y), Vec2::new(14.0, y + 6.0), Vec2::new(24.0, y - 8.0)],
                3.0,
                GREEN,
            );
        }
        self.para(s, col);
    }
}

/// A large tick over a photo-sized area at `at`: a black outline under the colour.
fn tick(c: &mut Canvas, at: Vec2, w: f32, h: f32, col: u8) {
    let pts = [(0.3, 0.55), (0.45, 0.75), (0.72, 0.28)].map(|(x, y)| at + Vec2::new(x * w, y * h));
    let pen = h * 0.07;
    c.polyline(&pts, pen + 4.0, BG);
    c.polyline(&pts, pen, col);
}

/// A ticked photo (`tick_photo`) by the photo it was made from.
type TickedPhoto = (usize, Arc<Picture>);

/// A copy of a recon photo with the tick of an achieved objective over it.
fn tick_photo(photo: &Picture) -> Picture {
    let mut mask = Canvas::new(photo.w, photo.h);
    tick(&mut mask, Vec2::ZERO, photo.w as f32, photo.h as f32, 1);
    let mut out = Picture { w: photo.w, h: photo.h, rgba: photo.rgba.clone() };
    for (o, &m) in out.rgba.as_chunks_mut::<4>().0.iter_mut().zip(&mask.px) {
        match m {
            1 => *o = [0x55, 0xFF, 0x55, 0xFF],
            BG => *o = [0, 0, 0, 0xFF],
            _ => {}
        }
    }
    out
}

fn clock(seconds: f64) -> String {
    let s = seconds.rem_euclid(86_400.0) as i64;
    format!("{:02}:{:02}", s / 3600, s / 60 % 60)
}

#[allow(clippy::too_many_arguments)]
fn draw(
    time: Res<Time>,
    mut kb: ResMut<Kneeboard>,
    mut boards: Query<&mut CanvasTexture, With<Board>>,
    mission: Option<Res<ActiveMission>>,
    route: Option<Res<PlannedRoute>>,
    wps: Res<crate::hud::Waypoints>,
    clock_res: Res<crate::mfd::MissionClock>,
    player: Query<&WorldPos, With<Player>>,
    combat: Option<Res<crate::combat::Combat>>,
    recon: Res<ReconPhotos>,
    sky: Res<crate::sky::Sky>,
    config: Res<InputConfig>,
    settings: Res<Settings>,
    orders: Res<crate::mfd::MissionOrders>,
    mut ticked: Local<[Option<TickedPhoto>; 2]>,
) {
    kb.timer += time.delta_secs();
    if recon.is_changed() || orders.is_changed() {
        kb.dirty = true;
    }
    let visible = !kb.hidden || kb.overlay;
    if !(kb.dirty || (visible && kb.timer >= 1.0)) {
        return;
    }
    let Ok(mut tex) = boards.single_mut() else { return };
    let Some(m) = mission else { return };
    kb.timer = 0.0;
    kb.dirty = false;
    let m = &m.0;
    tex.pictures.clear();
    let tex = &mut *tex;
    let c = &mut tex.canvas;
    c.clear(BG);
    // Tabs.
    let n = Page::ALL.len();
    for (i, p) in Page::ALL.iter().enumerate() {
        let x0 = (i * W / n) as i32;
        let on = *p == kb.page;
        c.fill_rect(x0 + 4, 4, (W / n) as i32 - 8, TABS as i32 - 8, if on { CYAN } else { TAB_BG });
        let mid = x0 as f32 + W as f32 / (2 * n) as f32;
        stroke_text_at(
            c,
            mid,
            TABS / 2.0,
            p.name(),
            22.0,
            3.0,
            if on { BG } else { WHITE },
            Align::Centre,
        );
    }
    let pts = route_points(m, route.as_deref());
    let me = player.single().ok().map(|p| {
        let (e, n, _) = crate::world::bevy_to_wu(p.0);
        (e / 32.0, 0x8000 as f64 - n / 32.0)
    });
    let now = clock_res.start_seconds as f64 + clock_res.elapsed.unwrap_or(0.0) as f64;
    let b = &m.briefing;
    match kb.page {
        Page::Brief => {
            let mut w = Writer { c, y: TABS + 34.0 };
            w.para(&b.title, WHITE);
            w.para(&b.base_date_line, GREY);
            w.heading("OBJECTIVES");
            w.objective(&format!("PRIMARY: {}", b.primary_objective), GREEN, orders.done[0]);
            w.objective(&format!("SECONDARY: {}", b.secondary_objective), WHITE, orders.done[1]);
            w.heading("TIMES");
            w.para(&b.depart_line, WHITE);
            w.para(&b.land_line, WHITE);
            if !m.time.deadline.is_empty() && m.time.has_deadline {
                w.para(&format!("DEADLINE {}", m.time.deadline), RED);
            }
            w.para(&format!("NOW {}", clock(now)), WHITE);
            w.heading("CONDITIONS");
            if !b.weather_line.is_empty() {
                w.para(&b.weather_line, WHITE);
            }
            let phase = sky.phase_at(now / 3600.0);
            w.para(
                &format!(
                    "MOON: {} ({:.0}% LIT)",
                    crate::sky::phase_name(phase),
                    crate::sky::illumination(phase) * 100.0
                ),
                WHITE,
            );
            w.para(&b.fuel_line, WHITE);
            w.heading("ARMAMENT");
            w.para(&b.armament.join(", "), WHITE);
            w.heading("RULES OF ENGAGEMENT");
            w.para(&b.roe_text, GREY);
        }
        Page::Route => {
            let mut y = TABS + 40.0;
            let cols = [28.0, 170.0, 330.0, 500.0, 740.0];
            for (x, h) in cols.iter().zip(["PT", "BRG", "DIST", "ETA"]) {
                stroke_text_at(c, *x, y, h, 20.0, 2.6, CYAN, Align::Left);
            }
            y += 44.0;
            let mut t = m.time.tod_ticks as f64 * 2.0 + m.time.base_hour as f64 * 3600.0;
            for i in 1..pts.len() {
                let (a, p) = (pts[i - 1], pts[i]);
                let (dx, dy) = ((p.x - a.x) as f64, (p.y - a.y) as f64);
                let km = dx.hypot(dy) / 64.0;
                let brg = dx.atan2(-dy).to_degrees().rem_euclid(360.0);
                t += km / (CRUISE_KTS * 1.852) * 3600.0;
                let label = match p.kind {
                    RouteKind::Primary => "PRI".to_string(),
                    RouteKind::Secondary => "SEC".to_string(),
                    RouteKind::Land => "LAND".to_string(),
                    RouteKind::Nav => format!("N{i}"),
                };
                let current = wps.current + 1 == i;
                if current {
                    c.fill_rect(16, (y - 20.0) as i32, W as i32 - 32, 40, TAB_BG);
                }
                let col = if current { GREEN } else { WHITE };
                let row = [label, format!("{brg:03.0}"), format!("{km:.0} KM"), clock(t)];
                for (x, s) in cols.iter().zip(row) {
                    stroke_text_at(c, *x, y, &s, 22.0, 2.8, col, Align::Left);
                }
                y += 48.0;
                if y > H as f32 - 60.0 {
                    break;
                }
            }
            let total: f64 = pts
                .windows(2)
                .map(|w| ((w[1].x - w[0].x) as f64).hypot((w[1].y - w[0].y) as f64) / 64.0)
                .sum();
            y += 20.0;
            stroke_text_at(
                c,
                28.0,
                y,
                &format!("TOTAL {total:.0} KM AT {CRUISE_KTS:.0} KT"),
                20.0,
                2.6,
                GREY,
                Align::Left,
            );
            stroke_text_at(
                c,
                28.0,
                y + 36.0,
                &format!("FUEL {} LBS", m.fuel_lbs),
                20.0,
                2.6,
                GREY,
                Align::Left,
            );
        }
        Page::Map => {
            let Some(pic) = &kb.route_map else {
                stroke_text_at(
                    c,
                    W as f32 / 2.0,
                    H as f32 / 2.0,
                    "PREPARING...",
                    24.0,
                    3.0,
                    GREY,
                    Align::Centre,
                );
                tex.dirty = true;
                return;
            };
            let (ox, oy) = (16.0, MAP_TOP as f32);
            c.copy_rect(&pic.canvas, 0, 0, MAP_W as i32, MAP_H as i32, ox as i32, oy as i32);
            c.set_clip(ox as i32, oy as i32, ox as i32 + MAP_W as i32, oy as i32 + MAP_H as i32);
            let at = |x: f64, y: f64| pic.to_px(x, y) + Vec2::new(ox, oy);
            // Radar sites with their missile range.
            if let Some(cb) = &combat {
                let sim = &cb.sim;
                for i in 3..sim.ground_unit_count() {
                    let o = sim.objects()[i];
                    if o.unit_type <= 0
                        || o.flags & (oflags::DESTROYED | oflags::FRIENDLY | oflags::NEUTRAL) != 0
                    {
                        continue;
                    }
                    let p = at(o.x as f64, o.y as f64);
                    let r = sim.site_lock_range(o.unit_type) as f32 * 64.0 / pic.scale as f32;
                    c.ring(p, r, 1.5, RED);
                    c.circle(p.x, p.y, 4.0, RED);
                }
            }
            let route: Vec<Vec2> = pts.iter().map(|p| at(p.x as f64, p.y as f64)).collect();
            c.polyline(&route, 3.0, WHITE);
            for (i, (p, rp)) in route.iter().zip(&pts).enumerate().skip(1) {
                let col = match rp.kind {
                    RouteKind::Primary => GREEN,
                    RouteKind::Secondary => CYAN,
                    _ => WHITE,
                };
                c.ring(*p, if wps.current + 1 == i { 11.0 } else { 7.0 }, 2.5, col);
            }
            if let Some((x, y)) = me {
                let p = at(x, y);
                c.circle(p.x, p.y, 6.0, BLUE);
            }
            c.reset_clip();
            c.frame(Vec2::new(ox, oy), Vec2::new(ox + MAP_W as f32, oy + MAP_H as f32), 2.0, GREY);
            let km = 20.0 * 64.0 / pic.scale as f32;
            let y = (MAP_TOP + MAP_H) as f32 + 36.0;
            c.stroke(Vec2::new(28.0, y), Vec2::new(28.0 + km, y), 3.0, WHITE);
            stroke_text_at(c, 40.0 + km, y, "20 KM", 18.0, 2.4, WHITE, Align::Left);
            stroke_text_at(c, W as f32 - 28.0, y, "N UP", 18.0, 2.4, GREY, Align::Right);
        }
        Page::Target => {
            let t = &m.primary;
            let mut w = Writer { c, y: TABS + 34.0 };
            w.para(&format!("{} - {}", t.type_name, t.place_name), GREEN);
            w.objective(&format!("PRIMARY: {}", b.primary_objective), WHITE, orders.done[0]);
            if !b.secondary_objective.is_empty() {
                w.objective(&format!("SECONDARY: {}", b.secondary_objective), CYAN, orders.done[1]);
            }
            // Recon photos of the primary and the secondary target, side by side.
            let top = w.y as i32 - 6;
            let c = w.c;
            let (pw, ph) = (PHOTO_W as i32, PHOTO_H as i32);
            for (k, tgt) in [&m.primary, &m.secondary].into_iter().enumerate() {
                // No secondary target (campaign sorties).
                if k > 0 && (tgt.position.x, tgt.position.y) == (0, 0) {
                    continue;
                }
                let x = 16 + k as i32 * (MAP_W as i32 - pw);
                let mid = Vec2::new((x + pw / 2) as f32, (top + ph / 2) as f32);
                let done = orders.done[k];
                match &recon.photos[k] {
                    // An achieved objective's photo carries a tick (made once per photo).
                    Some(photo) if done => {
                        let id = Arc::as_ptr(photo) as usize;
                        if ticked[k].as_ref().is_none_or(|t| t.0 != id) {
                            ticked[k] = Some((id, Arc::new(tick_photo(photo))));
                        }
                        let pic = ticked[k].as_ref().map(|t| t.1.clone()).unwrap();
                        tex.pictures.push((UVec2::new(x as u32, top as u32), pic))
                    }
                    Some(photo) => {
                        tex.pictures.push((UVec2::new(x as u32, top as u32), photo.clone()))
                    }
                    None => {
                        c.fill_rect(x, top, pw, ph, TAB_BG);
                        let note = if crate::recon::airborne(tgt) {
                            "AIRBORNE"
                        } else if recon.developing(k) {
                            "DEVELOPING..."
                        } else {
                            "NO PHOTO"
                        };
                        stroke_text_at(c, mid.x, mid.y, note, 18.0, 2.4, GREY, Align::Centre);
                        if done {
                            tick(c, Vec2::new(x as f32, top as f32), pw as f32, ph as f32, GREEN);
                        }
                    }
                }
                let (a, z) = (
                    Vec2::new(x as f32, top as f32),
                    Vec2::new((x + pw) as f32, (top + ph) as f32),
                );
                c.frame(a, z, 2.0, GREY);
                let label =
                    fit(&format!("{} {}", ["PRI", "SEC"][k], target_name(tgt)), pw as f32, 16.0);
                let col = if k == 0 { GREEN } else { CYAN };
                stroke_text_at(
                    c,
                    x as f32,
                    (top + ph) as f32 + 20.0,
                    &label,
                    16.0,
                    2.2,
                    col,
                    Align::Left,
                );
            }
            let top = top + ph + 44;
            if crate::recon::airborne(t) {
                let mut w = Writer { c, y: top as f32 + 10.0 };
                w.heading("AIRBORNE TARGET");
                w.para(&b.primary_summary, GREY);
            } else if let Some(pic) = &kb.target_map {
                // The target map's middle, as tall as leaves room for the summary.
                let h = (H as i32 - 150 - top).clamp(160, MAP_W as i32);
                let sy = (MAP_W as i32 - h) / 2;
                c.copy_rect(&pic.canvas, 0, sy, MAP_W as i32, h, 16, top);
                let centre = Vec2::new(16.0 + MAP_W as f32 / 2.0, (top + h / 2) as f32);
                c.ring(centre, 26.0, 3.0, GREEN);
                c.frame(
                    Vec2::new(16.0, top as f32),
                    Vec2::new(16.0 + MAP_W as f32, (top + h) as f32),
                    2.0,
                    GREY,
                );
                let km = 2.0 * 64.0 / pic.scale as f32;
                let y = (top + h) as f32 + 30.0;
                c.stroke(Vec2::new(28.0, y), Vec2::new(28.0 + km, y), 3.0, WHITE);
                stroke_text_at(c, 40.0 + km, y, "2 KM", 18.0, 2.4, WHITE, Align::Left);
                let p = m.waypoints[1];
                let grid = format!("GRID {:03}.{:03}", p.x / 64, (0x8000 - p.y) / 64);
                stroke_text_at(c, W as f32 - 28.0, y, &grid, 18.0, 2.4, GREY, Align::Right);
                let mut w = Writer { c, y: y + 40.0 };
                w.para(&b.primary_summary, GREY);
            }
        }
        Page::Keys => {
            let lines = key_guide::lines(&config, settings.vr);
            let style = key_guide::Style {
                size: 1.0,
                heading: CYAN,
                bar: TAB_BG,
                keys: GREEN,
                text: WHITE,
                dim: GREY,
            };
            let mut scroll = kb.keys_scroll;
            key_guide::draw(c, &lines, GUIDE, &mut scroll, &style);
            kb.keys_scroll = scroll;
            let hint = if settings.vr {
                "SCROLL: THUMBSTICK, FINGER ON THE BOARD"
            } else {
                "SCROLL: MOUSE WHEEL OVER THE BOARD"
            };
            stroke_text_at(
                c,
                W as f32 / 2.0,
                H as f32 - 26.0,
                hint,
                16.0,
                2.2,
                GREY,
                Align::Centre,
            );
        }
    }
    tex.dirty = true;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_words() {
        let lines = wrap("ONE TWO THREE FOUR FIVE", stroke_width("ONE TWO THREE", 10.0), 10.0);
        assert_eq!(lines, ["ONE TWO THREE", "FOUR FIVE"]);
        assert!(wrap("", 100.0, 10.0).is_empty());
    }

    #[test]
    fn tabs_and_board_pixels() {
        assert_eq!(tab_at(Vec2::new(10.0, 10.0)), Some(0));
        assert_eq!(tab_at(Vec2::new(W as f32 - 1.0, 10.0)), Some(Page::ALL.len() - 1));
        assert_eq!(tab_at(Vec2::new(10.0, TABS + 1.0)), None);
        // The board's top-left corner is the canvas origin.
        let p = to_canvas(Vec3::new(-SIZE.x / 2.0 + 1e-4, SIZE.y / 2.0 - 1e-4, 0.0)).unwrap();
        assert!(p.x < 1.0 && p.y < 1.0);
        assert!(to_canvas(Vec3::new(SIZE.x, 0.0, 0.0)).is_none());
    }
}
