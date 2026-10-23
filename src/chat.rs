//! Coop chat: the chat key (Y) opens a field under the cockpit's message panel
//! (`player_notices.rs`); Enter sends, Esc cancels, and while it is open the keyboard types
//! into it only. The Ops screen's CHAT tab sends through the same outbox. The messages go to
//! the others through the server, and show with the sender's name.

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use stealth_net::protocol::{MAX_CHAT, chat_text};

use crate::campaign::ActiveCampaign;
use crate::input::{Action, ActionEvent, InputBlock, InputSystemsSet};
use crate::net::{NetInbox, NetLink};
use crate::player_notices::PlayerNotices;
use crate::settings::AppState;

pub struct ChatPlugin;

impl Plugin for ChatPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PreUpdate,
            type_chat.after(bevy::input::InputSystems).before(InputSystemsSet),
        )
        .add_systems(
            Update,
            (open, send, receive)
                .chain()
                .run_if(in_state(AppState::Flight))
                .run_if(resource_exists::<NetLink>),
        );
    }
}

/// The keys typed into the cockpit's chat field; while it (or the Ops screen's) is open no
/// other system sees a key.
fn type_chat(
    mut events: MessageReader<KeyboardInput>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut notices: ResMut<PlayerNotices>,
) {
    let ops_field = std::mem::take(&mut notices.field_focus);
    let notices = &mut *notices;
    let Some(text) = notices.typing.as_mut() else {
        events.clear();
        if ops_field {
            keys.reset_all();
        }
        return;
    };
    let mut done = None;
    for e in events.read().filter(|e| e.state == ButtonState::Pressed) {
        match (&e.logical_key, e.key_code) {
            (Key::Escape, _) | (_, KeyCode::Escape) => done = Some(false),
            (Key::Enter, _) | (_, KeyCode::Enter | KeyCode::NumpadEnter) => done = Some(true),
            (Key::Backspace, _) => {
                text.pop();
            }
            _ => {
                if let Some(t) = &e.text {
                    for c in t.chars().filter(|c| !c.is_control()) {
                        if text.chars().count() < MAX_CHAT {
                            text.push(c);
                        }
                    }
                }
            }
        }
        if done.is_some() {
            break;
        }
    }
    events.clear();
    if let Some(send) = done {
        let text = notices.typing.take().unwrap_or_default();
        if send && !chat_text(&text).is_empty() {
            notices.outbox.push(text);
        }
    }
    keys.reset_all();
}

/// The chat key opens the cockpit's field (not over the Ops screen or the menu, which block
/// the keys, and not in the headset); they close it again.
fn open(
    mut actions: MessageReader<ActionEvent>,
    block: Res<InputBlock>,
    settings: Res<crate::settings::Settings>,
    mut notices: ResMut<PlayerNotices>,
) {
    let pressed = actions.read().any(|a| a.0 == Action::Chat);
    if block.0 {
        if notices.typing.is_some() {
            notices.typing = None;
        }
    } else if pressed && !settings.vr && notices.typing.is_none() {
        notices.typing = Some(String::new());
    }
}

/// The messages typed here to the others, and into the history with this player's name.
fn send(
    mut notices: ResMut<PlayerNotices>,
    mut link: ResMut<NetLink>,
    campaign: Option<Res<ActiveCampaign>>,
) {
    if notices.outbox.is_empty() {
        return;
    }
    let name = campaign
        .as_ref()
        .and_then(|c| c.state().player_name(c.me).map(str::to_string))
        .unwrap_or_else(|| "Pilot".to_string());
    for text in std::mem::take(&mut notices.outbox) {
        let text = chat_text(&text);
        if text.is_empty() {
            continue;
        }
        match &mut *link {
            NetLink::Client(client) => client.chat(&text),
            NetLink::Host(server) => server.say(&name, &text),
        }
        notices.chat(&name, &text, true);
    }
}

/// The others' messages: from the server (a client), or the players' (a listen server).
fn receive(
    mut notices: ResMut<PlayerNotices>,
    mut link: ResMut<NetLink>,
    mut inbox: ResMut<NetInbox>,
) {
    let came = match &mut *link {
        NetLink::Client(_) => std::mem::take(&mut inbox.chat),
        NetLink::Host(server) => server.take_chat(),
    };
    for (from, text) in came {
        notices.chat(&from, &text, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Keys type into the open field (and nothing else sees them); Enter sends, Esc cancels.
    #[test]
    fn typing_takes_the_keyboard() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<PlayerNotices>()
            .add_message::<KeyboardInput>()
            .add_systems(Update, type_chat);
        let key = |app: &mut App, key_code: KeyCode, logical_key: Key, text: Option<&str>| {
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(key_code);
            app.world_mut().write_message(KeyboardInput {
                key_code,
                logical_key,
                state: ButtonState::Pressed,
                text: text.map(Into::into),
                repeat: false,
                window: Entity::PLACEHOLDER,
            });
            app.update();
        };
        // Closed: keys pass.
        key(&mut app, KeyCode::KeyH, Key::Character("h".into()), Some("h"));
        assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::KeyH));
        app.world_mut().resource_mut::<PlayerNotices>().typing = Some(String::new());
        for (k, c) in [(KeyCode::KeyH, "H"), (KeyCode::KeyI, "i"), (KeyCode::Digit1, "!")] {
            key(&mut app, k, Key::Character(c.into()), Some(c));
        }
        key(&mut app, KeyCode::KeyX, Key::Character("x".into()), Some("x"));
        key(&mut app, KeyCode::Backspace, Key::Backspace, Some("\u{8}"));
        let n = app.world().resource::<PlayerNotices>();
        assert_eq!(n.typing.as_deref(), Some("Hi!"));
        assert!(!app.world().resource::<ButtonInput<KeyCode>>().get_pressed().any(|_| true));
        key(&mut app, KeyCode::Enter, Key::Enter, Some("\r"));
        let n = app.world().resource::<PlayerNotices>();
        assert_eq!((n.typing.is_none(), n.outbox.as_slice()), (true, &["Hi!".to_string()][..]));
        // Esc: nothing sent, and the menu does not see it.
        app.world_mut().resource_mut::<PlayerNotices>().outbox.clear();
        app.world_mut().resource_mut::<PlayerNotices>().typing = Some("draft".into());
        key(&mut app, KeyCode::Escape, Key::Escape, None);
        let n = app.world().resource::<PlayerNotices>();
        assert!(n.typing.is_none() && n.outbox.is_empty());
        assert!(!app.world().resource::<ButtonInput<KeyCode>>().just_pressed(KeyCode::Escape));
    }
}
