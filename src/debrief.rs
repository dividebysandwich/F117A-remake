//! Debrief screen after a flight: outcome, statistics, points, rating and the mission timeline.

use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPrimaryContextPass, egui};

use stealth_sim::score::MissionReport;

use crate::map_draw::{self, FRIENDLY_BLUE, MISSILE_RED, MapView};
use crate::menu::{AMBER, BG, BRIGHT, DIM, GREEN, apply_terminal_style, big_button, section};
use crate::mission::clean_text;
use crate::recorder::{FlightRecord, MarkKind, Sample};
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
    /// The recorded flight (the flight map).
    pub record: FlightRecord,
}

/// The flight map's view state.
#[derive(Default)]
struct MapState {
    open: bool,
    /// Frame shown (the scrubber).
    frame: f32,
    playing: bool,
    centre: egui::Vec2,
    scale: f32,
    /// The view was fitted to the track.
    fitted: bool,
}

/// A tour sortie goes into the pilot's record when the debrief opens.
fn record_career(
    mut commands: Commands,
    flight: Option<Res<crate::career::CareerFlight>>,
    debrief: Option<ResMut<Debrief>>,
    mut menu: ResMut<crate::menu::MenuState>,
    mut career: ResMut<crate::career::CareerWindow>,
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
    // The tour is over: back to the pilot records.
    menu.mission_screen = menu.career.is_some();
    if menu.career.is_none() {
        career.show();
    }
}

fn clock(t: i32) -> String {
    format!("{}:{:02}", t / 60, t % 60)
}

fn debrief_ui(
    mut contexts: EguiContexts,
    debrief: Option<Res<Debrief>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut next: ResMut<NextState<AppState>>,
    planner: Res<crate::planner::Planner>,
    mut map: Local<MapState>,
) -> Result {
    let Some(d) = debrief else {
        next.set(AppState::Menu);
        return Ok(());
    };
    if d.is_added() {
        *map = MapState::default();
        // Test aid: the flight map at mid-flight.
        if std::env::args().any(|a| a == "--debrief-map") {
            map.open = true;
            map.frame = d.record.last_frame() as f32 / 2.0;
        }
    }
    let tex = d
        .mission
        .as_ref()
        .and_then(|m| planner.map_texture(&m.theater_code.to_ascii_lowercase()))
        .cloned();
    let r = &d.report;
    let ctx = contexts.ctx_mut()?;
    crate::menu::apply_ui_zoom(ctx);
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
                        ui.label(
                            egui::RichText::new(clean_text(&m.briefing.title).to_ascii_uppercase())
                                .color(DIM),
                        );
                    });
                }
            });
            ui.separator();
            ui.label(
                egui::RichText::new(d.title.to_ascii_uppercase())
                    .size(24.0)
                    .color(if r.outcome.fatal() { AMBER } else { BRIGHT }),
            );
            if r.training {
                ui.label(egui::RichText::new("TRAINING MISSION").color(DIM));
            }
            ui.add_space(6.0);
            if map.open {
                flight_map(ui, &d, tex.as_ref(), &mut map, width);
            } else {
                summary(ui, &d, width);
            }
            ui.add_space(6.0);
            ui.separator();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                done |= ui.add(big_button("CONTINUE").fill(DIM)).clicked();
                if !d.record.samples.is_empty() {
                    let label = if map.open { "RESULTS" } else { "FLIGHT MAP" };
                    if ui.add(big_button(label)).clicked() {
                        map.open = !map.open;
                    }
                }
            });
        });
    if done {
        next.set(AppState::Menu);
    }
    Ok(())
}

