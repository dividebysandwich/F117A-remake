#![allow(dead_code)]

use bevy::prelude::*;
use bevy::reflect::TypePath;
use ::serde::Deserialize;

use crate::util::random_u64;

/* Someone installed an experimental AI in your stealth jet. 
   It may occasionally comment on the situation and provide hints */

pub enum F117AIEvent {
    Takeoff,
    Landing,
    Detected,
    GroundTargetLocked,
    AirTargetLocked,
    GroundTargetDestroyed,
    AirTargetDestroyed,
    FightersNearby,
    SAMPulseNearby,
    SAMDopplerNearby,
    SAMMissilesIncoming,
    AAMMissilesIncoming,
    MissilesDefeated,
    Damaged,
    EngineDamage,
}

#[derive(Deserialize, Asset, TypePath)]
pub struct F117AI {
    lines_takeoff: Vec<String>,
    lines_landing: Vec<String>,
    lines_detected: Vec<String>,
    lines_ground_target_locked: Vec<String>,
    lines_air_target_locked: Vec<String>,
    lines_ground_target_destroyed: Vec<String>,
    lines_air_target_destroyed: Vec<String>,
    lines_fighters_nearby: Vec<String>,
    lines_sam_pulse_nearby: Vec<String>,
    lines_sam_doppler_nearby: Vec<String>,
    lines_sam_missiles_incoming: Vec<String>,
    lines_aam_missiles_incoming: Vec<String>,
    lines_missiles_defeated: Vec<String>,
    lines_damaged: Vec<String>,
    lines_engine_damage: Vec<String>,
}

#[derive(Resource)]
pub struct F117AIHandle(Handle<F117AI>);

#[derive(Resource)]
pub struct F117AIState {
    pub cooldown_takeoff: f32,
    pub cooldown_landing: f32,
    pub cooldown_detected: f32,
    pub cooldown_ground_target_locked: f32,
    pub cooldown_air_target_locked: f32,
    pub cooldown_ground_target_destroyed: f32,
    pub cooldown_air_target_destroyed: f32,
    pub cooldown_fighters_nearby: f32,
    pub cooldown_sam_pulse_nearby: f32,
    pub cooldown_sam_doppler_nearby: f32,
    pub cooldown_sam_missiles_incoming: f32,
    pub cooldown_aam_missiles_incoming: f32,
    pub cooldown_missiles_defeated: f32,
    pub cooldown_damaged: f32,
    pub cooldown_engine_damage: f32,
    pub active_line: Option<F117AIEvent>,
    pub display_line: String,
    pub active_time: f32,
}

impl Default for F117AIState {
    fn default() -> Self {
        F117AIState {
            cooldown_takeoff: 0.0,
            cooldown_landing: 0.0,
            cooldown_detected: 0.0,
            cooldown_ground_target_locked: 0.0,
            cooldown_air_target_locked: 0.0,
            cooldown_ground_target_destroyed: 0.0,
            cooldown_air_target_destroyed: 0.0,
            cooldown_fighters_nearby: 0.0,
            cooldown_sam_pulse_nearby: 0.0,
            cooldown_sam_doppler_nearby: 0.0,
            cooldown_sam_missiles_incoming: 0.0,
            cooldown_aam_missiles_incoming: 0.0,
            cooldown_missiles_defeated: 0.0,
            cooldown_damaged: 0.0,
            cooldown_engine_damage: 0.0,
            active_line: None,
            active_time: 0.0,
            display_line: String::from(""),
        }
    }
    
}

pub fn load_f117_ai(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
) {

    let ai = F117AIHandle(asset_server.load("ai/f117_ai.toml"));
    commands.insert_resource(ai);

    let ai_state = F117AIState {
        ..default()
    };
    commands.insert_resource(ai_state);
}

