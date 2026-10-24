//! The campaign's Ops screen over the flight: mission board, war map, debrief and log, and the
//! prompt after landing at a friendly base. On the desktop an egui overlay over the 3D view,
//! in the headset the menu panel in front of the head (`vr_menu.rs`).

use bevy::prelude::*;
use bevy_egui::{EguiContexts, PrimaryEguiContext, egui};
use stealth_campaign::weather::Now;
use stealth_campaign::{
    Aircraft as Airframe, InsertionState, MissionKind, MissionOffer, Objective, PlayerId,
    RaidState, Side, SortieResult, tuning,
};
use stealth_host::glue::sortie_mission;
use stealth_mission::Sky;
use stealth_net::protocol::PlayerStatus;

use super::{Action, ActiveCampaign, Phase, Role, describe, kind_of, preview};
use crate::map_draw::{self, FRIENDLY_BLUE, MISSILE_RED, MapView};
use crate::menu::{AMBER, BG, BRIGHT, DIM, GREEN, apply_terminal_style, big_button, section};
use crate::settings::AppState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Board,
    Map,
    Forces,
    Players,
    Debrief,
    Log,
    Chat,
}

/// The Ops screen's own state.
#[derive(Default)]
pub struct OpsUi {
    pub tab: Tab,
    selected: Option<u32>,
    /// Base chosen for the selected mission (None: the offer's).
    base: Option<u16>,
    pub error: Option<String>,
    centre: egui::Vec2,
    scale: f32,
    fitted: bool,
    /// `--campaign-take` was done.
    pub test_taken: bool,
    /// The accepted mission's briefing (map and loadout) before take-off.
    briefing: Option<Briefing>,
    /// SAVE & EXIT asks first.
    confirm_exit: bool,
    /// The chat message being typed on the CHAT tab.
    chat: String,
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
    /// The aircraft to fly it in.
    aircraft: Airframe,
    loadout: crate::loadout::PlannedLoadout,
    window: crate::loadout::LoadoutWindow,
    page: BriefPage,
    /// The route to fly (None until the editor first shows it). In a coop game the plan's,
    /// with this player's changes not sent yet.
    route: Option<crate::mission::PlannedRoute>,
    /// Changed here, not sent to the plan yet.
    dirty: bool,
    /// When the last change went (egui time), and until when the plan's route is not taken
    /// over (its echo is on the way).
    sent_at: f64,
    hold_until: f64,
}

impl Briefing {
    fn new(
        offer: u32,
        base: u16,
        aircraft: Airframe,
        loadout: crate::loadout::PlannedLoadout,
        page: BriefPage,
    ) -> Self {
        Self {
            offer,
            base,
            aircraft,
            loadout,
            window: default(),
            page,
            route: None,
            dirty: false,
            sent_at: 0.0,
            hold_until: 0.0,
        }
    }
}

/// The camera that carries the Ops screen's egui context.
#[derive(Component, Clone)]
pub struct OpsCamera;