/// The results, points and timeline columns.
fn summary(ui: &mut egui::Ui, d: &Debrief, width: f32) {
    let r = &d.report;
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(width * 0.3);
            section(ui, "Results");
            let yes_no = |b: bool| if b { ("ACCOMPLISHED", GREEN) } else { ("FAILED", AMBER) };
            let rows =
                [("PRIMARY", yes_no(r.primary_done)), ("SECONDARY", yes_no(r.secondary_done))];
            egui::Grid::new("results").num_columns(2).spacing([14.0, 4.0]).show(ui, |ui| {
                for (k, (v, c)) in rows {
                    ui.label(egui::RichText::new(k).size(14.0).color(DIM));
                    ui.label(egui::RichText::new(v).size(18.0).color(c));
                    ui.end_row();
                }
                for (k, v) in [
                    ("AIRCRAFT", r.air_kills.to_string()),
                    (
                        "DRONES",
                        if r.drones_killed + r.drones_leaked > 0 {
                            format!("{} down, {} through", r.drones_killed, r.drones_leaked)
                        } else {
                            "-".into()
                        },
                    ),
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
                    ui.label(egui::RichText::new(format!("{p:+}")).size(16.0).color(if *p < 0 {
                        AMBER
                    } else {
                        GREEN
                    }));
                    ui.end_row();
                }
                ui.label(egui::RichText::new("DIFFICULTY").size(14.0).color(DIM));
                ui.label(
                    egui::RichText::new(format!("x{:.2}", r.multiplier)).size(16.0).color(GREEN),
                );
                ui.end_row();
                ui.label(egui::RichText::new("TOTAL").size(16.0).color(BRIGHT));
                ui.label(egui::RichText::new(r.total.to_string()).size(24.0).color(BRIGHT));
                ui.end_row();
            });
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(r.rating.to_ascii_uppercase())
                    .size(22.0)
                    .color(if r.total > 0 { BRIGHT } else { AMBER }),
            );
            if !d.career.is_empty() {
                section(ui, "Service record");
                for line in &d.career {
                    ui.label(egui::RichText::new(line.to_ascii_uppercase()).color(AMBER));
                }
            }
        });
        ui.vertical(|ui| {
            section(ui, "Timeline");
            egui::ScrollArea::vertical().max_height(320.0).auto_shrink([false, true]).show(
                ui,
                |ui| {
                    if r.timeline.is_empty() {
                        ui.label(egui::RichText::new("No events").color(DIM));
                    }
                    for e in &r.timeline {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(clock(e.tick)).color(DIM));
                            ui.label(&e.text);
                        });
                    }
                },
            );
        });
    });
}

/// Track colour by detection margin: green well clear, amber close, red detected.
fn margin_colour(margin: i32) -> egui::Color32 {
    if margin > 0 {
        MISSILE_RED
    } else if margin > -30 {
        AMBER
    } else {
        GREEN
    }
}

