//! The menu's MULTIPLAYER window: coop in the dynamic campaign. Join a server by its address,
//! or host a campaign here and fly in it (a listen server). A dedicated server is started
//! from the command line (`--dedicated`, `dedicated.rs`).
//!
//! The host's rules (realism, missile guidance, crashes, time of day, realistic weapons,
//! friendly fire) are
//! everybody's; each player keeps his own cockpit, flight model, avionics, HUD and graphics,
//! and picks his aircraft for each sortie.

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use std::sync::{Arc, Mutex};
use stealth_campaign::{CampaignSetup, PlayerId};
use stealth_host::{Host, save};
use stealth_master_proto::ServerEntry;
use stealth_net::client::{Client, ClientEvent};
use stealth_net::server::Server;

use crate::campaign::{ActiveCampaign, Role};
use crate::menu::{AMBER, BRIGHT, DIM, GREEN, MenuState, THEATERS, big_button, section};
use crate::mission::MissionData;
use crate::net::{NetLink, Notice};
use crate::settings::{AppState, MissionSetup};
use crate::text_entry::{Charset, text_entry};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Page {
    #[default]
    Join,
    Host,
}

#[derive(Resource, Default)]
pub struct MultiplayerWindow {
    pub open: bool,
    page: Page,
    /// Connecting to a server (until it takes the player in).
    joining: Option<(Client, String)>,
    status: Option<String>,
    error: Option<String>,
    saves: Vec<(String, String, f64)>,
    sky_ratio: f32,
    start_hour: f32,
    /// No weather: the classic clear sky.
    clear_sky: bool,
    /// `--host`: host a new campaign at once.
    auto_host: bool,
    /// The public server list: as last fetched, being fetched, or why not.
    servers: Vec<ServerEntry>,
    fetching: Option<Arc<Mutex<Option<Result<Vec<ServerEntry>, String>>>>>,
    list_error: Option<String>,
    listed_once: bool,
}

impl MultiplayerWindow {
    pub fn show(&mut self) {
        self.open = true;
        self.saves = save::list_saves();
        self.error = None;
        self.status = None;
        if self.sky_ratio == 0.0 {
            self.sky_ratio = 24.0;
            self.start_hour = 8.0;
        }
        if !self.listed_once {
            self.listed_once = true;
            self.refresh_list();
        }
    }

    /// Fetches the public server list (on a thread; `multiplayer_window` takes the answer).
    fn refresh_list(&mut self) {
        if self.fetching.is_some() {
            return;
        }
        let answer = Arc::new(Mutex::new(None));
        let (to, url) = (answer.clone(), master_url());
        std::thread::spawn(move || {
            let list = stealth_net::master::fetch_list(&url);
            if let Ok(mut a) = to.lock() {
                *a = Some(list);
            }
        });
        self.fetching = Some(answer);
        self.list_error = None;
    }
}

/// The public server list's master server (`--master <url>` for tests).
pub fn master_url() -> String {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|a| a == "--master")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_else(|| stealth_net::master::DEFAULT_MASTER.to_string())
}

pub struct MultiplayerMenuPlugin;

impl Plugin for MultiplayerMenuPlugin {
    fn build(&self, app: &mut App) {
        let open = std::env::args().any(|a| a == "--open-multiplayer");
        let mut w = MultiplayerWindow::default();
        if open {
            w.show();
        }
        app.insert_resource(w)
            .add_systems(
                bevy_egui::EguiPrimaryContextPass,
                multiplayer_window.run_if(in_state(AppState::Menu)),
            )
            .add_systems(Update, test_args.run_if(in_state(AppState::Menu)));
    }
}

