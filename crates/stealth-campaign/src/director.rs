//! The campaign's own decisions: the mission board, insertions, enemy patrols, raids and
//! offensives, repairs, intelligence going stale and the end of the war. Insertions are
//! written for a side: the enemy takes the friendly places the way the friendly side takes
//! its own.

use crate::state::*;
use crate::tuning::*;
use crate::{CampaignEvent, Report, WorldView};

impl CampaignState {
    pub(crate) fn report(&mut self, r: &Report, view: &WorldView, ev: &mut Vec<CampaignEvent>) {
        self.sortie_report(r);
        match *r {
            Report::ObjectDestroyed { object, .. } => {
                let now = self.clock;
                self.raid_hit(object, ev);
                // A city's stores and factories go with it.
                if let Some(s) = self.site(object).filter(|s| s.alive() && s.kind == SiteKind::City)
                    && let Some(side) = self.owner(s.object).filter(|&o| o != Side::Neutral)
                {
                    self.lose(side, CITY_LOSS);
                }
                if let Some(s) = self.site_mut(object).filter(|s| s.alive()) {
                    s.destroyed_at = Some(now);
                    let text = match s.kind {
                        SiteKind::Airfield => Some(format!("{} airfield destroyed", s.name)),
                        SiteKind::Sam => Some(format!("SAM site at {} destroyed", s.name)),
                        SiteKind::Ship => Some("Ship sunk".to_string()),
                        _ => None,
                    };
                    if let Some(t) = text {
                        self.note(t);
                    }
                    // Its offers go from the board at once.
                    self.board_at = f64::MIN;
                }
            }
            Report::SiteSpotted { object } => {
                if let Some(s) = self.site_mut(object).filter(|s| s.alive()) {
                    s.spotted = true;
                }
            }
            Report::AaaDestroyed { index, .. } => self.gun_lost(index, view),
            Report::FlightDestroyed { tag, .. } if tag != 0 => {
                self.patrols.retain(|p| p.0 != tag);
                self.cap_report(tag, true);
                self.raider_gone(tag, ev);
                self.awacs_gone(tag, true, ev);
                let Some(k) = self.insertions.iter().position(|i| i.helicopters.contains(&tag))
                else {
                    return;
                };
                let name = self.places[self.insertions[k].place as usize].name.clone();
                let ins = &mut self.insertions[k];
                let h = ins.helicopters.iter().position(|&t| t == tag).expect("contains");
                ins.alive[h] = false;
                let left = ins.alive.iter().filter(|&&a| a).count();
                let side = ins.side;
                // A player's helicopter carries on alone.
                if ins.state == InsertionState::Enroute && left == 0 && ins.pilots.is_empty() {
                    self.insertion_failed(k, "all helicopters lost", ev);
                } else if ins.state == InsertionState::Enroute {
                    ev.push(CampaignEvent::Message(if side == Side::Blue {
                        format!("Helicopter lost - {left} left for {name}")
                    } else {
                        format!("Enemy helicopter down - {left} left for {name}")
                    }));
                }
            }
            Report::FlightLanded { tag, x, y } if tag != 0 => {
                self.airframe_home(tag);
                self.patrols.retain(|p| p.0 != tag);
                self.cap_report(tag, false);
                self.raider_gone(tag, ev);
                self.awacs_gone(tag, false, ev);
                if self.outcome.is_some() {
                    return;
                }
                let Some(k) = self.insertions.iter().position(|i| {
                    i.state == InsertionState::Enroute && i.helicopters.contains(&tag)
                }) else {
                    return;
                };
                let target = self.insertions[k].target;
                let Some(t) = self.site(target) else { return };
                if dist(t.x - x, t.y - y) <= CAPTURE_RADIUS {
                    self.capture(k, ev);
                    return;
                }
                // Down somewhere else: that one takes nobody there.
                let ins = &mut self.insertions[k];
                let h = ins.helicopters.iter().position(|&t| t == tag).expect("contains");
                ins.alive[h] = false;
                if ins.alive.iter().all(|a| !a) && ins.pilots.is_empty() {
                    self.insertion_failed(k, "the helicopters landed short", ev);
                }
            }
            _ => {}
        }
    }

    /// Insertion `k` in the air failed (`why`, for the friendly side's log).
    pub(crate) fn insertion_failed(&mut self, k: usize, why: &str, ev: &mut Vec<CampaignEvent>) {
        let ins = &mut self.insertions[k];
        ins.state = InsertionState::Failed;
        let (id, place, side) = (ins.id, ins.place, ins.side);
        let name = self.places[place as usize].name.clone();
        self.settle_insertion(id, InsertionState::Failed);
        ev.push(CampaignEvent::InsertionFailed { id, place });
        if side == Side::Blue {
            self.note(format!("Insertion at {name} failed: {why}"));
            ev.push(CampaignEvent::Message(format!("Insertion at {name} failed")));
        } else {
            self.note(format!("Enemy insertion at {name} stopped"));
            ev.push(CampaignEvent::Message(format!("Enemy insertion at {name} stopped!")));
        }
    }

    /// Insertion `k` landed: its place changes hands.
    pub(crate) fn capture(&mut self, k: usize, ev: &mut Vec<CampaignEvent>) {
        let (id, place) = (self.insertions[k].id, self.insertions[k].place);
        self.insertions[k].state = InsertionState::Captured;
        let side = self.insertions[k].side;
        let p = &mut self.places[place as usize];
        p.owner = side;
        p.intel_until = None;
        // The new owner's guns, at full strength.
        p.guns_lost = 0;
        p.guns_repair_at = None;
        let airfield = p.airfield.is_some();
        let members = p.members.clone();
        let removed = std::mem::take(&mut p.defences);
        let name = p.name.clone();
        for &o in &members {
            if let Some(s) = self.site_mut(o) {
                s.destroyed_at = None;
            }
        }
        for &o in &removed {
            if let Some(s) = self.site_mut(o) {
                s.removed = true;
            }
        }
        // Other insertions against it are called off.
        let off: Vec<u32> = self
            .insertions
            .iter_mut()
            .filter(|i| i.place == place && matches!(i.state, InsertionState::Pending { .. }))
            .map(|i| {
                i.state = InsertionState::CalledOff;
                i.id
            })
            .collect();
        for i in off {
            self.settle_insertion(i, InsertionState::CalledOff);
        }
        self.settle_insertion(id, InsertionState::Captured);
        // Aircraft left on a taken airfield.
        if airfield {
            self.aircraft_back(side, CAPTURED_FIGHTERS, CAPTURED_HELICOPTERS);
        }
        if side == Side::Blue {
            self.note(format!("{name} captured"));
            ev.push(CampaignEvent::Message(format!("{name} captured!")));
        } else {
            self.note(format!("{name} lost to the enemy"));
            ev.push(CampaignEvent::Message(format!("{name} lost to the enemy!")));
        }
        ev.push(CampaignEvent::PlaceCaptured { place, side, members, removed });
    }

    /// The friendly side's insertion (a player's order, or the campaign's own).
    pub(crate) fn order_insertion(
        &mut self,
        by: Option<PlayerId>,
        place: PlaceId,
    ) -> Result<Vec<CampaignEvent>, String> {
        self.order_insertion_for(Side::Blue, by, place)
    }

