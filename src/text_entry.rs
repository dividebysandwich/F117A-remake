//! A single-line text field in the menu's terminal look: pilot names (career, multiplayer),
//! server addresses and chat messages. In VR, where there is no keyboard at hand, a virtual keyboard in the
//! same look opens under the field when it is clicked; its keys are pressed with the laser
//! pointer and edit the text at the field's cursor, which ← and → move.

use bevy_egui::egui;
use bevy_egui::egui::text::{CCursor, CCursorRange};

use crate::menu::{AMBER, BG, BRIGHT, DIM, GREEN};

/// What a field takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Charset {
    /// Letters, digits, spaces, `-`, `_`, `.`, `'`.
    Name,
    /// Host names, IPv4 and IPv6 addresses with a port: letters, digits, `.`, `-`, `:`,
    /// `[`, `]`.
    Address,
    /// Any text on one line (chat).
    Text,
}

impl Charset {
    fn allows(self, c: char) -> bool {
        match self {
            Charset::Name => c.is_alphanumeric() || " -_.'".contains(c),
            Charset::Address => c.is_ascii_alphanumeric() || ".-:[]".contains(c),
            Charset::Text => !c.is_control(),
        }
    }
}

/// A virtual key's edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Edit {
    Insert(char),
    Backspace,
    Left,
    Right,
    Clear,
}

/// Char index to byte index.
fn byte_at(text: &str, i: usize) -> usize {
    text.char_indices().nth(i).map_or(text.len(), |(b, _)| b)
}

/// Applies a virtual key at the cursor (`sel`: the selected characters, empty for a bare
/// cursor; typing replaces a selection). Returns where the cursor goes.
fn apply(text: &mut String, sel: std::ops::Range<usize>, edit: Edit, max: usize) -> usize {
    let len = text.chars().count();
    let (start, end) = (sel.start.min(len), sel.end.min(len));
    let cut = |text: &mut String| text.replace_range(byte_at(text, start)..byte_at(text, end), "");
    match edit {
        Edit::Insert(c) => {
            cut(text);
            if text.chars().count() >= max {
                return start;
            }
            text.insert(byte_at(text, start), c);
            start + 1
        }
        Edit::Backspace if start < end => {
            cut(text);
            start
        }
        Edit::Backspace if start > 0 => {
            let b = byte_at(text, start - 1);
            text.remove(b);
            start - 1
        }
        Edit::Backspace => 0,
        Edit::Left if start < end => start,
        Edit::Left => start.saturating_sub(1),
        Edit::Right if start < end => end,
        Edit::Right => (end + 1).min(len),
        Edit::Clear => {
            text.clear();
            0
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
    let tid = egui::Id::new(("text_entry", id));
    let field = egui::TextEdit::singleline(text)
        .id(tid)
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
        let mut edit = None;
        egui::Frame::new().fill(BG).stroke(egui::Stroke::new(1.0, DIM)).inner_margin(8.0).show(
            ui,
            |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                for (r, row) in ROWS.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.add_space(r as f32 * 10.0);
                        for c in row.chars() {
                            let c = if kb.shift { c } else { c.to_ascii_lowercase() };
                            if key_enabled(ui, &c.to_string(), KEY.x, false, charset.allows(c)) {
                                edit = Some(Edit::Insert(c));
                                kb.shift = false;
                            }
                        }
                        if r == 3 && charset == Charset::Address {
                            for c in ADDRESS_ROW.chars() {
                                if key(ui, &c.to_string(), KEY.x, false) {
                                    edit = Some(Edit::Insert(c));
                                }
                            }
                        }
                    });
                }
                ui.horizontal(|ui| {
                    if key(ui, "SHIFT", KEY.x * 2.0, kb.shift) {
                        kb.shift = !kb.shift;
                    }
                    if key_enabled(ui, "SPACE", KEY.x * 2.5, false, charset.allows(' ')) {
                        edit = Some(Edit::Insert(' '));
                    }
                    if key(ui, "←", KEY.x, false) {
                        edit = Some(Edit::Left);
                    }
                    if key(ui, "→", KEY.x, false) {
                        edit = Some(Edit::Right);
                    }
                    if key(ui, "DEL", KEY.x * 1.5, false) {
                        edit = Some(Edit::Backspace);
                    }
                    if key(ui, "CLEAR", KEY.x * 1.5, false) {
                        edit = Some(Edit::Clear);
                    }
                    let done = egui::Button::new(
                        egui::RichText::new("DONE").monospace().size(18.0).color(BG),
                    )
                    .fill(AMBER)
                    .min_size(egui::vec2(KEY.x * 2.0, KEY.y));
                    if ui.add(done).clicked() {
                        kb.open = false;
                        ui.memory_mut(|m| m.surrender_focus(tid));
                    }
                });
            },
        );
        // At the field's cursor (the end if it never had one), which stays in the field.
        if let Some(edit) = edit {
            let mut state = egui::TextEdit::load_state(ui.ctx(), tid).unwrap_or_default();
            let len = text.chars().count();
            let sel = state.cursor.char_range().map_or(len..len, |r| {
                let r = r.as_sorted_char_range();
                r.start.0..r.end.0
            });
            let at = apply(text, sel, edit, max);
            state.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(at))));
            state.store(ui.ctx(), tid);
            ui.memory_mut(|m| m.request_focus(tid));
        }
    }
    ui.data_mut(|d| d.insert_temp(kid, kb));
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_edit_at_the_cursor() {
        let mut t = String::from("Fox");
        assert_eq!(apply(&mut t, 1..1, Edit::Insert('l'), 24), 2);
        assert_eq!(t, "Flox");
        assert_eq!(apply(&mut t, 2..2, Edit::Backspace, 24), 1);
        assert_eq!(t, "Fox");
        assert_eq!(apply(&mut t, 0..0, Edit::Backspace, 24), 0);
        assert_eq!(apply(&mut t, 0..0, Edit::Left, 24), 0);
        assert_eq!(apply(&mut t, 3..3, Edit::Right, 24), 3);
        assert_eq!(apply(&mut t, 1..3, Edit::Insert('ä'), 24), 2);
        assert_eq!(t, "Fä");
        assert_eq!(apply(&mut t, 1..1, Edit::Insert('x'), 2), 1);
        assert_eq!(t, "Fä");
        assert_eq!(apply(&mut t, 0..2, Edit::Backspace, 24), 0);
        assert_eq!(t, "");
    }

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
