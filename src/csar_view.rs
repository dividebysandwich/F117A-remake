//! Status panel for the downed pilot: the rescue helicopter, the Hinds, flares and time on
//! the ground (the cockpit displays are gone with the aircraft).

use bevy::prelude::*;

use stealth_sim::csar::CsarState;
use stealth_sim::math::approx;
use stealth_sim::types::fflags;

use crate::camera::{View, ViewState};
use crate::combat::Combat;
use crate::settings::AppState;
use crate::world::WU;

pub struct CsarViewPlugin;

impl Plugin for CsarViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, update_panel.run_if(in_state(AppState::Flight)))
            .add_systems(OnExit(AppState::Flight), despawn_panel);
    }
}

#[derive(Component)]
struct CsarPanel;

/// Map units -> km.
fn km(map: i32) -> f32 {
    map as f32 * 32.0 * WU as f32 / 1000.0
}

/// Compass point of a bearing from (x, y) to (tx, ty) in map units (y grows south).
fn compass(x: i32, y: i32, tx: i32, ty: i32) -> &'static str {
    let deg = ((tx - x) as f32).atan2((y - ty) as f32).to_degrees().rem_euclid(360.0);
    ["N", "NE", "E", "SE", "S", "SW", "W", "NW"][((deg + 22.5) / 45.0) as usize % 8]
}

fn status(combat: &Combat) -> Option<String> {
    let sim = &combat.sim;
    let (x, y, alt) = sim.chute()?;
    let Some(c) = sim.csar() else {
        return Some(if alt > 0 { "UNDER THE PARACHUTE".into() } else { "ON THE GROUND".into() });
    };
    let mut lines = vec!["DOWN IN ENEMY TERRITORY".to_string()];
    let r = sim.flights()[c.rescue];
    let d = approx(r.x - x, r.y - y);
    lines.push(match c.state {
        CsarState::Pickup(_) => "RESCUE   UH-60 OVERHEAD - PICKUP".into(),
        _ if c.located => format!("RESCUE   UH-60 {:.1} KM {} - INBOUND", km(d), compass(x, y, r.x, r.y)),
        _ => format!("RESCUE   UH-60 {:.1} KM {} - FIRE A FLARE WHEN CLOSE", km(d), compass(x, y, r.x, r.y)),
    });
    let hinds: Vec<i32> = c
        .hunters
        .iter()
        .map(|&h| sim.flights()[h])
        .filter(|u| u.flags & fflags::DESTROYED == 0 && u.flags & fflags::EXISTS != 0)
        .map(|u| approx(u.x - x, u.y - y))
        .collect();
    if let Some(&n) = hinds.iter().min() {
        lines.push(format!("ENEMY    {} HIND, NEAREST {:.1} KM", hinds.len(), km(n)));
    }
    let secs = c.frames / 15;
    lines.push(format!("FLARES   {}      ON THE GROUND {}:{:02}", c.flares, secs / 60, secs % 60));
    lines.push("1: SIGNAL FLARE   SHIFT-Z: FASTER   SHIFT-X: NORMAL TIME".into());
    Some(lines.join("\n"))
}

fn update_panel(
    mut commands: Commands,
    combat: Option<Res<Combat>>,
    view: Res<ViewState>,
    mut panel: Query<(Entity, &Children), With<CsarPanel>>,
    mut texts: Query<&mut Text>,
) {
    let text = combat.as_ref().filter(|c| view.view == View::Pilot && c.end.is_none()).and_then(|c| status(c));
    match (text, panel.single_mut()) {
        (Some(t), Ok((_, children))) => {
            for c in children.iter() {
                if let Ok(mut s) = texts.get_mut(c)
                    && s.0 != t
                {
                    s.0 = t.clone();
                }
            }
        }
        (Some(t), Err(_)) => {
            commands
                .spawn((
                    CsarPanel,
                    Node { position_type: PositionType::Absolute, left: px(16), bottom: px(16), padding: UiRect::all(px(12)), ..default() },
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
                ))
                .with_children(|p| {
                    p.spawn((Text::new(t), TextFont { font_size: bevy::text::FontSize::Px(18.0), ..default() }, TextColor(Color::srgb(0.3, 1.0, 0.45))));
                });
        }
        (None, Ok((e, _))) => commands.entity(e).despawn(),
        (None, Err(_)) => {}
    }
}

fn despawn_panel(mut commands: Commands, q: Query<Entity, With<CsarPanel>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}
