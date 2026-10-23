//! Input mapping dialog (opened from the main menu): binds keyboard keys and joystick / HOTAS
//! axes and buttons to the flight controls and every action, picks what the VR controller buttons do, and saves the
//! mapping as JSON.

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;
use bevy_egui::egui;

use crate::hotas::{
    AxisControl, ButtonTarget, Hotas, InputConfig, Source, VrHand, VrInput, action_name,
    default_vr_buttons,
};
use crate::input::{Mods, all_actions, default_key_mappings};
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
    Key(ButtonTarget),
}

impl Binding {
    fn name(self) -> String {
        match self {
            Binding::Axis(a) => a.name().to_string(),
            Binding::Button(ButtonTarget::Gun) | Binding::Key(ButtonTarget::Gun) => {
                "Gun".to_string()
            }
            Binding::Button(ButtonTarget::Action(a)) | Binding::Key(ButtonTarget::Action(a)) => {
                action_name(a)
            }
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
            c.axes.extend(d.axes.iter().map(|(code, v)| ((d.key.clone(), *code), *v)));
            c.buttons.extend(d.buttons.iter().map(|code| (d.key.clone(), *code)));
        }
        c
    }

    /// The first input that moved since the capture started.
    fn poll(&mut self, hotas: &Hotas) -> Option<(String, Found)> {
        let mut found = None;
        for d in hotas.devices() {
            // Buttons held at the start count again once released.
            self.buttons.retain(|(n, c)| *n != d.key || d.buttons.contains(c));
            for &code in &d.buttons {
                if matches!(self.what, Binding::Button(_))
                    && !self.buttons.contains(&(d.key.clone(), code))
                {
                    found = found.or(Some((d.key.clone(), Found::Button(Source::Button(code)))));
                }
            }
            for (&code, &v) in &d.axes {
                // An axis seen for the first time is its own baseline.
                let base = *self.axes.entry((d.key.clone(), code)).or_insert(v);
                if (v - base).abs() > 0.5 {
                    let input = match self.what {
                        Binding::Axis(_) => Found::Axis(code),
                        Binding::Button(_) if v > 0.5 => Found::Button(Source::AxisPositive(code)),
                        Binding::Button(_) if v < -0.5 => Found::Button(Source::AxisNegative(code)),
                        Binding::Button(_) => continue,
                        Binding::Key(_) => continue,
                    };
                    found = found.or(Some((d.key.clone(), input)));
                }
            }
        }
        found
    }
}

/// "Device: Axis 1", the device name shortened but its ids and number kept.
fn binding_label(device: &str, source: &str) -> String {
    let (name, ids) = device.rfind(" [").map_or((device, ""), |i| device.split_at(i));
    let name: String = name.chars().take(28usize.saturating_sub(ids.len()).max(12)).collect();
    format!("{name}{ids}: {source}")
}

/// Modifier keys, which only count together with another key.
const MODIFIERS: [KeyCode; 8] = [
    KeyCode::ShiftLeft,
    KeyCode::ShiftRight,
    KeyCode::AltLeft,
    KeyCode::AltRight,
    KeyCode::ControlLeft,
    KeyCode::ControlRight,
    KeyCode::SuperLeft,
    KeyCode::SuperRight,
];

/// The first key pressed this frame (other than a modifier or Esc), with the modifier held.
fn pressed_key(keys: &ButtonInput<KeyCode>) -> Option<(Mods, KeyCode)> {
    let key = keys
        .get_just_pressed()
        .copied()
        .find(|k| !MODIFIERS.contains(k) && *k != KeyCode::Escape)?;
    let mods = if keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]) {
        Mods::Alt
    } else if keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) {
        Mods::Shift
    } else {
        Mods::None
    };
    Some((mods, key))
}

