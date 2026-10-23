//! The sides' means of war: materiel from the places they hold, spent on the AI's operations
//! and repairs; the AI's aircraft on the ground, which the launches take and the landings give
//! back; pilots down in enemy territory waiting for a rescue. Only the AI runs short: the
//! players always have aircraft and a base to fly from.

use crate::CampaignEvent;
use crate::state::*;
use crate::tuning::*;

/// Seconds of income settled at once.
const INCOME_STEP: f64 = 10.0;

impl CampaignState {
    /// Income for the time since the last settlement, and the hourly purchase of aircraft.
    pub(crate) fn supply(&mut self) {
        let elapsed = self.clock - self.income_at;
        if elapsed >= INCOME_STEP {
            self.income_at = self.clock;
            for side in [Side::Blue, Side::Red] {
                let income = self.income(side) * elapsed / 3600.0;
                let f = self.forces_mut(side);
                // In 64ths: exact in binary, so a save gives it back as it was.
                let was = f.materiel;
                f.materiel = ((f.materiel + income).min(MATERIEL_MAX) * 64.0).round() / 64.0;
                f.ledger.earned += f.materiel - was;
            }
        }
        if self.clock - self.resupply_at < RESUPPLY_EVERY {
            return;
        }
        self.resupply_at = self.clock;
        let skill = (self.setup.opponents as usize).min(3);
        for side in [Side::Blue, Side::Red] {
            let (most_f, most_h) = (2 * self.fighters_start(side), 2 * HELICOPTERS_START);
            let (nf, nh) = match side {
                Side::Blue => (RESUPPLY_FIGHTERS, RESUPPLY_HELICOPTERS),
                _ => (ENEMY_RESUPPLY_FIGHTERS[skill], ENEMY_RESUPPLY_HELICOPTERS[skill]),
            };
            let f = self.forces_mut(side);
            let buy = |f: &mut Forces, fighter: bool| {
                let (n, most, cost) = if fighter {
                    (&mut f.fighters, most_f, FIGHTER_COST)
                } else {
                    (&mut f.helicopters, most_h, HELICOPTER_COST)
                };
                if *n >= most || f.materiel < cost {
                    return false;
                }
                *n += 1;
                f.materiel -= cost;
                f.ledger.aircraft += cost;
                true
            };
            // Helicopters first: the offensive runs on them.
            for _ in 0..nh {
                buy(f, false);
            }
            for _ in 0..nf {
                buy(f, true);
            }
            // What the stores hold beyond the reserve goes on aircraft too, helicopters and
            // fighters in turn.
            let mut fighter = false;
            while f.materiel > MATERIEL_RESERVE {
                if !buy(f, fighter) && !buy(f, !fighter) {
                    break;
                }
                fighter = !fighter;
            }
        }
    }

    /// Materiel a side gets an hour: a base amount, its places and their intact cities.
    pub fn income(&self, side: Side) -> f64 {
        let (places, cities) = self.producers(side);
        INCOME_BASE + INCOME_PLACE * places as f64 + INCOME_CITY * cities as f64
    }

    /// What a side's income comes from: the places it holds, the intact cities among them.
    pub fn producers(&self, side: Side) -> (usize, usize) {
        let cities = self
            .sites
            .iter()
            .filter(|s| s.kind == SiteKind::City && s.alive())
            .filter(|s| s.place.is_some_and(|p| self.places[p as usize].owner == side))
            .count();
        (self.held(side), cities)
    }

    /// The most fighters a side keeps (twice what it started with), and helicopters.
    pub fn aircraft_limits(&self, side: Side) -> (u32, u32) {
        (2 * self.fighters_start(side), 2 * HELICOPTERS_START)
    }

    fn fighters_start(&self, side: Side) -> u32 {
        match side {
            Side::Blue => FIGHTERS_START,
            _ => FIGHTERS_START + ENEMY_FIGHTERS_PER_SKILL * self.setup.opponents.min(3) as u32,
        }
    }

    /// A side pays for something if it can; false: not enough materiel.
    pub(crate) fn pay(&mut self, side: Side, cost: f64, on: Spend) -> bool {
        let f = self.forces_mut(side);
        if f.materiel < cost {
            return false;
        }
        f.materiel -= cost;
        let l = &mut f.ledger;
        *match on {
            Spend::Repairs => &mut l.repairs,
            Spend::Operations => &mut l.operations,
            Spend::Aircraft => &mut l.aircraft,
        } += cost;
        true
    }

