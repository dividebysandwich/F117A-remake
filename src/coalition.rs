use bevy::prelude::*;

#[derive(PartialEq, Eq)]
pub enum CoalitionType {
    RED,
    BLUE,
}

#[derive(Component)]
pub struct Coalition {
    pub side: CoalitionType,
}
