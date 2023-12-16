use bevy::prelude::*;
use bevy::render::view::visibility::RenderLayers;
use bevy::text::Text2dBounds;

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
    let text_style = TextStyle {
        font: font.clone(),
        font_size: 25.0,
        color: Color::WHITE,
    };
    /*/
    commands.spawn(
        ImageBundle {
            image: UiImage::new(asset_server.load("avatars/f117a.png")),
            transform: Transform::from_translation(Vec3::new(300.0, 300.0, 0.0)),
            ..default()
        }
    )
    .insert(RenderLayers::layer(RENDERLAYER_COCKPIT))
    .insert(LabelAIDialogAvatar);
*/
    commands.spawn(
        SpriteBundle {
            texture: asset_server.load("avatars/f117a.png"),
            transform: Transform::from_translation(Vec3::new(-680.0, 450.0, 0.0)),
            ..default()
        }
    )
    .insert(RenderLayers::layer(RENDERLAYER_COCKPIT))
    .insert(Visibility::Hidden)
    .insert(LabelAIDialogAvatar);


    commands.spawn(
        Text2dBundle {
            text: Text::from_section("".to_string(), text_style.clone()).with_alignment(TextAlignment::Left),
            transform: Transform::from_translation(Vec3::new(-610.0, 450.0, 1.0)),
            text_2d_bounds: Text2dBounds {
                size: Vec2::new(380.0, 100.0),
                ..default()
            },
            ..default()
        },
    )
    .insert(RenderLayers::layer(RENDERLAYER_COCKPIT))
    .insert(Visibility::Hidden)
    .insert(LabelAIDialogText);

}

pub fn update_dialog_ui(
    mut text_query: Query<(&mut Text, &mut Visibility), (With<LabelAIDialogText>, Without<LabelAIDialogAvatar>)>,
    mut avatar_query: Query<&mut Visibility, (With<LabelAIDialogAvatar>, Without<LabelAIDialogText>)>,
    f117_ai_state: Res<F117AIState>,
    mut ui_state: ResMut<AIDialogUIState>,
) {

    // Hide dialog if there is no text to display
    if ui_state.is_visible == true && f117_ai_state.display_line.len() == 0 {
        ui_state.is_visible = false;
        for (mut text, mut visibility) in text_query.iter_mut() {
            text.sections[0].value = "".to_string();
            *visibility = Visibility::Hidden;
        }
        for mut visibility in avatar_query.iter_mut() {
            *visibility = Visibility::Hidden;
        }
    } else // Show dialog if there's text, but only after a delay of 1 second.
    if ui_state.is_visible == false && f117_ai_state.display_line.len() > 0 && f117_ai_state.active_time > 1.0 {
        ui_state.is_visible = true;
        for (mut text, mut visibility) in text_query.iter_mut() {
            text.sections[0].value = f117_ai_state.display_line.clone();
            *visibility = Visibility::Visible;
        }
        for mut visibility in avatar_query.iter_mut() {
            *visibility = Visibility::Visible;
        }
    }
}
