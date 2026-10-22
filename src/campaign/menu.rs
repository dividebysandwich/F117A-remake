//! The menu's campaign window: choose the options and start a dynamic campaign, or continue a
//! saved one.

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use stealth_campaign::CampaignSetup;

use super::ActiveCampaign;
use crate::menu::{AMBER, BRIGHT, DIM, MenuState, THEATERS, big_button, section};
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
    /// No weather: the original's clear sky.
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
    head: Res<crate::headtrack::HeadTrack>,
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
                crate::menu::theater_options(ui, &mut state.theater);
                crate::menu::opponent_options(ui, &mut state.opponents);
                crate::menu::situation_options(ui, &mut state.roe);
            });
            ui.vertical(|ui| {
                ui.set_width(190.0);
                crate::menu::aircraft_options(ui, &mut settings, None);
                crate::menu::realism_options(ui, &mut settings);
            });
            ui.vertical(|ui| {
                ui.set_width(200.0);
                crate::menu::classic_modern_options(ui, &mut settings, head.source);
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
            ui.horizontal(|ui| {
                ui.label("Day and night:");
                for (ratio, label) in [
                    (1.0, "real time"),
                    (12.0, "2 h / day"),
                    (24.0, "1 h / day"),
                    (48.0, "30 min / day"),
                ] {
                    if ui.selectable_label(w.sky_ratio == ratio, label).clicked() {
                        w.sky_ratio = ratio;
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label("Starts at");
                ui.add(
                    egui::Slider::new(&mut w.start_hour, 0.0..=23.0)
                        .step_by(1.0)
                        .custom_formatter(|v, _| format!("{v:02.0}:00")),
                );
            });
            ui.horizontal(|ui| {
                ui.label("Weather:");
                for (clear, label) in [(false, "changing"), (true, "clear sky")] {
                    if ui.selectable_label(w.clear_sky == clear, label).clicked() {
                        w.clear_sky = clear;
                    }
                }
            });
            if ui.add(big_button("START")).clicked() {
                let secs = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
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
            for (name, code, hours) in w.saves.clone().into_iter().take(8) {
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
        let mut delete = false;
        let mut cancel = escape;
        egui::Window::new("Delete the campaign")
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .title_bar(false)
            .order(egui::Order::Foreground)
            .frame(egui::Frame::window(&ctx.global_style()).inner_margin(18.0))
            .show(ctx, |ui| {
                ui.label(egui::RichText::new("DELETE THIS CAMPAIGN?").heading().color(BRIGHT));
                ui.label(egui::RichText::new(&label).color(BRIGHT));
                ui.label(egui::RichText::new("Deletion is permanent").color(DIM));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.add(big_button("DELETE").fill(super::ops::EXIT_RED)).clicked() {
                        delete = true;
                    }
                    if ui.add(big_button("CANCEL")).clicked() {
                        cancel = true;
                    }
                });
            });
        if delete {
            if let Err(e) = stealth_host::save::delete(&name) {
                w.error = Some(format!("The save could not be deleted: {e:#}"));
            }
            w.saves = stealth_host::save::list_saves();
        }
        if delete || cancel {
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
