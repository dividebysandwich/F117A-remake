use bevy::{prelude::*, camera::visibility::RenderLayers};

use crate::definitions::COLOR_GREEN;
use crate::CameraSettings;
use crate::aircraft::*;
use crate::definitions::RENDERLAYER_COCKPIT;
use crate::player::*;


#[derive(Component)]
pub struct LabelCurrentSpeed;

#[derive(Component)]
pub struct LabelCurrentAltitude;

pub fn setup_hud(mut commands: Commands, asset_server: Res<AssetServer>) {
    let font = asset_server.load("fonts/Brickshapers-eXPx.ttf");
    commands.spawn((
        Text2d::new("0"),
        TextFont {
            font: font.clone(),
            font_size: 30.0,
            ..default()
        },
        TextColor(COLOR_GREEN),
        TextLayout::new_with_justify(Justify::Left),
        Transform::from_translation(Vec3::new(-470.0, 0.0, 0.0)),
    ))
    .insert(RenderLayers::layer(RENDERLAYER_COCKPIT))
    .insert(LabelCurrentSpeed);

    commands.spawn((
        Text2d::new("0"),
        TextFont {
            font: font.clone(),
            font_size: 30.0,
            ..default()
        },
        TextColor(COLOR_GREEN),
        TextLayout::new_with_justify(Justify::Right),
        Transform::from_translation(Vec3::new(470.0, 0.0, 0.0)),
    ))
    .insert(RenderLayers::layer(RENDERLAYER_COCKPIT))
    .insert(LabelCurrentAltitude);

}

fn draw_vertical_ladder(gizmos: &mut Gizmos, value : f32, xpos : f32, hud_size_y : i32, tick_direction : f32) {

    let mut y = (value % 50.0) as i32;
    for i in 0 .. hud_size_y as i32 {
        if y % 10 == 0 {
            if y % 50 == 0 {
                gizmos.line_2d(Vec2::new(xpos + (tick_direction*20.0), i as f32), Vec2::new(xpos, i as f32), COLOR_GREEN);
            } else {
                gizmos.line_2d(Vec2::new(xpos + (tick_direction*10.0), i as f32), Vec2::new(xpos, i as f32), COLOR_GREEN);
            }
        }
        y += 1;
    }

    let mut y = -(value % 50.0) as i32;
    let mut value_limiter = value;
    for i in 0 .. hud_size_y as i32 {
        if value_limiter > 0.0 {
            if y % 10 == 0 {
                if y % 50 == 0 {
                    gizmos.line_2d(Vec2::new(xpos + (tick_direction*20.0), -i as f32), Vec2::new(xpos, -i as f32), COLOR_GREEN);
                } else {
                    gizmos.line_2d(Vec2::new(xpos + (tick_direction*10.0), -i as f32), Vec2::new(xpos, -i as f32), COLOR_GREEN);
                }
            }
        }
        y += 1;
        value_limiter -= 1.0;
    }
}

pub fn update_hud(mut aircrafts: Query<&Aircraft, With<Player>>,
    mut speedlabels: Query<&mut Text2d, (With<LabelCurrentSpeed>, Without<LabelCurrentAltitude>)>,
    mut altitudelabels: Query<&mut Text2d, (With<LabelCurrentAltitude>, Without<LabelCurrentSpeed>)>,
    camera_settings: ResMut<CameraSettings>,
    mut gizmos: Gizmos,
    ) {
    let mut speedlabel = speedlabels.single_mut().unwrap();
    let mut altitudelabel = altitudelabels.single_mut().unwrap();
    if camera_settings.render_hud == true {
        for aircraft in aircrafts.iter_mut() {
            speedlabel.0 = format!("{:.0}", aircraft.speed_knots);
            draw_vertical_ladder(&mut gizmos, aircraft.speed_knots * 2.0, -500.0, 400, -1.0);

            altitudelabel.0 = format!("{:.0}", aircraft.altitude);
            draw_vertical_ladder(&mut gizmos, aircraft.altitude, 500.0, 400, 1.0);
        }
    } else {
        speedlabel.0 = "".to_string();
        altitudelabel.0 = "".to_string();
    }
}
