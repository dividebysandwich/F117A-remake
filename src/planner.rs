//! Mission planner: the theater map with the enemy's radar and missile coverage, and the route
//! to fly. The targets and the landing base are fixed; the player adds, moves and removes
//! navigation points between them.

use std::collections::HashMap;

use bevy::gltf::{Gltf, GltfMesh, GltfNode};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};

use stealth_mission::Mission;
use stealth_mission::world::flags as F;
use stealth_sim::SimData;

use crate::canvas::Canvas;
use crate::map_draw::{self, MapView, UAV_PINK, drone_cross, missile_circle};
use crate::menu::{AMBER, BRIGHT, DIM, GREEN, MenuState, big_button, section};
use crate::mfd::{MapModel, map_models, rasterise_map};
use crate::mission::{PlannedRoute, RouteKind, RoutePoint};
use crate::models::ModelLibrary;
use crate::palette::{RemapTable, VgaPalette, load_theater_palettes};
use crate::world::Theater;

/// Side of the rendered theater map (pixels; 32 map units, 500 m, each).
const MAP_PX: usize = 1024;
/// Most route points.
const MAX_POINTS: usize = stealth_campaign::tuning::ROUTE_POINTS;
/// Cruise speed for the leg times (knots).
const CRUISE_KTS: f64 = 480.0;
/// Map units per km.
const PER_KM: f64 = 64.0;

/// Flight profile the radar coverage is drawn for (doors shut, wings level).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Profile {
    #[default]
    Low,
    Medium,
    High,
}

impl Profile {
    const ALL: [Profile; 3] = [Profile::Low, Profile::Medium, Profile::High];

    fn feet(self) -> u16 {
        match self {
            Profile::Low => 500,
            Profile::Medium => 5000,
            Profile::High => 20000,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Profile::Low => "LOW 500 FT",
            Profile::Medium => "MED 5000 FT",
            Profile::High => "HIGH 20000 FT",
        }
    }
}

/// Planner state (the route itself lives in the menu state).
#[derive(Resource, Default)]
pub struct Planner {
    pub open: bool,
    /// Theater whose map is being prepared or shown.
    theater: Option<Theater>,
    gltf: Option<Handle<Gltf>>,
    /// The map picture: theater, texture, and the egui context (camera) it was loaded into (a
    /// texture only exists in its own context: the menu, the debrief and the campaign's Ops
    /// screen each have theirs).
    texture: Option<(String, egui::TextureHandle, Entity)>,
    sim: Option<SimData>,
    /// View: map centre (map units) and map units per screen point.
    centre: egui::Vec2,
    scale: f32,
    /// Route point being dragged, and its offset from the pointer (screen points).
    drag: Option<(usize, egui::Vec2)>,
    /// The view was fitted to this route's mission.
    fitted: Option<(String, u8, u32)>,
    profile: Profile,
}

impl Planner {
    pub fn opened(open: bool) -> Self {
        Self { open, ..default() }
    }

    /// The next route shown is fitted to the view again (a new campaign sortie: its route
    /// key is the campaign's).
    pub fn refit(&mut self) {
        self.fitted = None;
        self.drag = None;
    }

    /// A route point is being dragged.
    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// The map picture of theater `id`, once prepared.
    pub fn map_texture(&self, id: &str) -> Option<&egui::TextureHandle> {
        self.texture.as_ref().filter(|(t, ..)| t == id).map(|(_, t, _)| t)
    }
}