    fn order_insertion_for(
        &mut self,
        side: Side,
        by: Option<PlayerId>,
        place: PlaceId,
    ) -> Result<Vec<CampaignEvent>, String> {
        let Some(p) = self.places.get(place as usize) else { return Err("No such place".into()) };
        if p.owner != side.enemy() {
            return Err(format!("{} is not held by the enemy", p.name));
        }
        // The players order what the board offers: the places on the front.
        if by.is_some() && !self.front(side).contains(&place) {
            return Err(format!("{} is not on the front", p.name));
        }
        if self.active_insertion(place) {
            return Err(format!("An insertion at {} is already under way", p.name));
        }
        let target = p.airfield.or(p.members.first().copied()).ok_or("Nothing to land at")?;
        let (x, y, name) = (p.x, p.y, p.name.clone());
        let from = self
            .insertion_base(side, x, y)
            .ok_or_else(|| format!("No friendly base within helicopter range of {name}"))?;
        // The campaign's own orders need the helicopters and pay for the operation.
        if by.is_none() {
            if (self.forces(side).helicopters as usize) < INSERTION_HELOS {
                return Err("Not enough helicopters".into());
            }
            if !self.pay(side, INSERTION_COST, Spend::Operations) {
                return Err("Not enough materiel".into());
            }
        }
        let id = self.id();
        let lead = if side == Side::Blue { INSERTION_LEAD } else { ENEMY_INSERTION_LEAD };
        self.insertions.push(Insertion {
            id,
            side,
            place,
            from,
            target,
            helicopters: Vec::new(),
            alive: Vec::new(),
            state: InsertionState::Pending { launch_at: self.clock + lead },
            scrambled: false,
            ordered_by: by,
            enroute_at: None,
            pilots: Vec::new(),
        });
        let base = self.site(from).map(|s| s.name.clone()).unwrap_or_default();
        let mins = (lead / 60.0).round();
        let text = if side == Side::Blue {
            let who = if by.is_some() { "ordered" } else { "Command orders" };
            format!("Insertion at {name} {who}: helicopters lift off from {base} in {mins} min")
        } else {
            format!("Intelligence: enemy helicopters lift off from {base} for {name} in {mins} min")
        };
        self.note(text.clone());
        self.board_at = f64::MIN;
        Ok(vec![CampaignEvent::Message(text)])
    }

    /// A player flies an insertion at `place` in his helicopter from `base`: he joins the one
    /// under way there, else leads a new one, his wingmen lifting off with him. Returns its id.
    pub(crate) fn fly_insertion(
        &mut self,
        player: PlayerId,
        place: PlaceId,
        base: ObjectId,
    ) -> Result<(u32, Vec<CampaignEvent>), String> {
        if let Some(i) = self.insertions.iter_mut().find(|i| {
            i.place == place
                && i.side == Side::Blue
                && matches!(i.state, InsertionState::Pending { .. } | InsertionState::Enroute)
        }) {
            i.pilots.push(player);
            return Ok((i.id, Vec::new()));
        }
        let Some(p) = self.places.get(place as usize) else { return Err("No such place".into()) };
        if p.owner != Side::Red {
            return Err(format!("{} is not held by the enemy", p.name));
        }
        let target = p.airfield.or(p.members.first().copied()).ok_or("Nothing to land at")?;
        let name = p.name.clone();
        let id = self.id();
        self.insertions.push(Insertion {
            id,
            side: Side::Blue,
            place,
            from: base,
            target,
            helicopters: Vec::new(),
            alive: Vec::new(),
            state: InsertionState::Pending { launch_at: self.clock },
            scrambled: false,
            ordered_by: Some(player),
            enroute_at: None,
            pilots: vec![player],
        });
        let who = self.player_name(player).unwrap_or("A pilot").to_string();
        let text = format!("Insertion at {name}: {who} leads the helicopters");
        self.note(text.clone());
        self.board_at = f64::MIN;
        Ok((id, vec![CampaignEvent::Message(text)]))
    }

    /// A player's sortie on insertion `id` ended: it goes on with the helicopters left, fails
    /// without any.
    pub(crate) fn left_insertion(
        &mut self,
        player: PlayerId,
        id: u32,
        ev: &mut Vec<CampaignEvent>,
    ) {
        let Some(k) = self.insertions.iter().position(|i| i.id == id) else { return };
        let ins = &mut self.insertions[k];
        ins.pilots.retain(|&p| p != player);
        if ins.pilots.is_empty() && ins.alive.iter().all(|a| !a) {
            match ins.state {
                InsertionState::Enroute => {
                    self.insertion_failed(k, "the helicopters were lost", ev)
                }
                InsertionState::Pending { .. } => {
                    ins.state = InsertionState::CalledOff;
                    self.settle_insertion(id, InsertionState::CalledOff);
                }
                _ => {}
            }
        }
    }

    fn active_insertion(&self, place: PlaceId) -> bool {
        self.insertions.iter().any(|i| {
            i.place == place
                && matches!(i.state, InsertionState::Pending { .. } | InsertionState::Enroute)
        })
    }

    /// The side's base nearest to a point within helicopter range.
    fn insertion_base(&self, side: Side, x: i32, y: i32) -> Option<ObjectId> {
        let b = self.nearest_base(side, x, y)?;
        let s = self.site(b)?;
        (dist(s.x - x, s.y - y) <= INSERTION_RANGE).then_some(b)
    }

