//! Events of opportunity and support: the enemy's AWACS, its drone waves (the Ukraine theater),
//! its forward batteries, and the AI SEAD a player calls in for his package.

use crate::state::*;
use crate::tuning::*;
use crate::{CampaignEvent, WorldView};

impl CampaignState {
    /// Enemy forward batteries: when no target on the board is a short helicopter flight from
    /// a player base, the enemy pushes a battery of guns forward near the base nearest its
    /// front, on land. It is gone once its guns are.
    pub(crate) fn forward_batteries(&mut self, view: &WorldView, ev: &mut Vec<CampaignEvent>) {
        for k in (0..self.batteries.len()).rev() {
            let b = &self.batteries[k];
            let up = self.guns_within(view, b.x, b.y, Side::Red, BATTERY_RADIUS).next().is_some();
            if up && !b.seen {
                // Its guns stand: on the board at once.
                self.batteries[k].seen = true;
                self.board_at = f64::MIN;
            } else if !up && b.seen {
                let b = self.batteries.remove(k);
                self.board_at = f64::MIN;
                let text = format!("Enemy battery near {} destroyed", b.name);
                self.note(text.clone());
                ev.push(CampaignEvent::Message(text));
            }
        }
        let now = self.clock;
        // Judged on a built board, with the terrain to keep it out of the water.
        if self.terrain.0.is_none()
            || self.board_at == f64::MIN
            || self.batteries.len() >= BATTERIES
            || now - self.battery_at < BATTERY_EVERY
        {
            return;
        }
        self.battery_at = now;
        let bases: Vec<(i32, i32)> = self
            .player_bases()
            .into_iter()
            .filter_map(|b| self.site(b).map(|s| (s.x, s.y)))
            .collect();
        let in_reach = self.board.iter().any(|o| {
            matches!(
                o.kind,
                MissionKind::Recon
                    | MissionKind::Sead
                    | MissionKind::Cas
                    | MissionKind::Strike
                    | MissionKind::AntiShip
                    | MissionKind::Interdiction
            ) && bases.iter().any(|&(x, y)| dist(o.x - x, o.y - y) <= BATTERY_REACH)
        });
        if in_reach {
            return;
        }
        let Some((x, y, name)) = self.battery_spot(&bases) else { return };
        if !self.pay(Side::Red, BATTERY_COST, Spend::Operations) {
            return;
        }
        let id = self.id();
        self.batteries.push(Battery { id, x, y, name: name.clone(), seen: false });
        let text =
            format!("Intelligence: the enemy has pushed a battery of guns forward near {name}");
        self.note(text.clone());
        ev.push(CampaignEvent::Message(text));
        ev.push(CampaignEvent::BatteryDeployed { id, x, y });
    }

    /// Where a forward battery goes: out from one of the player bases nearest the enemy's
    /// front, roughly towards it (else any way), on land (its guns too) and clear of every place, site, base
    /// and other battery; with the name of the nearest place.
    fn battery_spot(&mut self, bases: &[(i32, i32)]) -> Option<(i32, i32, String)> {
        let front: Vec<(i32, i32)> = self
            .front(Side::Blue)
            .into_iter()
            .map(|p| (self.places[p as usize].x, self.places[p as usize].y))
            .collect();
        let mut from: Vec<((i32, i32), (i32, i32), i32)> = bases
            .iter()
            .filter_map(|&b| {
                let f = *front.iter().min_by_key(|f| dist(f.0 - b.0, f.1 - b.1))?;
                Some((b, f, dist(f.0 - b.0, f.1 - b.1)))
            })
            .collect();
        from.sort_by_key(|f| f.2);
        from.truncate(3);
        if from.is_empty() {
            return None;
        }
        let dry = |c: &Self, x: i32, y: i32| c.terrain.is_water(x, y) == Some(false);
        for k in 0..120 {
            let (b, f, _) = from[self.rng.below(from.len() as i32) as usize];
            let toward = ((f.1 - b.1) as f64).atan2((f.0 - b.0) as f64);
            let spread = if k < 60 { 60 } else { 180 };
            let turn = self.rng.below(2 * spread + 1) - spread;
            let a = toward + turn as f64 * std::f64::consts::PI / 180.0;
            let r = (BATTERY_NEAR + self.rng.below(BATTERY_FAR - BATTERY_NEAR)) as f64;
            let (x, y) = (b.0 + (a.cos() * r) as i32, b.1 + (a.sin() * r) as i32);
            let clear = |px: i32, py: i32, d: i32| dist(px - x, py - y) >= d;
            if !(0x200..0x7E00).contains(&x)
                || !(0x200..0x7E00).contains(&y)
                || !self.sites.iter().all(|s| s.removed || clear(s.x, s.y, BATTERY_CLEAR))
                || !self.places.iter().all(|p| clear(p.x, p.y, BATTERY_CLEAR))
                || !bases.iter().all(|&(bx, by)| clear(bx, by, BATTERY_NEAR))
                || !self.batteries.iter().all(|o| clear(o.x, o.y, 2 * BATTERY_CLEAR))
            {
                continue;
            }
            // Its guns stand around it: the ground there is land too.
            let ring = (0..8).map(|k| {
                let t = k as f64 * std::f64::consts::FRAC_PI_4;
                let r = BATTERY_RADIUS as f64;
                (x + (t.cos() * r) as i32, y + (t.sin() * r) as i32)
            });
            if !dry(self, x, y) || !ring.into_iter().all(|(px, py)| dry(self, px, py)) {
                continue;
            }
            let name = self
                .places
                .iter()
                .min_by_key(|p| dist(p.x - x, p.y - y))
                .map_or(String::new(), |p| p.name.clone());
            return Some((x, y, name));
        }
        None
    }

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
            ev.push(CampaignEvent::Message("The enemy AWACS has landed".into()));
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
