//! Debrief screen after a flight: outcome, statistics, points, rating and the mission timeline.

use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPrimaryContextPass, egui};

use stealth_sim::score::MissionReport;

use crate::menu::{AMBER, BG, BRIGHT, DIM, GREEN, apply_terminal_style, big_button, section};
use crate::mission::clean_text;
use crate::settings::AppState;

pub struct DebriefPlugin;

impl Plugin for DebriefPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::Debrief), record_career)
            .add_systems(EguiPrimaryContextPass, debrief_ui.run_if(in_state(AppState::Debrief)));
    }
}

/// Results of the last flight (set when the flight ends).
#[derive(Resource, Debug, Clone)]
pub struct Debrief {
    /// End overlay title (how the flight ended).
    pub title: String,
    pub report: MissionReport,
    /// Everything destroyed on the ground (for a tour of duty).
    pub damage: Option<stealth_sim::WorldDamage>,
    /// The mission flown (the flight's `ActiveMission` is gone by now).
    pub mission: Option<stealth_mission::Mission>,
    /// Career news of a tour sortie (medals, promotion, the tour).
    pub career: Vec<String>,
}

/// A tour sortie goes into the pilot's record when the debrief opens.
fn record_career(
    mut commands: Commands,
    flight: Option<Res<crate::career::CareerFlight>>,
    debrief: Option<ResMut<Debrief>>,
    mut menu: ResMut<crate::menu::MenuState>,
) {
    let (Some(flight), Some(mut d)) = (flight, debrief) else { return };
    commands.remove_resource::<crate::career::CareerFlight>();
    let Some(mut p) = crate::career::Pilot::load(&flight.0) else { return };
    let d = &mut *d;
    let Some(m) = d.mission.clone() else { return };
    d.career = p.record(&d.report, d.damage.as_ref(), &m);
    p.save();
    // Back in the menu: the tour's next sortie, if there is one.
    menu.career = (p.tour.is_some() && p.status == crate::career::Status::Active).then(|| p.key());
    menu.seed = rand::random();
    menu.regenerate();
}

fn clock(t: i32) -> String {
    format!("{}:{:02}", t / 60, t % 60)
}

fn debrief_ui(
    mut contexts: EguiContexts,
    debrief: Option<Res<Debrief>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut next: ResMut<NextState<AppState>>,
) -> Result {
    let Some(d) = debrief else {
        next.set(AppState::Menu);
        return Ok(());
    };
    let r = &d.report;
    let ctx = contexts.ctx_mut()?;
    if ctx.global_style().visuals.window_fill != BG {
        ctx.all_styles_mut(apply_terminal_style);
    }
    let screen = ctx.content_rect();
    let width = (screen.width() - 32.0).clamp(600.0, 1100.0);
    let mut done = keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Escape);
    egui::Window::new("Debrief")
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .collapsible(false)
        .resizable(false)
        .title_bar(false)
        .min_width(width)
        .max_width(width)
        .frame(egui::Frame::window(&ctx.global_style()).inner_margin(16.0))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("MISSION DEBRIEF").heading().color(BRIGHT));
                if let Some(m) = &d.mission {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new(clean_text(&m.briefing.title).to_ascii_uppercase()).color(DIM));
                    });
                }
            });
            ui.separator();
            ui.label(egui::RichText::new(d.title.to_ascii_uppercase()).size(24.0).color(if r.outcome.fatal() { AMBER } else { BRIGHT }));
            if r.training {
                ui.label(egui::RichText::new("TRAINING MISSION").color(DIM));
            }
            ui.add_space(6.0);
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(width * 0.3);
                    section(ui, "Results");
                    let yes_no = |b: bool| if b { ("ACCOMPLISHED", GREEN) } else { ("FAILED", AMBER) };
                    let rows = [
                        ("PRIMARY", yes_no(r.primary_done)),
                        ("SECONDARY", yes_no(r.secondary_done)),
                    ];
                    egui::Grid::new("results").num_columns(2).spacing([14.0, 4.0]).show(ui, |ui| {
                        for (k, (v, c)) in rows {
                            ui.label(egui::RichText::new(k).size(14.0).color(DIM));
                            ui.label(egui::RichText::new(v).size(18.0).color(c));
                            ui.end_row();
                        }
                        for (k, v) in [
                            ("AIRCRAFT", r.air_kills.to_string()),
                            ("DRONES", if r.drones_killed + r.drones_leaked > 0 { format!("{} down, {} through", r.drones_killed, r.drones_leaked) } else { "-".into() }),
                            ("GROUND", r.ground_kills.to_string()),
                            ("WEAPONS", r.weapons_fired.to_string()),
                            ("RADAR IDS", r.radar_ids.to_string()),
                            ("VISUAL IDS", r.visual_ids.to_string()),
                            ("HITS TAKEN", r.hits_taken.to_string()),
                            ("TIME", clock(r.time)),
                        ] {
                            ui.label(egui::RichText::new(k).size(14.0).color(DIM));
                            ui.label(egui::RichText::new(v).size(18.0).color(GREEN));
                            ui.end_row();
                        }
                    });
                });
                ui.vertical(|ui| {
                    ui.set_width(width * 0.3);
                    section(ui, "Points");
                    egui::Grid::new("points").num_columns(2).spacing([14.0, 4.0]).show(ui, |ui| {
                        for (what, p) in &r.points {
                            ui.label(egui::RichText::new(what.to_ascii_uppercase()).size(14.0).color(DIM));
                            ui.label(egui::RichText::new(format!("{p:+}")).size(16.0).color(if *p < 0 { AMBER } else { GREEN }));
                            ui.end_row();
                        }
                        ui.label(egui::RichText::new("DIFFICULTY").size(14.0).color(DIM));
                        ui.label(egui::RichText::new(format!("x{:.2}", r.multiplier)).size(16.0).color(GREEN));
                        ui.end_row();
                        ui.label(egui::RichText::new("TOTAL").size(16.0).color(BRIGHT));
                        ui.label(egui::RichText::new(r.total.to_string()).size(24.0).color(BRIGHT));
                        ui.end_row();
                    });
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(r.rating.to_ascii_uppercase()).size(22.0).color(if r.total > 0 { BRIGHT } else { AMBER }));
                    if !d.career.is_empty() {
                        section(ui, "Service record");
                        for line in &d.career {
                            ui.label(egui::RichText::new(line.to_ascii_uppercase()).color(AMBER));
                        }
                    }
                });
                ui.vertical(|ui| {
                    section(ui, "Timeline");
                    egui::ScrollArea::vertical().max_height(320.0).auto_shrink([false, true]).show(ui, |ui| {
                        if r.timeline.is_empty() {
                            ui.label(egui::RichText::new("No events").color(DIM));
                        }
                        for e in &r.timeline {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(clock(e.tick)).color(DIM));
                                ui.label(&e.text);
                            });
                        }
                    });
                });
            });
            ui.add_space(6.0);
            ui.separator();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                done |= ui.add(big_button("CONTINUE").fill(DIM)).clicked();
            });
        });
    if done {
        next.set(AppState::Menu);
    }
    Ok(())
}