/// The flight on the theater map: the track coloured by radar exposure, the sites that tracked
/// the player and the events, with a time scrubber.
fn flight_map(
    ui: &mut egui::Ui,
    d: &Debrief,
    tex: Option<&egui::TextureHandle>,
    map: &mut MapState,
    width: f32,
) {
    let rec = &d.record;
    let last = rec.last_frame();
    let fs = crate::recorder::FS as f32;
    if map.playing {
        map.frame += ui.input(|i| i.stable_dt) * fs * 20.0;
        if map.frame >= last as f32 {
            map.frame = last as f32;
            map.playing = false;
        }
        ui.ctx().request_repaint();
    }
    let cursor = map.frame as u32;
    let side = 270.0;
    let size = egui::vec2(width - side - 12.0, 470.0);
    ui.horizontal_top(|ui| {
        let (resp, painter) = ui.allocate_painter(size, egui::Sense::click_and_drag());
        let rect = resp.rect;
        if !map.fitted || map.scale <= 0.0 {
            let (mut lo, mut hi) = (egui::vec2(f32::MAX, f32::MAX), egui::vec2(f32::MIN, f32::MIN));
            for s in &rec.samples {
                let (x, y) = s.map();
                lo = lo.min(egui::vec2(x, y));
                hi = hi.max(egui::vec2(x, y));
            }
            (map.centre, map.scale) = MapView::fit(rect, lo, hi);
            map.fitted = true;
        }
        let view = MapView { rect, centre: map.centre, scale: map.scale };
        let painter = painter.with_clip_rect(rect);
        map_draw::picture(&painter, &view, tex);
        if let Some(m) = &d.mission {
            map_draw::airfields(&painter, &view, m);
        }
        // Sites that tracked the player at some time; the ones tracking him now bright.
        let now = |i: usize| rec.spans.iter().any(|s| s.site == i && s.covers(cursor));
        let mut shown: Vec<usize> = rec.spans.iter().map(|s| s.site).collect();
        shown.sort_unstable();
        shown.dedup();
        let here = rec.at(cursor).copied();
        for &i in &shown {
            let Some(site) = rec.sites.get(i) else { continue };
            let c = view.to_screen(site.x, site.y);
            let alpha = if now(i) { 1.0 } else { 0.45 };
            painter.extend(map_draw::missile_circle(c, site.lock / view.scale, alpha));
            map_draw::site(&painter, &view, c, &site.kind, alpha);
            if now(i)
                && let Some(s) = here
            {
                let (x, y) = s.map();
                painter.line_segment(
                    [c, view.to_screen(x, y)],
                    egui::Stroke::new(1.0, MISSILE_RED.gamma_multiply(0.7)),
                );
            }
        }
        // The track, one point a second, coloured by the worst margin of that second; dim
        // after the scrubber.
        let mut runs: Vec<(egui::Color32, Vec<egui::Pos2>)> = Vec::new();
        for chunk in rec.samples.chunks(crate::recorder::FS as usize) {
            let s = chunk.last().unwrap();
            let worst = chunk.iter().map(|s| s.margin).max().unwrap_or(-100);
            let alpha = if s.frame <= cursor { 1.0 } else { 0.3 };
            let colour = margin_colour(worst).gamma_multiply(alpha);
            let (x, y) = s.map();
            let p = view.to_screen(x, y);
            match runs.last_mut() {
                Some((c, pts)) if *c == colour => pts.push(p),
                Some((_, pts)) => {
                    let joint = *pts.last().unwrap();
                    runs.push((colour, vec![joint, p]));
                }
                None => runs.push((colour, vec![p])),
            }
        }
        for (c, pts) in runs {
            painter.add(egui::Shape::line(pts, egui::Stroke::new(2.5, c)));
        }
        for m in rec.marks.iter().filter(|m| m.frame <= cursor) {
            mark(&painter, view.to_screen(m.x, m.y), m.kind);
        }
        if let Some(s) = here {
            aircraft(&painter, &view, &s);
        }
        view.pan_zoom(ui, &resp, &mut map.centre, &mut map.scale);

        ui.vertical(|ui| {
            ui.set_width(side);
            section(ui, "Radar exposure");
            let e = rec.exposure();
            let row = |ui: &mut egui::Ui, k: &str, v: String, c: egui::Color32| {
                ui.label(egui::RichText::new(k).size(13.0).color(DIM));
                ui.label(egui::RichText::new(v).size(15.0).color(c));
                ui.end_row();
            };
            egui::Grid::new("exposure").num_columns(2).spacing([12.0, 3.0]).show(ui, |ui| {
                let c = if e.tracked_s > 0.0 { AMBER } else { GREEN };
                row(ui, "TRACKED", clock(e.tracked_s as i32), c);
                row(ui, "CLOSEST", format!("{:+}", e.peak_margin), margin_colour(e.peak_margin));
                if let Some((i, t)) = e.worst
                    && let Some(site) = rec.sites.get(i)
                {
                    row(ui, "WORST", format!("{} {}", site.kind, clock(t as i32)), AMBER);
                    ui.label("");
                    ui.label(egui::RichText::new(clean_text(&site.place)).small().color(DIM));
                    ui.end_row();
                }
            });
            section(ui, &format!("At {}", clock((cursor / crate::recorder::FS) as i32)));
            if let Some(s) = here {
                egui::Grid::new("at").num_columns(2).spacing([12.0, 3.0]).show(ui, |ui| {
                    row(ui, "ALTITUDE", format!("{} ft", s.alt), GREEN);
                    row(ui, "SPEED", format!("{} kt", s.knots), GREEN);
                    row(ui, "EMV", s.emv.to_string(), GREEN);
                    row(ui, "MARGIN", format!("{:+}", s.margin), margin_colour(s.margin));
                    if let Some(site) = s.site.and_then(|i| rec.sites.get(i)) {
                        row(ui, "STRONGEST", site.kind.clone(), DIM);
                    }
                });
                let tracking: Vec<&str> = shown
                    .iter()
                    .filter(|&&i| now(i))
                    .filter_map(|&i| rec.sites.get(i).map(|s| s.kind.as_str()))
                    .collect();
                if !tracking.is_empty() {
                    ui.label(
                        egui::RichText::new(format!("TRACKED BY {}", tracking.join(", ")))
                            .color(MISSILE_RED),
                    );
                }
            }
            section(ui, "Map");
            map_draw::legend(ui, "Not detectable", |p, c| {
                p.line_segment(
                    [c - egui::vec2(7.0, 0.0), c + egui::vec2(7.0, 0.0)],
                    egui::Stroke::new(2.5, GREEN),
                );
            });
            map_draw::legend(ui, "Close to detection", |p, c| {
                p.line_segment(
                    [c - egui::vec2(7.0, 0.0), c + egui::vec2(7.0, 0.0)],
                    egui::Stroke::new(2.5, AMBER),
                );
            });
            map_draw::legend(ui, "Detectable", |p, c| {
                p.line_segment(
                    [c - egui::vec2(7.0, 0.0), c + egui::vec2(7.0, 0.0)],
                    egui::Stroke::new(2.5, MISSILE_RED),
                );
            });
            map_draw::legend(ui, "Tracked / missile launch", |p, c| {
                mark(p, c - egui::vec2(4.0, 0.0), MarkKind::Tracked(0));
                mark(p, c + egui::vec2(5.0, 0.0), MarkKind::Launch { site: None, spec: 0 });
            });
            map_draw::legend(ui, "Hit taken / kill", |p, c| {
                mark(p, c - egui::vec2(4.0, 0.0), MarkKind::Hit);
                mark(p, c + egui::vec2(5.0, 0.0), MarkKind::AirKill { spec: 0 });
            });
        });
    });
    ui.horizontal(|ui| {
        let label = if map.playing { "PAUSE" } else { "PLAY" };
        if ui.add(big_button(label)).clicked() {
            if map.frame >= last as f32 {
                map.frame = 0.0;
            }
            map.playing = !map.playing;
        }
        ui.spacing_mut().slider_width = ui.available_width() - 80.0;
        let slider = egui::Slider::new(&mut map.frame, 0.0..=last.max(1) as f32)
            .show_value(false)
            .clamping(egui::SliderClamping::Always);
        if ui.add(slider).dragged() {
            map.playing = false;
        }
        ui.label(egui::RichText::new(clock((map.frame / fs) as i32)).color(BRIGHT));
    });
}

