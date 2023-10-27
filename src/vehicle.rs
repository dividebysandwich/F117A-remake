use bevy::prelude::*;

use crate::util::*;

#[derive(Component)]
pub struct Vehicle {
    pub serialnumber : u64,
}

impl Default for Vehicle {
    fn default() -> Self {
         Vehicle {
            serialnumber: get_serial_number(),
         }
    }
}
