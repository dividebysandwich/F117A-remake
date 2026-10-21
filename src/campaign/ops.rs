//! The campaign's Ops screen over the flight: mission board, war map, debrief and log, and the
//! prompt after landing at a friendly base. On the desktop an egui overlay over the 3D view,
//! in the headset the menu panel in front of the head (`vr_menu.rs`).

use bevy::prelude::*;
use bevy_egui::{EguiContexts, PrimaryEguiContext, egui};
use stealth_campaign::{
    InsertionState, MissionKind, MissionOffer, Objective, Side, SiteKind, SortieResult,
};
use stealth_sim::fflags;

use super::{ActiveCampaign, ME, Phase, Session, apply, describe, sortie_mission};
use crate::map_draw::{self, FRIENDLY_BLUE, MISSILE_RED, MapView};
use crate::menu::{AMBER, BG, BRIGHT, DIM, GREEN, apply_terminal_style, big_button, section};
use crate::settings::AppState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Board,
    Map,
    Debrief,
    Log,
}

/// The Ops screen's own state.
#[derive(Default)]
pub struct OpsUi {
    pub tab: Tab,
    selected: Option<u32>,
    /// Base chosen for the selected mission (None: the offer's).
    base: Option<u16>,
    error: Option<String>,
    centre: egui::Vec2,
    scale: f32,
    fitted: bool,
    /// `--campaign-take` was done.
    pub test_taken: bool,
}

/// The camera that carries the Ops screen's egui context.
#[derive(Component, Clone)]
pub struct OpsCamera;

/// Shows and hides the Ops screen's egui context with the screen.
#[allow(clippy::too_many_arguments)]
pub fn overlay(
    mut commands: Commands,
    c: Option<Res<ActiveCampaign>>,
    cams: Query<Entity, With<OpsCamera>>,
    settings: Res<crate::settings::Settings>,
    root: Option<Res<crate::camera::XrRoot>>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let want = c.is_some_and(|c| c.screen_open() && !c.exit);
    if want == !cams.is_empty() {
        return;
    }
    if !want {
        for e in &cams {
            commands.entity(e).try_despawn();
        }
        #[cfg(feature = "xr")]
        commands.remove_resource::<crate::vr_menu::WorldPanel>();
        return;
    }
    #[cfg(feature = "xr")]
    if settings.vr {
        let root = root.and_then(|r| r.0);
        crate::vr_menu::spawn(
            &mut commands,
            &mut images,
            &mut meshes,
            &mut materials,
            root,
            OpsCamera,
        );
        commands.insert_resource(crate::vr_menu::WorldPanel);
        return;
    }
    let _ = (&settings, &root, &mut images, &mut meshes, &mut materials);
    commands.spawn((
        Camera2d,
        Camera { order: 50, clear_color: ClearColorConfig::None, ..default() },
        OpsCamera,
        PrimaryEguiContext,
    ));
}

/// The flight's controls are off while the screen is up.
pub fn block_input(
    c: Res<ActiveCampaign>,
    menu: Res<crate::flight_menu::FlightMenu>,
    mut block: ResMut<crate::input::InputBlock>,
) {
    let want = c.screen_open() || menu.open;
    if block.0 != want {
        block.0 = want;
    }
}

/// Test aids: `--campaign-tab board|map|debrief|log` opens a tab, `--campaign-take <n>`
/// takes the n-th mission of the board (0: the first) once the campaign is up.
pub fn test_tab() -> Option<Tab> {
    let args: Vec<String> = std::env::args().collect();
    let t = args.iter().position(|a| a == "--campaign-tab").and_then(|i| args.get(i + 1))?;
    Some(match t.as_str() {
        "map" => Tab::Map,
        "debrief" => Tab::Debrief,
        "log" => Tab::Log,
        _ => Tab::Board,
    })
}

pub fn test_take() -> Option<usize> {
    let args: Vec<String> = std::env::args().collect();
    args.iter().position(|a| a == "--campaign-take").and_then(|i| args.get(i + 1))?.parse().ok()
}