    pub(crate) fn run_insertions(&mut self, view: &WorldView, ev: &mut Vec<CampaignEvent>) {
        for k in 0..self.insertions.len() {
            let ins = &self.insertions[k];
            let place = &self.places[ins.place as usize];
            let side = ins.side;
            match ins.state {
                InsertionState::Pending { .. } if place.owner != side.enemy() => {
                    let id = ins.id;
                    self.insertions[k].state = InsertionState::CalledOff;
                    self.settle_insertion(id, InsertionState::CalledOff);
                }
                InsertionState::Pending { launch_at } if self.clock >= launch_at => {
                    // The base may have been lost meanwhile: fly from the nearest one.
                    let from = if self.bases(side).contains(&ins.from) {
                        Some(ins.from)
                    } else {
                        self.insertion_base(side, place.x, place.y)
                    };
                    let Some(from) = from else {
                        self.insertion_called_off(k, ev);
                        continue;
                    };
                    // From the side's helicopters; a player's order always gets the reserve. A
                    // player flying it leads the rest (alone, if there are none).
                    let left = self.forces(side).helicopters as usize;
                    let flown = !ins.pilots.is_empty();
                    let mut n = left.min(INSERTION_HELOS - flown as usize);
                    if ins.ordered_by.is_some() && !flown {
                        n = n.max(RESERVE_HELICOPTERS);
                    }
                    if n == 0 && !flown {
                        self.insertion_called_off(k, ev);
                        continue;
                    }
                    let f = self.forces_mut(side);
                    f.helicopters = f.helicopters.saturating_sub(n as u32);
                    let tags: Vec<u32> = (0..n).map(|_| self.id()).collect();
                    let name = self.places[self.insertions[k].place as usize].name.clone();
                    let ins = &mut self.insertions[k];
                    ins.from = from;
                    ins.helicopters = tags.clone();
                    ins.alive = vec![true; tags.len()];
                    ins.state = InsertionState::Enroute;
                    ins.enroute_at = Some(self.clock);
                    let (id, target) = (ins.id, ins.target);
                    if side == Side::Blue {
                        self.note(format!("Helicopters on the way to {name}"));
                        ev.push(CampaignEvent::Message(format!(
                            "Insertion helicopters lift off for {name}"
                        )));
                    } else {
                        self.note(format!("Enemy helicopters on the way to {name}"));
                        ev.push(CampaignEvent::Message(format!(
                            "Enemy helicopters inbound to {name}!"
                        )));
                    }
                    ev.push(CampaignEvent::InsertionLaunched { id, side, from, target, tags });
                }
                // Still not there long after: called home.
                InsertionState::Enroute
                    if ins.enroute_at.is_some_and(|t| self.clock - t > INSERTION_TIMEOUT) =>
                {
                    let tags: Vec<u32> = ins
                        .helicopters
                        .iter()
                        .zip(&ins.alive)
                        .filter(|(_, a)| **a)
                        .map(|(&t, _)| t)
                        .collect();
                    ev.extend(tags.into_iter().map(|tag| CampaignEvent::Recall { tag }));
                    self.insertion_failed(k, "the helicopters turned back", ev);
                }
                // Friendly airfields scramble their CAP at the enemy's helicopters on their own
                // (`friendly_cap`).
                InsertionState::Enroute if !ins.scrambled && side == Side::Blue => {
                    // The enemy sends fighters when the helicopters come near one of its bases.
                    let helos: Vec<(i32, i32)> = view
                        .flights
                        .iter()
                        .filter(|f| ins.helicopters.contains(&f.tag))
                        .map(|f| (f.x, f.y))
                        .collect();
                    let near = helos.iter().find_map(|&(x, y)| {
                        let b = self.nearest_base(Side::Red, x, y)?;
                        let s = self.site(b)?;
                        (dist(s.x - x, s.y - y) < SCRAMBLE_RANGE).then_some((b, x, y))
                    });
                    if let Some((from, x, y)) = near
                        && self.forces(Side::Red).fighters > 0
                        && self.pay(Side::Red, SORTIE_COST, Spend::Operations)
                    {
                        self.insertions[k].scrambled = true;
                        self.take_fighters(Side::Red, 1);
                        let tag = self.id();
                        self.patrols.push((tag, Side::Red, self.clock));
                        let task = FighterTask::Patrol;
                        let side = Side::Red;
                        ev.push(CampaignEvent::LaunchFighters { tag, side, from, x, y, task });
                        ev.push(CampaignEvent::Message("Enemy fighters scrambled".into()));
                    }
                }
                _ => {}
            }
        }
    }

    /// Insertion `k` cannot lift off (no base in reach, no helicopters left): it is not flown,
    /// and nobody stopped it.
    fn insertion_called_off(&mut self, k: usize, ev: &mut Vec<CampaignEvent>) {
        let ins = &mut self.insertions[k];
        ins.state = InsertionState::CalledOff;
        let (id, place, side) = (ins.id, ins.place, ins.side);
        self.settle_insertion(id, InsertionState::CalledOff);
        let name = self.places[place as usize].name.clone();
        let text = if side == Side::Blue {
            format!("Insertion at {name} called off: no helicopters to fly it")
        } else {
            format!("Enemy insertion at {name} called off")
        };
        self.note(text.clone());
        self.board_at = f64::MIN;
        ev.push(CampaignEvent::Message(text));
    }

    /// Where an interceptor meets an enemy insertion: half way along the helicopters' track.
    pub fn intercept_point(&self, ins: &Insertion) -> (i32, i32) {
        match (self.site(ins.from), self.site(ins.target)) {
            (Some(a), Some(b)) => ((a.x + b.x) / 2, (a.y + b.y) / 2),
            _ => {
                let p = &self.places[ins.place as usize];
                (p.x, p.y)
            }
        }
    }

    /// An insertion's timing for the briefings: when the helicopters lift off, or when they
    /// are due at their goal.
    pub fn insertion_status(&self, id: u32) -> Option<String> {
        let ins = self.insertion(id)?;
        let (from, to) = (self.site(ins.from)?, self.site(ins.target)?);
        let flight = dist(from.x - to.x, from.y - to.y) as f64 / HELO_SPEED;
        let mins = |s: f64| (s.max(0.0) / 60.0).ceil();
        let to_name = &self.places[ins.place as usize].name;
        match ins.state {
            InsertionState::Pending { launch_at } => Some(format!(
                "Helicopters lift off from {} in {} min, then about {} min to {to_name}",
                from.name,
                mins(launch_at - self.clock),
                mins(flight)
            )),
            InsertionState::Enroute => {
                let left = ins.enroute_at.map_or(flight, |t| flight - (self.clock - t));
                Some(format!(
                    "Helicopters from {} inbound to {to_name}, due in about {} min",
                    from.name,
                    mins(left)
                ))
            }
            _ => None,
        }
    }

    pub(crate) fn repairs(&mut self, ev: &mut Vec<CampaignEvent>) {
        let now = self.clock;
        // Due, and paid for by the owner (the neutrals' are free).
        let due: Vec<(usize, SiteKind)> = self
            .sites
            .iter()
            .enumerate()
            .filter(|(_, s)| !s.removed)
            .filter(|(_, s)| {
                let wait = if s.kind == SiteKind::Airfield { REPAIR_AIRFIELD } else { REPAIR_SITE };
                s.destroyed_at.is_some_and(|at| now - at >= wait)
            })
            .map(|(k, s)| (k, s.kind))
            .collect();
        let mut done = Vec::new();
        for (k, kind) in due {
            let cost = match kind {
                SiteKind::Airfield => REPAIR_COST_AIRFIELD,
                SiteKind::Sam => REPAIR_COST_SAM,
                SiteKind::City => REPAIR_COST_CITY,
                SiteKind::Ship | SiteKind::Carrier => REPAIR_COST_SHIP,
            };
            let side = self.owner(self.sites[k].object).unwrap_or(Side::Neutral);
            if side != Side::Neutral && !self.pay(side, cost, Spend::Repairs) {
                continue;
            }
            let s = &mut self.sites[k];
            s.destroyed_at = None;
            done.push((s.object, s.kind, s.name.clone()));
        }
        for (object, kind, name) in done {
            if matches!(kind, SiteKind::Airfield | SiteKind::Sam) {
                self.note(format!(
                    "{name}: {} repaired",
                    if kind == SiteKind::Sam { "SAM site" } else { "airfield" }
                ));
            }
            ev.push(CampaignEvent::Repaired { object });
        }
        for k in 0..self.places.len() {
            let p = &self.places[k];
            if p.guns_lost == 0 || !p.guns_repair_at.is_some_and(|t| now >= t) {
                continue;
            }
            if !self.pay(p.owner, GUN_COST, Spend::Repairs) {
                continue;
            }
            let p = &mut self.places[k];
            p.guns_lost -= 1;
            p.guns_repair_at = (p.guns_lost > 0).then_some(now + REPAIR_GUN);
            ev.push(CampaignEvent::GunRepaired { place: k as PlaceId });
        }
    }

    /// A gun or MANPADS team (AAA unit `index`) destroyed: one less at its place until it is
    /// replaced.
    fn gun_lost(&mut self, index: u32, view: &WorldView) {
        let Some(a) = view.aaa.get(index as usize) else { return };
        let near = self
            .places
            .iter()
            .enumerate()
            .filter(|(_, p)| p.owner == a.side)
            .map(|(k, p)| (k, dist(p.x - a.x, p.y - a.y)))
            .filter(|&(_, d)| d <= AAA_RANGE)
            .min_by_key(|&(_, d)| d);
        if let Some((k, _)) = near {
            let p = &mut self.places[k];
            p.guns_lost = p.guns_lost.saturating_add(1);
            p.guns_repair_at = Some(self.clock + REPAIR_GUN);
        }
    }

