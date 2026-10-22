//! Events of opportunity and support: the enemy's AWACS, its drone waves (the Ukraine theater),
//! and the AI SEAD a player calls in for his package.

use crate::CampaignEvent;
use crate::state::*;
use crate::tuning::*;

impl CampaignState {
    /// The enemy's AWACS: now and then it goes up to orbit behind its front for an hour, guiding
    /// its fighters (see `enemy_air`), then it is called home.
    pub(crate) fn run_awacs(&mut self, ev: &mut Vec<CampaignEvent>) {
        let now = self.clock;
        if let Some(a) = &self.awacs {
            if now >= a.until {
                let tag = a.tag;
                self.awacs = None;
                self.board_at = f64::MIN;
                self.note("The enemy AWACS turns for home");
                ev.push(CampaignEvent::Recall { tag });
            }
            return;
        }
        if now - self.awacs_at < AWACS_EVERY {
            return;
        }
        self.awacs_at = now;
        // Behind the enemy's place nearest the friendly side, away from it.
        let Some(&f) = self.front(Side::Blue).first() else { return };
        let p = &self.places[f as usize];
        let Some(b) = self
            .places
            .iter()
            .filter(|q| q.owner == Side::Blue)
            .min_by_key(|q| dist(q.x - p.x, q.y - p.y))
        else {
            return;
        };
        let d = dist(p.x - b.x, p.y - b.y).max(1);
        let x = (p.x + (p.x - b.x) * AWACS_STANDOFF / d).clamp(0x200, 0x7E00);
        let y = (p.y + (p.y - b.y) * AWACS_STANDOFF / d).clamp(0x200, 0x7E00);
        let name = p.name.clone();
        let Some(from) = self.nearest_base(Side::Red, x, y) else { return };
        if !self.pay(Side::Red, AWACS_COST, Spend::Operations) {
            return;
        }
        let tag = self.id();
        self.awacs = Some(Awacs { tag, side: Side::Red, x, y, until: now + AWACS_ENDURANCE });
        self.board_at = f64::MIN;
        let text = format!(
            "Intelligence: an enemy AWACS goes up behind {name}; it guides the enemy's fighters until it is shot down"
        );
        self.note(text.clone());
        ev.push(CampaignEvent::Message(text));
        ev.push(CampaignEvent::LaunchAwacs { tag, side: Side::Red, from, x, y });
    }

    /// A campaign aircraft shot down (`lost`) or landed: if it was the AWACS, it is gone.
    pub(crate) fn awacs_gone(&mut self, tag: u32, lost: bool, ev: &mut Vec<CampaignEvent>) {
        if self.awacs.as_ref().is_none_or(|a| a.tag != tag) {
            return;
        }
        self.awacs = None;
        self.board_at = f64::MIN;
        if lost {
            self.awacs_down.push(tag);
            if self.awacs_down.len() > 16 {
                self.awacs_down.remove(0);
            }
            self.lose(Side::Red, AWACS_LOSS);
            self.note("Enemy AWACS shot down");
            ev.push(CampaignEvent::Message("Enemy AWACS shot down!".into()));
        } else {
            self.note("The enemy AWACS landed");
        }
    }