    /// A loss that costs a side materiel whatever it has left.
    pub(crate) fn lose(&mut self, side: Side, cost: f64) {
        let f = self.forces_mut(side);
        let was = f.materiel;
        f.materiel = (f.materiel - cost).max(0.0);
        f.ledger.lost += was - f.materiel;
    }

    /// A side's aircraft in the air now: (fighters, strike aircraft, helicopters).
    pub fn airborne(&self, side: Side) -> (usize, usize, usize) {
        let fighters = self.patrols.iter().filter(|p| p.1 == side).count()
            + if side == Side::Blue { self.caps.iter().map(|c| c.tags.len()).sum() } else { 0 };
        let alive = |a: &[bool]| a.iter().filter(|&&x| x).count();
        let strike = self
            .raids
            .iter()
            .filter(|r| r.side == side && r.state == RaidState::Enroute)
            .map(|r| alive(&r.alive))
            .sum();
        let helos = self
            .insertions
            .iter()
            .filter(|i| i.side == side && i.state == InsertionState::Enroute)
            .map(|i| alive(&i.alive))
            .sum();
        (fighters, strike, helos)
    }

    /// `n` of a side's fighters take off if it has them (all or none).
    pub(crate) fn take_fighters(&mut self, side: Side, n: u32) -> bool {
        let f = self.forces_mut(side);
        if f.fighters < n {
            return false;
        }
        f.fighters -= n;
        true
    }

    /// A campaign aircraft landed: back on the ground, ready for the next launch.
    pub(crate) fn airframe_home(&mut self, tag: u32) {
        if let Some(&(_, side, _)) = self.patrols.iter().find(|p| p.0 == tag) {
            self.aircraft_back(side, 1, 0);
        } else if self.caps.iter().any(|c| c.tags.contains(&tag)) {
            self.aircraft_back(Side::Blue, 1, 0);
        } else if let Some(side) =
            self.raids.iter().find(|r| !r.drones && r.aircraft.contains(&tag)).map(|r| r.side)
        {
            self.aircraft_back(side, 1, 0);
        } else if let Some(side) =
            self.insertions.iter().find(|i| i.helicopters.contains(&tag)).map(|i| i.side)
        {
            self.aircraft_back(side, 0, 1);
        }
    }

    /// Fighters and helicopters back on a side's ground, no more than it keeps (the players'
    /// reserve helicopters come back too, but do not swell the pool).
    pub(crate) fn aircraft_back(&mut self, side: Side, fighters: u32, helicopters: u32) {
        let (most_f, most_h) = self.aircraft_limits(side);
        let f = self.forces_mut(side);
        f.fighters = (f.fighters + fighters).min(most_f.max(f.fighters));
        f.helicopters = (f.helicopters + helicopters).min(most_h.max(f.helicopters));
    }

    /// After loading a save the aircraft that were in the air are back on the ground (the
    /// simulation starts afresh).
    pub(crate) fn ground_everything(&mut self) {
        let patrols: Vec<Side> = self.patrols.iter().map(|p| p.1).collect();
        for side in patrols {
            self.aircraft_back(side, 1, 0);
        }
        let caps: usize = self.caps.iter().map(|c| c.tags.len()).sum();
        self.aircraft_back(Side::Blue, caps as u32, 0);
        // Drones are no fighters: the ones in the air are spent.
        let raids: Vec<(Side, u32)> = self
            .raids
            .iter()
            .filter(|r| r.state == RaidState::Enroute && !r.drones)
            .map(|r| (r.side, r.alive.iter().filter(|&&a| a).count() as u32))
            .collect();
        for (side, n) in raids {
            self.aircraft_back(side, n, 0);
        }
        let helos: Vec<(Side, u32)> = self
            .insertions
            .iter()
            .filter(|i| i.state == InsertionState::Enroute)
            .map(|i| (i.side, i.alive.iter().filter(|&&a| a).count() as u32))
            .collect();
        for (side, n) in helos {
            self.aircraft_back(side, 0, n);
        }
    }