/// Test aids: `--connect <host[:port]>` joins a server, `--host` serves a new campaign (in
/// `--theater`, with `--friendly-fire`), both as `--name <pilot>` (else the saved name).
fn test_args(
    mut done: Local<bool>,
    mut w: ResMut<MultiplayerWindow>,
    mut state: ResMut<MenuState>,
) {
    if std::mem::replace(&mut *done, true) {
        return;
    }
    let args: Vec<String> = std::env::args().collect();
    let get = |f: &str| args.iter().position(|a| a == f).and_then(|i| args.get(i + 1)).cloned();
    if let Some(name) = get("--name") {
        state.player_name = name;
    }
    if let Some(addr) = get("--connect") {
        w.show();
        state.server_addr = addr.clone();
        connect(&mut w, &addr, &state.player_name);
    }
    if let Some(code) = get("--theater")
        && let Some(i) = THEATERS.iter().position(|t| t.0.eq_ignore_ascii_case(&code))
    {
        state.theater = i;
    }
    if args.iter().any(|a| a == "--host") {
        state.friendly_fire |= args.iter().any(|a| a == "--friendly-fire");
        w.show();
        w.page = Page::Host;
        w.auto_host = true;
    }
}

/// The rules of a campaign hosted here.
fn host_rules(settings: &crate::settings::Settings, state: &MenuState) -> stealth_host::Rules {
    stealth_host::Rules {
        friendly_fire: state.friendly_fire,
        ..crate::campaign::rules_of(settings, state.lockheed)
    }
}

fn connect(w: &mut MultiplayerWindow, addr: &str, name: &str) {
    w.error = None;
    match Client::connect(addr, name.trim()) {
        Ok(c) => {
            w.status = Some(format!("Connecting to {addr}..."));
            w.joining = Some((c, addr.to_string()));
        }
        Err(e) => w.error = Some(format!("{addr}: {e}")),
    }
}