    /// The Ukraine theater: now and then the enemy launches a wave of attack drones at a
    /// friendly city (else an airfield) within their reach.
    pub(crate) fn drone_waves(&mut self, ev: &mut Vec<CampaignEvent>) {
        if !self.setup.theater_code.eq_ignore_ascii_case("ua") {
            return;
        }
        let skill = (self.setup.opponents as usize).min(3);
        let now = self.clock;
        if now - self.drones_at < DRONES_EVERY[skill] {
            return;
        }
        let retry = now - DRONES_EVERY[skill] + 60.0;
        let mut all: Vec<(ObjectId, PlaceId, ObjectId, i32, bool)> = self
            .sites
            .iter()
            .filter(|s| s.alive() && matches!(s.kind, SiteKind::City | SiteKind::Airfield))
            .filter(|s| self.owner(s.object) == Some(Side::Blue))
            .filter(|s| !self.raids.iter().any(|r| r.open() && r.target == s.object))
            .filter_map(|s| {
                let from = self.nearest_base(Side::Red, s.x, s.y)?;
                let b = self.site(from)?;
                let d = dist(b.x - s.x, b.y - s.y);
                (d <= DRONE_RANGE).then_some((
                    s.object,
                    s.place?,
                    from,
                    d,
                    s.kind == SiteKind::City,
                ))
            })
            .collect();
        // Cities first, the nearest of them.
        all.sort_by_key(|t| (!t.4, t.3));
        all.truncate(2);
        if all.is_empty() || !self.pay(Side::Red, DRONE_COST, Spend::Operations) {
            self.drones_at = retry;
            return;
        }
        let (target, place, from, _, _) = all[self.rng.below(all.len() as i32) as usize];
        self.drones_at = now;
        let id = self.id();
        self.raids.push(Raid {
            id,
            side: Side::Red,
            place,
            target,
            from,
            aircraft: Vec::new(),
            alive: Vec::new(),
            state: RaidState::Pending { launch_at: now + DRONE_LEAD },
            ended: None,
            escorted: true,
            drones: true,
            enroute_at: None,
        });
        let text = format!(
            "Intelligence: enemy attack drones to be launched at {} in {} min",
            self.target_name(target),
            (DRONE_LEAD / 60.0).round()
        );
        self.note(text.clone());
        self.board_at = f64::MIN;
        ev.push(CampaignEvent::Message(text));
    }

    /// A player calls in AI SEAD for an offer's place: strike aircraft of the friendly side go
    /// for one of its SAM sites (paid with the side's materiel and fighters).
    pub(crate) fn request_support(
        &mut self,
        player: PlayerId,
        offer: u32,
    ) -> Result<Vec<CampaignEvent>, String> {
        let o = self.board.iter().find(|o| o.id == offer).ok_or("That mission is gone")?;
        let place = self.objective_place(&o.objective).ok_or("No place to support")?;
        let p = &self.places[place as usize];
        if p.owner != Side::Red {
            return Err("Support goes against enemy places".into());
        }
        if self.raids.iter().any(|r| r.side == Side::Blue && r.open() && r.place == place) {
            return Err(format!("AI SEAD for {} is already on its way", p.name));
        }
        let sam = p
            .defences
            .iter()
            .copied()
            .find(|&d| self.site(d).is_some_and(Site::alive))
            .ok_or_else(|| format!("{} has no SAM site left", p.name))?;
        let s = self.site(sam).expect("a site");
        let (sx, sy) = (s.x, s.y);
        let from = self
            .nearest_base(Side::Blue, sx, sy)
            .filter(|&b| self.site(b).is_some_and(|b| dist(b.x - sx, b.y - sy) <= RAID_RANGE))
            .ok_or("No friendly airfield in range")?;
        if (self.blue.fighters as usize) < RAID_AIRCRAFT {
            return Err("No strike aircraft left on the ground".into());
        }
        let (name, base) = (p.name.clone(), self.site(from).map(|s| s.name.clone()));
        if !self.pay(Side::Blue, SUPPORT_COST, Spend::Operations) {
            return Err("Not enough materiel".into());
        }
        let id = self.id();
        self.raids.push(Raid {
            id,
            side: Side::Blue,
            place,
            target: sam,
            from,
            aircraft: Vec::new(),
            alive: Vec::new(),
            state: RaidState::Pending { launch_at: self.clock + SUPPORT_LEAD },
            ended: None,
            escorted: true,
            drones: false,
            enroute_at: None,
        });
        let who = self.player_name(player).unwrap_or("A pilot");
        let text = format!(
            "{who} called in AI SEAD on {name}: {RAID_AIRCRAFT} strike aircraft from {} in {} min",
            base.unwrap_or_default(),
            (SUPPORT_LEAD / 60.0).round()
        );
        self.note(text.clone());
        Ok(vec![CampaignEvent::Message(text)])
    }
}
