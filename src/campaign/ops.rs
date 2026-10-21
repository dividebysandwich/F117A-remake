//! The campaign's Ops screen over the flight: mission board, war map, debrief and log, and the
//! prompt after landing at a friendly base. On the desktop an egui overlay over the 3D view,
//! in the headset the menu panel in front of the head (`vr_menu.rs`).

use bevy::prelude::*;
use bevy_egui::{EguiContexts, PrimaryEguiContext, egui};
use stealth_campaign::{InsertionState, MissionKind, MissionOffer, Objective, Side, SortieResult};

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
    /// The accepted mission's briefing (map and loadout) before take-off.
    briefing: Option<Briefing>,
    /// SAVE & EXIT asks first.
    confirm_exit: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BriefPage {
    Map,
    Loadout,
}

/// A mission accepted from the board, briefed before take-off.
struct Briefing {
    offer: u32,
    base: u16,
    loadout: crate::loadout::PlannedLoadout,
    window: crate::loadout::LoadoutWindow,
    page: BriefPage,
    /// Map centre and scale (fitted to the route at first).
    view: Option<(egui::Vec2, f32)>,
}

/// The camera that carries the Ops screen's egui context.
#[derive(Component, Clone)]
pub struct OpsCamera;

/// Shows and hides the Ops screen's egui context with the screen.
#[allow(clippy::too_many_arguments)]
pub fn overlay(
    mut commands: Commands,
    c: Option<Res<ActiveCampaign>>,
    mut cams: Query<(Entity, &mut Msaa), With<OpsCamera>>,
    eye: Query<&Msaa, (With<crate::camera::EyeCamera>, Without<OpsCamera>)>,
    settings: Res<crate::settings::Settings>,
    root: Option<Res<crate::camera::XrRoot>>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let want = c.is_some_and(|c| c.screen_open() && !c.exit);
    // The overlay draws into the 3D view's picture only with the same anti-aliasing (else it
    // gets a picture of its own that nothing clears: the world gone behind the screen and
    // moved windows leaving trails). The 3D view's MSAA follows the shading settings.
    let msaa = eye.iter().next().copied().unwrap_or(Msaa::Off);
    for (_, mut m) in &mut cams {
        if *m != msaa {
            *m = msaa;
        }
    }
    if want == !cams.is_empty() {
        return;
    }
    if !want {
        for (e, _) in &cams {
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
        msaa,
        OpsCamera,
        PrimaryEguiContext,
    ));
}

