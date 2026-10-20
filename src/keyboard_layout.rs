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
    for ev in events.read() {
        if ev.state != ButtonState::Pressed || shifted || scancode(ev.key_code).is_none() {
            continue;
        }
        let Key::Character(text) = &ev.logical_key else { continue };
        if let Some(label) = printable(text) {
            LEARNED.lock().unwrap().insert(ev.key_code, label);
        }
    }
}

/// A single printable character, upper-cased.
fn printable(text: &str) -> Option<String> {
    let mut chars = text.chars();
    let c = chars.next().filter(|c| !c.is_control() && !c.is_whitespace())?;
    chars.next().is_none().then(|| c.to_uppercase().collect())
}

/// The label of a character key on the current layout ("Z" for the key at QWERTY's W on an
/// AZERTY keyboard), or None for the keys that are named the same everywhere (F-keys, arrows,
/// Space...) and the character keys whose label is not known.
pub fn key_name(key: KeyCode) -> Option<String> {
    let sc = scancode(key)?;
    layout_label(sc).or_else(|| LEARNED.lock().unwrap().get(&key).cloned())
}

#[cfg(windows)]
fn layout_label(scancode: u32) -> Option<String> {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn MapVirtualKeyW(code: u32, map_type: u32) -> u32;
    }
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
    char::from_u32(ch & 0x7fff_ffff).filter(|c| *c != '\0').and_then(|c| printable(&c.to_string()))
}

#[cfg(not(windows))]
fn layout_label(_scancode: u32) -> Option<String> {
    None
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
    fn labels_are_single_upper_case_characters() {
        assert_eq!(printable("w").as_deref(), Some("W"));
        assert_eq!(printable("é").as_deref(), Some("É"));
        assert_eq!(printable("ab"), None);
        assert_eq!(printable(" "), None);
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