/// Shows and hides the Ops screen's egui context with the screen.
#[allow(clippy::too_many_arguments)]
pub fn overlay(
    mut commands: Commands,
    c: Option<Res<ActiveCampaign>>,
    tagged: Query<Entity, With<OpsCamera>>,
    mut cams: Query<&mut Msaa, With<OpsCamera>>,
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
    // moved windows leaving trails). The 3D view's MSAA follows the MSAA option.
    let msaa = eye.iter().next().copied().unwrap_or(Msaa::Off);
    for mut m in &mut cams {
        if *m != msaa {
            *m = msaa;
        }
    }
    // Everything spawned with the screen carries the tag (in VR also the panel, the laser and
    // the desktop mirror), not only the cameras.
    if want == !tagged.is_empty() {
        return;
    }
    if !want {
        for e in &tagged {
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
            false,
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

/// Test aids: `--campaign-tab board|map|forces|players|debrief|log|chat` opens a tab, `--campaign-take <n>`
/// takes the n-th mission of the board (0: the first) once the campaign is up.
pub fn test_tab() -> Option<Tab> {
    let args: Vec<String> = std::env::args().collect();
    let t = args.iter().position(|a| a == "--campaign-tab").and_then(|i| args.get(i + 1))?;
    Some(match t.as_str() {
        "map" => Tab::Map,
        "forces" => Tab::Forces,
        "debrief" => Tab::Debrief,
        "log" => Tab::Log,
        "players" => Tab::Players,
        "chat" => Tab::Chat,
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
        MissionKind::Intercept | MissionKind::Defend | MissionKind::Csar | MissionKind::Awacs => {
            MISSILE_RED
        }
        _ => AMBER,
    }
}

/// The weather and the next change: a short form for the header and the briefing's line.
fn weather_text(c: &ActiveCampaign, now: &Now) -> (String, String) {
    let name = |s: Sky| match s {
        Sky::Storm => "THUNDERSTORMS".to_string(),
        s => s.name().to_ascii_uppercase(),
    };
    let setup = &c.state().setup;
    let h = setup.start_hour as f64 + now.next.1 * setup.sky_ratio as f64 / 3600.0;
    let mins = (h.rem_euclid(24.0) * 60.0).floor() as i64;
    let at = format!("{:02}:{:02}", mins / 60, mins % 60);
    let short = format!("WX {}, {} AT {at}", name(now.weather.sky), name(now.next.0));
    let line = format!("{} Changing to {} at {at}.", now.weather.describe(), now.next.0.name());
    (short, line.replace("Storm", "thunderstorms"))
}

/// The campaign clock as the sky shows it: day and time.
fn clock_text(c: &ActiveCampaign) -> String {
    let h = c.state().sky_hours();
    let day = (h / 24.0).floor() as i64 + 1;
    let hh = h.rem_euclid(24.0);
    let secs = (hh * 3600.0).floor() as i64;
    format!("DAY {day}  {:02}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
}

#[allow(clippy::too_many_arguments)]
pub fn ops_ui(
    mut contexts: EguiContexts,
    mut c: ResMut<ActiveCampaign>,
    combat: Res<crate::combat::Combat>,
    mut planner: ResMut<crate::planner::Planner>,
    loadout: Option<Res<crate::loadout::PlannedLoadout>>,
    mut next: ResMut<NextState<AppState>>,
    mut unit_view: ResMut<super::UnitView>,
    weather: Res<crate::weather::WeatherState>,
    mut notices: ResMut<crate::player_notices::PlayerNotices>,
    settings: Res<crate::settings::Settings>,
) -> Result {
    if !c.screen_open() || c.exit {
        return Ok(());
    }
    let wx = weather.now.map(|n| weather_text(&c, &n));
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
        let name = combat.sim.place_name(base);
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
                        if let Some(loadout) = loadout.as_deref().cloned() {
                            c.actions.push(Action::Rearm { loadout });
                        }
                    }
                    if ui.add(big_button("END SORTIE").fill(DIM)).clicked() {
                        c.actions.push(Action::EndSortie { outcome: 0 });
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
        && let Some(o) = c.state().board.iter().find(|o| o.kind.flyable_by(c.aircraft)).cloned()
    {
        let sortie = preview(&o, o.base, c.aircraft);
        let m = sortie_mission(&c.mission, c.state(), &sortie);
        let loadout = crate::loadout::PlannedLoadout::of(&m, kind_of(c.aircraft));
        c.ui.briefing = Some(Briefing::new(o.id, o.base, c.aircraft, loadout, page));
        if c.role != Role::Local {
            c.actions.push(Action::SelectMission { offer: Some(o.id), base: o.base });
        }
    }
    if c.ui.briefing.is_some() {
        let line = wx.as_ref().map(|w| w.1.as_str());
        briefing(ctx, c, &combat.sim, &mut planner, line, egui::vec2(width, height));
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
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    // The right side's clock, score and button keep their room.
                    ui.set_max_width((width - 380.0).max(240.0));
                    ui.horizontal_wrapped(|ui| {
                        ui.label(egui::RichText::new("DYNAMIC CAMPAIGN").heading().color(BRIGHT));
                        ui.label(egui::RichText::new(theater.to_ascii_uppercase()).color(DIM));
                        if let Some(server) = &c.server {
                            ui.label(
                                egui::RichText::new(format!("ON {}", server.to_ascii_uppercase()))
                                    .color(FRIENDLY_BLUE),
                            );
                        } else if c.role == Role::Host {
                            ui.label(egui::RichText::new("SERVING").color(FRIENDLY_BLUE));
                        }
                    });
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        for (tab, label) in [
                            (Tab::Board, "MISSIONS"),
                            (Tab::Map, "WAR MAP"),
                            (Tab::Forces, "FORCES"),
                            (Tab::Players, "PLAYERS"),
                            (Tab::Debrief, "DEBRIEF"),
                            (Tab::Log, "LOG"),
                            (Tab::Chat, "CHAT"),
                        ] {
                            if tab == Tab::Chat && c.role == Role::Local {
                                continue;
                            }
                            if ui.selectable_label(c.ui.tab == tab, label).clicked() {
                                c.ui.tab = tab;
                            }
                        }
                    });
                });
                // Top right: the clock and the score, the way out under them.
                ui.with_layout(egui::Layout::top_down(egui::Align::Max), |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(format!("BLUE {}", c.state().held(Side::Blue)))
                                .color(FRIENDLY_BLUE),
                        );
                        ui.label(
                            egui::RichText::new(format!("RED {}", c.state().held(Side::Red)))
                                .color(MISSILE_RED),
                        );
                        ui.label(egui::RichText::new(clock_text(c)).color(BRIGHT));
                        if let Some((short, _)) = &wx {
                            ui.label(egui::RichText::new(short).color(DIM));
                        }
                    });
                    if ui.add(exit_button(c.role)).clicked() {
                        c.ui.confirm_exit = true;
                    }
                });
            });
            ui.separator();
            ui.allocate_ui(egui::vec2(width, height), |ui| {
                ui.set_min_height(height);
                if c.state().outcome.is_some() {
                    end_screen(ui, c);
                    return;
                }
                match c.ui.tab {
                    Tab::Board => board(ui, c, width),
                    Tab::Map => war_map(
                        ui,
                        c,
                        &combat.sim,
                        &mut unit_view,
                        tex.as_ref(),
                        egui::vec2(width, height),
                    ),
                    Tab::Forces => forces(ui, c),
                    Tab::Players => players(ui, c),
                    Tab::Debrief => debrief(ui, c),
                    Tab::Log => log(ui, c),
                    Tab::Chat => chat(ui, c, &mut notices, settings.vr),
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                if let Some(e) = &c.ui.error {
                    ui.colored_label(AMBER, e);
                } else {
                    supply_line(ui, c);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if c.state().outcome.is_some() {
                        ui.label(egui::RichText::new("The war is over").color(DIM));
                    } else {
                        ui.add(egui::Spinner::new().color(GREEN));
                        ui.label(egui::RichText::new("War simulation running").color(DIM));
                    }
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
                let (title, text, button) = match c.role {
                    Role::Local => (
                        "SAVE AND LEAVE THE CAMPAIGN?",
                        "The war stops until you continue it. Aircraft in the air are lost; \
                         insertions under way start again from their base.",
                        "SAVE & EXIT",
                    ),
                    Role::Host => (
                        "SAVE AND STOP SERVING THE CAMPAIGN?",
                        "The players who joined are disconnected and the war stops until you \
                         continue it. Aircraft in the air are lost; insertions under way start \
                         again from their base.",
                        "SAVE & EXIT",
                    ),
                    Role::Client => (
                        "DISCONNECT FROM THE SERVER?",
                        "The campaign goes on on the server; nothing is saved here. Your sortie \
                         ends.",
                        "DISCONNECT",
                    ),
                };
                ui.label(egui::RichText::new(title).heading().color(BRIGHT));
                ui.label(egui::RichText::new(text).color(DIM));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.add(big_button(button).fill(EXIT_RED)).clicked() {
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
        c.save();
        c.exit = true;
        next.set(AppState::Menu);
    }
    Ok(())
}

/// The mission board: the offers, the selected one's details, the insertions under way.
fn board(ui: &mut egui::Ui, c: &mut ActiveCampaign, width: f32) {
    let from = |c: &ActiveCampaign, o: &MissionOffer| {
        c.state().site(o.base).map(|b| (b.x, b.y)).unwrap_or((o.x, o.y))
    };
    if c.ui.selected.is_none_or(|id| !c.state().board.iter().any(|o| o.id == id)) {
        c.ui.selected = c.state().board.first().map(|o| o.id);
        c.ui.base = None;
    }
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(width * 0.52);
            section(ui, "Missions");
            egui::ScrollArea::vertical().id_salt("offers").max_height(height_of(ui) * 0.62).show(
                ui,
                |ui| {
                    for o in c.state().board.clone() {
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
                        group_label(ui, c, o.id);
                        ui.add_space(2.0);
                    }
                    if c.state().board.is_empty() {
                        ui.label(egui::RichText::new("No missions: the front is quiet").color(DIM));
                    }
                },
            );
            ui.add_space(8.0);
            section(ui, "Insertions and raids");
            let mut any = false;
            let mins = |at: f64| ((at - c.state().clock) / 60.0).max(0.0).ceil();
            for ins in &c.state().insertions {
                let name = c.state().place(ins.place).map_or(String::new(), |p| p.name.clone());
                let enemy = ins.side != Side::Blue;
                let (text, col) = match ins.state {
                    InsertionState::Pending { launch_at } if enemy => (
                        format!("ENEMY at {name}: lift-off in {:.0} min", mins(launch_at)),
                        MISSILE_RED,
                    ),
                    InsertionState::Pending { launch_at } => {
                        (format!("{name}: lift-off in {:.0} min", mins(launch_at)), BRIGHT)
                    }
                    InsertionState::Enroute => {
                        let left = ins.alive.iter().filter(|&&a| a).count();
                        if enemy {
                            (format!("ENEMY at {name}: {left} helicopters inbound"), MISSILE_RED)
                        } else {
                            (format!("{name}: {left} helicopters on the way"), FRIENDLY_BLUE)
                        }
                    }
                    _ => continue,
                };
                any = true;
                ui.label(egui::RichText::new(text).color(col));
            }
            for r in &c.state().raids {
                let name = c.state().place(r.place).map_or(String::new(), |p| p.name.clone());
                let (what, col) = match (r.side, r.drones) {
                    (Side::Blue, _) => (format!("AI SEAD at {name}"), FRIENDLY_BLUE),
                    (_, true) => (format!("DRONES at {name}"), MISSILE_RED),
                    _ => (format!("RAID on {name} airfield"), MISSILE_RED),
                };
                let text = match r.state {
                    RaidState::Pending { launch_at } => {
                        format!("{what}: launch in {:.0} min", mins(launch_at))
                    }
                    RaidState::Enroute => {
                        let left = r.alive.iter().filter(|&&a| a).count();
                        format!("{what}: {left} on the way")
                    }
                    _ => continue,
                };
                any = true;
                ui.label(egui::RichText::new(text).color(col));
            }
            if let Some(a) = &c.state().awacs {
                any = true;
                let text = format!("ENEMY AWACS up for {:.0} min more", mins(a.until));
                ui.label(egui::RichText::new(text).color(MISSILE_RED));
            }
            for d in &c.state().downed {
                let text = match d.state {
                    stealth_campaign::DownedState::Waiting => {
                        format!("DOWN: {} - rescue within {:.0} min", d.name, mins(d.until))
                    }
                    stealth_campaign::DownedState::Aboard => {
                        format!("ABOARD: {} - on the way home", d.name)
                    }
                    _ => continue,
                };
                any = true;
                ui.label(egui::RichText::new(text).color(MISSILE_RED));
            }
            if !any {
                ui.label(egui::RichText::new("None under way").color(DIM));
            }
        });
        ui.separator();
        ui.vertical(|ui| {
            let Some(o) =
                c.ui.selected.and_then(|id| c.state().board.iter().find(|o| o.id == id)).cloned()
            else {
                return;
            };
            section(ui, o.kind.name());
            ui.label(egui::RichText::new(&o.title).size(20.0).color(BRIGHT));
            ui.label(egui::RichText::new(describe_offer(c, &o)).color(DIM));
            if let Objective::Intercept { insertion } | Objective::Escort { insertion } =
                o.objective
                && let Some(t) = c.state().insertion_status(insertion)
            {
                ui.label(egui::RichText::new(t).color(BRIGHT));
            }
            group_label(ui, c, o.id);
            ui.add_space(6.0);
            if let Some(p) = place_of(&o.objective).and_then(|p| c.state().place(p)) {
                let intel = p.intel(c.state().clock);
                ui.label(format!(
                    "Defences: {} SAM sites, threat {}{}",
                    p.defences
                        .iter()
                        .filter(|&&d| c.state().site(d).is_some_and(|s| s.alive()))
                        .count(),
                    o.threat,
                    if intel { "  -  RECON INTEL" } else { "" }
                ));
            }
            package_info(ui, c, &o);
            if o.kind.ai_flown() {
                ui.add_space(10.0);
                if ui.add(big_button("ORDER INSERTION")).clicked()
                    && let Objective::Insertion { place } = o.objective
                {
                    c.ui.error = None;
                    c.actions.push(Action::OrderInsertion { place });
                }
                ui.label(
                    egui::RichText::new("Fly the escort to cover them, or lead them in the UH-60")
                        .small()
                        .color(DIM),
                );
                ui.add_space(6.0);
            }
            // The aircraft that flies it: the one last flown if it can, else the first that can.
            let aircraft = flyable(&o, c.aircraft);
            // From which base.
            let mut bases: Vec<(u16, String, i32)> = c
                .state()
                .player_bases()
                .into_iter()
                .filter_map(|b| {
                    let site = c.state().site(b)?;
                    Some((b, site.name.clone(), dist(site.x, site.y, o.x, o.y)))
                })
                .collect();
            bases.sort_by_key(|b| b.2);
            // Another player's plan sets the base (a plan nobody is on: its base unless the
            // player picks another).
            let plan = c.state().plan(o.id).filter(|_| c.role != Role::Local);
            let taken = plan.is_some_and(|p| !p.crew.is_empty() || !p.flying.is_empty());
            let planned = plan.map(|p| p.base).filter(|b| bases.iter().any(|x| x.0 == *b));
            let chosen = match planned {
                Some(b) if taken => b,
                _ => {
                    c.ui.base
                        .filter(|b| bases.iter().any(|x| x.0 == *b))
                        .or(planned)
                        .unwrap_or(o.base)
                }
            };
            let label = bases
                .iter()
                .find(|b| b.0 == chosen)
                .map_or(String::new(), |b| format!("{} ({:.0} km)", b.1, km(b.2)));
            ui.horizontal(|ui| {
                ui.label("From");
                ui.add_enabled_ui(!taken, |ui| {
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
                if taken {
                    ui.label(egui::RichText::new("Planned by the group").small().color(DIM));
                }
            });
            let sortie = preview(&o, chosen, aircraft);
            let m = sortie_mission(&c.mission, c.state(), &sortie);
            let loadout = crate::loadout::PlannedLoadout::of(&m, kind_of(aircraft));
            ui.label(egui::RichText::new(format!("Stores: {}", loadout.summary())).color(DIM));
            support_button(ui, c, &o);
            ui.add_space(10.0);
            let accept = if o.kind.ai_flown() { "FLY THE UH-60" } else { "ACCEPT MISSION" };
            if ui.add(big_button(accept).fill(DIM)).clicked() {
                c.ui.error = None;
                if c.role != Role::Local {
                    c.actions.push(Action::SelectMission { offer: Some(o.id), base: chosen });
                }
                c.ui.briefing =
                    Some(Briefing::new(o.id, chosen, aircraft, loadout, BriefPage::Map));
            }
        });
    });
}

/// The players on an offer's plan (coop): planning it, flying it.
fn group(c: &ActiveCampaign, offer: u32) -> (Vec<String>, Vec<String>) {
    let st = c.state();
    let names = |ids: &[PlayerId]| -> Vec<String> {
        ids.iter().map(|&p| st.player_name(p).unwrap_or("?").to_string()).collect()
    };
    match st.plan(offer) {
        Some(p) if c.role != Role::Local => (names(&p.crew), names(&p.flying)),
        _ => default(),
    }
}

/// Who is planning and flying an offer, under it on the board.
fn group_label(ui: &mut egui::Ui, c: &ActiveCampaign, offer: u32) {
    let (planning, flying) = group(c, offer);
    if planning.is_empty() && flying.is_empty() {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        if !planning.is_empty() {
            let text = format!("PLANNING: {}", planning.join(", "));
            ui.label(egui::RichText::new(text).small().color(FRIENDLY_BLUE));
        }
        if !flying.is_empty() {
            let text = format!("FLYING: {}", flying.join(", "));
            ui.label(egui::RichText::new(text).small().color(GREEN));
        }
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
        | Objective::Interdict { place, .. }
        | Objective::Insertion { place } => Some(place),
        _ => None,
    }
}

/// What an offer asks for (a drone wave is no air raid).
fn describe_offer(c: &ActiveCampaign, o: &MissionOffer) -> &'static str {
    match o.objective {
        Objective::Defend { raid } if c.state().raid(raid).is_some_and(|r| r.drones) => {
            "Shoot down the attack drones before they dive into the target: one getting through destroys it"
        }
        Objective::Battery { .. } => stealth_host::glue::BATTERY_TASK,
        _ => describe(o.kind),
    }
}