    pub(crate) fn expire_intel(&mut self, ev: &mut Vec<CampaignEvent>) {
        let now = self.clock;
        for (k, p) in self.places.iter_mut().enumerate() {
            if p.intel_until.is_some_and(|t| t <= now) {
                p.intel_until = None;
                let objects = p.members.iter().chain(&p.defences).copied().collect();
                ev.push(CampaignEvent::IntelExpired { place: k as PlaceId, objects });
            }
        }
    }

    /// Enemy patrols take off now and then from live airfields near the front.
    pub(crate) fn enemy_air(&mut self, view: &WorldView, ev: &mut Vec<CampaignEvent>) {
        // Fighters the simulation no longer has (shot down unreported, landed, lost on load).
        let now = self.clock;
        self.patrols
            .retain(|&(tag, _, at)| now - at < 30.0 || view.flights.iter().any(|f| f.tag == tag));
        let max = PATROLS + self.setup.roe as usize;
        let red = self.patrols.iter().filter(|p| p.1 == Side::Red).count();
        // With its AWACS up the enemy sees more and sends patrols twice as often, at the
        // players.
        let awacs = self.awacs.as_ref().filter(|a| a.side == Side::Red).map(|a| (a.x, a.y));
        let every = if awacs.is_some() { PATROL_EVERY / 2.0 } else { PATROL_EVERY };
        if now - self.patrol_at < every || red >= max {
            return;
        }
        self.patrol_at = now;
        let front = self.front(Side::Blue);
        // The enemy bases nearest the front.
        let mut bases: Vec<(ObjectId, i32)> = self
            .bases(Side::Red)
            .into_iter()
            .filter_map(|b| {
                let s = self.site(b)?;
                let d = self.blue_places().map(|p| dist(p.x - s.x, p.y - s.y)).min()?;
                Some((b, d))
            })
            .collect();
        bases.sort_by_key(|b| b.1);
        bases.truncate(3);
        if bases.is_empty() {
            return;
        }
        let from = bases[self.rng.below(bases.len() as i32) as usize].0;
        // Vectored by the AWACS at the player nearest it, else towards an insertion or a raid
        // in the air, else a friendly place near the front.
        let vectored = awacs.and_then(|(ax, ay)| {
            view.players
                .iter()
                .filter(|p| p.airborne)
                .min_by_key(|p| dist(p.x - ax, p.y - ay))
                .map(|p| (p.x, p.y))
        });
        let goal = vectored.or_else(|| {
            self.insertions
                .iter()
                .filter(|i| i.state == InsertionState::Enroute)
                .map(|i| i.target)
                .chain(
                    self.raids.iter().filter(|r| r.state == RaidState::Enroute).map(|r| r.target),
                )
                .find_map(|t| self.site(t).map(|s| (s.x, s.y)))
                .or_else(|| {
                    let p = front.first()?;
                    let (px, py) = (self.places[*p as usize].x, self.places[*p as usize].y);
                    self.blue_places().min_by_key(|b| dist(b.x - px, b.y - py)).map(|b| (b.x, b.y))
                })
        });
        let Some((x, y)) = goal else { return };
        // The fighters a raid needs stay home.
        if (self.forces(Side::Red).fighters as usize) <= RAID_AIRCRAFT + RAID_ESCORTS
            || !self.pay(Side::Red, SORTIE_COST, Spend::Operations)
        {
            return;
        }
        self.take_fighters(Side::Red, 1);
        let tag = self.id();
        self.patrols.push((tag, Side::Red, now));
        let task = FighterTask::Patrol;
        ev.push(CampaignEvent::LaunchFighters { tag, side: Side::Red, from, x, y, task });
    }

    /// A friendly airfield with enemy aircraft close scrambles a CAP of one or two fighters
    /// (two against two or more); none while its CAP flies, nor for a while after one of its
    /// fighters was shot down.
    pub(crate) fn friendly_cap(&mut self, view: &WorldView, ev: &mut Vec<CampaignEvent>) {
        let now = self.clock;
        // Fighters gone from the air without landing: shot down (the simulation reports that a
        // frame later).
        let gone: Vec<u32> = self
            .caps
            .iter()
            .filter(|c| now - c.launched >= 30.0)
            .flat_map(|c| c.tags.iter().copied())
            .filter(|&t| !view.flights.iter().any(|f| f.tag == t))
            .collect();
        for tag in gone {
            self.cap_report(tag, true);
        }
        self.cap_lockout.retain(|l| l.1 > now);
        for base in self.bases(Side::Blue) {
            if self.caps.iter().any(|c| c.base == base)
                || self.cap_lockout.iter().any(|l| l.0 == base)
            {
                continue;
            }
            let Some(s) = self.site(base) else { continue };
            let (bx, by, name) = (s.x, s.y, s.name.clone());
            let threats: Vec<(i32, i32, i32)> = view
                .flights
                .iter()
                .filter(|f| f.side == Side::Red)
                .map(|f| (f.x, f.y, dist(f.x - bx, f.y - by)))
                .filter(|t| t.2 <= AI_CAP_TRIGGER)
                .collect();
            let Some(&(tx, ty, d)) = threats.iter().min_by_key(|t| t.2) else { continue };
            let n = if threats.len() >= 2 { 2 } else { 1 };
            // As many as the airfields have left and the materiel pays for.
            let n = n.min(self.blue.fighters as usize);
            if n == 0 || !self.pay(Side::Blue, SORTIE_COST * n as f64, Spend::Operations) {
                continue;
            }
            self.take_fighters(Side::Blue, n as u32);
            // The orbit: out from the airfield towards the threat.
            let r = AI_CAP_STATION.min(d);
            let d = d.max(1);
            let (x, y) = (bx + (tx - bx) * r / d, by + (ty - by) * r / d);
            let tags: Vec<u32> = (0..n).map(|_| self.id()).collect();
            for &tag in &tags {
                let task = FighterTask::Cap;
                ev.push(CampaignEvent::LaunchFighters {
                    tag,
                    side: Side::Blue,
                    from: base,
                    x,
                    y,
                    task,
                });
            }
            self.caps.push(CapFlight { base, tags, launched: now });
            self.note(format!("CAP scrambled from {name}"));
        }
    }

    /// A CAP fighter shot down (`lost`) or landed. A loss keeps its airfield from sending
    /// another CAP for a while.
    fn cap_report(&mut self, tag: u32, lost: bool) {
        let Some(k) = self.caps.iter().position(|c| c.tags.contains(&tag)) else { return };
        let base = self.caps[k].base;
        self.caps[k].tags.retain(|&t| t != tag);
        if self.caps[k].tags.is_empty() {
            self.caps.remove(k);
        }
        if lost {
            self.cap_lockout.retain(|l| l.0 != base);
            self.cap_lockout.push((base, self.clock + AI_CAP_RESPAWN));
        }
    }