pub fn update_f117_ai_cooldown(
    mut f117_ai_state: ResMut<F117AIState>,
    time: Res<Time>,
) {
    let deltatime = time.delta_seconds();
    f117_ai_state.cooldown_landing = (f117_ai_state.cooldown_landing - deltatime).max(0.0);
    f117_ai_state.cooldown_takeoff = (f117_ai_state.cooldown_takeoff - deltatime).max(0.0);
    f117_ai_state.cooldown_detected = (f117_ai_state.cooldown_detected - deltatime).max(0.0);
    f117_ai_state.cooldown_ground_target_locked = (f117_ai_state.cooldown_ground_target_locked - deltatime).max(0.0);
    f117_ai_state.cooldown_air_target_locked = (f117_ai_state.cooldown_air_target_locked - deltatime).max(0.0);
    f117_ai_state.cooldown_ground_target_destroyed = (f117_ai_state.cooldown_ground_target_destroyed - deltatime).max(0.0);
    f117_ai_state.cooldown_air_target_destroyed = (f117_ai_state.cooldown_air_target_destroyed - deltatime).max(0.0);
    f117_ai_state.cooldown_fighters_nearby = (f117_ai_state.cooldown_fighters_nearby - deltatime).max(0.0);
    f117_ai_state.cooldown_sam_pulse_nearby = (f117_ai_state.cooldown_sam_pulse_nearby - deltatime).max(0.0);
    f117_ai_state.cooldown_sam_doppler_nearby = (f117_ai_state.cooldown_sam_doppler_nearby - deltatime).max(0.0);
    f117_ai_state.cooldown_sam_missiles_incoming = (f117_ai_state.cooldown_sam_missiles_incoming - deltatime).max(0.0);
    f117_ai_state.cooldown_aam_missiles_incoming = (f117_ai_state.cooldown_aam_missiles_incoming - deltatime).max(0.0);
    f117_ai_state.cooldown_missiles_defeated = (f117_ai_state.cooldown_missiles_defeated - deltatime).max(0.0);
    f117_ai_state.cooldown_damaged = (f117_ai_state.cooldown_damaged - deltatime).max(0.0);
    f117_ai_state.cooldown_engine_damage = (f117_ai_state.cooldown_engine_damage - deltatime).max(0.0);

    if f117_ai_state.active_line.is_some() {
        f117_ai_state.active_time += deltatime;
        if f117_ai_state.active_time > 4.0 {
            f117_ai_state.active_line = None;
            f117_ai_state.active_time = 0.0;
        }
    }

}

// Call this function to select a random line from the list and start the timer.
pub fn activate_f117ai (
    mut f117_ai_state: ResMut<F117AIState>,
    f117_ai_handle: Res<F117AIHandle>,
    f117_ai_res: Res<Assets<F117AI>>,
    event_type: F117AIEvent,
) {

    let f117_ai_lines = f117_ai_res.get(&f117_ai_handle.0).unwrap();

    if f117_ai_state.active_line.is_none() {
        #[allow(unused_assignments)]
        let mut lines: Vec<String> = Vec::new();
        match event_type {
            F117AIEvent::Takeoff => {
                lines = f117_ai_lines.lines_takeoff.clone();
            }
            F117AIEvent::Landing => {
                lines = f117_ai_lines.lines_landing.clone();
            }
            F117AIEvent::Detected => {
                lines = f117_ai_lines.lines_detected.clone();
            }
            F117AIEvent::GroundTargetLocked => {
                lines = f117_ai_lines.lines_ground_target_locked.clone();
            }
            F117AIEvent::AirTargetLocked => {
                lines = f117_ai_lines.lines_air_target_locked.clone();
            }
            F117AIEvent::GroundTargetDestroyed => {
                lines = f117_ai_lines.lines_ground_target_destroyed.clone();
            }
            F117AIEvent::AirTargetDestroyed => {
                lines = f117_ai_lines.lines_air_target_destroyed.clone();
            }
            F117AIEvent::FightersNearby => {
                lines = f117_ai_lines.lines_fighters_nearby.clone();
            }
            F117AIEvent::SAMPulseNearby => {
                lines = f117_ai_lines.lines_sam_pulse_nearby.clone();
            }
            F117AIEvent::SAMDopplerNearby => {
                lines = f117_ai_lines.lines_sam_doppler_nearby.clone();
            }
            F117AIEvent::SAMMissilesIncoming => {
                lines = f117_ai_lines.lines_sam_missiles_incoming.clone();
            }
            F117AIEvent::AAMMissilesIncoming => {
                lines = f117_ai_lines.lines_aam_missiles_incoming.clone();
            }
            F117AIEvent::MissilesDefeated => {
                lines = f117_ai_lines.lines_missiles_defeated.clone();
            }
            F117AIEvent::Damaged => {
                lines = f117_ai_lines.lines_damaged.clone();
            }
            F117AIEvent::EngineDamage => {
                lines = f117_ai_lines.lines_engine_damage.clone();
            }

            
        }

        // Select a random line out of the given possibilities
        let selected_num = random_u64(0,lines.len() as u64);
        f117_ai_state.display_line = lines[selected_num as usize].clone();
        f117_ai_state.active_time = 0.0;
        println!("AI: {}", f117_ai_state.display_line);
    }
}