/// Draws the dialog when open (called from the menu's egui pass).
pub fn show(
    ctx: &egui::Context,
    dialog: &mut InputDialog,
    config: &mut InputConfig,
    hotas: &Hotas,
    keys: &ButtonInput<KeyCode>,
) {
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
        } else if let Binding::Key(target) = cap.what {
            if let Some((mods, key)) = pressed_key(keys) {
                config.bind_key(target, mods, key);
                dialog.status = Some(format!(
                    "{} -> {}",
                    cap.what.name().to_ascii_uppercase(),
                    crate::input::key_label(mods, key)
                ));
                dialog.capture = None;
            }
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
            dialog.status = Some(format!(
                "{} -> {}",
                what.name().to_ascii_uppercase(),
                binding_label(&device, &label)
            ));
            dialog.capture = None;
        }
        // Keep polling while nothing else redraws the window.
        ctx.request_repaint();
    }

    let value = |control| {
        config
            .axes
            .iter()
            .find(|m| m.control == control)
            .and_then(|m| hotas.axis(&m.device, m.code).map(|raw| m.output(raw)))
    };
    let stick = Stick {
        roll: value(AxisControl::Roll).unwrap_or(0.0),
        pitch: value(AxisControl::Pitch).unwrap_or(0.0),
        yaw: value(AxisControl::Yaw).unwrap_or(0.0),
        throttle: value(AxisControl::Throttle),
    };
    // The preview follows the stick without other repaint triggers.
    ctx.request_repaint();

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
                    if ui.button("DEFAULT KEYS").clicked() {
                        config.keys = default_key_mappings();
                    }
                });
            });
            ui.separator();
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width((ui.available_width() - PREVIEW.x - 16.0).max(200.0));
                    let devices = hotas.devices();
                    if devices.is_empty() {
                        ui.colored_label(AMBER, "NO JOYSTICKS DETECTED  (connect a device; it shows up here)");
                    } else {
                        for d in &devices {
                            ui.label(egui::RichText::new(format!("> {}", d.key)).color(GREEN));
                        }
                    }
                    ui.checkbox(&mut config.gamepad_defaults, "Standard gamepad layout")
                        .on_hover_text("Left stick, right stick rudder, triggers throttle, face buttons fire / gun / weapons / bay");
                    match &dialog.status {
                        Some(text) => ui.colored_label(AMBER, text),
                        None => ui.label(egui::RichText::new("Click BIND, then move an axis or press a button; + adds a key. Esc cancels.").color(DIM)),
                    };
                    ui.label(
                        egui::RichText::new("The aircraft on the right shows what the mapped axes do: invert an axis if it moves the wrong way.")
                            .color(DIM)
                            .small(),
                    );
                });
                preview(ui, stick);
            });
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
                egui::Grid::new("buttons").num_columns(5).spacing([14.0, 4.0]).striped(true).show(ui, |ui| {
                    ui.label("");
                    ui.label(egui::RichText::new("KEYBOARD").color(GREEN));
                    ui.label("");
                    ui.label(egui::RichText::new("JOYSTICK").color(GREEN));
                    ui.end_row();
                    let targets = std::iter::once(ButtonTarget::Gun).chain(all_actions().into_iter().map(ButtonTarget::Action));
                    for target in targets {
                        let key = Binding::Key(target);
                        ui.label(key.name());
                        let capturing_key = dialog.capture.as_ref().is_some_and(|c| c.what == key);
                        let labels: Vec<String> = config.keys_for(target).map(|m| m.label()).collect();
                        let text = match (capturing_key, labels.is_empty()) {
                            (true, _) => "PRESS A KEY...".to_string(),
                            (false, true) => "-".to_string(),
                            (false, false) => labels.join(", "),
                        };
                        ui.label(egui::RichText::new(text).color(if labels.is_empty() || capturing_key { DIM } else { BRIGHT }));
                        ui.horizontal(|ui| {
                            let add = egui::Button::new(if capturing_key { "WAITING" } else { "+" }).selected(capturing_key);
                            if ui.add(add).on_hover_text("Add a key (it is taken from any other action)").clicked() && !capturing_key {
                                start = Some(key);
                            }
                            if ui.button("CLEAR").clicked() {
                                config.keys.retain(|m| m.target != target);
                            }
                        });
                        let what = Binding::Button(target);
                        let capturing = dialog.capture.as_ref().is_some_and(|c| c.what == what);
                        match config.button(target) {
                            Some(m) => ui.label(binding_label(&m.device, &m.source.label())),
                            None => ui.label(egui::RichText::new(if capturing { "PRESS A BUTTON..." } else { "-" }).color(DIM)),
                        };
                        row_buttons(ui, what, capturing, &mut start, || config.buttons.retain(|m| m.target != target));
                        ui.end_row();
                    }
                });
                ui.add_space(8.0);
                vr_buttons(ui, config);
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

fn target_name(t: Option<ButtonTarget>) -> String {
    match t {
        None => "-".to_string(),
        Some(t) => Binding::Button(t).name(),
    }
}

