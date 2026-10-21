//! A single-line text field in the menu's terminal look: pilot names (career, multiplayer)
//! and server addresses. In VR, where there is no keyboard at hand, a virtual keyboard in the
//! same look opens under the field when it is clicked; its keys are pressed with the laser
//! pointer and edit the text directly.

use bevy_egui::egui;

use crate::menu::{AMBER, BG, BRIGHT, DIM, GREEN};

/// What a field takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Charset {
    /// Letters, digits, spaces, `-`, `_`, `.`, `'`.
    Name,
    /// Host names, IPv4 and IPv6 addresses with a port: letters, digits, `.`, `-`, `:`,
    /// `[`, `]`.
    Address,
}

impl Charset {
    fn allows(self, c: char) -> bool {
        match self {
            Charset::Name => c.is_alphanumeric() || " -_.'".contains(c),
            Charset::Address => c.is_ascii_alphanumeric() || ".-:[]".contains(c),
        }
    }
}

/// The virtual keyboard's state (egui memory, per field).
#[derive(Debug, Clone, Copy, Default)]
struct Keyboard {
    open: bool,
    shift: bool,
}

const ROWS: [&str; 4] = ["1234567890", "QWERTYUIOP", "ASDFGHJKL'", "ZXCVBNM.-_"];
const ADDRESS_ROW: &str = ":[]";
const KEY: egui::Vec2 = egui::vec2(46.0, 42.0);
const KEY_FILL: egui::Color32 = egui::Color32::from_rgb(8, 28, 13);

fn key(ui: &mut egui::Ui, label: &str, width: f32, lit: bool) -> bool {
    key_enabled(ui, label, width, lit, true)
}

/// A key; one the field does not take is greyed out.
fn key_enabled(ui: &mut egui::Ui, label: &str, width: f32, lit: bool, enabled: bool) -> bool {
    let text =
        egui::RichText::new(label).monospace().size(18.0).color(if lit { BG } else { GREEN });
    ui.add_enabled(
        enabled,
        egui::Button::new(text)
            .fill(if lit { GREEN } else { KEY_FILL })
            .stroke(egui::Stroke::new(1.0, DIM))
            .min_size(egui::vec2(width, KEY.y)),
    )
    .clicked()
}

/// The field (and in VR its keyboard). `max`: characters at most.
pub fn text_entry(
    ui: &mut egui::Ui,
    id: &str,
    text: &mut String,
    max: usize,
    charset: Charset,
    vr: bool,
    hint: &str,
) -> egui::Response {
    let kid = egui::Id::new(("text_entry_keyboard", id));
    // Test aid: `--open-keyboard` starts with the keyboards open (screenshots).
    let mut kb: Keyboard = ui.data(|d| d.get_temp(kid)).unwrap_or(Keyboard {
        open: std::env::args().any(|a| a == "--open-keyboard"),
        shift: false,
    });
    let field = egui::TextEdit::singleline(text)
        .id(egui::Id::new(("text_entry", id)))
        .char_limit(max)
        .hint_text(egui::RichText::new(hint).color(DIM))
        .text_color(BRIGHT)
        .font(egui::TextStyle::Monospace)
        .desired_width(KEY.x * 10.0 + 40.0);
    let resp = ui.add(field);
    // Typed on a keyboard: only what the field takes.
    if resp.changed() {
        text.retain(|c| charset.allows(c));
    }
    if vr && resp.clicked() {
        kb.open = true;
    }
    if vr && kb.open {
        egui::Frame::new().fill(BG).stroke(egui::Stroke::new(1.0, DIM)).inner_margin(8.0).show(
            ui,
            |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                for (r, row) in ROWS.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.add_space(r as f32 * 10.0);
                        for c in row.chars() {
                            let c = if kb.shift { c } else { c.to_ascii_lowercase() };
                            if key_enabled(ui, &c.to_string(), KEY.x, false, charset.allows(c))
                                && text.chars().count() < max
                            {
                                text.push(c);
                                kb.shift = false;
                            }
                        }
                        if r == 3 && charset == Charset::Address {
                            for c in ADDRESS_ROW.chars() {
                                if key(ui, &c.to_string(), KEY.x, false)
                                    && text.chars().count() < max
                                {
                                    text.push(c);
                                }
                            }
                        }
                    });
                }
                ui.horizontal(|ui| {
                    if key(ui, "SHIFT", KEY.x * 2.0, kb.shift) {
                        kb.shift = !kb.shift;
                    }
                    if key(ui, "SPACE", KEY.x * 4.5, false)
                        && charset.allows(' ')
                        && text.chars().count() < max
                    {
                        text.push(' ');
                    }
                    if key(ui, "DEL", KEY.x * 1.5, false) {
                        text.pop();
                    }
                    if key(ui, "CLEAR", KEY.x * 1.5, false) {
                        text.clear();
                    }
                    let done = egui::Button::new(
                        egui::RichText::new("DONE").monospace().size(18.0).color(BG),
                    )
                    .fill(AMBER)
                    .min_size(egui::vec2(KEY.x * 2.0, KEY.y));
                    if ui.add(done).clicked() {
                        kb.open = false;
                    }
                });
            },
        );
    }
    ui.data_mut(|d| d.insert_temp(kid, kb));
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_take_their_characters() {
        assert!(
            Charset::Name.allows('Ä') && Charset::Name.allows(' ') && !Charset::Name.allows(':')
        );
        assert!(
            Charset::Address.allows(':')
                && Charset::Address.allows('[')
                && !Charset::Address.allows(' ')
        );
    }
}