    /// Friendly airfields with a Patriot battery, and where each stands: those well behind
    /// the front (else the rearmost one), nearest the front first.
    pub fn patriot_sites(&self) -> Vec<(PlaceId, i32, i32)> {
        let mut all: Vec<(PlaceId, i32)> = self
            .places
            .iter()
            .enumerate()
            .filter(|(_, p)| p.owner == Side::Blue && p.airfield.is_some())
            .map(|(k, p)| {
                let d = self
                    .places
                    .iter()
                    .filter(|q| q.owner == Side::Red)
                    .map(|q| dist(q.x - p.x, q.y - p.y))
                    .min()
                    .unwrap_or(i32::MAX);
                (k as PlaceId, d)
            })
            .collect();
        all.sort_by_key(|a| a.1);
        let max = PATRIOT_BATTERIES + self.setup.roe as usize;
        let mut rear: Vec<PlaceId> =
            all.iter().filter(|a| a.1 >= PATRIOT_REAR).take(max).map(|a| a.0).collect();
        if rear.is_empty() {
            rear.extend(all.last().map(|a| a.0));
        }
        rear.into_iter()
            .map(|k| {
                let p = &self.places[k as usize];
                // Off the airfield at an angle of its own (the golden angle per place).
                let a = k as f64 * 2.399_963;
                let r = (PATRIOT_OFFSET + (k as i32 * 37) % (PATRIOT_OFFSET / 2)) as f64;
                (k, p.x + (a.sin() * r) as i32, p.y - (a.cos() * r) as i32)
            })
            .collect()
    }

    /// The campaign sends an insertion of its own when a front place is (nearly) undefended.
    pub(crate) fn auto_insertion(&mut self, view: &WorldView, ev: &mut Vec<CampaignEvent>) {
        // A place that just changed hands: its new guns are not in the view yet.
        if self.clock - self.auto_insert_at < AUTO_INSERT_EVERY
            || ev.iter().any(|e| matches!(e, CampaignEvent::PlaceCaptured { .. }))
        {
            return;
        }
        // Guns count too: helicopters landing among them do not get through.
        if (self.blue.helicopters as usize) < INSERTION_HELOS || self.blue.materiel < INSERTION_COST
        {
            return;
        }
        let candidate = self.front(Side::Blue).into_iter().find(|&p| {
            let q = &self.places[p as usize];
            !self.active_insertion(p)
                && self.threat_of(p, Some(view)) <= AUTO_INSERT_THREAT
                && self.insertion_base(Side::Blue, q.x, q.y).is_some()
        });
        let Some(place) = candidate else { return };
        if let Ok(e) = self.order_insertion(None, place) {
            self.auto_insert_at = self.clock;
            ev.extend(e);
        }
    }

    /// The enemy's offensive: now and then an insertion against the weakest friendly place
    /// on its front, and a raid on a friendly airfield near it (paced by its skill).
    pub(crate) fn enemy_offensive(&mut self, view: &WorldView, ev: &mut Vec<CampaignEvent>) {
        let skill = (self.setup.opponents as usize).min(3);
        let now = self.clock;
        if now - self.enemy_insert_at >= ENEMY_INSERT_EVERY[skill] {
            // One of the weakest places, those it failed at lately weighed stronger.
            let mut weak: Vec<(PlaceId, i32)> = self
                .front(Side::Red)
                .into_iter()
                .filter(|&p| {
                    let p = &self.places[p as usize];
                    self.insertion_base(Side::Red, p.x, p.y).is_some()
                })
                .filter(|&p| !self.active_insertion(p))
                .map(|p| (p, self.threat_of(p, Some(view)) + SAM_THREAT * self.failed_at(p)))
                .collect();
            weak.sort_by_key(|w| w.1);
            weak.truncate(ENEMY_TARGET_CHOICE);
            let target =
                (!weak.is_empty()).then(|| weak[self.rng.below(weak.len() as i32) as usize].0);
            match target.map(|p| self.order_insertion_for(Side::Red, None, p)) {
                Some(Ok(e)) => {
                    self.enemy_insert_at = now;
                    ev.extend(e);
                }
                // Nothing in reach now: another look in a minute.
                _ => self.enemy_insert_at = now - ENEMY_INSERT_EVERY[skill] + 60.0,
            }
        }
        if now - self.raid_at >= RAID_EVERY[skill] {
            let ready = self.forces(Side::Red).fighters as usize >= RAID_AIRCRAFT + RAID_ESCORTS
                && self.forces(Side::Red).materiel >= RAID_COST;
            match self.raid_target().filter(|_| ready) {
                Some((place, target, from)) => {
                    self.pay(Side::Red, RAID_COST, Spend::Operations);
                    self.raid_at = now;
                    let id = self.id();
                    self.raids.push(Raid {
                        id,
                        side: Side::Red,
                        place,
                        target,
                        from,
                        aircraft: Vec::new(),
                        alive: Vec::new(),
                        state: RaidState::Pending { launch_at: now + RAID_LEAD },
                        ended: None,
                        escorted: false,
                        drones: false,
                        enroute_at: None,
                    });
                    let name = self.places[place as usize].name.clone();
                    let text = format!(
                        "Intelligence: enemy strike aircraft to raid {name} airfield in {} min",
                        (RAID_LEAD / 60.0).round()
                    );
                    self.note(text.clone());
                    self.board_at = f64::MIN;
                    ev.push(CampaignEvent::Message(text));
                }
                None => self.raid_at = now - RAID_EVERY[skill] + 60.0,
            }
        }
    }

    /// The enemy's insertions at a place that failed lately.
    fn failed_at(&self, place: PlaceId) -> i32 {
        let now = self.clock;
        self.insertions
            .iter()
            .filter(|i| i.side == Side::Red && i.place == place)
            .filter(|i| i.state == InsertionState::Failed)
            .filter(|i| i.enroute_at.is_some_and(|t| now - t < FAILED_MEMORY))
            .count() as i32
    }

    /// The friendly airfield the enemy raids next: an intact one in reach of an enemy base
    /// and not raided yet, one of the two nearest the enemy (place, airfield, enemy base).
    fn raid_target(&mut self) -> Option<(PlaceId, ObjectId, ObjectId)> {
        let mut all: Vec<(PlaceId, ObjectId, ObjectId, i32)> = self
            .places
            .iter()
            .enumerate()
            .filter(|(_, p)| p.owner == Side::Blue)
            .filter_map(|(k, p)| {
                let a = p.airfield.filter(|&a| self.site(a).is_some_and(Site::alive))?;
                if self.raids.iter().any(|r| r.open() && r.target == a) {
                    return None;
                }
                let from = self.nearest_base(Side::Red, p.x, p.y)?;
                let s = self.site(from)?;
                let d = dist(s.x - p.x, s.y - p.y);
                (d <= RAID_RANGE).then_some((k as PlaceId, a, from, d))
            })
            .collect();
        all.sort_by_key(|a| a.3);
        all.truncate(2);
        if all.is_empty() {
            return None;
        }
        let (place, target, from, _) = all[self.rng.below(all.len() as i32) as usize];
        Some((place, target, from))
    }