/// The flight's controls are off while the screen is up.
pub fn block_input(
    c: Res<ActiveCampaign>,
    menu: Res<crate::flight_menu::FlightMenu>,
    mut block: ResMut<crate::input::InputBlock>,
    settings: Res<crate::settings::Settings>,
    mut hide: ResMut<crate::camera::HideOwnAircraft>,
) {
    let want = c.screen_open() || menu.open;
    if block.0 != want {
        block.0 = want;
    }
    // VR: the parked aircraft and its cockpit are not drawn around the Ops screen (the
    // in-flight menu's screen is in the cockpit: then it is drawn).
    let hidden = settings.vr && c.screen_open() && !menu.open;
    if hide.0 != hidden {
        hide.0 = hidden;
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

fn test_accept() -> Option<BriefPage> {
    let args: Vec<String> = std::env::args().collect();
    let p = args.iter().position(|a| a == "--campaign-accept").and_then(|i| args.get(i + 1))?;
    Some(if p == "loadout" { BriefPage::Loadout } else { BriefPage::Map })
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
    let secs = (hh * 3600.0).floor() as i64;
    format!("DAY {day}  {:02}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
}

#[allow(clippy::too_many_arguments)]
pub fn ops_ui(
    mut contexts: EguiContexts,
    mut c: ResMut<ActiveCampaign>,
    mut s: Session,
    planner: Res<crate::planner::Planner>,
    loadout: Option<Res<crate::loadout::PlannedLoadout>>,
    mut next: ResMut<NextState<AppState>>,
    mut unit_view: ResMut<super::UnitView>,
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
    // Test aid: `--campaign-accept map|loadout` opens the first mission's briefing.
    if let Some(page) = test_accept()
        && !std::mem::replace(&mut c.ui.test_taken, true)
        && let Some(o) = c.state.board.iter().find(|o| !o.kind.ai_flown()).cloned()
    {
        let sortie = stealth_campaign::Sortie {
            id: 0,
            offer: o.clone(),
            base: o.base,
            started: 0.0,
            progress: Default::default(),
        };
        let m = sortie_mission(&c.mission, &c.state, &sortie);
        c.ui.briefing = Some(Briefing {
            offer: o.id,
            base: o.base,
            loadout: crate::loadout::PlannedLoadout::of(&m, s.settings.aircraft),
            window: default(),
            page,
            view: None,
        });
    }
    if c.ui.briefing.is_some() {
        briefing(ctx, c, &mut s, tex.as_ref(), egui::vec2(width, height));
        return Ok(());
    }
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
                    Tab::Map => {
                        war_map(ui, c, &s, &mut unit_view, tex.as_ref(), egui::vec2(width, height))
                    }
                    Tab::Debrief => debrief(ui, c),
                    Tab::Log => log(ui, c),
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                if ui.add(big_button("SAVE & EXIT")).clicked() {
                    c.ui.confirm_exit = true;
                }
                if let Some(e) = &c.ui.error {
                    ui.colored_label(AMBER, e);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add(egui::Spinner::new().color(GREEN));
                    ui.label(egui::RichText::new("War simulation running").color(DIM));
                });
            });
        });
    if c.ui.confirm_exit {
        egui::Window::new("Leave the campaign")
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .title_bar(false)
            .order(egui::Order::Foreground)
            .frame(egui::Frame::window(&ctx.global_style()).inner_margin(18.0))
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new("SAVE AND LEAVE THE CAMPAIGN?").heading().color(BRIGHT),
                );
                ui.label(
                    egui::RichText::new(
                        "The war stops until you continue it. Aircraft in the air are lost; \
                         insertions under way start again from their base.",
                    )
                    .color(DIM),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.add(big_button("SAVE & EXIT").fill(DIM)).clicked() {
                        leave = true;
                    }
                    if ui.add(big_button("CANCEL")).clicked() {
                        c.ui.confirm_exit = false;
                    }
                });
            });
    }
    if leave {
        c.ui.confirm_exit = false;
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
            let loadout = crate::loadout::PlannedLoadout::of(&m, s.settings.aircraft);
            ui.label(egui::RichText::new(format!("Stores: {}", loadout.summary())).color(DIM));
            ui.add_space(10.0);
            if ui.add(big_button("ACCEPT MISSION").fill(DIM)).clicked() {
                c.ui.error = None;
                c.ui.briefing = Some(Briefing {
                    offer: o.id,
                    base: chosen,
                    loadout,
                    window: default(),
                    page: BriefPage::Map,
                    view: None,
                });
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
    unit_view: &mut super::UnitView,
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
        // What the friendly side sees: its aircraft, enemy fighters on radar, known defences.
        let pic = super::picture::picture(st, &s.combat.sim);
        super::picture::draw(&painter, &view, &pic, false);
        // Test aid: `--campaign-watch` watches the first friendly aircraft.
        if std::env::args().any(|a| a == "--campaign-watch")
            && unit_view.target.is_none()
            && let Some(a) = pic.friendly.first()
            && !std::mem::replace(&mut c.ui.test_taken, true)
        {
            let unit = super::unit_view::UnitRef::Flight { index: a.unit, spec: a.spec };
            unit_view.watch(unit, s.combat.sim.aircraft_name(a.spec));
        }
        // A unit under the pointer: its name; a click watches it live.
        if let Some(pos) = resp.hover_pos()
            && let Some((unit, name)) = super::picture::pick(&view, &pic, &s.combat.sim, pos)
        {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            painter.text(
                pos + egui::vec2(10.0, -10.0),
                egui::Align2::LEFT_BOTTOM,
                format!("{name} - click to watch"),
                egui::FontId::monospace(12.0),
                BRIGHT,
            );
            if resp.clicked() {
                unit_view.watch(unit, name);
            }
        }
        if pic.player.is_none()
            && let Some(b) = c.parked.and_then(|b| st.site(b as u16))
        {
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
            super::picture::legend(ui);
            ui.add_space(8.0);
            ui.label(egui::RichText::new("Click a unit to watch it live").small().color(BRIGHT));
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

/// The accepted mission's briefing: the route and what is known of the defences on a map, the
/// stores to carry; then take off (or back to the board).
fn briefing(
    ctx: &egui::Context,
    c: &mut ActiveCampaign,
    s: &mut Session,
    tex: Option<&egui::TextureHandle>,
    size: egui::Vec2,
) {
    let Some(mut b) = c.ui.briefing.take() else { return };
    let Some(o) = c.state.board.iter().find(|o| o.id == b.offer).cloned() else {
        c.ui.error = Some("That mission is no longer on the board".into());
        return;
    };
    let sortie = stealth_campaign::Sortie {
        id: 0,
        offer: o.clone(),
        base: b.base,
        started: 0.0,
        progress: Default::default(),
    };
    let m = sortie_mission(&c.mission, &c.state, &sortie);
    let mut back = false;
    let mut take_off = false;
    egui::Window::new("Briefing")
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .collapsible(false)
        .resizable(false)
        .title_bar(false)
        .min_width(size.x)
        .max_width(size.x)
        .frame(egui::Frame::window(&ctx.global_style()).inner_margin(14.0))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("MISSION BRIEFING").heading().color(BRIGHT));
                ui.label(egui::RichText::new(o.kind.name()).color(kind_colour(o.kind)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new(clock_text(c)).color(BRIGHT));
                });
            });
            ui.label(egui::RichText::new(&o.title).size(20.0).color(BRIGHT));
            ui.label(egui::RichText::new(describe(o.kind)).color(DIM));
            let d = km(dist(m.takeoff.position.x, m.takeoff.position.y, o.x, o.y));
            ui.label(format!(
                "From {} - {d:.0} km to the target - stores: {}",
                m.takeoff.name,
                b.loadout.summary()
            ));
            ui.horizontal(|ui| {
                for (page, label) in
                    [(BriefPage::Map, "BRIEFING MAP"), (BriefPage::Loadout, "LOADOUT")]
                {
                    if ui.selectable_label(b.page == page, label).clicked() {
                        b.page = page;
                    }
                }
            });
            ui.separator();
            let height = size.y - 70.0;
            ui.allocate_ui(egui::vec2(size.x, height), |ui| {
                ui.set_min_height(height);
                match b.page {
                    BriefPage::Map => {
                        brief_map(ui, c, s, &m, &mut b, tex, egui::vec2(size.x, height))
                    }
                    BriefPage::Loadout => {
                        crate::loadout::editor(ui, &mut b.window, &mut b.loadout, &m, height);
                        ui.horizontal(|ui| {
                            if ui.add(big_button("BRIEFED")).clicked() {
                                b.loadout =
                                    crate::loadout::PlannedLoadout::of(&m, s.settings.aircraft);
                            }
                        });
                    }
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                back = ui.add(big_button("BACK")).clicked();
                if let Some(e) = &c.ui.error {
                    ui.colored_label(AMBER, e);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    take_off = ui.add(big_button("TAKE OFF").fill(DIM)).clicked();
                    ui.add(egui::Spinner::new().color(GREEN));
                    ui.label(egui::RichText::new("War simulation running").color(DIM));
                });
            });
        });
    if take_off {
        let cmd = stealth_campaign::Command::TakeMission { offer: o.id, base: Some(b.base) };
        c.take_loadout = Some(b.loadout.clone());
        match c.state.apply(ME, cmd) {
            Ok(ev) => {
                c.ui.error = None;
                apply(c, s, ev);
                return;
            }
            Err(e) => {
                c.take_loadout = None;
                c.ui.error = Some(e);
            }
        }
    }
    if !back {
        c.ui.briefing = Some(b);
    }
}

