//! The menu's campaign window: choose the options and start a dynamic campaign, or continue a
//! saved one.

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use stealth_campaign::CampaignSetup;

use super::ActiveCampaign;
use crate::menu::{AMBER, BRIGHT, DIM, MenuState, THEATERS, big_button, dropdown, group, section};
use crate::mission::MissionData;
use crate::settings::{AppState, MissionSetup};

#[derive(Resource)]
pub struct CampaignWindow {
    pub open: bool,
    /// Saved campaigns (name, theater, hours played), read when the window opens.
    saves: Vec<(String, String, f64)>,
    /// Sky speed: game time per real time.
    sky_ratio: f32,
    start_hour: f32,
    /// No weather: the classic clear sky.
    clear_sky: bool,
    error: Option<String>,
    /// The save whose DELETE was clicked: (name, label), awaiting confirmation.
    confirm_delete: Option<(String, String)>,
}

impl Default for CampaignWindow {
    fn default() -> Self {
        Self {
            open: false,
            saves: Vec::new(),
            sky_ratio: 24.0,
            start_hour: 8.0,
            clear_sky: false,
            error: None,
            confirm_delete: None,
        }
    }
}

impl CampaignWindow {
    pub fn opened(open: bool) -> Self {
        let mut w = Self::default();
        if open {
            w.show();
        }
        w
    }

    pub fn show(&mut self) {
        self.open = true;
        self.saves = stealth_host::save::list_saves();
        self.error = None;
        self.confirm_delete = None;
    }
}

/// How tall the list of saved campaigns grows before it scrolls.
pub(crate) const SAVES_HEIGHT: f32 = 240.0;

/// The "delete this campaign?" popup over the menu: Some(true) to delete, Some(false) to keep it
/// (CANCEL, or `cancel` such as Escape), None while it waits.
pub(crate) fn delete_popup(ctx: &egui::Context, label: &str, cancel: bool) -> Option<bool> {
    let mut answer = cancel.then_some(false);
    egui::Window::new("Delete the campaign")
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .collapsible(false)
        .resizable(false)
        .title_bar(false)
        .order(egui::Order::Foreground)
        .frame(egui::Frame::window(&ctx.global_style()).inner_margin(18.0))
        .show(ctx, |ui| {
            ui.label(egui::RichText::new("DELETE THIS CAMPAIGN?").heading().color(BRIGHT));
            ui.label(egui::RichText::new(label).color(BRIGHT));
            ui.label(egui::RichText::new("Deletion is permanent").color(DIM));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.add(big_button("DELETE").fill(super::ops::EXIT_RED)).clicked() {
                    answer = Some(true);
                }
                if ui.add(big_button("CANCEL")).clicked() {
                    answer = Some(false);
                }
            });
        });
    answer
}

fn roe_name(roe: u8) -> &'static str {
    match roe {
        0 => "Cold War",
        1 => "Limited War",
        _ => "Conventional War",
    }
}