/// VR controller buttons: a target picked per hand and input (no capture: the controllers are
/// usually not awake at the desktop menu).
fn vr_buttons(ui: &mut egui::Ui, config: &mut InputConfig) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("[ VR CONTROLLERS ]").color(AMBER).size(14.0));
        if ui.button("DEFAULTS").clicked() {
            config.vr_buttons = default_vr_buttons();
        }
    });
    ui.label(
        egui::RichText::new("A trigger only counts while its hand holds the throttle (left) or stick (right); otherwise it presses the control under the fingertip.")
            .color(DIM)
            .small(),
    );
    // The stick keys are for the keyboard and joystick buttons: VR has the real stick.
    let targets: Vec<ButtonTarget> = std::iter::once(ButtonTarget::Gun)
        .chain(
            all_actions()
                .into_iter()
                .filter(|a| a.stick_direction().is_none())
                .map(ButtonTarget::Action),
        )
        .collect();
    egui::Grid::new("vr_buttons").num_columns(3).spacing([14.0, 4.0]).striped(true).show(
        ui,
        |ui| {
            ui.label("");
            ui.label(egui::RichText::new("LEFT HAND").color(GREEN));
            ui.label(egui::RichText::new("RIGHT HAND").color(GREEN));
            ui.end_row();
            for input in VrInput::ALL {
                let (l, r) = (input.name(VrHand::Left), input.name(VrHand::Right));
                ui.label(if l == r { l.to_string() } else { format!("{l} / {r}") });
                for hand in [VrHand::Left, VrHand::Right] {
                    let current = config.vr_button(hand, input);
                    let mut picked = current;
                    egui::ComboBox::from_id_salt(("vr", hand, input))
                        .width(190.0)
                        .selected_text(target_name(current))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut picked, None, "-");
                            for &t in &targets {
                                ui.selectable_value(&mut picked, Some(t), target_name(Some(t)));
                            }
                        });
                    if picked != current {
                        config.bind_vr(hand, input, picked);
                    }
                }
                ui.end_row();
            }
        },
    );
}

fn row_buttons(
    ui: &mut egui::Ui,
    what: Binding,
    capturing: bool,
    start: &mut Option<Binding>,
    clear: impl FnOnce(),
) {
    ui.horizontal(|ui| {
        let bind =
            egui::Button::new(if capturing { "WAITING" } else { "BIND" }).selected(capturing);
        if ui.add(bind).clicked() && !capturing {
            *start = Some(what);
        }
        if ui.button("CLEAR").clicked() {
            clear();
        }
    });
}

/// Mapped axis outputs, for the preview.
#[derive(Clone, Copy)]
struct Stick {
    roll: f32,
    pitch: f32,
    yaw: f32,
    /// None: no throttle axis (or it hasn't moved yet).
    throttle: Option<f32>,
}

const PREVIEW: egui::Vec2 = egui::vec2(300.0, 190.0);

/// Planform outline (x right, z forward), clockwise from the nose, and the faceted ridge above it.
const OUTLINE: [(f32, f32); 8] = [
    (0.0, 3.0),
    (2.6, -1.6),
    (1.0, -1.0),
    (0.55, -2.1),
    (0.0, -1.7),
    (-0.55, -2.1),
    (-1.0, -1.0),
    (-2.6, -1.6),
];
const RIDGE: Vec3 = Vec3::new(0.0, 0.4, -0.4);
/// V tails (root front, root rear, tip).
const TAILS: [[Vec3; 3]; 2] = [
    [Vec3::new(0.35, 0.15, -1.2), Vec3::new(0.55, 0.15, -1.95), Vec3::new(1.15, 0.95, -2.25)],
    [Vec3::new(-0.35, 0.15, -1.2), Vec3::new(-0.55, 0.15, -1.95), Vec3::new(-1.15, 0.95, -2.25)],
];
/// Exhaust slot centres.
const SLOTS: [Vec3; 2] = [Vec3::new(0.32, 0.12, -1.8), Vec3::new(-0.32, 0.12, -1.8)];

/// Exhaust colour: dull red at idle through orange and yellow to blue-white at full thrust.
fn exhaust_color(t: f32) -> egui::Color32 {
    const STOPS: [(f32, [f32; 3]); 4] = [
        (0.0, [110.0, 25.0, 10.0]),
        (0.4, [235.0, 95.0, 20.0]),
        (0.75, [255.0, 205.0, 80.0]),
        (1.0, [200.0, 225.0, 255.0]),
    ];
    let t = t.clamp(0.0, 1.0);
    let i = STOPS.iter().rposition(|s| s.0 <= t).unwrap_or(0).min(STOPS.len() - 2);
    let (a, b) = (STOPS[i], STOPS[i + 1]);
    let f = (t - a.0) / (b.0 - a.0);
    let c: Vec<u8> = (0..3).map(|k| (a.1[k] + (b.1[k] - a.1[k]) * f) as u8).collect();
    egui::Color32::from_rgb(c[0], c[1], c[2])
}