/// Prepares the map texture of the menu's theater while the planner is open.
#[allow(clippy::too_many_arguments)]
pub fn prepare_map(
    mut planner: ResMut<Planner>,
    state: Res<MenuState>,
    mut library: ResMut<ModelLibrary>,
    server: Res<AssetServer>,
    gltfs: Res<Assets<Gltf>>,
    nodes: Res<Assets<GltfNode>>,
    meshes: Res<Assets<GltfMesh>>,
    mesh_data: Res<Assets<Mesh>>,
    mut contexts: EguiContexts,
    debrief: Option<Res<crate::debrief::Debrief>>,
    app_state: Res<State<crate::settings::AppState>>,
    campaign: Option<Res<crate::campaign::ActiveCampaign>>,
    egui_context: Query<Entity, With<bevy_egui::PrimaryEguiContext>>,
) {
    // The planner's mission, the one flown on the debrief's flight map, or the campaign's
    // theater (its war map).
    let code = if *app_state.get() == crate::settings::AppState::Flight {
        campaign.map(|c| c.mission.theater_code.clone())
    } else if *app_state.get() == crate::settings::AppState::Debrief {
        debrief.as_ref().and_then(|d| d.mission.as_ref()).map(|m| m.theater_code.clone())
    } else if planner.open {
        match &state.mission {
            Some(Ok(m)) => Some(m.theater_code.clone()),
            _ => None,
        }
    } else {
        None
    };
    let Some(code) = code else { return };
    let id = code.to_ascii_lowercase();
    let Ok(context) = egui_context.single() else { return };
    if planner.texture.as_ref().is_some_and(|(t, _, c)| *t == id && *c == context) {
        return;
    }
    if planner.sim.is_none() {
        planner.sim = stealth_host::theater::sim_tables().map_err(|e| warn!("planner: {e:#}")).ok();
    }
    if planner.theater.as_ref().is_none_or(|t| t.id != id) {
        planner.theater = Some(Theater::load(&id));
        planner.gltf = Some(library.load(&server, &id));
    }
    let Some(g) = planner.gltf.as_ref().and_then(|h| gltfs.get(h)) else { return };
    let models = map_models(&id, g, &nodes, &meshes, &mesh_data);
    let Some(theater) = planner.theater.as_ref() else { return };
    let image = render_theater(theater, &models);
    let Ok(ctx) = contexts.ctx_mut() else { return };
    let tex = ctx.load_texture(format!("planner_{id}"), image, egui::TextureOptions::LINEAR);
    planner.texture = Some((id, tex, context));
}

/// `--export-maps`: the theaters still to write, the one loading, and where to.
#[derive(Resource)]
pub struct MapExport {
    dir: std::path::PathBuf,
    next: usize,
    loading: Option<(Theater, Handle<Gltf>)>,
}

impl MapExport {
    pub fn new(dir: String) -> Self {
        Self { dir: dir.into(), next: 0, loading: None }
    }
}

/// Side of an exported map picture (pixels).
const EXPORT_PX: u32 = 512;

/// Writes each theater's map picture as `<dir>/<id>.png`, then exits.
#[allow(clippy::too_many_arguments)]
pub fn export_maps(
    mut export: ResMut<MapExport>,
    mut library: ResMut<ModelLibrary>,
    server: Res<AssetServer>,
    gltfs: Res<Assets<Gltf>>,
    nodes: Res<Assets<GltfNode>>,
    meshes: Res<Assets<GltfMesh>>,
    mesh_data: Res<Assets<Mesh>>,
    mut exit: MessageWriter<AppExit>,
) {
    let theaters = stealth_host::theater::THEATERS;
    if export.loading.is_none() {
        let Some((id, _)) = theaters.get(export.next) else {
            exit.write(AppExit::Success);
            return;
        };
        export.loading = Some((Theater::load(id), library.load(&server, id)));
    }
    let Some((theater, handle)) = export.loading.as_ref() else { return };
    let Some(g) = gltfs.get(handle) else { return };
    let models = map_models(&theater.id, g, &nodes, &meshes, &mesh_data);
    let image = render_theater(theater, &models);
    let rgba: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
    let path = export.dir.join(format!("{}.png", theater.id));
    let saved = image::RgbaImage::from_raw(MAP_PX as u32, MAP_PX as u32, rgba)
        .map(|i| {
            image::imageops::resize(&i, EXPORT_PX, EXPORT_PX, image::imageops::FilterType::Triangle)
        })
        .ok_or_else(|| "bad picture".to_string())
        .and_then(|i| i.save(&path).map_err(|e| e.to_string()));
    match saved {
        Ok(()) => info!("map picture {}", path.display()),
        Err(e) => error!("map picture {}: {e}", path.display()),
    }
    export.loading = None;
    export.next += 1;
}