/// The briefing map: the route, the target and what is known around them.
fn brief_map(
    ui: &mut egui::Ui,
    c: &ActiveCampaign,
    s: &Session,
    m: &stealth_mission::Mission,
    b: &mut Briefing,
    tex: Option<&egui::TextureHandle>,
    size: egui::Vec2,
) {
    let legend_w = 210.0;
    ui.horizontal_top(|ui| {
        let (rect, resp) = ui.allocate_exact_size(
            egui::vec2(size.x - legend_w, size.y),
            egui::Sense::click_and_drag(),
        );
        let route: Vec<egui::Vec2> = std::iter::once(m.takeoff.position)
            .chain(m.waypoints.iter().copied())
            .map(|p| egui::vec2(p.x as f32, p.y as f32))
            .collect();
        let (mut centre, mut scale) = *b.view.get_or_insert_with(|| {
            let lo = route.iter().fold(egui::vec2(f32::MAX, f32::MAX), |a, p| a.min(*p));
            let hi = route.iter().fold(egui::vec2(f32::MIN, f32::MIN), |a, p| a.max(*p));
            // Room around the target for its defences.
            MapView::fit(
                rect,
                lo - egui::vec2(0x300 as f32, 0x300 as f32),
                hi + egui::vec2(0x300 as f32, 0x300 as f32),
            )
        });
        let view = MapView { rect, centre, scale };
        view.pan_zoom(ui, &resp, &mut centre, &mut scale);
        b.view = Some((centre, scale));
        let painter = ui.painter_at(rect);
        map_draw::picture(&painter, &view, tex);
        let st = &c.state;
        let at = |x: f32, y: f32| view.to_screen(x, y);
        for p in &st.places {
            let col = match p.owner {
                Side::Blue => FRIENDLY_BLUE,
                Side::Red => MISSILE_RED,
                Side::Neutral => DIM,
            };
            let c0 = at(p.x as f32, p.y as f32);
            painter.circle_stroke(c0, 5.0, egui::Stroke::new(1.5, col));
            painter.text(
                c0 + egui::vec2(7.0, -7.0),
                egui::Align2::LEFT_BOTTOM,
                &p.name,
                egui::FontId::monospace(11.0),
                col,
            );
        }
        let pic = super::picture::picture(st, &s.combat.sim);
        super::picture::draw(&painter, &view, &pic, true);
        // The route: out to the target and home.
        let pts: Vec<egui::Pos2> = route.iter().map(|p| at(p.x, p.y)).collect();
        painter.add(egui::Shape::line(pts.clone(), egui::Stroke::new(2.0, GREEN)));
        for (k, p) in pts.iter().enumerate().skip(1) {
            painter.circle_filled(*p, 3.5, GREEN);
            painter.text(
                *p + egui::vec2(6.0, 6.0),
                egui::Align2::LEFT_TOP,
                format!("{k}"),
                egui::FontId::monospace(11.0),
                GREEN,
            );
        }
        let t = at(m.waypoints[1].x as f32, m.waypoints[1].y as f32);
        painter.circle_stroke(t, 12.0, egui::Stroke::new(2.0, AMBER));
        painter.circle_stroke(t, 4.0, egui::Stroke::new(2.0, AMBER));
        ui.vertical(|ui| {
            ui.set_width(legend_w - 12.0);
            section(ui, "Legend");
            map_draw::legend(ui, "Route, waypoints", |p, c0| {
                p.line_segment(
                    [c0 - egui::vec2(7.0, 0.0), c0 + egui::vec2(7.0, 0.0)],
                    egui::Stroke::new(2.0, GREEN),
                );
            });
            map_draw::legend(ui, "Target", |p, c0| {
                p.circle_stroke(c0, 6.0, egui::Stroke::new(2.0, AMBER));
            });
            map_draw::legend(ui, "SAM missile reach", |p, c0| {
                p.extend(map_draw::missile_circle(c0, 6.0, 1.0));
            });
            super::picture::legend(ui);
            ui.add_space(8.0);
            let known = pic.sams.len() + pic.guns.len();
            ui.label(egui::RichText::new(format!("{known} defences known")).color(BRIGHT));
            if o_intel(c, m) {
                ui.label(egui::RichText::new("Recon photos of the target").color(GREEN));
            } else {
                ui.label(
                    egui::RichText::new("No recon of the target: expect more").small().color(AMBER),
                );
            }
            ui.label(egui::RichText::new("Wheel: zoom, drag: pan").small().color(DIM));
        });
    });
}

/// The sortie's target place has recon intelligence.
fn o_intel(c: &ActiveCampaign, m: &stealth_mission::Mission) -> bool {
    let (x, y) = (m.waypoints[1].x, m.waypoints[1].y);
    c.state.places.iter().any(|p| p.intel(c.state.clock) && dist(p.x, p.y, x, y) < 0x200)
}