#[allow(clippy::too_many_arguments)]
pub fn campaign_window(
    mut commands: Commands,
    mut contexts: EguiContexts,
    mut w: ResMut<CampaignWindow>,
    mut state: ResMut<MenuState>,
    data: Option<Res<MissionData>>,
    tables: Res<crate::flight::SimTables>,
    mut setup: ResMut<MissionSetup>,
    mut next: ResMut<NextState<AppState>>,
    mut settings: ResMut<crate::settings::Settings>,
    vr: Res<crate::menu::VrAvailable>,
    mut exit: MessageWriter<AppExit>,
) -> Result {
    if !w.open {
        return Ok(());
    }
    let Some(data) = data else { return Ok(()) };
    let ctx = contexts.ctx_mut()?;
    let (code, theater) = THEATERS[state.theater.min(THEATERS.len() - 1)];
    let mut start: Option<ActiveCampaign> = None;
    let mut vr_name: Option<String> = None;
    let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    // Escape cancels the delete confirmation first, then closes the window.
    let mut close = escape && w.confirm_delete.is_none();
    let width = crate::menu::wide_width(ctx);
    crate::menu::menu_window(ctx, "Dynamic campaign", width).show(ctx, |ui| {
        ui.label(egui::RichText::new("DYNAMIC CAMPAIGN").heading().color(BRIGHT));
        ui.separator();
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(210.0);
                group(ui, "Campaign", |ui| {
                    crate::menu::theater_options(ui, &mut state.theater);
                    crate::menu::situation_options(ui, &mut state.roe);
                    crate::menu::opponent_options(ui, &mut state.opponents);
                });
            });
            ui.vertical(|ui| {
                ui.set_width(210.0);
                group(ui, "Conditions", |ui| {
                    section(ui, "Weather");
                    dropdown(
                        ui,
                        "campaign_weather",
                        &mut w.clear_sky,
                        [
                            (false, "Dynamic", &["The weather changes every 10-22 hours of the sky's clock", "Clouds block line of sight for both sides"][..]),
                            (true, "Clear sky", &["No clouds for the whole campaign"][..]),
                        ],
                    );
                    section(ui, "Start time");
                    ui.add(
                        egui::Slider::new(&mut w.start_hour, 0.0..=23.0)
                            .step_by(1.0)
                            .custom_formatter(|v, _| format!("{v:02.0}:00")),
                    );
                    section(ui, "Day and night");
                    dropdown(
                        ui,
                        "campaign_sky_ratio",
                        &mut w.sky_ratio,
                        [
                            (1.0, "Real time", &["A day lasts 24 hours"][..]),
                            (12.0, "2 h / day", &["A day lasts 2 hours"][..]),
                            (24.0, "1 h / day", &["A day lasts 1 hour"][..]),
                            (48.0, "30 min / day", &["A day lasts 30 minutes"][..]),
                        ],
                    );
                });
                ui.label(
                    egui::RichText::new("Conditions apply to a new campaign")
                        .small()
                        .color(DIM),
                );
            });
            ui.vertical(|ui| {
                ui.set_width(220.0);
                crate::menu::simulation_options(
                    ui,
                    &mut settings,
                    Some("A saved campaign keeps the rules it was started with"),
                );
            });
            ui.vertical(|ui| {
            for line in [
                "The war runs in real time while you play, also while the menu is open.",
                "Fly the missions on the board: recon, SEAD, CAP, CAS, anti-ship, strike, escort.",
                "Helicopter insertions take the enemy's cities and airfields.",
                "You are never killed or captured: after a sortie you are back at the board.",
            ] {
                ui.label(egui::RichText::new(line).color(DIM));
            }
            ui.add_space(6.0);
            section(ui, "New campaign");
            ui.label(format!(
                "{theater}  -  {}  -  opponents {}",
                roe_name(state.roe),
                ["green", "regular", "veteran", "elite"][state.opponents.min(3) as usize]
            ));
            if ui.add(big_button("START")).clicked() {
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
                let rules = super::rules_of(&settings, state.realistic(crate::aircraft::AircraftKind::F117));
                match ActiveCampaign::new(format!("{code}-{secs}"), setup, &data, rules) {
                    Ok(c) => start = Some(c),
                    Err(e) => w.error = Some(format!("{e:#}")),
                }
            }
            ui.add_space(6.0);
            section(ui, "Continue");
            if w.saves.is_empty() {
                ui.label(egui::RichText::new("No saved campaigns").color(DIM));
            }
            egui::ScrollArea::vertical().id_salt("campaign_saves").max_height(SAVES_HEIGHT).show(ui, |ui| {
            for (name, code, hours) in w.saves.clone() {
                let theater = THEATERS.iter().find(|t| t.0 == code).map_or(code.as_str(), |t| t.1);
                ui.horizontal(|ui| {
                    ui.label(format!("{theater:<24} {hours:>5.1} h played"));
                    if ui.add(big_button("CONTINUE")).clicked() {
                        let rules = super::rules_of(&settings, state.realistic(crate::aircraft::AircraftKind::F117));
                        match stealth_host::save::load(&name) {
                            Ok(Some(st)) => match ActiveCampaign::resume(name.clone(), st, &data, rules) {
                                Ok(c) => start = Some(c),
                                Err(e) => w.error = Some(format!("{e:#}")),
                            },
                            Ok(None) => w.error = Some("The save is gone".into()),
                            Err(e) => w.error = Some(format!("The save could not be read: {e:#}")),
                        }
                    }
                    if !settings.vr && vr.0 && ui.add(big_button("IN VR")).clicked() {
                        vr_name = Some(name.clone());
                    }
                    if ui.add(big_button("DELETE")).on_hover_text("Delete this saved campaign").clicked() {
                        w.confirm_delete = Some((name.clone(), format!("{theater}, {hours:.1} h played")));
                    }
                });
            }
            });
            if let Some(e) = &w.error {
                ui.colored_label(AMBER, e);
            }
            });
        });
        ui.separator();
        if ui.add(big_button("BACK")).on_hover_text("Back to the main menu").clicked() {
            close = true;
        }
    });
    if let Some((name, label)) = w.confirm_delete.clone() {
        let answer = delete_popup(ctx, &label, escape);
        if answer == Some(true) {
            if let Err(e) = stealth_host::save::delete(&name) {
                w.error = Some(format!("The save could not be deleted: {e:#}"));
            }
            w.saves = stealth_host::save::list_saves();
        }
        if answer.is_some() {
            w.confirm_delete = None;
        }
    }
    if let Some(c) = start {
        c.save();
        super::start(&mut commands, c, &mut setup, &tables.start_dir);
        w.open = false;
        next.set(AppState::Flight);
    }
    if let Some(name) = vr_name {
        match crate::menu::relaunch_campaign_in_vr(&name, &settings) {
            Ok(()) => {
                exit.write(AppExit::Success);
            }
            Err(e) => w.error = Some(format!("could not start in VR: {e}")),
        }
    }
    if close {
        w.open = false;
    }
    Ok(())
}