/// The whole theater top-down in the day palette (the MFD map's colours).
fn render_theater(theater: &Theater, models: &HashMap<u8, MapModel>) -> egui::ColorImage {
    let mut c = Canvas::new(MAP_PX, MAP_PX);
    let px = 0x8000 as f64 / MAP_PX as f64;
    let half = 0x4000 as f64 * 32.0;
    let to_px = |east: f64, north: f64| {
        Vec2::new((east / 32.0 / px) as f32, ((0x8000 as f64 - north / 32.0) / px) as f32)
    };
    let text = crate::asset_path::read_to_string("palettes/remap.ron").unwrap_or_default();
    let remaps: Vec<RemapTable> = ron::from_str(&text).unwrap_or_default();
    let map16 =
        remaps.iter().find(|t| t.name == "map16").map(|t| t.entries.clone()).unwrap_or_default();
    let remap = |col: u8| map16.get(col as usize).map_or(col, |w| (*w & 0xFF) as u8);
    rasterise_map(&mut c, models, theater, &to_px, (half, half, half, half), 5, &remap);
    let pal: VgaPalette = load_theater_palettes(&theater.id).day;
    let rgba: Vec<u8> =
        c.px.iter()
            .flat_map(|&i| {
                let [r, g, b] = pal.colors.get(i as usize).copied().unwrap_or([0, 0, 0]);
                [r * 4 + r / 16, g * 4 + g / 16, b * 4 + b / 16, 255]
            })
            .collect();
    egui::ColorImage::from_rgba_unmultiplied([MAP_PX, MAP_PX], &rgba)
}

/// A site's coverage: where it detects the player flying the profile, and its missile range
/// (map units).
struct Ring {
    x: f32,
    y: f32,
    detect: f32,
    launch: f32,
    name: String,
    active: bool,
    /// Doppler radar (dotted detection circle; pulse radars solid).
    doppler: bool,
}

/// The game's radar detection ([`stealth_sim::radar`]): a site detects the player when its
/// score plus his EMV passes 100; the score falls linearly with range from
/// `terrain * (2 * opponents + tier + 1)` (an average aspect) to 0 at the radar's lethality.
fn threat_rings(
    m: &Mission,
    sim: &SimData,
    profile: Profile,
    known: &dyn Fn(usize) -> bool,
) -> Vec<Ring> {
    let w = &m.world;
    let diff = m.settings.opponents as i32;
    let lockheed = m.settings.plane == stealth_mission::Plane::Lockheed;
    let emv = stealth_sim::emv(
        stealth_sim::math::view_z(profile.feet()),
        (CRUISE_KTS * 27.0) as i32,
        0,
        0,
        diff,
        lockheed,
    ) as f32;
    (3..(w.ground_unit_count as usize).min(w.objects.len()))
        .filter_map(|i| {
            let o = w.objects[i];
            if o.unit_type <= 0 || o.flags & (F::FRIENDLY | F::NEUTRAL) != 0 || !known(i) {
                return None;
            }
            let t = sim.threat(o.unit_type as i32)?;
            let missile = sim.missile(t.missile.map_or(o.unit_type as i32, i32::from));
            let (x, y) = (o.x as u16 as i32, o.y as u16 as i32);
            let terrain = (w.grid[((y >> 11).clamp(0, 15) * 16 + (x >> 11).clamp(0, 15)) as usize]
                & 0xC) as f32;
            let peak =
                terrain * (2 * diff + t.tier as i32 + 1) as f32 / if lockheed { 2.0 } else { 1.0 };
            let detect = if peak > 0.0 {
                (t.lethality as f32 * (1.0 - (100.0 - emv) / peak)).max(0.0)
            } else {
                0.0
            };
            Some(Ring {
                x: x as f32,
                y: y as f32,
                detect: detect * PER_KM as f32,
                launch: missile.lock_range as f32 * PER_KM as f32,
                name: t.name.clone(),
                active: o.flags & (F::OFF_THE_AIR | F::UNDER_REPAIR) == 0,
                doppler: t.flags & 1 != 0,
            })
        })
        .collect()
}