    /// A player ejected at (x, y): over enemy territory (the nearest place held by a side is
    /// the enemy's) he waits for a rescue.
    pub(crate) fn ejected(
        &mut self,
        player: PlayerId,
        x: i32,
        y: i32,
        ev: &mut Vec<CampaignEvent>,
    ) {
        let near = self
            .places
            .iter()
            .filter(|p| p.owner != Side::Neutral)
            .min_by_key(|p| dist(p.x - x, p.y - y));
        let Some(near) = near.filter(|p| p.owner == Side::Red) else { return };
        let place = near.name.clone();
        let name = self.player_name(player).unwrap_or("A pilot").to_string();
        let id = self.id();
        self.downed.push(Downed {
            id,
            player,
            name: name.clone(),
            x,
            y,
            until: self.clock + RESCUE_WINDOW,
            state: DownedState::Waiting,
        });
        self.board_at = f64::MIN;
        let text = format!(
            "{name} is down near {place}: a rescue is needed within {} min",
            (RESCUE_WINDOW / 60.0).round()
        );
        self.note(text.clone());
        ev.push(CampaignEvent::Message(text));
    }

    /// Downed pilots nobody rescued in time: the campaign's own helicopter may still get them
    /// out, else they are captured (which costs the AI materiel and a fighter, never the
    /// players their aircraft).
    pub(crate) fn run_downed(&mut self, ev: &mut Vec<CampaignEvent>) {
        let now = self.clock;
        for k in 0..self.downed.len() {
            let d = &self.downed[k];
            if d.state != DownedState::Waiting || now < d.until || self.heli_rescue(d.id) {
                continue;
            }
            let (id, name) = (d.id, d.name.clone());
            let lucky = self.blue.helicopters > 0 && self.rng.below(100) < RESCUE_AI_CHANCE;
            if lucky {
                self.downed[k].state = DownedState::Rescued;
                let text = format!("A rescue helicopter got {name} out");
                self.note(text.clone());
                ev.push(CampaignEvent::Message(text));
            } else {
                self.downed[k].state = DownedState::Captured;
                self.lose(Side::Blue, CAPTURE_COST);
                self.blue.fighters = self.blue.fighters.saturating_sub(1);
                let text = format!("{name} was captured by the enemy");
                self.note(text.clone());
                ev.push(CampaignEvent::Message(text));
            }
            self.settle_rescue(id, lucky);
        }
        // Old rescues go (the debriefs have them).
        self.downed.retain(|d| {
            matches!(d.state, DownedState::Waiting | DownedState::Aboard)
                || now - d.until < RESCUE_WINDOW
        });
    }

    /// A player's helicopter is on its way for downed pilot `id` (he waits for it).
    fn heli_rescue(&self, id: u32) -> bool {
        self.players.iter().filter_map(|p| p.sortie.as_ref()).any(|s| {
            s.progress.heli
                && matches!(s.offer.objective, Objective::Rescue { downed } if downed == id)
        })
    }

    /// The helicopter with downed pilot `id` aboard was lost: he is captured.
    pub(crate) fn lost_aboard(&mut self, id: u32, ev: &mut Vec<CampaignEvent>) {
        let Some(d) = self.downed.iter_mut().find(|d| d.id == id) else { return };
        if d.state != DownedState::Aboard {
            return;
        }
        d.state = DownedState::Captured;
        let text = format!("{} was lost with the helicopter", d.name);
        self.lose(Side::Blue, CAPTURE_COST);
        self.note(text.clone());
        ev.push(CampaignEvent::Message(text));
        self.settle_rescue(id, false);
    }

    /// A rescue sortie covered the helicopter in: the pilot is out.
    pub(crate) fn rescued(&mut self, id: u32, ev: &mut Vec<CampaignEvent>) {
        let Some(d) = self.downed.iter_mut().find(|d| d.id == id) else { return };
        if !matches!(d.state, DownedState::Waiting | DownedState::Aboard) {
            return;
        }
        d.state = DownedState::Rescued;
        let text = format!("{} rescued!", d.name);
        self.note(text.clone());
        ev.push(CampaignEvent::Message(text));
        self.settle_rescue(id, true);
    }

    /// A rescue is over: settles the rescue sorties that already ended.
    fn settle_rescue(&mut self, id: u32, rescued: bool) {
        for r in &mut self.history {
            if r.downed == Some(id) {
                crate::record::settle(r, rescued);
            }
        }
    }
}