#[allow(clippy::too_many_arguments)]
fn multiplayer_window(
    mut commands: Commands,
    mut contexts: EguiContexts,
    mut w: ResMut<MultiplayerWindow>,
    mut state: ResMut<MenuState>,
    data: Option<Res<MissionData>>,
    tables: Res<crate::flight::SimTables>,
    mut setup: ResMut<MissionSetup>,
    mut next: ResMut<NextState<AppState>>,
    mut settings: ResMut<crate::settings::Settings>,
    mut notice: ResMut<Notice>,
) -> Result {
    // A coop game that ended brings the menu back here, with why.
    if let Some(n) = notice.0.take() {
        w.show();
        w.error = Some(n);
    }
    if !w.open {
        return Ok(());
    }
    let Some(data) = data else { return Ok(()) };
    let ctx = contexts.ctx_mut()?;
    let mut close = ctx.input(|i| i.key_pressed(egui::Key::Escape)) && w.joining.is_none();
    let mut join = false;
    let auto_host = std::mem::take(&mut w.auto_host);
    let mut host: Option<Result<(String, Host), String>> = None;
    // The public server list, when it came.
    let came = w.fetching.as_ref().and_then(|a| a.lock().ok().and_then(|mut a| a.take()));
    match came {
        Some(Ok(list)) => {
            w.servers = list;
            w.fetching = None;
        }
        Some(Err(e)) => {
            w.list_error = Some(e);
            w.fetching = None;
        }
        None => {}
    }
    // Joining: what the server says.
    if let Some((client, addr)) = w.joining.as_mut() {
        let now = std::time::Instant::now();
        let events = client.poll(now);
        client.flush(now);
        let addr = addr.clone();
        for e in events {
            match e {
                ClientEvent::Connected => {
                    w.status = Some(format!("Connected to {addr}, joining..."))
                }
                ClientEvent::Welcome { id, server, rules, state: st, resume } => {
                    let Some((client, _)) = w.joining.take() else { break };
                    match ActiveCampaign::client(server, *st, &data, rules, id) {
                        Ok(mut c) => {
                            c.resume = resume;
                            commands.insert_resource(NetLink::Client(client));
                            crate::campaign::start(&mut commands, c, &mut setup, &tables.start_dir);
                            w.open = false;
                            w.status = None;
                            next.set(AppState::Flight);
                        }
                        Err(e) => w.error = Some(format!("{e:#}")),
                    }
                    break;
                }
                ClientEvent::Closed(why) => {
                    w.joining = None;
                    w.status = None;
                    w.error = Some(format!("{addr}: {why}"));
                }
                _ => {}
            }
        }
    }
    let width = crate::menu::wide_width(ctx).min(1100.0);
    crate::menu::menu_window(ctx, "Multiplayer", width).show(ctx, |ui| {
        ui.label(egui::RichText::new("MULTIPLAYER").heading().color(BRIGHT));
        ui.label(egui::RichText::new("Coop in the dynamic campaign: fly the war together").color(DIM));
        ui.separator();
        section(ui, "Your name");
        text_entry(ui, "mp_name", &mut state.player_name, 24, Charset::Name, settings.vr, "Pilot name");
        let named = !state.player_name.trim().is_empty();
        if !named {
            ui.label(egui::RichText::new("The other players know you by it").small().color(AMBER));
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            for (page, label) in [(Page::Join, "JOIN A GAME"), (Page::Host, "HOST A GAME")] {
                if ui.selectable_label(w.page == page, egui::RichText::new(label).size(18.0)).clicked() {
                    w.page = page;
                    w.error = None;
                }
            }
        });
        ui.separator();
        match w.page {
            Page::Join => {
                if let Some(addr) = server_list(ui, &mut w, named) {
                    state.server_addr = addr;
                    join = true;
                }
                section(ui, "Server");
                text_entry(
                    ui,
                    "mp_server",
                    &mut state.server_addr,
                    64,
                    Charset::Address,
                    settings.vr,
                    "address or name[:port]",
                );
                ui.label(
                    egui::RichText::new(format!(
                        "An IP address or host name, port {} if none is given",
                        stealth_net::DEFAULT_PORT
                    ))
                    .small()
                    .color(DIM),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if w.joining.is_some() {
                        ui.add(egui::Spinner::new().color(GREEN));
                        if ui.add(big_button("CANCEL")).clicked() {
                            if let Some((mut c, _)) = w.joining.take() {
                                c.leave();
                            }
                            w.status = None;
                        }
                    } else {
                        let ready = named && !state.server_addr.trim().is_empty();
                        // `|=`: a JOIN in the server list above joins too.
                        join |= ui.add_enabled(ready, big_button("CONNECT")).clicked();
                    }
                    if let Some(s) = &w.status {
                        ui.label(egui::RichText::new(s).color(BRIGHT));
                    }
                });
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(
                        "The server decides the rules (realism, guidance, crashes, time of day, \
                         friendly fire). \
                         Your cockpit, flight model, avionics, HUD and graphics stay your own; \
                         you pick your aircraft in each briefing.",
                    )
                    .color(DIM),
                );
            }
            Page::Host => {
                ui.horizontal(|ui| {
                    ui.label("Server name");
                    let hint = server_title(&state);
                    text_entry(ui, "mp_server", &mut state.server_name, 32, Charset::Name, settings.vr, &hint);
                });
                ui.horizontal(|ui| {
                    ui.label("UDP port");
                    ui.add(egui::DragValue::new(&mut state.host_port).range(1024..=65535));
                    ui.add_space(20.0);
                    ui.label("Players");
                    ui.add(egui::Slider::new(
                        &mut state.max_players,
                        2..=stealth_net::protocol::MAX_PLAYERS as u8,
                    ));
                });
                ui.label(
                    egui::RichText::new("Other players join with this computer's address; open the UDP port in your router to play over the internet")
                        .small()
                        .color(DIM),
                );
                ui.checkbox(&mut state.list_public, "List on the public server list (others find the game in JOIN A GAME)");
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(210.0);
                        crate::menu::theater_options(ui, &mut state.theater);
                    });
                    ui.vertical(|ui| {
                        ui.set_width(200.0);
                        crate::menu::opponent_options(ui, &mut state.opponents);
                        crate::menu::situation_options(ui, &mut state.roe);
                    });
                    ui.vertical(|ui| {
                        ui.set_width(230.0);
                        section(ui, "Rules for everybody");
                        crate::menu::realism_options(ui, &mut settings);
                        ui.checkbox(&mut settings.continuous_time, "Time of day (moonlit nights)");
                        ui.checkbox(&mut state.lockheed, "Realistic F-117A weapons");
                        ui.checkbox(&mut state.friendly_fire, "Friendly fire");
                    });
                    ui.vertical(|ui| {
                        section(ui, "New campaign");
                        let (_, theater) = THEATERS[state.theater.min(THEATERS.len() - 1)];
                        ui.label(theater);
                        ui.horizontal(|ui| {
                            ui.label("Day and night:");
                            for (ratio, label) in [(1.0, "real time"), (24.0, "1 h / day"), (48.0, "30 min / day")] {
                                if ui.selectable_label(w.sky_ratio == ratio, label).clicked() {
                                    w.sky_ratio = ratio;
                                }
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label("Weather:");
                            for (clear, label) in [(false, "dynamic"), (true, "clear sky")] {
                                if ui.selectable_label(w.clear_sky == clear, label).clicked() {
                                    w.clear_sky = clear;
                                }
                            }
                        });
                        if ui.add_enabled(named, big_button("HOST NEW")).clicked() || auto_host {
                            let (code, _) = THEATERS[state.theater.min(THEATERS.len() - 1)];
                            let secs = web_time::SystemTime::now()
                                .duration_since(web_time::UNIX_EPOCH)
                                .map_or(0, |d| d.as_secs());
                            let setup = CampaignSetup {
                                theater: state.theater,
                                theater_code: code.to_string(),
                                roe: state.roe,
                                opponents: state.opponents,
                                seed: rand::random(),
                                sky_ratio: w.sky_ratio,
                                start_hour: w.start_hour,
                                dynamic_weather: !w.clear_sky,
                            };
                            let rules = host_rules(&settings, &state);
                            host = Some(
                                Host::new(setup, &data.0, rules)
                                    .map(|h| (format!("coop-{code}-{secs}"), h))
                                    .map_err(|e| format!("{e:#}")),
                            );
                        }
                        ui.add_space(6.0);
                        section(ui, "Host a saved campaign");
                        if w.saves.is_empty() {
                            ui.label(egui::RichText::new("No saved campaigns").color(DIM));
                        }
                        for (name, code, hours) in w.saves.clone().into_iter().take(6) {
                            let theater = THEATERS.iter().find(|t| t.0 == code).map_or(code.as_str(), |t| t.1);
                            ui.horizontal(|ui| {
                                ui.label(format!("{theater:<22} {hours:>5.1} h"));
                                if ui.add_enabled(named, egui::Button::new("HOST")).clicked() {
                                    let rules = host_rules(&settings, &state);
                                    host = Some(match save::load(&name) {
                                        Ok(Some(st)) => Host::resume(st, &data.0, rules)
                                            .map(|h| (name.clone(), h))
                                            .map_err(|e| format!("{e:#}")),
                                        Ok(None) => Err("The save is gone".into()),
                                        Err(e) => Err(format!("{e:#}")),
                                    });
                                }
                            });
                        }
                    });
                });
            }
        }
        if let Some(e) = &w.error {
            ui.add_space(6.0);
            ui.colored_label(AMBER, e);
        }
        ui.separator();
        if ui.add_enabled(w.joining.is_none(), big_button("BACK")).clicked() {
            close = true;
        }
    });
    if join {
        let (addr, name) = (state.server_addr.clone(), state.player_name.clone());
        connect(&mut w, &addr, &name);
    }
    match host {
        Some(Ok((name, h))) => {
            let title = server_title(&state);
            match Server::new(
                &title,
                state.host_port,
                state.max_players as usize,
                Some(PlayerId(0)),
            ) {
                Ok(mut server) => {
                    if state.list_public {
                        server.list_on(&master_url());
                    }
                    let c = ActiveCampaign::with_host(name, h, Role::Host);
                    c.save();
                    commands.insert_resource(NetLink::Host(server));
                    crate::campaign::start(&mut commands, c, &mut setup, &tables.start_dir);
                    w.open = false;
                    next.set(AppState::Flight);
                }
                Err(e) => w.error = Some(format!("UDP port {}: {e}", state.host_port)),
            }
        }
        Some(Err(e)) => w.error = Some(e),
        None => {}
    }
    if close {
        w.open = false;
    }
    Ok(())
}

