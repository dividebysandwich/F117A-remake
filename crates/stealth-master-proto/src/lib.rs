//! The server list's REST API (JSON over HTTPS), shared by the master server, the game
//! servers that report to it and the games that list them.
//!
//! * `POST /api/servers` with a [`ServerReport`]: a game server is there (again); the answer
//!   is a [`Registered`]. The server's address is where the request came from, with the
//!   report's UDP port.
//! * `DELETE /api/servers/<port>`: the game server at the request's address goes.
//! * `GET /api/servers`: the servers there now, as [`ServerEntry`]s.
//! * `GET /`: the same as a web page.

use serde::{Deserialize, Serialize};

/// The master server the game reports to and lists from.
pub const DEFAULT_MASTER: &str = "https://sm-master.playspoon.com";
/// Seconds between a game server's reports, and after which one not heard of is gone.
pub const REPORT_EVERY: u64 = 30;
pub const EXPIRE_AFTER: u64 = 90;
/// Limits on what a report may carry.
pub const MAX_TEXT: usize = 64;
pub const MAX_PLAYERS: usize = 64;

/// A game server's state.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerReport {
    /// The coop protocol (a game joins only its own).
    pub protocol: u16,
    /// The game's version.
    pub version: String,
    pub name: String,
    /// UDP port the players join on.
    pub port: u16,
    /// A dedicated server (else a player's game hosts).
    pub dedicated: bool,
    pub max_players: u16,
    /// Theater name, the campaign's clock ("day 3 14:05"), situation, enemy skill, rules.
    pub theater: String,
    pub clock: String,
    pub situation: String,
    pub opponents: String,
    pub rules: Vec<String>,
    /// Places (and airfields among them) each side holds.
    pub sides: Vec<SideHolding>,
    pub players: Vec<ReportPlayer>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SideHolding {
    pub side: String,
    pub places: u16,
    pub airfields: u16,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReportPlayer {
    pub name: String,
    /// "On the mission board", "Flying Strike Tabor (F-16)", ...
    pub status: String,
    pub host: bool,
}

/// The master server's answer to a report: when to report again.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Registered {
    pub every: u64,
}

/// A listed server: its address ("host:port", to join) and its last report, seconds ago.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerEntry {
    pub addr: String,
    pub age: u64,
    pub report: ServerReport,
}

impl ServerReport {
    /// Cut to the limits (a master server takes nothing longer).
    pub fn trimmed(mut self) -> Self {
        let cut = |s: &mut String| {
            if let Some((i, _)) = s.char_indices().nth(MAX_TEXT) {
                s.truncate(i);
            }
        };
        for s in [
            &mut self.version,
            &mut self.name,
            &mut self.theater,
            &mut self.clock,
            &mut self.situation,
            &mut self.opponents,
        ] {
            cut(s);
        }
        self.rules.truncate(16);
        self.rules.iter_mut().for_each(cut);
        self.sides.truncate(4);
        self.sides.iter_mut().for_each(|s| cut(&mut s.side));
        self.players.truncate(MAX_PLAYERS);
        for p in &mut self.players {
            cut(&mut p.name);
            cut(&mut p.status);
        }
        self
    }
}