    /// The raids' take-off, and the ones that cannot go any more.
    pub(crate) fn run_raids(&mut self, ev: &mut Vec<CampaignEvent>) {
        let now = self.clock;
        for k in 0..self.raids.len() {
            let r = &self.raids[k];
            // Still not there long after: called home, the raid turned back.
            if r.state == RaidState::Enroute && r.enroute_at.is_some_and(|t| now - t > RAID_TIMEOUT)
            {
                let tags: Vec<u32> =
                    r.aircraft.iter().zip(&r.alive).filter(|(_, a)| **a).map(|(&t, _)| t).collect();
                let text = format!(
                    "{} turned back from {}",
                    raid_name(r),
                    self.places[r.place as usize].name
                );
                ev.extend(tags.into_iter().map(|tag| CampaignEvent::Recall { tag }));
                self.end_raid(k, RaidState::Repelled);
                self.note(text);
                continue;
            }
            let RaidState::Pending { launch_at } = r.state else { continue };
            let (id, side, target, base, escorted) = (r.id, r.side, r.target, r.from, r.escorted);
            let (drones, what) = (r.drones, raid_name(r));
            let name = self.places[r.place as usize].name.clone();
            let gone = self.owner(target) != Some(side.enemy())
                || !self.site(target).is_some_and(Site::alive);
            if gone {
                self.end_raid(k, RaidState::CalledOff);
                continue;
            }
            // The escort goes first, to patrol over the target.
            if !escorted && now >= launch_at - RAID_ESCORT_LEAD && self.bases(side).contains(&base)
            {
                let from = base;
                self.raids[k].escorted = true;
                let (x, y) = self.site(target).map_or((0, 0), |s| (s.x, s.y));
                let n = RAID_ESCORTS.min(self.forces(side).fighters as usize);
                self.take_fighters(side, n as u32);
                for _ in 0..n {
                    let tag = self.id();
                    self.patrols.push((tag, side, now));
                    let task = FighterTask::Patrol;
                    ev.push(CampaignEvent::LaunchFighters { tag, side, from, x, y, task });
                }
            }
            if now < launch_at {
                continue;
            }
            let range = if drones { DRONE_RANGE } else { RAID_RANGE };
            let from = if self.bases(side).contains(&base) {
                Some(base)
            } else {
                let t = self.site(target).expect("target is a site");
                let (x, y) = (t.x, t.y);
                self.nearest_base(side, x, y)
                    .filter(|&b| self.site(b).is_some_and(|s| dist(s.x - x, s.y - y) <= range))
            };
            let Some(from) = from else {
                self.end_raid(k, RaidState::CalledOff);
                continue;
            };
            // Drones are no fighters; strike aircraft come out of the side's.
            let n = if drones { DRONE_WAVE } else { RAID_AIRCRAFT };
            if !drones && !self.take_fighters(side, n as u32) {
                self.end_raid(k, RaidState::CalledOff);
                continue;
            }
            let tags: Vec<u32> = (0..n).map(|_| self.id()).collect();
            let r = &mut self.raids[k];
            r.from = from;
            r.aircraft = tags.clone();
            r.alive = vec![true; tags.len()];
            r.state = RaidState::Enroute;
            r.enroute_at = Some(now);
            self.note(format!("{what} on the way to {name}"));
            ev.push(CampaignEvent::Message(if side == Side::Blue {
                format!("{what} on the way to {name}")
            } else {
                format!("{what} inbound to {name}!")
            }));
            ev.push(CampaignEvent::RaidLaunched { id, side, from, target, tags, drones });
        }
        self.raids.retain(|r| r.ended.is_none_or(|t| now - t < RAID_KEEP));
    }

    fn end_raid(&mut self, k: usize, state: RaidState) {
        let r = &mut self.raids[k];
        r.state = state;
        r.ended = Some(self.clock);
        let id = r.id;
        self.settle_raid(id, state == RaidState::Repelled);
    }

    /// A world object was destroyed: the raid on it (if one is under way) hit.
    fn raid_hit(&mut self, object: ObjectId, ev: &mut Vec<CampaignEvent>) {
        let Some(k) =
            self.raids.iter().position(|r| r.state == RaidState::Enroute && r.target == object)
        else {
            return;
        };
        let r = &self.raids[k];
        let what = raid_name(r);
        let target = self.target_name(r.target);
        self.end_raid(k, RaidState::Hit);
        let text = format!("{what} hit {target}");
        self.note(text.clone());
        ev.push(CampaignEvent::Message(format!("{text}!")));
    }

    /// A raid's target for the news: "the Rhein-Main airfield", "the city of Kiev".
    pub(crate) fn target_name(&self, object: ObjectId) -> String {
        let Some(s) = self.site(object) else { return "the target".into() };
        match s.kind {
            SiteKind::Airfield => format!("the {} airfield", s.name),
            SiteKind::City => format!("the city of {}", s.name),
            SiteKind::Sam => format!("the SAM site at {}", s.name),
            _ => s.name.clone(),
        }
    }

    /// A strike aircraft shot down or landed: once none flies on to the target, the raid is
    /// repelled.
    fn raider_gone(&mut self, tag: u32, ev: &mut Vec<CampaignEvent>) {
        let Some(k) = self.raids.iter().position(|r| r.aircraft.contains(&tag)) else { return };
        let r = &mut self.raids[k];
        let h = r.aircraft.iter().position(|&t| t == tag).expect("contains");
        r.alive[h] = false;
        if r.state == RaidState::Enroute && r.alive.iter().all(|a| !a) {
            let name = self.places[r.place as usize].name.clone();
            let text = match (r.side, r.drones) {
                (Side::Blue, _) => format!("{} did not get through to {name}", raid_name(r)),
                (_, true) => format!("Drone attack on {name} stopped"),
                _ => format!("Enemy raid on {name} repelled"),
            };
            self.end_raid(k, RaidState::Repelled);
            self.note(text.clone());
            ev.push(CampaignEvent::Message(format!("{text}!")));
        }
    }

    /// The war is won when the enemy holds no airfield (no place, on a theater where it had
    /// none), lost when the friendly side holds no airfield and no carrier. True: over now.
    pub(crate) fn check_outcome(&mut self, ev: &mut Vec<CampaignEvent>) -> bool {
        let had_airfields =
            self.sites.iter().any(|s| s.kind == SiteKind::Airfield && s.side == Side::Red);
        let red = self
            .places
            .iter()
            .any(|p| p.owner == Side::Red && (p.airfield.is_some() || !had_airfields));
        let blue = self.places.iter().any(|p| p.owner == Side::Blue && p.airfield.is_some())
            || self
                .sites
                .iter()
                .any(|s| s.kind == SiteKind::Carrier && s.side == Side::Blue && !s.removed);
        let winner = match (red, blue) {
            (false, _) => Side::Blue,
            (true, false) => Side::Red,
            _ => return false,
        };
        self.outcome = Some(Outcome { winner, at: self.clock });
        self.board.clear();
        let text = if winner == Side::Blue {
            "VICTORY: the enemy holds no airfield any more"
        } else {
            "DEFEAT: no friendly airfield is left"
        };
        self.note(text);
        ev.push(CampaignEvent::BoardChanged);
        if self.prune_plans() {
            ev.push(self.plans_changed());
        }
        ev.push(CampaignEvent::Message(text.into()));
        ev.push(CampaignEvent::Ended { winner });
        true
    }

    fn blue_places(&self) -> impl Iterator<Item = &Place> {
        self.places.iter().filter(|p| p.owner == Side::Blue)
    }