/// Km of the route inside any of the circles of `radius`.
fn exposure(route: &[RoutePoint], rings: &[Ring], radius: impl Fn(&Ring) -> f32) -> f64 {
    let mut km = 0.0;
    for w in route.windows(2) {
        let (a, b) =
            (egui::vec2(w[0].x as f32, w[0].y as f32), egui::vec2(w[1].x as f32, w[1].y as f32));
        let len = (b - a).length() as f64 / PER_KM;
        let n = len.ceil().max(1.0) as usize;
        for k in 0..n {
            let p = a + (b - a) * ((k as f32 + 0.5) / n as f32);
            if rings.iter().any(|r| r.active && (p - egui::vec2(r.x, r.y)).length() < radius(r)) {
                km += len / n as f64;
            }
        }
    }
    km
}

fn leg_time(km: f64) -> String {
    let s = (km / (CRUISE_KTS * 1.852) * 3600.0) as i32;
    format!("{}:{:02}", s / 60, s % 60)
}

/// The route from the takeoff base (the base is not one of the INS waypoints).
fn with_base(m: &Mission, route: &[RoutePoint]) -> Vec<RoutePoint> {
    let base =
        RoutePoint { x: m.takeoff.position.x, y: m.takeoff.position.y, kind: RouteKind::Nav };
    std::iter::once(base).chain(route.iter().copied()).collect()
}

fn point_label(route: &[RoutePoint], i: usize) -> String {
    match route[i].kind {
        RouteKind::Primary => "PRI".into(),
        RouteKind::Secondary => "SEC".into(),
        RouteKind::Land => "LAND".into(),
        RouteKind::Nav => {
            format!("N{}", route[..=i].iter().filter(|p| p.kind == RouteKind::Nav).count())
        }
    }
}

/// The planner window (over the menu).
pub fn planner_ui(ctx: &egui::Context, planner: &mut Planner, state: &mut MenuState) {
    if !planner.open {
        return;
    }
    let Some(Ok(m)) = state.mission.clone() else {
        planner.open = false;
        return;
    };
    let route = state.route.get_or_insert_with(|| PlannedRoute::of(&m));
    if !route.fits(&m) {
        *route = PlannedRoute::of(&m);
    }
    let screen = ctx.content_rect();
    let size = egui::vec2(
        (screen.width() - 32.0).clamp(600.0, 1600.0),
        (screen.height() - 32.0).clamp(400.0, 1000.0),
    );
    let mut open = true;
    egui::Window::new("Mission planner")
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .collapsible(false)
        .resizable(false)
        .title_bar(false)
        .fixed_size(size)
        .frame(egui::Frame::window(&ctx.global_style()).inner_margin(12.0))
        .show(ctx, |ui| {
            ui.label(egui::RichText::new("MISSION PLANNER").heading().color(BRIGHT));
            ui.separator();
            let size = ui.available_size();
            route_panel(ui, planner, &m, route, &|_| true, None, size, true, |ui| {
                ui.add_space(12.0);
                if ui.add(big_button("DONE").fill(DIM)).clicked() {
                    open = false;
                }
            });
        });
    if !open || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        planner.open = false;
        planner.drag = None;
    }
}

/// Drawn over the route editor's map instead of the mission's airfields.
pub type MapOverlay<'a> = Option<&'a dyn Fn(&egui::Painter, &MapView)>;

