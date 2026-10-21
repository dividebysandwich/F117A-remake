//! The menu's campaign window: start a dynamic campaign in the menu's theater, or continue a
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
    error: Option<String>,
}

impl Default for CampaignWindow {
    fn default() -> Self {
        Self { open: false, saves: Vec::new(), sky_ratio: 24.0, start_hour: 8.0, error: None }
    }
}

impl CampaignWindow {
    pub fn show(&mut self) {
        self.open = true;
        self.saves = super::save::list_saves();
        self.error = None;
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
    state: Res<MenuState>,
    data: Option<Res<MissionData>>,
    tables: Res<crate::flight::SimTables>,
    mut setup: ResMut<MissionSetup>,
    mut next: ResMut<NextState<AppState>>,
    settings: Res<crate::settings::Settings>,
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
    let mut close = false;
    egui::Window::new("Dynamic campaign")
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .collapsible(false)
        .resizable(false)
        .title_bar(false)
        .min_width(640.0)
        .show(ctx, |ui| {
            ui.label(egui::RichText::new("DYNAMIC CAMPAIGN").heading().color(BRIGHT));
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
            ui.label(
                egui::RichText::new("Theater, war level and opponents as chosen in the menu")
                    .small()
                    .color(DIM),
            );
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
                ui.add(egui::Slider::new(&mut w.start_hour, 0.0..=23.0).step_by(1.0).suffix(":00"));
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
                };
                match ActiveCampaign::new(format!("{code}-{secs}"), setup, &data) {
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
                        match super::save::load(&name)
                            .map(|st| ActiveCampaign::resume(name.clone(), st, &data))
                        {
                            Some(Ok(c)) => start = Some(c),
                            Some(Err(e)) => w.error = Some(format!("{e:#}")),
                            None => w.error = Some("The save could not be read".into()),
                        }
                    }
                    if !settings.vr && vr.0 && ui.add(big_button("IN VR")).clicked() {
                        vr_name = Some(name.clone());
                    }
                });
            }
            if let Some(e) = &w.error {
                ui.colored_label(AMBER, e);
            }
            ui.separator();
            if ui.add(big_button("CLOSE")).clicked() {
                close = true;
            }
        });
    if let Some(c) = start {
        super::save::save(&c.name, &c.state);
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