/// The aircraft an offer is flown in: `last` if it may fly it, else the first that may.
pub fn flyable(o: &MissionOffer, last: Airframe) -> Airframe {
    [last, Airframe::F117, Airframe::F16, Airframe::UH60, Airframe::AH1Z]
        .into_iter()
        .find(|a| o.kind.flyable_by(*a))
        .unwrap_or(last)
}

/// The other offers at an offer's place: flown together, they are a package.
fn package_info(ui: &mut egui::Ui, c: &ActiveCampaign, o: &MissionOffer) {
    use stealth_campaign::tuning::{PACKAGE_BONUS, PACKAGE_WINDOW};
    let st = c.state();
    let Some(place) = st.objective_place(&o.objective) else { return };
    let mut kinds: Vec<&str> = st
        .board
        .iter()
        .filter(|x| x.id != o.id && x.kind != MissionKind::Insertion && x.kind != o.kind)
        .filter(|x| st.objective_place(&x.objective) == Some(place))
        .map(|x| x.kind.name())
        .collect();
    kinds.dedup();
    if kinds.is_empty() {
        return;
    }
    ui.label(
        egui::RichText::new(format!(
            "PACKAGE with {}: achieve two kinds here within {:.0} min, +{PACKAGE_BONUS} points each",
            kinds.join(", "),
            PACKAGE_WINDOW / 60.0
        ))
        .small()
        .color(GREEN),
    );
}

