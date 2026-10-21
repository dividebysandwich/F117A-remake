//! The players' sorties: taking a mission, following its objective, ending it.

use crate::state::*;
use crate::tuning::*;
use crate::{CampaignEvent, Report, WorldView};

impl CampaignState {
    pub(crate) fn take_mission(
        &mut self,
        player: PlayerId,
        offer: u32,
        base: Option<ObjectId>,
    ) -> Result<Vec<CampaignEvent>, String> {
        let Some(o) = self.board.iter().find(|o| o.id == offer).cloned() else {
            return Err("That mission is no longer on the board".into());
        };
        if o.kind.ai_flown() {
            return Err("Insertions are flown by the helicopter crews: order one instead".into());
        }
        if self.sortie(player).is_some() {
            return Err("Already flying a sortie".into());
        }
        let base = base.unwrap_or(o.base);
        if !self.bases(Side::Blue).contains(&base) {
            return Err("Not a friendly base".into());
        }
        let sortie = Sortie {
            id: self.id(),
            offer: o.clone(),
            base,
            started: self.clock,
            progress: Progress::default(),
        };
        let from = self.site(base).map(|s| s.name.clone()).unwrap_or_default();
        self.note(format!("{} from {from}", o.title));
        self.slot_mut(player).sortie = Some(sortie.clone());
        Ok(vec![CampaignEvent::SortieStarted { player, sortie }])
    }

    pub(crate) fn end_sortie(&mut self, player: PlayerId, end: SortieEnd) -> Option<CampaignEvent> {
        let s = self.slot_mut(player).sortie.take()?;
        let insertion = match s.offer.objective {
            Objective::Escort { insertion } => Some(insertion),
            _ => None,
        };
        let escort_open = insertion.and_then(|i| self.insertion(i)).is_some_and(|i| {
            matches!(i.state, InsertionState::Pending { .. } | InsertionState::Enroute)
        });
        let result = if s.progress.achieved {
            SortieResult::Success
        } else if escort_open {
            SortieResult::Pending
        } else {
            SortieResult::Failed
        };
        let record = SortieRecord {
            id: s.id,
            player,
            kind: s.offer.kind,
            title: s.offer.title.clone(),
            end,
            result,
            started: s.started,
            ended: self.clock,
            air_kills: s.progress.air_kills,
            ground_kills: (s.progress.sites_destroyed.len() + s.progress.aaa_destroyed.len())
                as u32,
            insertion,
        };
        let verdict = match result {
            SortieResult::Success => "objective achieved",
            SortieResult::Failed => "objective not achieved",
            SortieResult::Pending => "waiting for the insertion",
        };
        self.note(format!("{}: {} - {verdict}", s.offer.title, end.name()));
        self.history.push(record.clone());
        Some(CampaignEvent::SortieFinished { player, record })
    }

    /// A report's effect on the sorties being flown.
    pub(crate) fn sortie_report(&mut self, r: &Report) {
        for slot in &mut self.players {
            let Some(s) = slot.sortie.as_mut() else { continue };
            let mine = |by: &Option<PlayerId>| *by == Some(slot.id);
            match (r, &s.offer.objective) {
                (Report::ObjectDestroyed { object, by }, _) if mine(by) => {
                    s.progress.sites_destroyed.push(*object);
                }
                (Report::AaaDestroyed { index, by }, _) if mine(by) => {
                    s.progress.aaa_destroyed.push(*index);
                }
                (Report::FlightDestroyed { side: Side::Red, x, y, by, .. }, obj) if mine(by) => {
                    s.progress.air_kills += 1;
                    if let Objective::Cap { x: cx, y: cy } = obj
                        && dist(x - cx, y - cy) < CAP_RADIUS
                    {
                        s.progress.on_station += CAP_SECONDS / CAP_KILLS as f64;
                    }
                }
                (
                    Report::FlightDestroyed { side: Side::Blue, x, y, .. },
                    Objective::Cap { x: cx, y: cy },
                ) if dist(x - cx, y - cy) < CAP_RADIUS => {
                    s.progress.losses += 1;
                }
                (Report::PhotoTaken { player, object }, Objective::Recon { object: o, .. })
                    if *player == slot.id && object == o =>
                {
                    s.progress.photo = true;
                }
                _ => {}
            }
        }
    }

