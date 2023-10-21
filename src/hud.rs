use bevy::prelude::*;
use bevy::render::view::visibility::RenderLayers;

use crate::aircraft::*;
use crate::player::*;


#[derive(Component)]
pub struct LabelCurrentSpeed;

pub fn setup_hud(mut commands: Commands, asset_server: Res<AssetServer>) {
    let font = asset_server.load("fonts/Brickshapers-eXPx.ttf");
    let text_style = TextStyle {
        font: font.clone(),
        font_size: 30.0,
        color: Color::GREEN,
    };
    let text_alignment = TextAlignment::Left;
    commands.spawn(
        Text2dBundle {
            text: Text::from_section("0".to_string(), text_style.clone()).with_alignment(text_alignment),
            transform: Transform::from_translation(Vec3::new(-370.0, 0.0, 0.0)),
            ..default()
        }
    ).insert(RenderLayers::layer(1))
    .insert(LabelCurrentSpeed);
}

pub fn update_hud(mut aircrafts: Query<&Aircraft, With<Player>>, mut speedlabels: Query<&mut Text, With<LabelCurrentSpeed>>, mut gizmos: Gizmos, time: Res<Time>) {
    for aircraft in aircrafts.iter_mut() {
        let mut speedlabel = speedlabels.get_single_mut().unwrap();
        speedlabel.sections[0].value = format!("{:.0}", aircraft.speed_knots);
        info!("Speed: {}",aircraft.speed_knots);

        let hud_size_y = 400.0;
        let mut y = (aircraft.speed_knots % 50.0) as i32;
        for i in 0 .. hud_size_y as i32 {
            if y % 10 == 0 {
                if y % 50 == 0 {
                    gizmos.line_2d(Vec2::new(-420.0, i as f32), Vec2::new(-400.0, i as f32), Color::GREEN);
                } else {
                    gizmos.line_2d(Vec2::new(-410.0, i as f32), Vec2::new(-400.0, i as f32), Color::GREEN);
                }
            }
            y += 1;
        }

        let mut y = -(aircraft.speed_knots % 50.0) as i32;
        for i in 0 .. hud_size_y as i32 {
            if y % 10 == 0 {
                if y % 50 == 0 {
                    gizmos.line_2d(Vec2::new(-420.0, -i as f32), Vec2::new(-400.0, -i as f32), Color::GREEN);
                } else {
                    gizmos.line_2d(Vec2::new(-410.0, -i as f32), Vec2::new(-400.0, -i as f32), Color::GREEN);
                }
            }
            y += 1;
        }



        //gizmos.line_2d(Vec2::new(-400.0, -400.0), Vec2::new(-400.0, 400.0), Color::GREEN);
        gizmos.circle_2d(Vec2::ZERO, 300., Color::GREEN).segments(32);
    }
}