/// A small F-117 seen from behind and above, deflected by the stick, rudder and throttle.
fn preview(ui: &mut egui::Ui, s: Stick) {
    let (rect, _) = ui.allocate_exact_size(PREVIEW, egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_stroke(rect, 0.0, egui::Stroke::new(1.0, DIM), egui::StrokeKind::Inside);
    // Stick right banks right, pull pitches up, right rudder yaws right.
    let attitude = Quat::from_rotation_y(s.yaw * 0.35)
        * Quat::from_rotation_x(-s.pitch * 0.35)
        * Quat::from_rotation_z(-s.roll * 0.6);
    let view = Quat::from_rotation_x(-0.32);
    let centre = rect.center() + egui::vec2(0.0, 6.0);
    let project = |p: Vec3| {
        let v = view * p;
        let depth = v.z + 13.0;
        centre + egui::vec2(v.x, -v.y) * (380.0 / depth)
    };
    let depth = |p: Vec3| (view * p).z;

    // Fixed ground grid as the reference.
    let grid = egui::Stroke::new(1.0, egui::Color32::from_rgb(14, 50, 24));
    for i in -4..=4 {
        let x = i as f32 * 1.5;
        painter.line_segment(
            [project(Vec3::new(x, -2.0, -4.0)), project(Vec3::new(x, -2.0, 8.0))],
            grid,
        );
    }
    for i in -2..=5 {
        let z = i as f32 * 1.5;
        painter.line_segment(
            [project(Vec3::new(-6.0, -2.0, z)), project(Vec3::new(6.0, -2.0, z))],
            grid,
        );
    }

    // Facets: the ridge fan over the outline, the tails; far ones first.
    let outline: Vec<Vec3> = OUTLINE.iter().map(|&(x, z)| Vec3::new(x, 0.0, z)).collect();
    let mut faces: Vec<[Vec3; 3]> =
        (0..outline.len()).map(|i| [RIDGE, outline[i], outline[(i + 1) % outline.len()]]).collect();
    faces.extend(TAILS);
    let mut faces: Vec<[Vec3; 3]> = faces.into_iter().map(|f| f.map(|p| attitude * p)).collect();
    faces.sort_by(|a, b| {
        let d = |f: &[Vec3; 3]| f.iter().map(|p| depth(*p)).sum::<f32>();
        d(b).total_cmp(&d(a))
    });
    let light = Vec3::new(0.3, 1.0, -0.4).normalize();
    for f in faces {
        let n = (f[1] - f[0]).cross(f[2] - f[0]).normalize_or_zero();
        let lit = n.dot(light).abs();
        let g = (20.0 + 70.0 * lit) as u8;
        let fill = egui::Color32::from_rgb(g / 4, g, g / 3);
        painter.add(egui::Shape::convex_polygon(
            f.iter().map(|p| project(*p)).collect(),
            fill,
            egui::Stroke::new(1.0, GREEN),
        ));
    }

    // Exhaust flames behind the slots: colour and length from the throttle.
    let t = s.throttle.unwrap_or(0.0);
    let color = exhaust_color(t);
    let len = 0.6 + 2.2 * t;
    let glow = egui::Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 70);
    for slot in SLOTS {
        let wide = [
            slot + Vec3::new(-0.38, 0.0, 0.05),
            slot + Vec3::new(0.38, 0.0, 0.05),
            slot + Vec3::new(0.0, 0.0, -len * 1.3),
        ];
        let flame = [
            slot + Vec3::new(-0.25, 0.0, 0.0),
            slot + Vec3::new(0.25, 0.0, 0.0),
            slot + Vec3::new(0.0, 0.0, -len),
        ];
        painter.circle_filled(project(attitude * slot), 4.0 + 8.0 * t, glow);
        painter.add(egui::Shape::convex_polygon(
            wide.iter().map(|p| project(attitude * *p)).collect(),
            glow,
            egui::Stroke::NONE,
        ));
        painter.add(egui::Shape::convex_polygon(
            flame.iter().map(|p| project(attitude * *p)).collect(),
            color,
            egui::Stroke::NONE,
        ));
    }

    // Readouts.
    let dir = |v: f32, neg: &str, pos: &str| match v {
        v if v > 0.02 => format!("{pos} {:.0}%", v * 100.0),
        v if v < -0.02 => format!("{neg} {:.0}%", -v * 100.0),
        _ => "CENTRE".to_string(),
    };
    let font = egui::FontId::monospace(11.0);
    let lines = [
        format!("ROLL   {}", dir(s.roll, "LEFT", "RIGHT")),
        format!("PITCH  {}", dir(s.pitch, "DOWN", "UP")),
        format!("RUDDER {}", dir(s.yaw, "LEFT", "RIGHT")),
    ];
    for (i, l) in lines.iter().enumerate() {
        painter.text(
            rect.left_top() + egui::vec2(6.0, 5.0 + i as f32 * 13.0),
            egui::Align2::LEFT_TOP,
            l,
            font.clone(),
            GREEN,
        );
    }
    let thr =
        s.throttle.map_or("THROTTLE -".to_string(), |t| format!("THROTTLE {:.0}%", t * 100.0));
    painter.text(
        rect.right_bottom() + egui::vec2(-6.0, -5.0),
        egui::Align2::RIGHT_BOTTOM,
        thr,
        font,
        if s.throttle.is_some() { color } else { DIM },
    );
}