/// The route editor: the map with the known radars' coverage, the route's legs and its
/// exposure (the menu's planner, the campaign's briefing). `known`: the radar sites (world
/// object indices) to show; `overlay`: drawn over the map instead of the mission's airfields;
/// `editable`: the route can be changed (else only looked at); `buttons`: below the side column.
#[allow(clippy::too_many_arguments)]
pub fn route_panel(
    ui: &mut egui::Ui,
    planner: &mut Planner,
    m: &Mission,
    route: &mut PlannedRoute,
    known: &dyn Fn(usize) -> bool,
    overlay: MapOverlay,
    size: egui::Vec2,
    editable: bool,
    buttons: impl FnOnce(&mut egui::Ui),
) {
    let rings = planner
        .sim
        .as_ref()
        .map(|s| threat_rings(m, s, planner.profile, known))
        .unwrap_or_default();
    ui.allocate_ui(size, |ui| {
    ui.horizontal_top(|ui| {
        let side = 300.0;
        let map_size = egui::vec2(ui.available_width() - side - 12.0, ui.available_height() - 8.0);
        map_view(ui, planner, m, route, &rings, overlay, map_size, editable);
        ui.vertical(|ui| {
            ui.set_width(side);
            section(ui, "Route");
            let pts = with_base(m, &route.points);
            let label = |i: usize| if i == 0 { "BASE".to_string() } else { point_label(&route.points, i - 1) };
            egui::Grid::new("legs").num_columns(3).spacing([12.0, 3.0]).show(ui, |ui| {
                for i in 1..pts.len() {
                    let km = (((pts[i].x - pts[i - 1].x) as f64).powi(2) + ((pts[i].y - pts[i - 1].y) as f64).powi(2)).sqrt() / PER_KM;
                    ui.label(format!("{} > {}", label(i - 1), label(i)));
                    ui.label(format!("{km:5.0} km"));
                    ui.label(leg_time(km));
                    ui.end_row();
                }
            });
            let length = |r: &[RoutePoint]| PlannedRoute { mission: Default::default(), points: with_base(m, r) }.length() / PER_KM;
            let total = length(&route.points);
            let default = length(&PlannedRoute::of(m).points);
            ui.add_space(4.0);
            ui.label(egui::RichText::new(format!("TOTAL {total:.0} KM  {}", leg_time(total))).color(BRIGHT));
            ui.label(egui::RichText::new(format!("Briefed route {default:.0} km, fuel {} lbs", m.fuel_lbs)).color(DIM));
            section(ui, "Radar coverage for");
            ui.horizontal(|ui| {
                for p in Profile::ALL {
                    ui.radio_value(&mut planner.profile, p, egui::RichText::new(p.name()).small());
                }
            });
            let detect = exposure(&pts, &rings, |r| r.detect);
            ui.label(egui::RichText::new(format!("{detect:.0} km of the route detected")).color(if detect > 0.0 { AMBER } else { GREEN }));
            ui.label(egui::RichText::new("Radar coverage assumes 480kt level flight, bay doors closed.").small().color(DIM));
            section(ui, "Map");
            legend(ui, Legend::Pulse, "Pulse radar detection range");
            legend(ui, Legend::Doppler, "Doppler radar detection range");
            legend(ui, Legend::Missile, "Missile range");
            if m.drones.is_some() {
                legend(ui, Legend::Drone, "Enemy UAV");
            }
            if editable {
                ui.label(egui::RichText::new("Drag a point to move it, the map to pan. Click a leg to add a point, right click to remove it. Wheel: zoom.").small().color(DIM));
                ui.add_space(12.0);
                if ui.add(big_button("RESET")).clicked() {
                    *route = PlannedRoute::of(m);
                }
            } else {
                ui.label(egui::RichText::new("Drag the map to pan. Wheel: zoom.").small().color(DIM));
            }
            buttons(ui);
        });
    });
    });
}

