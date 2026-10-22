//! Coop multiplayer for the dynamic campaign: a host (a listen server playing along, or a
//! dedicated server) runs the campaign and the one simulation; clients fly their own aircraft
//! and keep a mirror of the host's simulation (`stealth_sim::net`).
//!
//! * [`transport`]: connections on UDP with a reliable and an unreliable channel.
//! * [`protocol`]: the messages and their encoding.
//! * [`server`]: the host's side, [`client`]: a player's.
//! * [`dedicated`]: the server without a game of its own.
//! * [`master`]: the public server list.

pub mod client;
#[cfg(not(target_arch = "wasm32"))]
pub mod console;
#[cfg(not(target_arch = "wasm32"))]
pub mod dedicated;
pub mod master;
pub mod protocol;
pub mod server;
pub mod transport;
#[cfg(target_arch = "wasm32")]
mod websocket;

pub use protocol::{DEFAULT_PORT, PROTOCOL};
