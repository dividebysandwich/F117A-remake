//! The dynamic campaign's host without the renderer: the campaign model (stealth-campaign)
//! driving the combat simulation (stealth-sim) for all its players. The single-player game,
//! a listen server and a dedicated server all run their campaign through [`Host`].

pub mod assets;
pub mod glue;
pub mod host;
pub mod rules;
pub mod save;
pub mod sky;
pub mod store;
pub mod theater;

pub use host::{Debrief, Host, HostEvent, Stores};
pub use rules::Rules;