    /// Enemy places off the front with something to strike, within deep-strike range of a
    /// player base, nearest first.
    fn deep_targets(&self, front: &[PlaceId]) -> Vec<PlaceId> {
        let bases: Vec<(i32, i32)> = self
            .player_bases()
            .into_iter()
            .filter_map(|b| self.site(b).map(|s| (s.x, s.y)))
            .collect();
        let mut all: Vec<(PlaceId, i32)> = self
            .places
            .iter()
            .enumerate()
            .filter(|(k, p)| p.owner == Side::Red && !front.contains(&(*k as PlaceId)))
            .filter(|(_, p)| {
                p.members.iter().any(|&m| {
                    self.site(m).is_some_and(|s| {
                        s.alive() && matches!(s.kind, SiteKind::City | SiteKind::Airfield)
                    })
                })
            })
            .filter_map(|(k, p)| {
                let d = bases.iter().map(|&(x, y)| dist(p.x - x, p.y - y)).min()?;
                (d <= DEEP_RANGE).then_some((k as PlaceId, d))
            })
            .collect();
        all.sort_by_key(|a| a.1);
        all.into_iter().take(DEEP_TARGETS).map(|a| a.0).collect()
    }

    /// The enemy places nearest the attacking side's (within the front range; else the
    /// nearest one), nearest first.
    pub fn front(&self, side: Side) -> Vec<PlaceId> {
        let mut all: Vec<(PlaceId, i32)> = self
            .places
            .iter()
            .enumerate()
            .filter(|(_, p)| p.owner == side.enemy())
            .filter_map(|(k, p)| {
                let d = self
                    .places
                    .iter()
                    .filter(|q| q.owner == side)
                    .map(|q| dist(q.x - p.x, q.y - p.y))
                    .min()?;
                Some((k as PlaceId, d))
            })
            .collect();
        all.sort_by_key(|a| a.1);
        let near: Vec<PlaceId> =
            all.iter().filter(|a| a.1 <= FRONT_RANGE).take(FRONT_PLACES).map(|a| a.0).collect();
        if near.is_empty() { all.first().map(|a| vec![a.0]).unwrap_or_default() } else { near }
    }

    /// Live SAM sites (weighted) and guns defending a place (view: the guns; None counts none).
    pub fn threat_of(&self, place: PlaceId, view: Option<&WorldView>) -> i32 {
        let Some(p) = self.place(place) else { return 0 };
        let sams =
            p.defences.iter().filter(|&&d| self.site(d).is_some_and(Site::alive)).count() as i32;
        let guns = view.map_or(0, |v| self.guns_near(v, p.x, p.y, p.owner).len() as i32);
        sams * SAM_THREAT + guns
    }

    /// Live guns of a side near a point (AAA unit indices).
    fn guns_near(&self, view: &WorldView, x: i32, y: i32, side: Side) -> Vec<u32> {
        self.guns_within(view, x, y, side, AAA_RANGE).collect()
    }

