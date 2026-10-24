//! Boss key (Alt B): a spreadsheet covers the whole window until Alt B is pressed again. In any
//! screen; a single-player flight pauses (as in the in-flight menu), and every sound is muted
//! while it shows (a campaign or a multiplayer game goes on behind it, silently).

use bevy::audio::{AudioSink, AudioSinkPlayback, SpatialAudioSink};
use bevy::image::{ImageLoaderSettings, ImageSampler};
use bevy::prelude::*;

use crate::flight_menu::FlightMenu;
use crate::input::InputBlock;
use crate::settings::AppState;

pub struct BossKeyPlugin;

impl Plugin for BossKeyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BossKey>().add_systems(Update, (toggle, mute_new_sounds).chain());
    }
}

#[derive(Resource, Default)]
pub struct BossKey {
    /// The boss screen's root node while it shows.
    screen: Option<Entity>,
    /// It paused the flight / blocked the flight's input (undone when it goes away).
    paused: bool,
    blocked: bool,
}

#[allow(clippy::too_many_arguments)]
fn toggle(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    server: Res<AssetServer>,
    mut boss: ResMut<BossKey>,
    state: Res<State<AppState>>,
    menu: Res<FlightMenu>,
    campaign: Option<Res<crate::campaign::ActiveCampaign>>,
    link: Option<Res<crate::net::NetLink>>,
    mut time: ResMut<Time<Virtual>>,
    mut block: ResMut<InputBlock>,
    mut sinks: Query<&mut AudioSink>,
    mut spatial: Query<&mut SpatialAudioSink>,
) {
    if !(keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight])
        && keys.just_pressed(KeyCode::KeyB))
    {
        return;
    }
    if let Some(screen) = boss.screen.take() {
        commands.entity(screen).despawn();
        if std::mem::take(&mut boss.paused) {
            time.unpause();
        }
        if std::mem::take(&mut boss.blocked) {
            block.0 = false;
        }
        for mut s in &mut sinks {
            s.unmute();
        }
        for mut s in &mut spatial {
            s.unmute();
        }
        return;
    }
    // The in-flight menu has already paused the flight (and blocks its input).
    let flight = *state.get() == AppState::Flight;
    boss.paused = flight && !menu.open && campaign.is_none() && link.is_none() && !time.is_paused();
    if boss.paused {
        time.pause();
    }
    boss.blocked = flight && !block.0;
    if boss.blocked {
        block.0 = true;
    }
    for mut s in &mut sinks {
        s.mute();
    }
    for mut s in &mut spatial {
        s.mute();
    }
    let image = server
        .load_builder()
        .with_settings(|s: &mut ImageLoaderSettings| s.sampler = ImageSampler::nearest())
        .load("images/boss.png");
    boss.screen = Some(
        commands
            .spawn((
                ImageNode::new(image),
                Node {
                    position_type: PositionType::Absolute,
                    width: percent(100),
                    height: percent(100),
                    ..default()
                },
                BackgroundColor(Color::WHITE),
                GlobalZIndex(i32::MAX),
            ))
            .id(),
    );
}

impl BossKey {
    pub fn showing(&self) -> bool {
        self.screen.is_some()
    }
}

/// Sounds started while the boss screen shows start muted.
fn mute_new_sounds(
    boss: Res<BossKey>,
    mut sinks: Query<&mut AudioSink, Added<AudioSink>>,
    mut spatial: Query<&mut SpatialAudioSink, Added<SpatialAudioSink>>,
) {
    if boss.screen.is_none() {
        return;
    }
    for mut s in &mut sinks {
        s.mute();
    }
    for mut s in &mut spatial {
        s.mute();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Alt B shows the boss screen and pauses a single-player flight; again hides it and resumes.
    #[test]
    fn alt_b_toggles_and_pauses() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), bevy::state::app::StatesPlugin))
            .init_asset::<Image>()
            .insert_state(AppState::Flight)
            .init_resource::<FlightMenu>()
            .init_resource::<InputBlock>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_plugins(BossKeyPlugin);
        let press = |app: &mut App| {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.clear();
            keys.press(KeyCode::AltLeft);
            keys.press(KeyCode::KeyB);
            app.update();
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.release_all();
            keys.clear();
        };
        press(&mut app);
        assert!(app.world().resource::<BossKey>().showing());
        assert!(app.world().resource::<Time<Virtual>>().is_paused());
        assert!(app.world().resource::<InputBlock>().0);
        press(&mut app);
        assert!(!app.world().resource::<BossKey>().showing());
        assert!(!app.world().resource::<Time<Virtual>>().is_paused());
        assert!(!app.world().resource::<InputBlock>().0);
    }
}