#[allow(clippy::too_many_arguments)]
fn map_view(
    ui: &mut egui::Ui,
    planner: &mut Planner,
    m: &Mission,
    route: &mut PlannedRoute,
    rings: &[Ring],
    overlay: MapOverlay,
    size: egui::Vec2,
    editable: bool,
) {
    let (resp, painter) = ui.allocate_painter(size, egui::Sense::click_and_drag());
    let rect = resp.rect;
    painter.rect_filled(rect, 0.0, egui::Color32::BLACK);
    // Fit the route on first showing.
    if planner.fitted.as_ref() != Some(&route.mission) || planner.scale <= 0.0 {
        let (mut lo, mut hi) = (egui::vec2(f32::MAX, f32::MAX), egui::vec2(f32::MIN, f32::MIN));
        for p in &route.points {
            lo = lo.min(egui::vec2(p.x as f32, p.y as f32));
            hi = hi.max(egui::vec2(p.x as f32, p.y as f32));
        }
        (planner.centre, planner.scale) = MapView::fit(rect, lo, hi);
        planner.fitted = Some(route.mission.clone());
    }
    let view = MapView { rect, centre: planner.centre, scale: planner.scale };
    let scale = view.scale;
    let to_screen = |x: f32, y: f32| view.to_screen(x, y);
    let to_map = |p: egui::Pos2| view.to_map(p);
    let painter = painter.with_clip_rect(rect);

    map_draw::picture(&painter, &view, planner.texture.as_ref().map(|(_, t, _)| t));
    // Threat coverage: filled radar detection circles (solid pulse, dotted Doppler), dashed
    // missile range circles; dim when off the air.
    for ring in rings {
        let c = to_screen(ring.x, ring.y);
        let alpha = if ring.active { 1.0 } else { 0.35 };
        painter.extend(missile_circle(c, ring.launch / scale, alpha));
        if ring.detect > 0.0 {
            painter.extend(detection_circle(c, ring.detect / scale, ring.doppler, alpha));
        }
        map_draw::site(&painter, &view, c, &ring.name, alpha);
    }
    match overlay {
        Some(draw) => draw(&painter, &view),
        None => map_draw::airfields(&painter, &view, m),
    }
    // Drones on their way (drone defense).
    if let Some(d) = &m.drones {
        let t = &m.world.objects[d.target as usize];
        for f in m.world.flights.iter().skip(d.first_unit).take(d.count) {
            let p = to_screen(f.x as u16 as f32, f.y as u16 as f32);
            drone_cross(&painter, p);
            painter.extend(egui::Shape::dashed_line(
                &[p, to_screen(t.x as u16 as f32, t.y as u16 as f32)],
                egui::Stroke::new(0.8, UAV_PINK.gamma_multiply(0.5)),
                3.0,
                4.0,
            ));
        }
    }
    // The route, from the takeoff base.
    let pts: Vec<egui::Pos2> =
        route.points.iter().map(|p| to_screen(p.x as f32, p.y as f32)).collect();
    let base = to_screen(m.takeoff.position.x as f32, m.takeoff.position.y as f32);
    let legs: Vec<egui::Pos2> = std::iter::once(base).chain(pts.iter().copied()).collect();
    painter.add(egui::Shape::line(legs.clone(), egui::Stroke::new(2.0, BRIGHT)));
    for (i, &p) in pts.iter().enumerate() {
        let (colour, r) = match route.points[i].kind {
            RouteKind::Nav => (GREEN, 5.0),
            RouteKind::Primary => (egui::Color32::from_rgb(255, 255, 80), 7.0),
            RouteKind::Secondary => (AMBER, 7.0),
            RouteKind::Land => (egui::Color32::from_rgb(90, 160, 255), 7.0),
        };
        painter.circle(
            p,
            r,
            egui::Color32::BLACK.gamma_multiply(0.5),
            egui::Stroke::new(2.0, colour),
        );
        painter.text(
            p + egui::vec2(9.0, -9.0),
            egui::Align2::LEFT_BOTTOM,
            point_label(&route.points, i),
            egui::FontId::monospace(13.0),
            colour,
        );
    }

    // Input.
    let hit_point = |pos: egui::Pos2| pts.iter().position(|&p| p.distance(pos) < 9.0);
    // Leg i ends at route point i (leg 0 starts at the base).
    let hit_leg = |pos: egui::Pos2| {
        (1..legs.len()).map(|i| i - 1).find(|&i| {
            let (a, b) = (legs[i], legs[i + 1]);
            let ab = b - a;
            let t = ((pos - a).dot(ab) / ab.length_sq().max(1e-3)).clamp(0.0, 1.0);
            (a + ab * t).distance(pos) < 6.0
        })
    };
    if let Some(pos) = resp.hover_pos().filter(|_| editable) {
        if hit_point(pos).is_some_and(|i| route.points[i].kind == RouteKind::Nav) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        } else if hit_leg(pos).is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        }
    }
    // A drag starts once the pointer has moved a little: what it grabs is where the button went
    // down (where the grab cursor was shown).
    if resp.drag_started()
        && editable
        && let Some(pos) = ui.input(|i| i.pointer.press_origin()).or(resp.interact_pointer_pos())
    {
        planner.drag = hit_point(pos)
            .filter(|&i| route.points[i].kind == RouteKind::Nav)
            .map(|i| (i, pts[i] - pos));
    }
    match planner.drag {
        Some((i, offset)) => {
            if resp.dragged()
                && let Some(pos) = resp.interact_pointer_pos()
            {
                let p = to_map(pos + offset);
                route.points[i].x = p.x.clamp(0x100 as f32, 0x7F00 as f32) as i32;
                route.points[i].y = p.y.clamp(0x100 as f32, 0x7F00 as f32) as i32;
            }
        }
        // Wheel zoom around the cursor; a drag that grabbed no point pans.
        None => view.pan_zoom(ui, &resp, &mut planner.centre, &mut planner.scale),
    }
    if resp.drag_stopped() {
        planner.drag = None;
    }
    if resp.clicked()
        && editable
        && let Some(pos) = resp.interact_pointer_pos()
        && hit_point(pos).is_none()
        && let Some(i) = hit_leg(pos)
        && route.points.len() < MAX_POINTS
    {
        let p = to_map(pos);
        route.points.insert(i, RoutePoint { x: p.x as i32, y: p.y as i32, kind: RouteKind::Nav });
    }
    if resp.secondary_clicked()
        && editable
        && let Some(pos) = resp.interact_pointer_pos()
        && let Some(i) = hit_point(pos).filter(|&i| route.points[i].kind == RouteKind::Nav)
        && route.points.len() > 2
    {
        route.points.remove(i);
    }
}