    /// Live guns of a side within `r` of a point (AAA unit indices).
    pub(crate) fn guns_within<'v>(
        &self,
        view: &'v WorldView,
        x: i32,
        y: i32,
        side: Side,
        r: i32,
    ) -> impl Iterator<Item = u32> + 'v {
        view.aaa
            .iter()
            .enumerate()
            .filter(move |(_, a)| a.alive && a.side == side && dist(a.x - x, a.y - y) <= r)
            .map(|(k, _)| k as u32)
    }

    /// Rebuild the mission board; true if it changed. Offers keep their id while they stay.
    pub(crate) fn rebuild_board(&mut self, view: &WorldView) -> bool {
        let side = Side::Blue;
        let mut offers: Vec<MissionOffer> = Vec::new();
        let now = self.clock;
        let mut add = |c: &mut CampaignState, kind, title: String, objective, x, y, threat| {
            let Some(base) = c.nearest_player_base(x, y) else { return };
            let key = objective_key(&objective);
            let (id, posted) = match c.board.iter().find(|o| objective_key(&o.objective) == key) {
                Some(o) => (o.id, o.posted),
                None => (c.id(), now),
            };
            offers.push(MissionOffer { id, kind, title, objective, x, y, base, threat, posted });
        };
        // The enemy's attacks first: they do not wait.
        for ins in self.insertions.clone() {
            if ins.side != side.enemy()
                || !matches!(ins.state, InsertionState::Pending { .. } | InsertionState::Enroute)
            {
                continue;
            }
            let name = self.places[ins.place as usize].name.clone();
            let (x, y) = self.intercept_point(&ins);
            add(
                self,
                MissionKind::Intercept,
                format!("Intercept - {name}"),
                Objective::Intercept { insertion: ins.id },
                x,
                y,
                0,
            );
        }
        for r in self.raids.clone() {
            if r.side != side.enemy() || !r.open() {
                continue;
            }
            let p = &self.places[r.place as usize];
            let (x, y, name) = (p.x, p.y, p.name.clone());
            add(
                self,
                MissionKind::Defend,
                if r.drones {
                    format!("Drone defense - {name}")
                } else {
                    format!("Defend - {name} airfield")
                },
                Objective::Defend { raid: r.id },
                x,
                y,
                0,
            );
        }
        // The enemy's AWACS while it is up.
        if let Some(a) = self.awacs.clone().filter(|a| a.side == side.enemy()) {
            add(
                self,
                MissionKind::Awacs,
                "AWACS - shoot it down".into(),
                Objective::Awacs { tag: a.tag },
                a.x,
                a.y,
                0,
            );
        }
        // Pilots down behind the lines.
        for d in self.downed.clone() {
            if d.state != DownedState::Waiting {
                continue;
            }
            add(
                self,
                MissionKind::Csar,
                format!("Rescue - {}", d.name),
                Objective::Rescue { downed: d.id },
                d.x,
                d.y,
                0,
            );
        }
        let front = self.front(side);
        for &pid in &front {
            let p = self.places[pid as usize].clone();
            let threat = self.threat_of(pid, Some(view));
            let (x, y, name) = (p.x, p.y, p.name.clone());
            if !p.intel(now) {
                let object = p.airfield.or(p.members.first().copied()).unwrap_or(0);
                add(
                    self,
                    MissionKind::Recon,
                    format!("Recon - {name}"),
                    Objective::Recon { place: pid, object },
                    x,
                    y,
                    threat,
                );
            }
            let sites: Vec<ObjectId> = p
                .defences
                .iter()
                .copied()
                .filter(|&d| self.site(d).is_some_and(Site::alive))
                .collect();
            if !sites.is_empty() {
                let (sx, sy) = self.site(sites[0]).map_or((x, y), |s| (s.x, s.y));
                add(
                    self,
                    MissionKind::Sead,
                    format!("SEAD - {name}"),
                    Objective::Sead { place: pid, sites },
                    sx,
                    sy,
                    threat,
                );
            }
            let aaa = self.guns_near(view, x, y, p.owner);
            if !aaa.is_empty() {
                add(
                    self,
                    MissionKind::Cas,
                    format!("CAS - {name}"),
                    Objective::Cas { place: pid, aaa },
                    x,
                    y,
                    threat,
                );
            }
            if let Some(a) = p.airfield.filter(|&a| self.site(a).is_some_and(Site::alive)) {
                add(
                    self,
                    MissionKind::Strike,
                    format!("Strike - {name} airfield"),
                    Objective::Strike { place: pid, airfield: a },
                    x,
                    y,
                    threat,
                );
            }
            if !self.active_insertion(pid) && self.insertion_base(side, x, y).is_some() {
                add(
                    self,
                    MissionKind::Insertion,
                    format!("Insertion - {name}"),
                    Objective::Insertion { place: pid },
                    x,
                    y,
                    threat,
                );
            }
        }
        // Deep strikes on the enemy's rear: its cities (the materiel it fights with), else its
        // airfields.
        for pid in self.deep_targets(&front) {
            let p = self.places[pid as usize].clone();
            let threat = self.threat_of(pid, Some(view));
            let city = p
                .members
                .iter()
                .copied()
                .find(|&m| self.site(m).is_some_and(|s| s.kind == SiteKind::City && s.alive()));
            let airfield = p.airfield.filter(|&a| self.site(a).is_some_and(Site::alive));
            if let Some(object) = city {
                add(
                    self,
                    MissionKind::Interdiction,
                    format!("Interdiction - {}", p.name),
                    Objective::Interdict { place: pid, object },
                    p.x,
                    p.y,
                    threat,
                );
            } else if let Some(a) = airfield {
                add(
                    self,
                    MissionKind::Interdiction,
                    format!("Deep strike - {} airfield", p.name),
                    Objective::Strike { place: pid, airfield: a },
                    p.x,
                    p.y,
                    threat,
                );
            }
        }
        // The enemy's forward batteries, once their guns stand.
        for b in self.batteries.clone() {
            let aaa: Vec<u32> =
                self.guns_within(view, b.x, b.y, side.enemy(), BATTERY_RADIUS).collect();
            if aaa.is_empty() {
                continue;
            }
            let threat = aaa.len() as i32;
            add(
                self,
                MissionKind::Cas,
                format!("CAS - battery near {}", b.name),
                Objective::Battery { battery: b.id, aaa },
                b.x,
                b.y,
                threat,
            );
        }
        // Escorts for the insertions under way.
        for ins in self.insertions.clone() {
            if ins.side != side
                || !matches!(ins.state, InsertionState::Pending { .. } | InsertionState::Enroute)
            {
                continue;
            }
            let p = &self.places[ins.place as usize];
            let (x, y, name) = (p.x, p.y, p.name.clone());
            let threat = self.threat_of(ins.place, Some(view));
            add(
                self,
                MissionKind::Escort,
                format!("Escort - insertion at {name}"),
                Objective::Escort { insertion: ins.id },
                x,
                y,
                threat,
            );
        }
        // Enemy ships near friendly places.
        let mut ships: Vec<(ObjectId, i32, i32, i32)> = self
            .sites
            .iter()
            .filter(|s| s.kind == SiteKind::Ship && s.side == side.enemy() && s.alive())
            .filter_map(|s| {
                let d = self.blue_places().map(|p| dist(p.x - s.x, p.y - s.y)).min()?;
                (d <= SHIP_RANGE).then_some((s.object, s.x, s.y, d))
            })
            .collect();
        ships.sort_by_key(|s| s.3);
        for &(ship, x, y, _) in ships.iter().take(2) {
            add(
                self,
                MissionKind::AntiShip,
                "Anti-ship - enemy ship".into(),
                Objective::AntiShip { ship },
                x,
                y,
                0,
            );
        }
        // CAP: where enemy fighters are near friendly places or insertions, else over the
        // friendly place nearest the front.
        let mut guarded: Vec<(i32, i32, String)> = self
            .blue_places()
            .map(|p| (p.x, p.y, p.name.clone()))
            .chain(
                self.insertions.iter().filter(|i| i.state == InsertionState::Enroute).filter_map(
                    |i| {
                        let s = self.site(i.target)?;
                        Some((s.x, s.y, format!("{} insertion", s.name)))
                    },
                ),
            )
            .collect();
        let threatened: Vec<(i32, i32, String)> = guarded
            .iter()
            .filter(|g| {
                view.flights
                    .iter()
                    .any(|f| f.side == side.enemy() && dist(f.x - g.0, f.y - g.1) < CAP_TRIGGER)
            })
            .cloned()
            .collect();
        let caps = if threatened.is_empty() {
            front
                .first()
                .map(|&f| {
                    let (fx, fy) = (self.places[f as usize].x, self.places[f as usize].y);
                    guarded.sort_by_key(|g| dist(g.0 - fx, g.1 - fy));
                    guarded.into_iter().take(1).collect()
                })
                .unwrap_or_default()
        } else {
            threatened.into_iter().take(2).collect::<Vec<_>>()
        };
        for (x, y, name) in caps {
            add(self, MissionKind::Cap, format!("CAP - {name}"), Objective::Cap { x, y }, x, y, 0);
        }
        // Too many: the urgent ones stay, the front's furthest places go first.
        if offers.len() > BOARD_SIZE {
            let rank = |o: &MissionOffer| match o.kind {
                // Near the bases for the helicopters, when nothing else is.
                _ if matches!(o.objective, Objective::Battery { .. }) => 2,
                MissionKind::Intercept
                | MissionKind::Defend
                | MissionKind::Csar
                | MissionKind::Awacs => 0,
                MissionKind::Escort => 1,
                MissionKind::Interdiction => 2,
                MissionKind::Cap | MissionKind::AntiShip => 3,
                _ => 4 + front_index(&front, &o.objective),
            };
            let mut keep: Vec<usize> = (0..offers.len()).collect();
            keep.sort_by_key(|&k| rank(&offers[k]));
            keep.truncate(BOARD_SIZE);
            keep.sort_unstable();
            offers = keep.into_iter().map(|k| offers[k].clone()).collect();
        }
        let changed = offers != self.board;
        self.board = offers;
        changed
    }
}

/// What flies a raid, for the news.
fn raid_name(r: &Raid) -> &'static str {
    match (r.side, r.drones) {
        (Side::Blue, _) => "AI SEAD aircraft",
        (_, true) => "Enemy drones",
        _ => "Enemy strike aircraft",
    }
}

/// Where an offer's place is on the front (nearest first; past the end: not on it).
fn front_index(front: &[PlaceId], o: &Objective) -> usize {
    let place = match *o {
        Objective::Recon { place, .. }
        | Objective::Sead { place, .. }
        | Objective::Cas { place, .. }
        | Objective::Strike { place, .. }
        | Objective::Insertion { place } => place,
        _ => return front.len(),
    };
    front.iter().position(|&p| p == place).unwrap_or(front.len())
}

/// What identifies an offer across rebuilds.
fn objective_key(o: &Objective) -> (u8, i64) {
    match *o {
        Objective::Recon { place, .. } => (0, place as i64),
        Objective::Sead { place, .. } => (1, place as i64),
        Objective::Cas { place, .. } => (2, place as i64),
        Objective::Strike { place, .. } => (3, place as i64),
        Objective::AntiShip { ship } => (4, ship as i64),
        // CAP points snap to a 20 km grid.
        Objective::Cap { x, y } => (5, ((x / 0x500) as i64) << 16 | (y / 0x500) as i64),
        Objective::Escort { insertion } => (6, insertion as i64),
        Objective::Insertion { place } => (7, place as i64),
        Objective::Intercept { insertion } => (8, insertion as i64),
        Objective::Defend { raid } => (9, raid as i64),
        Objective::Interdict { object, .. } => (10, object as i64),
        Objective::Rescue { downed } => (11, downed as i64),
        Objective::Awacs { tag } => (12, tag as i64),
        Objective::Battery { battery, .. } => (13, battery as i64),
    }
}
