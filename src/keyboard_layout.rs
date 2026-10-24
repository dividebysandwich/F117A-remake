//! What a key is called on the player's keyboard layout.
//!
//! Keys are bound by position (`KeyCode` is a physical key, so a binding works the same on
//! QWERTY, AZERTY and QWERTZ), but the labels must show what is printed on the key: the key
//! at the position of QWERTY's W reads Z on an AZERTY keyboard. On Windows the layout is asked
//! for the character of each position. Elsewhere the labels are learned from the keys the
//! player presses (the character the layout gives them), the others keep their QWERTY names.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;

pub struct KeyboardLayoutPlugin;

impl Plugin for KeyboardLayoutPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreUpdate, learn_labels.after(bevy::input::InputSystems));
    }
}

/// Labels seen on pressed keys, by position.
static LEARNED: LazyLock<Mutex<HashMap<KeyCode, String>>> = LazyLock::new(Default::default);

/// Remembers the character the layout gives a pressed key. Shifted presses are skipped: they
/// give the shifted symbol.
fn learn_labels(mut events: MessageReader<KeyboardInput>, keys: Res<ButtonInput<KeyCode>>) {
    let shifted = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight, KeyCode::AltRight]);
    let lower_accents = accents_lower();
    for ev in events.read() {
        if ev.state != ButtonState::Pressed || shifted || scancode(ev.key_code).is_none() {
            continue;
        }
        let Key::Character(text) = &ev.logical_key else { continue };
        if let Some(label) = printable(text, lower_accents) {
            LEARNED.lock().unwrap().insert(ev.key_code, label);
        }
    }
}

/// A single printable character as printed on a keycap: the letters A-Z in capitals; the other
/// characters in capitals too, except on the layouts that print their accented letters in lower
/// case (`lower_accents`: AZERTY's "é", "è", "ç", "à", "ù"). The API and the key events give
/// capitals or small letters whatever the keycap says. A character whose capital is not a single
/// character ("ß" -> "SS") or does not turn back into it (the micro sign "µ" -> Greek "Μ") is
/// kept as it is.
fn printable(text: &str, lower_accents: bool) -> Option<String> {
    let mut chars = text.chars();
    let c = chars.next().filter(|c| !c.is_control() && !c.is_whitespace())?;
    if chars.next().is_some() {
        return None;
    }
    if lower_accents && !c.is_ascii() {
        return Some(c.to_lowercase().collect());
    }
    let mut upper = c.to_uppercase();
    Some(match (upper.next(), upper.next()) {
        (Some(u), None) if u.to_lowercase().eq([c]) || u == c => u.to_string(),
        _ => c.to_string(),
    })
}

/// The label of a character key on the current layout ("Z" for the key at QWERTY's W on an
/// AZERTY keyboard), or None for the keys that are named the same everywhere (F-keys, arrows,
/// Space...) and the character keys whose label is not known.
pub fn key_name(key: KeyCode) -> Option<String> {
    let sc = scancode(key)?;
    layout_label(sc).or_else(|| LEARNED.lock().unwrap().get(&key).cloned())
}

#[cfg(windows)]
#[link(name = "user32")]
unsafe extern "system" {
    fn MapVirtualKeyW(code: u32, map_type: u32) -> u32;
    fn GetKeyboardLayout(thread: u32) -> isize;
}

#[cfg(windows)]
fn layout_label(scancode: u32) -> Option<String> {
    const MAPVK_VSC_TO_VK: u32 = 1;
    const MAPVK_VK_TO_CHAR: u32 = 2;
    // SAFETY: MapVirtualKeyW only reads its two integer arguments and the thread's keyboard layout.
    let ch = unsafe {
        let vk = MapVirtualKeyW(scancode, MAPVK_VSC_TO_VK);
        if vk == 0 {
            return None;
        }
        MapVirtualKeyW(vk, MAPVK_VK_TO_CHAR)
    };
    // The top bit marks a dead key (^, ¨...): its character is still the printed one.
    let c = char::from_u32(ch & 0x7fff_ffff).filter(|c| *c != '\0')?;
    printable(c.encode_utf8(&mut [0; 4]), accents_lower())
}

#[cfg(not(windows))]
fn layout_label(_scancode: u32) -> Option<String> {
    None
}

