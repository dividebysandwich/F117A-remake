use bevy::{prelude::*, camera::visibility::RenderLayers, text::TextBounds};

use crate::definitions::*;
use crate::f117_ai::*;

#[derive(Component)]
pub struct LabelAIDialogAvatar;

#[derive(Component)]
pub struct LabelAIDialogBox;

#[derive(Component)]
pub struct LabelAIDialogText;

#[allow(dead_code)]
#[derive(Resource)]
pub struct AIDialogUIState {
    pub is_visible: bool,
}

pub fn setup_dialog_ui(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
) {
    let ui_state = AIDialogUIState{
        is_visible: false,
    };
    commands.insert_resource(ui_state);

    let font = asset_server.load("fonts/BigBlueTerm437NerdFontMono-Regular.ttf");

    commands.spawn((
        Sprite::from_image(asset_server.load("avatars/f117a.png")),
        Transform::from_translation(Vec3::new(-680.0, 450.0, 0.0)),
        RenderLayers::layer(RENDERLAYER_COCKPIT),
        Visibility::Hidden,
        LabelAIDialogAvatar,
    ));

    commands.spawn((
        Text2d::new(""),
        TextFont {
            font: font.clone(),
            font_size: 25.0,
            ..default()
        },
        TextColor(Color::WHITE),
        TextLayout::new_with_justify(Justify::Left),
        TextBounds::from(Vec2::new(380.0, 100.0)),
        Transform::from_translation(Vec3::new(-610.0, 450.0, 1.0)),
        RenderLayers::layer(RENDERLAYER_COCKPIT),
        Visibility::Hidden,
        LabelAIDialogText,
    ));

}

pub fn update_dialog_ui(
    mut text_query: Query<(&mut Text2d, &mut Visibility), (With<LabelAIDialogText>, Without<LabelAIDialogAvatar>)>,
    mut avatar_query: Query<&mut Visibility, (With<LabelAIDialogAvatar>, Without<LabelAIDialogText>)>,
    f117_ai_state: Res<F117AIState>,
    mut ui_state: ResMut<AIDialogUIState>,
) {

    // Hide dialog if there is no text to display
    if ui_state.is_visible == true && f117_ai_state.display_line.len() == 0 {
        ui_state.is_visible = false;
        for (mut text, mut visibility) in text_query.iter_mut() {
            text.0 = "".to_string();
            *visibility = Visibility::Hidden;
        }
        for mut visibility in avatar_query.iter_mut() {
            *visibility = Visibility::Hidden;
        }
    } else // Show dialog if there's text, but only after a delay of 1 second.
    if ui_state.is_visible == false && f117_ai_state.display_line.len() > 0 && f117_ai_state.active_time > 1.0 {
        ui_state.is_visible = true;
        for (mut text, mut visibility) in text_query.iter_mut() {
            text.0 = f117_ai_state.display_line.clone();
            *visibility = Visibility::Visible;
        }
        for mut visibility in avatar_query.iter_mut() {
            *visibility = Visibility::Visible;
        }
    }
}