/// Background of an unselected mission row.
const ROW: egui::Color32 = egui::Color32::from_rgb(10, 34, 18);

/// Map units to km.
fn km(d: i32) -> f32 {
    d as f32 * 15.625 / 1000.0
}

fn dist(ax: i32, ay: i32, bx: i32, by: i32) -> i32 {
    stealth_campaign::dist(ax - bx, ay - by)
}

fn kind_colour(k: MissionKind) -> egui::Color32 {
    match k {
        MissionKind::Recon => DIM,
        MissionKind::Insertion | MissionKind::Escort => FRIENDLY_BLUE,
        MissionKind::Cap => GREEN,
        _ => AMBER,
    }
}

/// The campaign clock as the sky shows it: day and time.
fn clock_text(c: &ActiveCampaign) -> String {
    let h = c.state.sky_hours();
    let day = (h / 24.0).floor() as i64 + 1;
    let hh = h.rem_euclid(24.0);
    format!("DAY {day}  {:02}:{:02}", hh.floor() as i32, ((hh.fract()) * 60.0).floor() as i32)
}

#[allow(clippy::too_many_arguments)]
pub fn ops_ui(
    mut contexts: EguiContexts,
    mut c: ResMut<ActiveCampaign>,
    mut s: Session,
    planner: Res<crate::planner::Planner>,
    loadout: Option<Res<crate::loadout::PlannedLoadout>>,
    mut next: ResMut<NextState<AppState>>,
) -> Result {
    if !c.screen_open() || c.exit {
        return Ok(());
    }
    let Ok(ctx) = contexts.ctx_mut() else { return Ok(()) };
    crate::menu::apply_ui_zoom(ctx);
    if ctx.global_style().visuals.window_fill != BG {
        ctx.all_styles_mut(apply_terminal_style);
    }
    let c = &mut *c;
    // Landed at a friendly base during a sortie: rearm, or end it.
    if c.phase == Phase::Sortie
        && let Some(base) = c.landed
    {
        let name = s.combat.sim.place_name(base);
        egui::Window::new("Landed")
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -40.0))
            .collapsible(false)
            .resizable(false)
            .title_bar(false)
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new(format!("LANDED AT {}", name.to_ascii_uppercase()))
                        .heading()
                        .color(BRIGHT),
                );
                ui.label(egui::RichText::new("The ground crew is ready").color(DIM));
                ui.horizontal(|ui| {
                    if ui.add(big_button("REARM & REFUEL")).clicked() {
                        let stations = loadout.as_ref().map(|l| l.stations()).unwrap_or_default();
                        s.combat.sim.rearm(&stations);
                        let fuel = s.constants.fuel;
                        if let Ok((mut a, _)) = s.player.single_mut() {
                            a.fuel = fuel;
                            a.ended = None;
                            a.has_flown = false;
                        }
                        let _ = s.combat.sim.take_events();
                        s.msgs.show("Rearmed and refuelled");
                        c.landed = None;
                    }
                    if ui.add(big_button("END SORTIE").fill(DIM)).clicked() {
                        s.combat.sim.end_sortie(0);
                        let ev = s.combat.sim.take_events();
                        s.combat.campaign_events.extend(ev);
                        c.landed = None;
                    }
                });
            });
        return Ok(());
    }
    let theater = c.mission.theater_name.clone();
    let tex = planner.map_texture(&c.mission.theater_code.to_ascii_lowercase()).cloned();
    let screen = ctx.content_rect();
    let width = (screen.width() - 32.0).clamp(640.0, 1200.0);
    let height = (screen.height() - 190.0).clamp(240.0, 700.0);
    let mut leave = false;
    egui::Window::new("Ops")
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .collapsible(false)
        .resizable(false)
        .title_bar(false)
        .min_width(width)
        .max_width(width)
        .frame(egui::Frame::window(&ctx.global_style()).inner_margin(14.0))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("DYNAMIC CAMPAIGN").heading().color(BRIGHT));
                ui.label(egui::RichText::new(theater.to_ascii_uppercase()).color(DIM));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new(clock_text(c)).color(BRIGHT));
                    ui.label(
                        egui::RichText::new(format!("RED {}", c.state.held(Side::Red)))
                            .color(MISSILE_RED),
                    );
                    ui.label(
                        egui::RichText::new(format!("BLUE {}", c.state.held(Side::Blue)))
                            .color(FRIENDLY_BLUE),
                    );
                });
            });
            ui.horizontal(|ui| {
                for (tab, label) in [
                    (Tab::Board, "MISSIONS"),
                    (Tab::Map, "WAR MAP"),
                    (Tab::Debrief, "DEBRIEF"),
                    (Tab::Log, "LOG"),
                ] {
                    if ui.selectable_label(c.ui.tab == tab, label).clicked() {
                        c.ui.tab = tab;
                    }
                }
            });
            ui.separator();
            ui.allocate_ui(egui::vec2(width, height), |ui| {
                ui.set_min_height(height);
                match c.ui.tab {
                    Tab::Board => board(ui, c, &mut s, width),
                    Tab::Map => war_map(ui, c, &s, tex.as_ref(), egui::vec2(width, height)),
                    Tab::Debrief => debrief(ui, c),
                    Tab::Log => log(ui, c),
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                if ui.add(big_button("SAVE & EXIT")).clicked() {
                    leave = true;
                }
                if let Some(e) = &c.ui.error {
                    ui.colored_label(AMBER, e);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new("The war goes on while you choose").small().color(DIM),
                    );
                });
            });
        });
    if leave {
        super::save::save(&c.name, &c.state);
        c.exit = true;
        next.set(AppState::Menu);
    }
    Ok(())
}

