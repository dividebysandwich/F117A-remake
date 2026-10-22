//! The pilots' record in the campaign: points for each sortie, the package bonus for
//! objectives flown together, and every pilot's totals (who has left the server too).

use crate::CampaignEvent;
use crate::state::*;
use crate::tuning::*;

/// A pilot's totals over the campaign.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PilotRecord {
    pub name: String,
    pub sorties: usize,
    pub achieved: usize,
    pub air_kills: u32,
    pub ground_kills: u32,
    pub points: i32,
    /// The most points of one sortie (the medals go by it).
    pub best: i32,
}

/// Points of a sortie without its objective's (kills, the aircraft brought home, a bonus).
pub(crate) fn flight_points(air: u32, ground: u32, end: SortieEnd, bonus: i32) -> i32 {
    air as i32 * POINTS_AIR_KILL
        + ground as i32 * POINTS_GROUND_KILL
        + if end == SortieEnd::Landed { POINTS_LANDED } else { 0 }
        + bonus
}

/// A pending sortie's objective is settled: its result, and its points when achieved.
pub(crate) fn settle(r: &mut SortieRecord, success: bool) {
    if r.result != SortieResult::Pending {
        return;
    }
    r.result = if success { SortieResult::Success } else { SortieResult::Failed };
    if success {
        r.points += r.kind.points();
    }
}

impl CampaignState {
    /// Every pilot's totals, the most points first (pilots who left keep theirs; the players
    /// connected now are there without a sortie too).
    pub fn pilot_records(&self) -> Vec<PilotRecord> {
        let mut all: Vec<PilotRecord> = Vec::new();
        let name_of = |r: &SortieRecord| {
            if r.pilot.is_empty() {
                self.player_name(r.player).unwrap_or("?").to_string()
            } else {
                r.pilot.clone()
            }
        };
        for r in &self.history {
            let name = name_of(r);
            let k = match all.iter().position(|p| p.name.eq_ignore_ascii_case(&name)) {
                Some(k) => k,
                None => {
                    all.push(PilotRecord { name, ..Default::default() });
                    all.len() - 1
                }
            };
            let p = &mut all[k];
            p.sorties += 1;
            p.achieved += (r.result == SortieResult::Success) as usize;
            p.air_kills += r.air_kills;
            p.ground_kills += r.ground_kills;
            p.points += r.points;
            p.best = p.best.max(r.points);
        }
        for s in &self.players {
            if !all.iter().any(|p| p.name.eq_ignore_ascii_case(&s.name)) {
                all.push(PilotRecord { name: s.name.clone(), ..Default::default() });
            }
        }
        all.sort_by_key(|p| std::cmp::Reverse((p.points, p.achieved)));
        all
    }

    /// A player's totals (by his name).
    pub fn pilot_record(&self, player: PlayerId) -> PilotRecord {
        let name = self.player_name(player).unwrap_or("");
        self.pilot_records()
            .into_iter()
            .find(|p| p.name.eq_ignore_ascii_case(name))
            .unwrap_or_default()
    }

    /// The place an objective is about (packages, the board).
    pub fn objective_place(&self, o: &Objective) -> Option<PlaceId> {
        match *o {
            Objective::Recon { place, .. }
            | Objective::Sead { place, .. }
            | Objective::Cas { place, .. }
            | Objective::Strike { place, .. }
            | Objective::Interdict { place, .. }
            | Objective::Insertion { place } => Some(place),
            Objective::Escort { insertion } | Objective::Intercept { insertion } => {
                self.insertion(insertion).map(|i| i.place)
            }
            Objective::Defend { raid } => self.raid(raid).map(|r| r.place),
            _ => None,
        }
    }

    /// A player's objective was achieved now: with objectives of another kind achieved at the
    /// same place lately (still flown or not), it is a package and every sortie of it earns
    /// the bonus once.
    pub(crate) fn package(&mut self, player: PlayerId, ev: &mut Vec<CampaignEvent>) {
        let now = self.clock;
        let Some(s) = self.sortie(player) else { return };
        let Some(place) = self.objective_place(&s.offer.objective) else { return };
        let recent = |at: Option<f64>| at.is_some_and(|t| now - t <= PACKAGE_WINDOW);
        // Members: (kind, flying player or history index).
        let mut flying: Vec<(MissionKind, usize)> = Vec::new();
        for (k, slot) in self.players.iter().enumerate() {
            let Some(s) = &slot.sortie else { continue };
            if s.progress.achieved
                && recent(s.progress.achieved_at)
                && self.objective_place(&s.offer.objective) == Some(place)
            {
                flying.push((s.offer.kind, k));
            }
        }
        let done: Vec<(MissionKind, usize)> = self
            .history
            .iter()
            .enumerate()
            .filter(|(_, r)| {
                r.place == Some(place) && r.result == SortieResult::Success && recent(r.achieved_at)
            })
            .map(|(k, r)| (r.kind, k))
            .collect();
        let mut kinds: Vec<MissionKind> = flying.iter().chain(&done).map(|m| m.0).collect();
        kinds.sort_by_key(|k| k.name());
        kinds.dedup();
        if kinds.len() < 2 {
            return;
        }
        let mut names = Vec::new();
        for (_, k) in flying {
            let slot = &mut self.players[k];
            let p = &mut slot.sortie.as_mut().expect("flying").progress;
            if p.bonus == 0 {
                p.bonus = PACKAGE_BONUS;
                names.push(slot.name.clone());
            }
        }
        for (_, k) in done {
            let r = &mut self.history[k];
            if r.bonus == 0 {
                r.bonus = PACKAGE_BONUS;
                r.points += PACKAGE_BONUS;
                names.push(r.pilot.clone());
            }
        }
        if names.is_empty() {
            return;
        }
        let what: Vec<&str> = kinds.iter().map(|k| k.name()).collect();
        let text = format!(
            "Package over {}: {} - +{PACKAGE_BONUS} for {}",
            self.places[place as usize].name,
            what.join(" + "),
            names.join(", ")
        );
        self.note(text.clone());
        ev.push(CampaignEvent::Message(text));
    }
}
