//! Input mapping dialog (opened from the main menu): binds joystick / HOTAS axes and buttons to
//! the flight controls and every action, and saves the mapping as JSON.

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;
use bevy_egui::egui;

use crate::hotas::{AxisControl, ButtonTarget, Hotas, InputConfig, Source, action_list, action_name};
use crate::menu::{AMBER, BRIGHT, DIM, GREEN};

#[derive(Resource, Default)]
pub struct InputDialog {
    pub open: bool,
    capture: Option<Capture>,
    status: Option<String>,
    /// What is on disk, to save only real changes.
    saved: Option<InputConfig>,
}

#[derive(Clone, Copy, PartialEq)]
enum Binding {
    Axis(AxisControl),
    Button(ButtonTarget),
}

impl Binding {
    fn name(self) -> String {
        match self {
            Binding::Axis(a) => a.name().to_string(),
            Binding::Button(ButtonTarget::Gun) => "Gun".to_string(),
            Binding::Button(ButtonTarget::Action(a)) => action_name(a),
        }
    }
}

/// A device input that moved during a capture.
enum Found {
    Axis(u32),
    Button(Source),
}

/// Waiting for a device input to bind; the state when it started is the baseline, so axes
/// resting off centre (throttles) and buttons already held don't bind.
struct Capture {
    what: Binding,
    axes: HashMap<(String, u32), f32>,
    buttons: HashSet<(String, u32)>,
}

impl Capture {
    fn new(what: Binding, hotas: &Hotas) -> Self {
        let mut c = Self { what, axes: HashMap::new(), buttons: HashSet::new() };
        for d in hotas.devices() {
            c.axes.extend(d.axes.iter().map(|(code, v)| ((d.name.clone(), *code), *v)));
            c.buttons.extend(d.buttons.iter().map(|code| (d.name.clone(), *code)));
        }
        c
    }

    /// The first input that moved since the capture started.
    fn poll(&mut self, hotas: &Hotas) -> Option<(String, Found)> {
        let mut found = None;
        for d in hotas.devices() {
            // Buttons held at the start count again once released.
            self.buttons.retain(|(n, c)| *n != d.name || d.buttons.contains(c));
            for &code in &d.buttons {
                if matches!(self.what, Binding::Button(_)) && !self.buttons.contains(&(d.name.clone(), code)) {
                    found = found.or(Some((d.name.clone(), Found::Button(Source::Button(code)))));
                }
            }
            for (&code, &v) in &d.axes {
                // An axis seen for the first time is its own baseline.
                let base = *self.axes.entry((d.name.clone(), code)).or_insert(v);
                if (v - base).abs() > 0.5 {
                    let input = match self.what {
                        Binding::Axis(_) => Found::Axis(code),
                        Binding::Button(_) if v > 0.5 => Found::Button(Source::AxisPositive(code)),
                        Binding::Button(_) if v < -0.5 => Found::Button(Source::AxisNegative(code)),
                        Binding::Button(_) => continue,
                    };
                    found = found.or(Some((d.name.clone(), input)));
                }
            }
        }
        found
    }
}

fn binding_label(device: &str, source: &str) -> String {
    let device: String = device.chars().take(28).collect();
    format!("{device}: {source}")
}