/// An event symbol on the flight map.
fn mark(painter: &egui::Painter, p: egui::Pos2, kind: MarkKind) {
    let stroke = |c| egui::Stroke::new(1.8, c);
    match kind {
        MarkKind::Tracked(_) => {
            let pts = vec![
                p + egui::vec2(0.0, -4.0),
                p + egui::vec2(4.0, 0.0),
                p + egui::vec2(0.0, 4.0),
                p + egui::vec2(-4.0, 0.0),
            ];
            painter.add(egui::Shape::closed_line(pts, stroke(AMBER)));
        }
        MarkKind::Launch { .. } => {
            let pts = vec![
                p + egui::vec2(0.0, -5.0),
                p + egui::vec2(4.5, 3.5),
                p + egui::vec2(-4.5, 3.5),
            ];
            painter.add(egui::Shape::convex_polygon(pts, MISSILE_RED, egui::Stroke::NONE));
        }
        MarkKind::Hit => {
            painter.line_segment(
                [p + egui::vec2(-4.0, -4.0), p + egui::vec2(4.0, 4.0)],
                stroke(MISSILE_RED),
            );
            painter.line_segment(
                [p + egui::vec2(-4.0, 4.0), p + egui::vec2(4.0, -4.0)],
                stroke(MISSILE_RED),
            );
        }
        MarkKind::AirKill { .. } | MarkKind::GroundKill { .. } | MarkKind::AaaKill(_) => {
            painter
                .line_segment([p + egui::vec2(-5.0, 0.0), p + egui::vec2(5.0, 0.0)], stroke(GREEN));
            painter
                .line_segment([p + egui::vec2(0.0, -5.0), p + egui::vec2(0.0, 5.0)], stroke(GREEN));
        }
        MarkKind::Photo => {
            painter.circle_stroke(p, 4.0, stroke(FRIENDLY_BLUE));
        }
        MarkKind::Objective { .. } => {
            painter.circle_stroke(p, 6.0, stroke(egui::Color32::from_rgb(255, 255, 80)));
        }
        MarkKind::Ejected => {
            painter.circle_filled(p, 4.0, BRIGHT);
        }
    }
}

/// The player's aircraft at the scrubber: a triangle along the heading.
fn aircraft(painter: &egui::Painter, view: &MapView, s: &Sample) {
    let (x, y) = s.map();
    let p = view.to_screen(x, y);
    let a = s.heading as u16 as f32 / 65536.0 * std::f32::consts::TAU;
    // Heading 0 north (up on the map), clockwise.
    let fwd = egui::vec2(a.sin(), -a.cos());
    let right = egui::vec2(-fwd.y, fwd.x);
    let pts = vec![p + fwd * 9.0, p - fwd * 6.0 + right * 6.0, p - fwd * 6.0 - right * 6.0];
    painter.add(egui::Shape::convex_polygon(
        pts,
        BRIGHT,
        egui::Stroke::new(1.0, egui::Color32::BLACK),
    ));
}