/// The mission board: the offers, the selected one's details, the insertions under way.
fn board(ui: &mut egui::Ui, c: &mut ActiveCampaign, s: &mut Session, width: f32) {
    let from = |c: &ActiveCampaign, o: &MissionOffer| {
        c.state.site(o.base).map(|b| (b.x, b.y)).unwrap_or((o.x, o.y))
    };
    if c.ui.selected.is_none_or(|id| !c.state.board.iter().any(|o| o.id == id)) {
        c.ui.selected = c.state.board.first().map(|o| o.id);
        c.ui.base = None;
    }
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(width * 0.52);
            section(ui, "Missions");
            egui::ScrollArea::vertical().id_salt("offers").max_height(height_of(ui) * 0.62).show(
                ui,
                |ui| {
                    for o in c.state.board.clone() {
                        let (bx, by) = from(c, &o);
                        let selected = c.ui.selected == Some(o.id);
                        // Each mission is a framed button row: a marker, the text, an arrow.
                        let text = egui::RichText::new(format!(
                            "{} {:<10} {:<32} {:>4.0} km  {:<9} >",
                            if selected { '\u{25B6}' } else { ' ' },
                            o.kind.name(),
                            o.title,
                            km(dist(bx, by, o.x, o.y)),
                            if o.threat > 0 {
                                format!("THREAT {}", o.threat)
                            } else {
                                String::new()
                            }
                        ))
                        .monospace()
                        .color(if selected {
                            BG
                        } else {
                            kind_colour(o.kind)
                        });
                        let row = egui::Button::new(text)
                            .fill(if selected { kind_colour(o.kind) } else { ROW })
                            .stroke(egui::Stroke::new(1.0, if selected { BRIGHT } else { DIM }))
                            .min_size(egui::vec2(ui.available_width(), 30.0));
                        let resp = ui.add(row).on_hover_cursor(egui::CursorIcon::PointingHand);
                        if resp.hovered() && !selected {
                            ui.painter().rect_stroke(
                                resp.rect,
                                2.0,
                                egui::Stroke::new(2.0, kind_colour(o.kind)),
                                egui::StrokeKind::Inside,
                            );
                        }
                        if resp.clicked() {
                            c.ui.selected = Some(o.id);
                            c.ui.base = None;
                            c.ui.error = None;
                        }
                        ui.add_space(2.0);
                    }
                    if c.state.board.is_empty() {
                        ui.label(egui::RichText::new("No missions: the front is quiet").color(DIM));
                    }
                },
            );
            ui.add_space(8.0);
            section(ui, "Insertions");
            let mut any = false;
            for ins in &c.state.insertions {
                let name = c.state.place(ins.place).map_or(String::new(), |p| p.name.clone());
                let (text, col) = match ins.state {
                    InsertionState::Pending { launch_at } => (
                        format!(
                            "{name}: lift-off in {:.0} min",
                            ((launch_at - c.state.clock) / 60.0).max(0.0).ceil()
                        ),
                        BRIGHT,
                    ),
                    InsertionState::Enroute => {
                        let left = ins.alive.iter().filter(|&&a| a).count();
                        (format!("{name}: {left} helicopters on the way"), FRIENDLY_BLUE)
                    }
                    _ => continue,
                };
                any = true;
                ui.label(egui::RichText::new(text).color(col));
            }
            if !any {
                ui.label(egui::RichText::new("None under way").color(DIM));
            }
        });
        ui.separator();
        ui.vertical(|ui| {
            let Some(o) =
                c.ui.selected.and_then(|id| c.state.board.iter().find(|o| o.id == id)).cloned()
            else {
                return;
            };
            section(ui, o.kind.name());
            ui.label(egui::RichText::new(&o.title).size(20.0).color(BRIGHT));
            ui.label(egui::RichText::new(describe(o.kind)).color(DIM));
            ui.add_space(6.0);
            if let Some(p) = place_of(&o.objective).and_then(|p| c.state.place(p)) {
                let intel = p.intel(c.state.clock);
                ui.label(format!(
                    "Defences: {} SAM sites, threat {}{}",
                    p.defences
                        .iter()
                        .filter(|&&d| c.state.site(d).is_some_and(|s| s.alive()))
                        .count(),
                    o.threat,
                    if intel { "  -  RECON INTEL" } else { "" }
                ));
            }
            if o.kind.ai_flown() {
                ui.add_space(10.0);
                if ui.add(big_button("ORDER INSERTION")).clicked()
                    && let Objective::Insertion { place } = o.objective
                {
                    match c.state.apply(ME, stealth_campaign::Command::OrderInsertion { place }) {
                        Ok(ev) => {
                            c.ui.error = None;
                            apply(c, s, ev);
                        }
                        Err(e) => c.ui.error = Some(e),
                    }
                }
                ui.label(egui::RichText::new("Fly the escort to cover them").small().color(DIM));
                return;
            }
            // From which base.
            let mut bases: Vec<(u16, String, i32)> = c
                .state
                .bases(Side::Blue)
                .into_iter()
                .filter_map(|b| {
                    let site = c.state.site(b)?;
                    Some((b, site.name.clone(), dist(site.x, site.y, o.x, o.y)))
                })
                .collect();
            bases.sort_by_key(|b| b.2);
            let chosen = c.ui.base.filter(|b| bases.iter().any(|x| x.0 == *b)).unwrap_or(o.base);
            let label = bases
                .iter()
                .find(|b| b.0 == chosen)
                .map_or(String::new(), |b| format!("{} ({:.0} km)", b.1, km(b.2)));
            ui.horizontal(|ui| {
                ui.label("From");
                egui::ComboBox::from_id_salt("base").selected_text(label).width(260.0).show_ui(
                    ui,
                    |ui| {
                        for (b, name, d) in &bases {
                            if ui
                                .selectable_label(
                                    *b == chosen,
                                    format!("{name} ({:.0} km)", km(*d)),
                                )
                                .clicked()
                            {
                                c.ui.base = Some(*b);
                            }
                        }
                    },
                );
            });
            let sortie = stealth_campaign::Sortie {
                id: 0,
                offer: o.clone(),
                base: chosen,
                started: 0.0,
                progress: Default::default(),
            };
            let m = sortie_mission(&c.mission, &c.state, &sortie);
            let stores = crate::loadout::PlannedLoadout::of(&m, s.settings.aircraft).summary();
            ui.label(egui::RichText::new(format!("Stores: {stores}")).color(DIM));
            ui.add_space(10.0);
            if ui.add(big_button("TAKE OFF").fill(DIM)).clicked() {
                let cmd =
                    stealth_campaign::Command::TakeMission { offer: o.id, base: Some(chosen) };
                match c.state.apply(ME, cmd) {
                    Ok(ev) => {
                        c.ui.error = None;
                        apply(c, s, ev);
                    }
                    Err(e) => c.ui.error = Some(e),
                }
            }
        });
    });
}