/// The name the server goes by: the one given, else "<player>'s campaign".
fn server_title(state: &crate::menu::MenuState) -> String {
    match state.server_name.trim() {
        "" => format!("{}'s campaign", state.player_name.trim()),
        name => name.to_string(),
    }
}

/// The public server list on the Join page: a server's JOIN gives its address.
fn server_list(ui: &mut egui::Ui, w: &mut MultiplayerWindow, named: bool) -> Option<String> {
    let mut pick = None;
    ui.horizontal(|ui| {
        section(ui, "Public servers");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if w.fetching.is_some() {
                ui.add(egui::Spinner::new().color(GREEN));
            } else if ui.button("REFRESH").clicked() {
                w.refresh_list();
            }
            let n: usize = w.servers.iter().map(|s| s.report.players.len()).sum();
            if !w.servers.is_empty() {
                ui.label(
                    egui::RichText::new(format!("{} servers, {n} pilots", w.servers.len()))
                        .color(DIM),
                );
            }
        });
    });
    if let Some(e) = &w.list_error {
        ui.label(
            egui::RichText::new(format!("The server list is not reachable: {e}")).color(AMBER),
        );
    } else if w.servers.is_empty() && w.fetching.is_none() {
        ui.label(egui::RichText::new("No public servers right now").color(DIM));
    }
    if w.servers.is_empty() {
        ui.add_space(6.0);
        return None;
    }
    egui::ScrollArea::vertical().max_height(220.0).auto_shrink([false, true]).show(ui, |ui| {
        egui::Grid::new("mp_servers")
            .num_columns(5)
            .spacing(egui::vec2(18.0, 6.0))
            .striped(true)
            .show(ui, |ui| {
                for h in ["SERVER", "PILOTS", "THEATER", "WAR", ""] {
                    ui.label(egui::RichText::new(h).color(DIM));
                }
                ui.end_row();
                for s in &w.servers {
                    let r = &s.report;
                    let ours = r.protocol == stealth_net::PROTOCOL;
                    let full = r.max_players > 0 && r.players.len() >= r.max_players as usize;
                    let col = if ours { BRIGHT } else { DIM };
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(&r.name).strong().color(col));
                        if r.dedicated {
                            ui.label(egui::RichText::new("DEDICATED").small().color(DIM));
                        }
                    })
                    .response
                    .on_hover_text(s.addr.as_str());
                    let count = format!("{}/{}", r.players.len(), r.max_players);
                    ui.label(egui::RichText::new(count).monospace().color(if full {
                        AMBER
                    } else {
                        GREEN
                    }));
                    ui.label(egui::RichText::new(&r.theater).color(col));
                    ui.label(
                        egui::RichText::new(format!("{}, {}", r.situation, r.clock)).color(DIM),
                    );
                    if !ours {
                        ui.label(egui::RichText::new("other version").small().color(DIM));
                    } else if ui
                        .add_enabled(
                            named && !full && w.joining.is_none(),
                            egui::Button::new("JOIN"),
                        )
                        .clicked()
                    {
                        pick = Some(s.addr.clone());
                    }
                    ui.end_row();
                }
            });
    });
    ui.add_space(6.0);
    pick
}