/// Calling in AI SEAD for an offer's place (the friendly side's materiel and fighters).
fn support_button(ui: &mut egui::Ui, c: &mut ActiveCampaign, o: &MissionOffer) {
    use stealth_campaign::tuning::{RAID_AIRCRAFT, SUPPORT_COST};
    let st = c.state();
    let Some(place) = st.objective_place(&o.objective) else { return };
    let Some(p) = st.place(place).filter(|p| p.owner == Side::Red) else { return };
    let sams = p.defences.iter().filter(|&&d| st.site(d).is_some_and(|s| s.alive())).count();
    if sams == 0 {
        return;
    }
    let busy = st.raids.iter().any(|r| r.side == Side::Blue && r.open() && r.place == place);
    if busy {
        ui.label(egui::RichText::new("AI SEAD is on its way").small().color(FRIENDLY_BLUE));
        return;
    }
    let can = st.blue.materiel >= SUPPORT_COST && st.blue.fighters as usize >= RAID_AIRCRAFT;
    let label = format!("REQUEST AI SEAD ({SUPPORT_COST:.0} materiel, {RAID_AIRCRAFT} aircraft)");
    let offer = o.id;
    if ui
        .add_enabled(can, egui::Button::new(egui::RichText::new(label).color(FRIENDLY_BLUE)))
        .on_hover_text("Friendly strike aircraft go for a SAM site of this place ahead of you")
        .on_disabled_hover_text("Not enough materiel or aircraft on the ground (see FORCES)")
        .clicked()
    {
        c.ui.error = None;
        c.actions.push(Action::RequestSupport { offer });
    }
}

/// A rank for campaign points (the career's ranks).
fn rank_of(points: i32) -> &'static str {
    let k = crate::career::RANKS.iter().rposition(|r| points >= r.2).unwrap_or(0);
    crate::career::RANKS[k].1
}

/// The medal a sortie's points earn (the career's), if any.
fn medal_of(points: i32) -> Option<&'static str> {
    crate::career::MEDALS.iter().rev().find(|m| points >= m.1).map(|m| m.0)
}

/// The campaign's rules in words.
fn rules_text(r: &stealth_host::Rules) -> String {
    let on: Vec<&str> = [
        (r.realistic_damage, "realistic damage"),
        (r.realistic_guidance, "realistic guidance"),
        (r.no_crashes, "no crashes"),
        (r.continuous_time, "continuous time"),
        (r.realistic_weapons, "realistic F-117A weapons"),
        (r.friendly_fire, "friendly fire"),
    ]
    .iter()
    .filter(|x| x.0)
    .map(|x| x.1)
    .collect();
    if on.is_empty() { "classic".into() } else { on.join(", ") }
}

/// Both sides' materiel and income at a glance (the footer).
fn supply_line(ui: &mut egui::Ui, c: &ActiveCampaign) {
    let st = c.state();
    ui.label(egui::RichText::new("MATERIEL").color(DIM));
    for (side, name, col) in [(Side::Blue, "BLUE", FRIENDLY_BLUE), (Side::Red, "RED", MISSILE_RED)]
    {
        ui.label(
            egui::RichText::new(format!(
                "{name} {:.0} (+{:.0}/h)",
                st.forces(side).materiel.floor(),
                st.income(side)
            ))
            .color(col),
        );
    }
}