/// A radar's detection circle: filled, its edge solid for a pulse radar, dotted for a Doppler
/// one.
fn detection_circle(c: egui::Pos2, r: f32, doppler: bool, alpha: f32) -> Vec<egui::Shape> {
    let stroke = egui::Stroke::new(1.5, AMBER.gamma_multiply(alpha));
    let fill = AMBER.gamma_multiply(alpha * 0.18);
    if !doppler {
        return vec![
            egui::Shape::circle_filled(c, r, fill),
            egui::Shape::circle_stroke(c, r, stroke),
        ];
    }
    let n = ((r * std::f32::consts::TAU / 2.0) as usize).clamp(24, 720);
    let pts: Vec<egui::Pos2> = (0..=n)
        .map(|k| c + egui::Vec2::angled(k as f32 / n as f32 * std::f32::consts::TAU) * r)
        .collect();
    let mut shapes = vec![egui::Shape::circle_filled(c, r, fill)];
    shapes.extend(egui::Shape::dotted_line(&pts, stroke.color, 4.0, 0.9));
    shapes
}

#[derive(Clone, Copy)]
enum Legend {
    Pulse,
    Doppler,
    Missile,
    Drone,
}

/// A map legend line: the symbol as drawn on the map, then its meaning.
fn legend(ui: &mut egui::Ui, symbol: Legend, text: &str) {
    map_draw::legend(ui, text, |painter, c| match symbol {
        Legend::Pulse => painter.extend(detection_circle(c, 6.0, false, 1.0)),
        Legend::Doppler => painter.extend(detection_circle(c, 6.0, true, 1.0)),
        Legend::Missile => painter.extend(missile_circle(c, 6.0, 1.0)),
        Legend::Drone => drone_cross(painter, c),
    });
}