/// Whether the current keyboard layout prints its accented letters in lower case on the keycaps
/// (French, Italian, Hungarian, Czech, Slovak). The other layouts (German, Nordic, Spanish,
/// Cyrillic, Greek...) print capitals. Only Windows tells the layout's language; elsewhere the
/// capitals are kept.
#[cfg(windows)]
fn accents_lower() -> bool {
    const LANG_CZECH: isize = 0x05;
    const LANG_FRENCH: isize = 0x0c;
    const LANG_HUNGARIAN: isize = 0x0e;
    const LANG_ITALIAN: isize = 0x10;
    const LANG_SLOVAK: isize = 0x1b;
    // SAFETY: GetKeyboardLayout only reads the keyboard layout of the calling thread (id 0), the
    // one MapVirtualKeyW uses.
    let hkl = unsafe { GetKeyboardLayout(0) };
    // The low word is the language id, whose low 10 bits are the primary language.
    matches!(hkl & 0x3ff, LANG_CZECH | LANG_FRENCH | LANG_HUNGARIAN | LANG_ITALIAN | LANG_SLOVAK)
}

#[cfg(not(windows))]
fn accents_lower() -> bool {
    false
}

/// Set 1 scancode of the character keys (the layout-dependent ones).
fn scancode(key: KeyCode) -> Option<u32> {
    use KeyCode::*;
    Some(match key {
        Digit1 => 0x02,
        Digit2 => 0x03,
        Digit3 => 0x04,
        Digit4 => 0x05,
        Digit5 => 0x06,
        Digit6 => 0x07,
        Digit7 => 0x08,
        Digit8 => 0x09,
        Digit9 => 0x0A,
        Digit0 => 0x0B,
        Minus => 0x0C,
        Equal => 0x0D,
        KeyQ => 0x10,
        KeyW => 0x11,
        KeyE => 0x12,
        KeyR => 0x13,
        KeyT => 0x14,
        KeyY => 0x15,
        KeyU => 0x16,
        KeyI => 0x17,
        KeyO => 0x18,
        KeyP => 0x19,
        BracketLeft => 0x1A,
        BracketRight => 0x1B,
        KeyA => 0x1E,
        KeyS => 0x1F,
        KeyD => 0x20,
        KeyF => 0x21,
        KeyG => 0x22,
        KeyH => 0x23,
        KeyJ => 0x24,
        KeyK => 0x25,
        KeyL => 0x26,
        Semicolon => 0x27,
        Quote => 0x28,
        Backquote => 0x29,
        Backslash => 0x2B,
        KeyZ => 0x2C,
        KeyX => 0x2D,
        KeyC => 0x2E,
        KeyV => 0x2F,
        KeyB => 0x30,
        KeyN => 0x31,
        KeyM => 0x32,
        Comma => 0x33,
        Period => 0x34,
        Slash => 0x35,
        IntlBackslash => 0x56,
        _ => return None,
    })
}

/// The characters on the keycaps of the European layouts (AZERTY, QWERTZ, Nordic, Spanish,
/// Portuguese, Italian, Hungarian, Czech, Slovak): the accented letters in lower case and in
/// capitals (see `printable`), and the symbols. Every text showing key labels draws them.
#[cfg(test)]
pub const EUROPEAN_KEYCAPS: &str = concat!(
    "éèçàùìòöüóőúűáíěščřžýůľťäňô",
    "ÉÈÇÀÙÌÒÖÜÓŐÚŰÁÍĚŠČŘŽÝŮĽŤÄŇÔÅÆØÑß",
    "²&\"'(-_)=^$*<,;:!§µ\\«»º¡´`~+#½¨°£¤|",
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_are_physical() {
        assert_eq!(scancode(KeyCode::KeyW), Some(0x11));
        assert_eq!(scancode(KeyCode::KeyZ), Some(0x2C));
        assert_eq!(scancode(KeyCode::F1), None);
        assert_eq!(scancode(KeyCode::Space), None);
    }

    #[test]
    fn labels_are_single_characters() {
        assert_eq!(printable("w", false).as_deref(), Some("W"));
        assert_eq!(printable("w", true).as_deref(), Some("W"));
        assert_eq!(printable("é", true).as_deref(), Some("é"));
        assert_eq!(printable("É", true).as_deref(), Some("é"));
        assert_eq!(printable("ü", false).as_deref(), Some("Ü"));
        assert_eq!(printable("ф", false).as_deref(), Some("Ф"));
        assert_eq!(printable("ß", false).as_deref(), Some("ß"));
        assert_eq!(printable("µ", false).as_deref(), Some("µ"));
        assert_eq!(printable("ö", false).as_deref(), Some("Ö"));
        assert_eq!(printable("Ö", false).as_deref(), Some("Ö"));
        assert_eq!(printable("ab", false), None);
        assert_eq!(printable(" ", false), None);
    }

    /// On a Latin layout every letter position has a label.
    #[cfg(windows)]
    #[test]
    fn windows_layout_labels_letters() {
        for key in [KeyCode::KeyA, KeyCode::KeyW, KeyCode::KeyZ, KeyCode::KeyM] {
            assert!(key_name(key).is_some(), "{key:?}");
        }
    }
}