/// Height left in the tab.
fn height_of(ui: &egui::Ui) -> f32 {
    ui.available_height().max(120.0)
}

fn place_of(o: &Objective) -> Option<u16> {
    match *o {
        Objective::Recon { place, .. }
        | Objective::Sead { place, .. }
        | Objective::Cas { place, .. }
        | Objective::Strike { place, .. }
        | Objective::Insertion { place } => Some(place),
        _ => None,
    }
}

/// The war map: who holds which place, the insertions, the aircraft in the air.
fn war_map(
    ui: &mut egui::Ui,
    c: &mut ActiveCampaign,
    s: &Session,
    tex: Option<&egui::TextureHandle>,
    size: egui::Vec2,
) {
    let legend_w = 190.0;
    ui.horizontal_top(|ui| {
        let (rect, resp) = ui.allocate_exact_size(
            egui::vec2(size.x - legend_w, size.y),
            egui::Sense::click_and_drag(),
        );
        if !c.ui.fitted {
            let (lo, hi) = c.state.places.iter().fold(
                (egui::vec2(f32::MAX, f32::MAX), egui::vec2(f32::MIN, f32::MIN)),
                |(lo, hi), p| {
                    let v = egui::vec2(p.x as f32, p.y as f32);
                    (lo.min(v), hi.max(v))
                },
            );
            (c.ui.centre, c.ui.scale) = MapView::fit(rect, lo, hi);
            c.ui.fitted = true;
        }
        let view = MapView { rect, centre: c.ui.centre, scale: c.ui.scale };
        let (mut centre, mut scale) = (c.ui.centre, c.ui.scale);
        view.pan_zoom(ui, &resp, &mut centre, &mut scale);
        (c.ui.centre, c.ui.scale) = (centre, scale);
        let painter = ui.painter_at(rect);
        map_draw::picture(&painter, &view, tex);
        let st = &c.state;
        let at = |x: i32, y: i32| view.to_screen(x as f32, y as f32);
        // Insertion routes.
        for ins in &st.insertions {
            if !matches!(ins.state, InsertionState::Pending { .. } | InsertionState::Enroute) {
                continue;
            }
            if let (Some(a), Some(b)) = (st.site(ins.from), st.site(ins.target)) {
                let pts = [at(a.x, a.y), at(b.x, b.y)];
                painter.extend(egui::Shape::dashed_line(
                    &pts,
                    egui::Stroke::new(1.5, FRIENDLY_BLUE),
                    6.0,
                    4.0,
                ));
            }
        }
        // SAM sites still standing.
        for site in st.sites.iter().filter(|s| s.kind == SiteKind::Sam && s.alive()) {
            painter.circle_filled(at(site.x, site.y), 2.5, MISSILE_RED.gamma_multiply(0.8));
        }
        // The places.
        for p in &st.places {
            let col = match p.owner {
                Side::Blue => FRIENDLY_BLUE,
                Side::Red => MISSILE_RED,
                Side::Neutral => DIM,
            };
            let c0 = at(p.x, p.y);
            if p.airfield.is_some() {
                let r = egui::Rect::from_center_size(c0, egui::vec2(10.0, 10.0));
                painter.rect_filled(r, 0.0, col.gamma_multiply(0.5));
                painter.rect_stroke(r, 0.0, egui::Stroke::new(1.5, col), egui::StrokeKind::Middle);
            } else {
                painter.circle_filled(c0, 5.0, col.gamma_multiply(0.5));
                painter.circle_stroke(c0, 5.0, egui::Stroke::new(1.5, col));
            }
            if p.intel(st.clock) {
                painter.circle_stroke(c0, 10.0, egui::Stroke::new(1.0, BRIGHT));
            }
            if 1.0 / view.scale > 0.012 || p.owner != Side::Neutral {
                painter.text(
                    c0 + egui::vec2(7.0, -7.0),
                    egui::Align2::LEFT_BOTTOM,
                    &p.name,
                    egui::FontId::monospace(11.0),
                    col,
                );
            }
        }
        // Aircraft in the air: helicopters, fighters, the player.
        let sim = &s.combat.sim;
        for (i, u) in sim.flights().iter().enumerate() {
            if u.flags & fflags::AIRBORNE == 0 || u.flags & fflags::DESTROYED != 0 || u.speed == 0 {
                continue;
            }
            let Some(f) = sim.campaign_flight(i) else { continue };
            let p = at(u.x, u.y);
            let col = if f.blue { FRIENDLY_BLUE } else { MISSILE_RED };
            match f.role {
                stealth_sim::campaign::Role::Insertion => {
                    painter.circle_filled(p, 3.0, col);
                }
                stealth_sim::campaign::Role::Fighter => {
                    let pts = vec![
                        p + egui::vec2(0.0, -5.0),
                        p + egui::vec2(4.0, 4.0),
                        p + egui::vec2(-4.0, 4.0),
                    ];
                    painter.add(egui::Shape::convex_polygon(pts, col, egui::Stroke::NONE));
                }
            }
        }
        if !sim.player_absent() {
            let p = sim.player();
            painter.circle_stroke(at(p.mx, p.my), 5.0, egui::Stroke::new(2.0, GREEN));
        } else if let Some(b) = c.parked.and_then(|b| st.site(b as u16)) {
            painter.circle_stroke(at(b.x, b.y), 8.0, egui::Stroke::new(1.5, GREEN));
        }
        // The selected mission's target.
        if let Some(o) = c.ui.selected.and_then(|id| st.board.iter().find(|o| o.id == id)) {
            painter.circle_stroke(at(o.x, o.y), 14.0, egui::Stroke::new(2.0, kind_colour(o.kind)));
        }
        ui.vertical(|ui| {
            ui.set_width(legend_w - 12.0);
            section(ui, "Legend");
            map_draw::legend(ui, "Friendly airfield", |p, c0| {
                p.rect_stroke(
                    egui::Rect::from_center_size(c0, egui::vec2(9.0, 9.0)),
                    0.0,
                    egui::Stroke::new(1.5, FRIENDLY_BLUE),
                    egui::StrokeKind::Middle,
                );
            });
            map_draw::legend(ui, "Enemy airfield", |p, c0| {
                p.rect_stroke(
                    egui::Rect::from_center_size(c0, egui::vec2(9.0, 9.0)),
                    0.0,
                    egui::Stroke::new(1.5, MISSILE_RED),
                    egui::StrokeKind::Middle,
                );
            });
            map_draw::legend(ui, "Friendly / enemy city", |p, c0| {
                p.circle_stroke(
                    c0 - egui::vec2(4.0, 0.0),
                    3.5,
                    egui::Stroke::new(1.5, FRIENDLY_BLUE),
                );
                p.circle_stroke(
                    c0 + egui::vec2(5.0, 0.0),
                    3.5,
                    egui::Stroke::new(1.5, MISSILE_RED),
                );
            });
            map_draw::legend(ui, "SAM site", |p, c0| {
                p.circle_filled(c0, 2.5, MISSILE_RED);
            });
            map_draw::legend(ui, "Recon intel", |p, c0| {
                p.circle_stroke(c0, 6.0, egui::Stroke::new(1.0, BRIGHT));
            });
            map_draw::legend(ui, "Insertion helicopter", |p, c0| {
                p.circle_filled(c0, 3.0, FRIENDLY_BLUE);
            });
            map_draw::legend(ui, "Enemy fighter", |p, c0| {
                let pts = vec![
                    c0 + egui::vec2(0.0, -5.0),
                    c0 + egui::vec2(4.0, 4.0),
                    c0 + egui::vec2(-4.0, 4.0),
                ];
                p.add(egui::Shape::convex_polygon(pts, MISSILE_RED, egui::Stroke::NONE));
            });
            map_draw::legend(ui, "You", |p, c0| {
                p.circle_stroke(c0, 5.0, egui::Stroke::new(2.0, GREEN));
            });
            ui.add_space(8.0);
            ui.label(egui::RichText::new("Wheel: zoom, drag: pan").small().color(DIM));
        });
    });
}