/// The two sides' means of war side by side: materiel, income and where it comes from, the
/// AI's aircraft, where the materiel went, and what everything costs.
fn forces(ui: &mut egui::Ui, c: &ActiveCampaign) {
    use stealth_campaign::tuning::*;
    let st = c.state();
    let sides = [(Side::Blue, "FRIENDLY", FRIENDLY_BLUE), (Side::Red, "ENEMY", MISSILE_RED)];
    egui::ScrollArea::vertical().id_salt("forces").show(ui, |ui| {
        section(ui, "Forces");
        egui::Grid::new("forces").num_columns(3).spacing(egui::vec2(40.0, 6.0)).striped(true).show(
            ui,
            |ui| {
                let row = |ui: &mut egui::Ui, label: &str, f: &dyn Fn(Side) -> String| {
                    ui.label(egui::RichText::new(label).color(DIM));
                    for (side, _, col) in sides {
                        ui.label(egui::RichText::new(f(side)).monospace().color(col));
                    }
                    ui.end_row();
                };
                ui.label("");
                for (_, name, col) in sides {
                    ui.label(egui::RichText::new(name).strong().color(col));
                }
                ui.end_row();
                row(ui, "Places held", &|s| st.held(s).to_string());
                row(ui, "Materiel", &|s| {
                    format!("{:.0} / {MATERIEL_MAX:.0}", st.forces(s).materiel.floor())
                });
                row(ui, "Income per hour", &|s| format!("+{:.0}", st.income(s)));
                row(ui, "  from", &|s| {
                    let (places, cities) = st.producers(s);
                    format!(
                        "{INCOME_BASE:.0} + {places} places x {INCOME_PLACE:.0} + {cities} cities x {INCOME_CITY:.0}"
                    )
                });
                row(ui, "Fighters on the ground", &|s| {
                    format!("{} / {}", st.forces(s).fighters, st.aircraft_limits(s).0)
                });
                row(ui, "Helicopters on the ground", &|s| {
                    format!("{} / {}", st.forces(s).helicopters, st.aircraft_limits(s).1)
                });
                row(ui, "In the air", &|s| {
                    let (f, strike, h) = st.airborne(s);
                    format!("{f} fighters, {strike} strike, {h} helicopters")
                });
                ui.label(egui::RichText::new("Since the start").color(AMBER));
                ui.end_row();
                let l = |s: Side| st.forces(s).ledger.clone();
                let out = |v: f64| if v < 0.5 { "0".to_string() } else { format!("-{v:.0}") };
                row(ui, "  earned", &|s| format!("+{:.0}", l(s).earned));
                row(ui, "  repairs", &|s| out(l(s).repairs));
                row(ui, "  sorties, raids, insertions", &|s| out(l(s).operations));
                row(ui, "  new aircraft", &|s| out(l(s).aircraft));
                row(ui, "  lost (cities, pilots)", &|s| out(l(s).lost));
            },
        );
        ui.add_space(10.0);
        section(ui, "How it works");
        for line in [
            "Each side earns materiel every hour: a base amount, more for every place it holds and for \
             every intact city among them. Bomb the enemy's cities (INTERDICTION) to cut its income.",
            "The AI pays for everything it does: repairs, every fighter sortie, raids and its own \
             insertions. Short of materiel, its damage stays unrepaired and its aircraft stay home.",
            "The AI flies only the aircraft it has; shot down they are gone, landed they fly again. \
             Every hour each side buys new ones while its materiel lasts.",
            "You never run short: your aircraft, your bases (bombed or not) and the helicopters of \
             your own insertion orders are always there.",
        ] {
            ui.label(egui::RichText::new(line).color(BRIGHT));
            ui.add_space(2.0);
        }
        ui.add_space(8.0);
        section(ui, "Prices");
        egui::Grid::new("prices").num_columns(4).spacing(egui::vec2(28.0, 4.0)).show(ui, |ui| {
            let prices = [
                ("Airfield repair", REPAIR_COST_AIRFIELD),
                ("SAM site repair", REPAIR_COST_SAM),
                ("City rebuilt", REPAIR_COST_CITY),
                ("Ship repair", REPAIR_COST_SHIP),
                ("Gun or MANPADS team", GUN_COST),
                ("Fighter sortie", SORTIE_COST),
                ("Raid", RAID_COST),
                ("AI insertion", INSERTION_COST),
                ("New fighter", FIGHTER_COST),
                ("New helicopter", HELICOPTER_COST),
                ("City destroyed (lost)", CITY_LOSS),
                ("Pilot captured (lost)", CAPTURE_COST),
            ];
            for pair in prices.chunks(2) {
                for (name, cost) in pair {
                    ui.label(egui::RichText::new(*name).color(DIM));
                    ui.label(egui::RichText::new(format!("{cost:.0}")).monospace().color(BRIGHT));
                }
                ui.end_row();
            }
        });
    });
}

