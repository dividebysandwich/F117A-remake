#![allow(dead_code)]

use bevy::prelude::*;
use bevy::reflect::TypePath;
use ::serde::Deserialize;

/* Someone installed an experimental AI in your stealth jet. 
   It may occasionally comment on the situation and provide hints */

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


pub fn load_f117_ai(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
) {

    let ai = F117AIHandle(asset_server.load("ai/f117_ai.toml"));
    commands.insert_resource(ai);

}