/// Draws the dialog when open (called from the menu's egui pass).
pub fn show(ctx: &egui::Context, dialog: &mut InputDialog, config: &mut InputConfig, hotas: &Hotas) {
    if !dialog.open {
        return;
    }
    if dialog.saved.is_none() {
        dialog.saved = Some(config.clone());
    }
    if let Some(cap) = &mut dialog.capture {
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            dialog.capture = None;
            dialog.status = None;
        } else if let Some((device, input)) = cap.poll(hotas) {
            let what = cap.what;
            let label = match (what, input) {
                (Binding::Axis(control), Found::Axis(code)) => {
                    config.bind_axis(control, device.clone(), code);
                    format!("Axis {}", code & 0xffff)
                }
                (Binding::Button(target), Found::Button(source)) => {
                    config.bind_button(target, device.clone(), source);
                    source.label()
                }
                _ => unreachable!("capture only reports inputs of its binding's kind"),
            };
            dialog.status = Some(format!("{} -> {}", what.name().to_ascii_uppercase(), binding_label(&device, &label)));
            dialog.capture = None;
        }
        // Keep polling while nothing else redraws the window.
        ctx.request_repaint();
    }

    let screen = ctx.content_rect();
    let mut open = true;
    let mut start: Option<Binding> = None;
    egui::Window::new("INPUT MAPPING")
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .collapsible(false)
        .resizable(false)
        .title_bar(false)
        .fixed_size(egui::vec2((screen.width() - 80.0).clamp(560.0, 980.0), (screen.height() - 120.0).max(300.0)))
        .frame(egui::Frame::window(&ctx.global_style()).inner_margin(16.0))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("INPUT MAPPING").heading().color(BRIGHT));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("DONE").clicked() {
                        open = false;
                    }
                    if ui.button("CLEAR ALL").clicked() {
                        config.axes.clear();
                        config.buttons.clear();
                    }
                });
            });
            ui.separator();
            let devices = hotas.devices();
            if devices.is_empty() {
                ui.colored_label(AMBER, "NO JOYSTICKS DETECTED  (connect a device; it shows up here)");
            } else {
                for d in &devices {
                    ui.label(egui::RichText::new(format!("> {}", d.name)).color(GREEN));
                }
            }
            ui.checkbox(&mut config.gamepad_defaults, "Standard gamepad layout")
                .on_hover_text("Left stick, right stick rudder, triggers throttle, face buttons fire / gun / weapons / bay");
            match &dialog.status {
                Some(text) => ui.colored_label(AMBER, text),
                None => ui.label(egui::RichText::new("Click BIND, then move an axis or press a button. Esc cancels.").color(DIM)),
            };
            ui.separator();

            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.label(egui::RichText::new("[ AXES ]").color(AMBER).size(14.0));
                egui::Grid::new("axes").num_columns(6).spacing([14.0, 6.0]).striped(true).show(ui, |ui| {
                    for control in AxisControl::ALL {
                        let what = Binding::Axis(control);
                        ui.label(control.name());
                        let capturing = dialog.capture.as_ref().is_some_and(|c| c.what == what);
                        let mapping = config.axes.iter_mut().find(|m| m.control == control);
                        match mapping {
                            Some(m) => {
                                ui.label(binding_label(&m.device, &format!("Axis {}", m.code & 0xffff)));
                                ui.checkbox(&mut m.invert, "Invert");
                                if control == AxisControl::Throttle {
                                    ui.label("");
                                } else {
                                    ui.add(egui::Slider::new(&mut m.deadzone, 0.0..=0.5).text("dead zone").fixed_decimals(2));
                                }
                                let v = hotas.axis(&m.device, m.code).map(|raw| m.output(raw));
                                let fill = match (control, v) {
                                    (_, None) => 0.0,
                                    (AxisControl::Throttle, Some(v)) => v,
                                    (_, Some(v)) => (v + 1.0) / 2.0,
                                };
                                let text = v.map_or("move it".to_string(), |v| format!("{v:+.2}"));
                                ui.add(egui::ProgressBar::new(fill).desired_width(120.0).text(text));
                            }
                            None => {
                                ui.label(egui::RichText::new(if capturing { "MOVE AN AXIS..." } else { "-" }).color(DIM));
                                ui.label("");
                                ui.label("");
                                ui.label("");
                            }
                        }
                        row_buttons(ui, what, capturing, &mut start, || config.axes.retain(|m| m.control != control));
                        ui.end_row();
                    }
                });
                ui.add_space(8.0);
                ui.label(egui::RichText::new("[ BUTTONS ]").color(AMBER).size(14.0));
                egui::Grid::new("buttons").num_columns(4).spacing([14.0, 4.0]).striped(true).show(ui, |ui| {
                    let targets = std::iter::once((ButtonTarget::Gun, "Backspace (held)".to_string()))
                        .chain(action_list().into_iter().map(|(a, keys)| (ButtonTarget::Action(a), keys)));
                    for (target, keys) in targets {
                        let what = Binding::Button(target);
                        ui.label(what.name());
                        ui.label(egui::RichText::new(keys).color(DIM).small());
                        let capturing = dialog.capture.as_ref().is_some_and(|c| c.what == what);
                        match config.button(target) {
                            Some(m) => ui.label(binding_label(&m.device, &m.source.label())),
                            None => ui.label(egui::RichText::new(if capturing { "PRESS A BUTTON..." } else { "-" }).color(DIM)),
                        };
                        row_buttons(ui, what, capturing, &mut start, || config.buttons.retain(|m| m.target != target));
                        ui.end_row();
                    }
                });
            });
        });

    if let Some(what) = start {
        dialog.status = None;
        dialog.capture = Some(Capture::new(what, hotas));
    }
    if dialog.saved.as_ref() != Some(config) {
        match config.save() {
            Ok(_) => dialog.saved = Some(config.clone()),
            Err(e) => {
                dialog.status = Some(format!("could not save the input mapping: {e}"));
                dialog.saved = Some(config.clone());
            }
        }
    }
    if !open {
        dialog.open = false;
        dialog.capture = None;
        dialog.status = None;
    }
}

fn row_buttons(ui: &mut egui::Ui, what: Binding, capturing: bool, start: &mut Option<Binding>, clear: impl FnOnce()) {
    ui.horizontal(|ui| {
        let bind = egui::Button::new(if capturing { "WAITING" } else { "BIND" }).selected(capturing);
        if ui.add(bind).clicked() && !capturing {
            *start = Some(what);
        }
        if ui.button("CLEAR").clicked() {
            clear();
        }
    });
}