/// The war map: who holds which place, the insertions, the aircraft in the air.
fn war_map(
    ui: &mut egui::Ui,
    c: &mut ActiveCampaign,
    sim: &stealth_sim::Sim,
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
            let (lo, hi) = c.state().places.iter().fold(
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
        let (parked, selected) = (c.parked, c.ui.selected);
        let test_taken = &mut c.ui.test_taken;
        let st = c.host.as_ref().map(|h| &h.state).or(c.copy.as_ref()).expect("campaign");
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
        let pic = super::picture::picture(st, &sim);
        super::picture::draw(&painter, &view, &pic, false);
        // Test aid: `--campaign-watch` watches the first friendly aircraft.
        if std::env::args().any(|a| a == "--campaign-watch")
            && unit_view.target.is_none()
            && let Some(a) = pic.friendly.first()
            && !std::mem::replace(test_taken, true)
        {
            let unit = super::unit_view::UnitRef::Flight { index: a.unit, spec: a.spec };
            unit_view.watch(unit, sim.aircraft_name(a.spec));
        }
        // A unit under the pointer: its name; a click watches it live.
        if let Some(pos) = resp.hover_pos()
            && let Some((unit, name)) = super::picture::pick(&view, &pic, &sim, pos)
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
            && let Some(b) = parked.and_then(|b| st.site(b as u16))
        {
            painter.circle_stroke(at(b.x, b.y), 8.0, egui::Stroke::new(1.5, GREEN));
        }
        // The selected mission's target.
        if let Some(o) = selected.and_then(|id| st.board.iter().find(|o| o.id == id)) {
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
        SortieResult::Pending if r.raid.is_some() => ("WAITING FOR THE RAID", FRIENDLY_BLUE),
        SortieResult::Pending => ("WAITING FOR THE INSERTION", FRIENDLY_BLUE),
    };
    // An escort settled after the sortie shows its final result.
    let settled = c.state().history.iter().find(|h| h.id == r.id).map_or(r.result, |h| h.result);
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
    let points = c.state().history.iter().find(|h| h.id == r.id).map_or(r.points, |h| h.points);
    let bonus = c.state().history.iter().find(|h| h.id == r.id).map_or(r.bonus, |h| h.bonus);
    ui.label(
        egui::RichText::new(format!(
            "{points} points{}",
            if bonus > 0 { format!(" (package bonus +{bonus})") } else { String::new() }
        ))
        .color(BRIGHT),
    );
    if let Some(m) = medal_of(points) {
        ui.label(egui::RichText::new(format!("Awarded the {m}")).color(AMBER));
    }
    let me = c.state().pilot_record(c.me);
    ui.label(
        egui::RichText::new(format!("{} - {} campaign points", rank_of(me.points), me.points))
            .color(DIM),
    );
    let done = c.state().history.iter().filter(|h| h.player == c.me).count();
    let won = c
        .state()
        .history
        .iter()
        .filter(|h| h.player == c.me && h.result == SortieResult::Success)
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

/// The war is won or lost: the result, every pilot's record, and (on a server) the time left
/// before the next campaign starts.
fn end_screen(ui: &mut egui::Ui, c: &ActiveCampaign) {
    let st = c.state();
    let Some(o) = st.outcome else { return };
    let won = o.winner == Side::Blue;
    ui.vertical_centered(|ui| {
        ui.add_space(16.0);
        let (title, col) = if won { ("VICTORY", GREEN) } else { ("DEFEAT", MISSILE_RED) };
        ui.label(egui::RichText::new(title).size(44.0).strong().color(col));
        ui.label(
            egui::RichText::new(if won {
                "The enemy holds no airfield any more: the theater is ours."
            } else {
                "No friendly airfield is left: the theater is lost."
            })
            .size(18.0)
            .color(BRIGHT),
        );
        let day = (st.sky_hours() / 24.0).floor() as i64 + 1;
        ui.label(
            egui::RichText::new(format!(
                "Day {day}. Friendly places {}, enemy places {}.",
                st.held(Side::Blue),
                st.held(Side::Red)
            ))
            .color(DIM),
        );
        ui.add_space(10.0);
        if c.role == Role::Client {
            let left = (tuning::END_SCREEN_SECONDS - st.ended_for().unwrap_or(0.0)).max(0.0);
            let text = if left > 0.0 {
                let s = left.ceil() as i64;
                format!("THE NEXT CAMPAIGN STARTS IN {}:{:02}", s / 60, s % 60)
            } else {
                "THE NEXT CAMPAIGN IS STARTING".to_string()
            };
            ui.label(egui::RichText::new(text).size(20.0).color(AMBER));
        } else {
            ui.label(egui::RichText::new("Start the next campaign from the main menu.").color(DIM));
        }
        ui.add_space(12.0);
    });
    section(ui, "Pilots");
    egui::ScrollArea::vertical().id_salt("pilots").show(ui, |ui| leaderboard(ui, c, "end"));
}

fn log(ui: &mut egui::Ui, c: &ActiveCampaign) {
    section(ui, "Campaign log");
    egui::ScrollArea::vertical().id_salt("log").show(ui, |ui| {
        for e in c.state().log.iter().rev() {
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

/// A coop game's chat: every message so far, newest at the bottom, and a field to write one.
fn chat(
    ui: &mut egui::Ui,
    c: &mut ActiveCampaign,
    notices: &mut crate::player_notices::PlayerNotices,
    vr: bool,
) {
    use crate::text_entry::{Charset, text_entry};
    use stealth_net::protocol::MAX_CHAT;
    section(ui, "Chat");
    // The field's room under the list (in the headset with its keyboard).
    let below = if vr { 330.0 } else { 60.0 };
    let height = (ui.available_height() - below).max(120.0);
    egui::ScrollArea::vertical()
        .id_salt("chat")
        .max_height(height)
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show(ui, |ui| {
            if notices.log().is_empty() {
                ui.label(egui::RichText::new("No messages yet").color(DIM));
            }
            for m in notices.log() {
                ui.horizontal_wrapped(|ui| match &m.from {
                    Some(from) => {
                        let ink = if m.own { GREEN } else { AMBER };
                        ui.label(egui::RichText::new(format!("{from}:")).color(ink));
                        ui.label(egui::RichText::new(&m.text).color(BRIGHT));
                    }
                    None => {
                        ui.label(egui::RichText::new(&m.text).color(FRIENDLY_BLUE));
                    }
                });
            }
        });
    ui.add_space(8.0);
    let mut send = false;
    let mut field = |ui: &mut egui::Ui| {
        let resp = if vr {
            text_entry(ui, "ops_chat", &mut c.ui.chat, MAX_CHAT, Charset::Text, true, "Message")
        } else {
            let field = egui::TextEdit::singleline(&mut c.ui.chat)
                .id(egui::Id::new("ops_chat"))
                .char_limit(MAX_CHAT)
                .hint_text(egui::RichText::new("Type a message, Enter sends").color(DIM))
                .text_color(BRIGHT)
                .font(egui::FontId::monospace(18.0))
                .desired_width(ui.available_width() - 110.0);
            ui.add(field)
        };
        notices.field_focus |= resp.has_focus();
        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            send = true;
            resp.request_focus();
        }
        if ui.add(big_button("SEND")).clicked() {
            send = true;
        }
    };
    // In the headset the keyboard opens under the field.
    if vr {
        field(ui);
    } else {
        ui.horizontal(field);
    }
    if send && !stealth_net::protocol::chat_text(&c.ui.chat).is_empty() {
        notices.outbox.push(std::mem::take(&mut c.ui.chat));
    }
}

/// The accepted mission's briefing: the route and what is known of the defences on a map, the
/// stores to carry; then take off (or back to the board).
fn briefing(
    ctx: &egui::Context,
    c: &mut ActiveCampaign,
    sim: &stealth_sim::Sim,
    planner: &mut crate::planner::Planner,
    weather: Option<&str>,
    size: egui::Vec2,
) {
    let Some(mut b) = c.ui.briefing.take() else { return };
    let Some(o) = c.state().board.iter().find(|o| o.id == b.offer).cloned() else {
        c.ui.error = Some("That mission is no longer on the board".into());
        return;
    };
    // A coop game: the group's plan sets the base.
    let coop = c.role != Role::Local;
    if coop && let Some(p) = c.state().plan(b.offer) {
        b.base = p.base;
    }
    let sortie = preview(&o, b.base, b.aircraft);
    let m = sortie_mission(&c.mission, c.state(), &sortie);
    let mut back = false;
    let mut take_off = false;
    let mut edit = None;
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
            ui.label(egui::RichText::new(describe_offer(c, &o)).color(DIM));
            let d = km(dist(m.takeoff.position.x, m.takeoff.position.y, o.x, o.y));
            ui.label(format!(
                "From {} - {d:.0} km to the target - {} - stores: {}",
                m.takeoff.name,
                kind_of(b.aircraft).name(),
                b.loadout.summary()
            ));
            if let Some(line) = weather {
                ui.label(egui::RichText::new(line).color(DIM));
            }
            if coop {
                group_label(ui, c, o.id);
                let (_, flying) = group(c, o.id);
                if !flying.is_empty() {
                    let text = format!("Route locked: {} flying it", flying.join(", "));
                    ui.label(egui::RichText::new(text).color(AMBER));
                }
            }
            ui.horizontal(|ui| {
                for (page, label) in
                    [(BriefPage::Map, "ROUTE & THREATS"), (BriefPage::Loadout, "LOADOUT")]
                {
                    if ui.selectable_label(b.page == page, label).clicked() {
                        b.page = page;
                    }
                }
            });
            ui.separator();
            // Room for the heading lines above and the buttons below.
            let height = size.y - 150.0;
            ui.allocate_ui(egui::vec2(size.x, height), |ui| {
                ui.set_min_height(height);
                match b.page {
                    BriefPage::Map => {
                        edit = route_editor(
                            ui,
                            c,
                            sim,
                            planner,
                            &m,
                            &mut b,
                            egui::vec2(size.x, height),
                        )
                    }
                    BriefPage::Loadout => {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("AIRCRAFT").color(DIM));
                            let flyable =
                                [Airframe::F117, Airframe::F16, Airframe::UH60, Airframe::AH1Z]
                                    .into_iter()
                                    .filter(|a| o.kind.flyable_by(*a));
                            for a in flyable {
                                let label = kind_of(a).name();
                                if ui.selectable_label(b.aircraft == a, label).clicked()
                                    && b.aircraft != a
                                {
                                    b.aircraft = a;
                                    b.loadout = crate::loadout::PlannedLoadout::of(&m, kind_of(a));
                                }
                            }
                        });
                        ui.add_space(4.0);
                        crate::loadout::editor(
                            ui,
                            &mut b.window,
                            &mut b.loadout,
                            &m,
                            height - 30.0,
                        );
                        ui.horizontal(|ui| {
                            if ui.add(big_button("BRIEFED")).clicked() {
                                b.loadout =
                                    crate::loadout::PlannedLoadout::of(&m, kind_of(b.aircraft));
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
    // This player's changes to the group's route.
    if let Some(route) = edit {
        c.actions.push(Action::EditPlan { offer: o.id, base: b.base, route });
    }
    if take_off {
        if coop
            && std::mem::take(&mut b.dirty)
            && let Some(r) = &b.route
        {
            let route = r.points.clone();
            c.actions.push(Action::EditPlan { offer: o.id, base: b.base, route });
        }
        c.ui.error = None;
        c.aircraft = b.aircraft;
        c.actions.push(Action::TakeMission {
            offer: o.id,
            base: Some(b.base),
            aircraft: b.aircraft,
            loadout: b.loadout.clone(),
            route: b.route.clone(),
        });
        return;
    }
    if !back {
        c.ui.briefing = Some(b);
    } else if coop {
        c.actions.push(Action::SelectMission { offer: None, base: b.base });
    }
}

/// Seconds between a dragged point's updates to the group.
const SEND_EVERY: f64 = 0.1;
/// Seconds the group's route is not taken over after a change of this player's went (until
/// its echo is back).
const HOLD: f64 = 1.0;

/// The route editor (the mission planner's): drag, add and remove waypoints over the radars
/// friendly aircraft have detected (and that still stand), with their detection and missile
/// ranges and the route's exposure; the campaign's places and air picture over the map. In a
/// coop game the route is the group's (locked while one of them flies it): what to send it.
#[allow(clippy::too_many_arguments)]
fn route_editor(
    ui: &mut egui::Ui,
    c: &ActiveCampaign,
    sim: &stealth_sim::Sim,
    planner: &mut crate::planner::Planner,
    m: &stealth_mission::Mission,
    b: &mut Briefing,
    size: egui::Vec2,
) -> Option<Vec<crate::mission::RoutePoint>> {
    let now = ui.input(|i| i.time);
    let plan = c.state().plan(b.offer).filter(|_| c.role != Role::Local);
    let locked = plan.is_some_and(|p| !p.flying.is_empty());
    let route = b.route.get_or_insert_with(|| {
        planner.refit();
        crate::mission::PlannedRoute::of(m)
    });
    // The group's route (the others' changes), unless this player's own are on the way.
    if let Some(p) = plan
        && (locked || (!planner.dragging() && !b.dirty && now >= b.hold_until))
    {
        b.dirty = false;
        if p.route.is_empty() {
            route.points = crate::mission::PlannedRoute::of(m).points;
        } else if route.points != p.route {
            route.points = p.route.clone();
        }
    }
    let before = route.points.clone();
    let st = c.state();
    let known = |i: usize| st.site(i as u16).is_some_and(|x| x.spotted && x.alive());
    // The SAM sites are the editor's own rings.
    let mut pic = super::picture::picture(st, &sim);
    pic.sams.clear();
    let overlay = |painter: &egui::Painter, view: &MapView| {
        for p in &st.places {
            let col = match p.owner {
                Side::Blue => FRIENDLY_BLUE,
                Side::Red => MISSILE_RED,
                Side::Neutral => DIM,
            };
            let c0 = view.to_screen(p.x as f32, p.y as f32);
            if p.airfield.is_some() {
                let r = egui::Rect::from_center_size(c0, egui::vec2(9.0, 9.0));
                painter.rect_stroke(r, 0.0, egui::Stroke::new(1.5, col), egui::StrokeKind::Middle);
            } else {
                painter.circle_stroke(c0, 4.5, egui::Stroke::new(1.5, col));
            }
            painter.text(
                c0 + egui::vec2(7.0, 7.0),
                egui::Align2::LEFT_TOP,
                &p.name,
                egui::FontId::monospace(11.0),
                col,
            );
        }
        super::picture::draw(painter, view, &pic, false);
    };
    crate::planner::route_panel(
        ui,
        planner,
        m,
        route,
        &known,
        Some(&overlay),
        size,
        !locked,
        |ui| {
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new("Radars shown: detected by friendly aircraft, still standing")
                    .small()
                    .color(AMBER),
            );
        },
    );
    plan?;
    b.dirty |= route.points != before;
    // A dragged point goes a few times a second, everything else at once.
    if b.dirty && (!planner.dragging() || now - b.sent_at >= SEND_EVERY) {
        b.dirty = false;
        b.sent_at = now;
        b.hold_until = now + HOLD;
        return Some(route.points.clone());
    }
    None
}

/// The red way out: SAVE & EXIT, or DISCONNECT from a server.
pub(crate) const EXIT_RED: egui::Color32 = egui::Color32::from_rgb(150, 24, 20);

fn exit_button(role: Role) -> egui::Button<'static> {
    let label = if role == Role::Client { "DISCONNECT" } else { "SAVE & EXIT" };
    egui::Button::new(egui::RichText::new(label).strong().color(egui::Color32::WHITE))
        .fill(EXIT_RED)
        .stroke(egui::Stroke::new(1.0, MISSILE_RED))
        .min_size(egui::vec2(150.0, 30.0))
}

/// The players in the campaign: who flies what, who waits on the board.
fn players(ui: &mut egui::Ui, c: &ActiveCampaign) {
    section(ui, "Players");
    let list = c.players();
    // Up to 64 of them: the list scrolls; the leaderboard under it.
    let height = (ui.available_height() * 0.45).max(120.0);
    egui::ScrollArea::vertical().max_height(height).auto_shrink([false, true]).show(ui, |ui| {
        egui::Grid::new("players")
            .num_columns(6)
            .spacing(egui::vec2(24.0, 8.0))
            .striped(true)
            .show(ui, |ui| {
                for h in ["#", "NAME", "STATUS", "SORTIES", "POINTS", "PING"] {
                    ui.label(egui::RichText::new(h).color(DIM));
                }
                ui.end_row();
                for p in &list {
                    ui.label(egui::RichText::new(format!("{}", p.id.0 + 1)).monospace().color(DIM));
                    let mut name = egui::RichText::new(&p.name).color(BRIGHT);
                    if p.id == c.me {
                        name = name.strong();
                    }
                    ui.horizontal(|ui| {
                        ui.label(name);
                        if p.id == c.me {
                            ui.label(egui::RichText::new("(you)").small().color(DIM));
                        }
                        if p.host {
                            ui.label(egui::RichText::new("HOST").small().color(FRIENDLY_BLUE));
                        }
                    });
                    let (status, col) = match &p.status {
                        PlayerStatus::Ops => {
                            let planning = c
                                .state()
                                .plans
                                .iter()
                                .find(|x| x.crew.contains(&p.id))
                                .and_then(|x| c.state().board.iter().find(|o| o.id == x.offer));
                            match planning {
                                Some(o) => (format!("Planning {}", o.title), FRIENDLY_BLUE),
                                None => ("On the mission board".to_string(), DIM),
                            }
                        }
                        PlayerStatus::Flying { title, aircraft } => {
                            (format!("Flying {title} - {}", kind_of(*aircraft).name()), GREEN)
                        }
                        PlayerStatus::Reconnecting { seconds } => {
                            (format!("Connection lost - aircraft held {seconds} s"), AMBER)
                        }
                    };
                    ui.label(egui::RichText::new(status).color(col));
                    let flown = c.state().records(p.id).count();
                    let won = c
                        .state()
                        .records(p.id)
                        .filter(|r| r.result == SortieResult::Success)
                        .count();
                    ui.label(format!("{flown} ({won} achieved)"));
                    let points = c.state().pilot_record(p.id).points;
                    ui.label(format!("{points} {}", rank_of(points)));
                    ui.label(if p.ping > 0 { format!("{} ms", p.ping) } else { "-".into() });
                    ui.end_row();
                }
            });
    });
    ui.add_space(8.0);
    section(ui, "Leaderboard");
    let height = (ui.available_height() - 70.0).max(100.0);
    egui::ScrollArea::vertical()
        .id_salt("leaders")
        .max_height(height)
        .auto_shrink([false, true])
        .show(ui, |ui| leaderboard(ui, c, "leaders"));
    if c.role == Role::Local {
        ui.add_space(10.0);
        ui.label(
            egui::RichText::new(
                "Flying alone. To fly together, host this campaign from MULTIPLAYER in the main menu.",
            )
            .color(DIM),
        );
    }
    ui.add_space(10.0);
    ui.label(
        egui::RichText::new(format!(
            "Rules of this campaign (kept from its start): {}",
            rules_text(&c.rules)
        ))
        .color(DIM),
    );
}

/// Every pilot of the campaign (who has left too), the most points first.
fn leaderboard(ui: &mut egui::Ui, c: &ActiveCampaign, id: &str) {
    let rows: Vec<_> = c.state().pilot_records().into_iter().filter(|p| p.sorties > 0).collect();
    egui::Grid::new(id).num_columns(7).spacing(egui::vec2(22.0, 6.0)).striped(true).show(
        ui,
        |ui| {
            for h in
                ["PILOT", "RANK", "POINTS", "SORTIES", "OBJECTIVES", "KILLS AIR/GND", "BEST MEDAL"]
            {
                ui.label(egui::RichText::new(h).color(DIM));
            }
            ui.end_row();
            for p in &rows {
                let me =
                    c.state().player_name(c.me).is_some_and(|n| n.eq_ignore_ascii_case(&p.name));
                let mut name = egui::RichText::new(&p.name).color(BRIGHT);
                if me {
                    name = name.strong();
                }
                ui.label(name);
                ui.label(rank_of(p.points));
                ui.label(egui::RichText::new(p.points.to_string()).color(BRIGHT));
                ui.label(p.sorties.to_string());
                ui.label(p.achieved.to_string());
                ui.label(format!("{}/{}", p.air_kills, p.ground_kills));
                ui.label(egui::RichText::new(medal_of(p.best).unwrap_or("-")).color(AMBER));
                ui.end_row();
            }
        },
    );
    if rows.is_empty() {
        ui.label(egui::RichText::new("No sorties flown yet").color(DIM));
    }
}