fn debrief(ui: &mut egui::Ui, c: &ActiveCampaign) {
    let Some(d) = &c.debrief else {
        ui.label(egui::RichText::new("No sortie flown yet").color(DIM));
        return;
    };
    let r = &d.record;
    section(ui, "Sortie");
    ui.label(egui::RichText::new(&r.title).size(22.0).color(BRIGHT));
    ui.label(egui::RichText::new(d.title.to_ascii_uppercase()).color(AMBER));
    let (text, col) = match r.result {
        SortieResult::Success => ("OBJECTIVE ACHIEVED", GREEN),
        SortieResult::Failed => ("OBJECTIVE NOT ACHIEVED", AMBER),
        SortieResult::Pending => ("WAITING FOR THE INSERTION", FRIENDLY_BLUE),
    };
    // An escort settled after the sortie shows its final result.
    let settled = c.state.history.iter().find(|h| h.id == r.id).map_or(r.result, |h| h.result);
    let (text, col) = if settled != r.result {
        match settled {
            SortieResult::Success => ("OBJECTIVE ACHIEVED", GREEN),
            _ => ("OBJECTIVE NOT ACHIEVED", AMBER),
        }
    } else {
        (text, col)
    };
    ui.label(egui::RichText::new(text).size(20.0).color(col));
    ui.add_space(6.0);
    let mins = ((r.ended - r.started) / 60.0).round();
    ui.label(format!("Flight time {mins:.0} min"));
    ui.label(format!("Kills: {} aircraft, {} on the ground", r.air_kills, r.ground_kills));
    let done = c.state.history.iter().filter(|h| h.player == ME).count();
    let won = c
        .state
        .history
        .iter()
        .filter(|h| h.player == ME && h.result == SortieResult::Success)
        .count();
    ui.label(
        egui::RichText::new(format!("Campaign: {done} sorties, {won} objectives achieved"))
            .color(DIM),
    );
    ui.add_space(8.0);
    section(ui, "Meanwhile");
    egui::ScrollArea::vertical().id_salt("news").max_height(300.0).show(ui, |ui| {
        for n in &d.news {
            ui.label(egui::RichText::new(n).color(BRIGHT));
        }
    });
}

fn log(ui: &mut egui::Ui, c: &ActiveCampaign) {
    section(ui, "Campaign log");
    egui::ScrollArea::vertical().id_salt("log").show(ui, |ui| {
        for e in c.state.log.iter().rev() {
            let m = (e.time / 60.0).floor() as i64;
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("T+{:02}:{:02}", m / 60, m % 60))
                        .monospace()
                        .color(DIM),
                );
                ui.label(egui::RichText::new(&e.text).color(BRIGHT));
            });
        }
    });
}