    /// Time on station and the objectives' state, every tick.
    pub(crate) fn track_sorties(&mut self, dt: f64, view: &WorldView, ev: &mut Vec<CampaignEvent>) {
        let mut achieved = Vec::new();
        for k in 0..self.players.len() {
            let id = self.players[k].id;
            let Some(s) = self.players[k].sortie.as_ref() else { continue };
            let mut p = s.progress.clone();
            if let Objective::Cap { x, y } = s.offer.objective
                && let Some(me) = view.players.iter().find(|p| p.id == id)
                && me.airborne
                && dist(me.x - x, me.y - y) < CAP_RADIUS
            {
                p.on_station += dt;
            }
            if !p.achieved && self.objective_met(&s.offer.objective, &p) {
                p.achieved = true;
                achieved.push((id, s.offer.title.clone(), s.offer.objective.clone()));
            }
            self.players[k].sortie.as_mut().expect("checked").progress = p;
        }
        for (player, title, objective) in achieved {
            if let Objective::Recon { place, .. } = objective {
                self.gain_intel(place, ev);
            }
            self.note(format!("{title}: objective achieved"));
            ev.push(CampaignEvent::ObjectiveAchieved { player, title });
        }
    }

    fn objective_met(&self, o: &Objective, p: &Progress) -> bool {
        let share = |total: usize, done: usize| {
            total > 0 && done as f32 >= (total as f32 * SUPPRESS_SHARE).ceil()
        };
        match o {
            Objective::Recon { .. } => p.photo,
            Objective::Sead { sites, .. } => {
                let done =
                    sites.iter().filter(|&&s| !self.site(s).is_some_and(Site::alive)).count();
                share(sites.len(), done)
            }
            Objective::Cas { aaa, .. } => {
                share(aaa.len(), aaa.iter().filter(|a| p.aaa_destroyed.contains(a)).count())
            }
            Objective::Strike { airfield, .. } => !self.site(*airfield).is_some_and(Site::alive),
            Objective::AntiShip { ship } => !self.site(*ship).is_some_and(Site::alive),
            Objective::Cap { .. } => p.on_station >= CAP_SECONDS && p.losses == 0,
            Objective::Escort { insertion } => {
                self.insertion(*insertion).is_some_and(|i| i.state == InsertionState::Captured)
            }
            Objective::Insertion { .. } => false,
        }
    }

    /// An escorted insertion is over: settles the escorts that already ended their sortie.
    pub(crate) fn settle_escorts(&mut self, insertion: u32, captured: bool) {
        for r in &mut self.history {
            if r.insertion == Some(insertion) && r.result == SortieResult::Pending {
                r.result = if captured { SortieResult::Success } else { SortieResult::Failed };
            }
        }
    }

    /// Recon intelligence on a place: its cities, airfields and defences.
    pub(crate) fn gain_intel(&mut self, place: PlaceId, ev: &mut Vec<CampaignEvent>) {
        let until = self.clock + INTEL_SECONDS;
        let Some(p) = self.places.get_mut(place as usize) else { return };
        p.intel_until = Some(until);
        let objects: Vec<ObjectId> = p.members.iter().chain(&p.defences).copied().collect();
        let name = p.name.clone();
        for &o in &objects {
            if let Some(s) = self.site_mut(o) {
                s.spotted = true;
            }
        }
        self.note(format!("Recon photos of {name}: its defences are mapped"));
        ev.push(CampaignEvent::IntelGained { place, objects });
    }
}
