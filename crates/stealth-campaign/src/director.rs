//! The campaign's own decisions: the mission board, insertions, enemy patrols, repairs and
//! intelligence going stale. Written for a side, so the enemy can later run the same logic
//! against the player's places (only the friendly side attacks for now).

use crate::state::*;
use crate::tuning::*;
use crate::{CampaignEvent, Report, WorldView};

impl CampaignState {
    pub(crate) fn report(&mut self, r: &Report, ev: &mut Vec<CampaignEvent>) {
        self.sortie_report(r);
        match *r {
            Report::ObjectDestroyed { object, .. } => {
                let now = self.clock;
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
                }
            }
            Report::SiteSpotted { object } => {
                if let Some(s) = self.site_mut(object).filter(|s| s.alive()) {
                    s.spotted = true;
                }
            }
            Report::FlightDestroyed { tag, .. } if tag != 0 => {
                self.patrols.retain(|p| p.0 != tag);
                let Some(k) = self.insertions.iter().position(|i| i.helicopters.contains(&tag))
                else {
                    return;
                };
                let name = self.places[self.insertions[k].place as usize].name.clone();
                let ins = &mut self.insertions[k];
                let h = ins.helicopters.iter().position(|&t| t == tag).expect("contains");
                ins.alive[h] = false;
                let left = ins.alive.iter().filter(|&&a| a).count();
                let (id, place) = (ins.id, ins.place);
                if ins.state == InsertionState::Enroute && left == 0 {
                    ins.state = InsertionState::Failed;
                    self.note(format!("Insertion at {name} failed: all helicopters lost"));
                    self.settle_escorts(id, false);
                    ev.push(CampaignEvent::InsertionFailed { id, place });
                    ev.push(CampaignEvent::Message(format!("Insertion at {name} failed")));
                } else if ins.state == InsertionState::Enroute {
                    ev.push(CampaignEvent::Message(format!(
                        "Helicopter lost - {left} left for {name}"
                    )));
                }
            }
            Report::FlightLanded { tag, x, y } if tag != 0 => {
                self.patrols.retain(|p| p.0 != tag);
                let Some(k) = self.insertions.iter().position(|i| {
                    i.state == InsertionState::Enroute && i.helicopters.contains(&tag)
                }) else {
                    return;
                };
                let target = self.insertions[k].target;
                let Some(t) = self.site(target) else { return };
                if dist(t.x - x, t.y - y) <= CAPTURE_RADIUS {
                    self.capture(k, ev);
                }
            }
            _ => {}
        }
    }

    /// Insertion `k` landed: its place changes hands.
    fn capture(&mut self, k: usize, ev: &mut Vec<CampaignEvent>) {
        let (id, place) = (self.insertions[k].id, self.insertions[k].place);
        self.insertions[k].state = InsertionState::Captured;
        let side = Side::Blue;
        let p = &mut self.places[place as usize];
        p.owner = side;
        p.intel_until = None;
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
        for ins in &mut self.insertions {
            if ins.place == place && matches!(ins.state, InsertionState::Pending { .. }) {
                ins.state = InsertionState::Failed;
            }
        }
        self.settle_escorts(id, true);
        self.note(format!("{name} captured"));
        ev.push(CampaignEvent::PlaceCaptured { place, side, members, removed });
        ev.push(CampaignEvent::Message(format!("{name} captured!")));
    }

    pub(crate) fn order_insertion(
        &mut self,
        by: Option<PlayerId>,
        place: PlaceId,
    ) -> Result<Vec<CampaignEvent>, String> {
        let side = Side::Blue;
        let Some(p) = self.places.get(place as usize) else { return Err("No such place".into()) };
        if p.owner != side.enemy() {
            return Err(format!("{} is not held by the enemy", p.name));
        }
        if self.active_insertion(place) {
            return Err(format!("An insertion at {} is already under way", p.name));
        }
        let target = p.airfield.or(p.members.first().copied()).ok_or("Nothing to land at")?;
        let (x, y, name) = (p.x, p.y, p.name.clone());
        let from = self
            .insertion_base(side, x, y)
            .ok_or_else(|| format!("No friendly base within helicopter range of {name}"))?;
        let id = self.id();
        self.insertions.push(Insertion {
            id,
            place,
            from,
            target,
            helicopters: Vec::new(),
            alive: Vec::new(),
            state: InsertionState::Pending { launch_at: self.clock + INSERTION_LEAD },
            scrambled: false,
            ordered_by: by,
        });
        let base = self.site(from).map(|s| s.name.clone()).unwrap_or_default();
        let who = if by.is_some() { "ordered" } else { "Command orders" };
        let text = format!(
            "Insertion at {name} {who}: helicopters lift off from {base} in {} min",
            (INSERTION_LEAD / 60.0).round()
        );
        self.note(text.clone());
        self.board_at = f64::MIN;
        Ok(vec![CampaignEvent::Message(text)])
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
            match ins.state {
                InsertionState::Pending { .. } if place.owner != Side::Red => {
                    self.insertions[k].state = InsertionState::Failed;
                }
                InsertionState::Pending { launch_at } if self.clock >= launch_at => {
                    // The base may have been lost meanwhile: fly from the nearest one.
                    let from = if self.bases(Side::Blue).contains(&ins.from) {
                        Some(ins.from)
                    } else {
                        self.insertion_base(Side::Blue, place.x, place.y)
                    };
                    let Some(from) = from else {
                        self.insertions[k].state = InsertionState::Failed;
                        continue;
                    };
                    let tags: Vec<u32> = (0..INSERTION_HELOS).map(|_| self.id()).collect();
                    let name = self.places[self.insertions[k].place as usize].name.clone();
                    let ins = &mut self.insertions[k];
                    ins.from = from;
                    ins.helicopters = tags.clone();
                    ins.alive = vec![true; tags.len()];
                    ins.state = InsertionState::Enroute;
                    let (id, target) = (ins.id, ins.target);
                    self.note(format!("Helicopters on the way to {name}"));
                    ev.push(CampaignEvent::Message(format!(
                        "Insertion helicopters lift off for {name}"
                    )));
                    ev.push(CampaignEvent::InsertionLaunched { id, from, target, tags });
                }
                InsertionState::Enroute if !ins.scrambled => {
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
                    if let Some((from, x, y)) = near {
                        self.insertions[k].scrambled = true;
                        let tag = self.id();
                        self.patrols.push((tag, Side::Red, self.clock));
                        ev.push(CampaignEvent::LaunchFighters { tag, side: Side::Red, from, x, y });
                        ev.push(CampaignEvent::Message("Enemy fighters scrambled".into()));
                    }
                }
                _ => {}
            }
        }
    }

    pub(crate) fn repairs(&mut self, ev: &mut Vec<CampaignEvent>) {
        let now = self.clock;
        let mut done = Vec::new();
        for s in &mut self.sites {
            let Some(at) = s.destroyed_at else { continue };
            if s.removed {
                continue;
            }
            let wait = if s.kind == SiteKind::Airfield { REPAIR_AIRFIELD } else { REPAIR_SITE };
            if now - at >= wait {
                s.destroyed_at = None;
                done.push((s.object, s.kind, s.name.clone()));
            }
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
        if now - self.patrol_at < PATROL_EVERY || red >= max {
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
        // Towards an insertion in the air, else a friendly place near the front.
        let goal = self
            .insertions
            .iter()
            .filter(|i| i.state == InsertionState::Enroute)
            .find_map(|i| self.site(i.target).map(|s| (s.x, s.y)))
            .or_else(|| {
                let p = front.first()?;
                let (px, py) = (self.places[*p as usize].x, self.places[*p as usize].y);
                self.blue_places().min_by_key(|b| dist(b.x - px, b.y - py)).map(|b| (b.x, b.y))
            });
        let Some((x, y)) = goal else { return };
        let tag = self.id();
        self.patrols.push((tag, Side::Red, now));
        ev.push(CampaignEvent::LaunchFighters { tag, side: Side::Red, from, x, y });
    }

    /// The campaign sends an insertion of its own when a front place is (nearly) undefended.
    pub(crate) fn auto_insertion(&mut self, view: &WorldView, ev: &mut Vec<CampaignEvent>) {
        if self.clock - self.auto_insert_at < AUTO_INSERT_EVERY {
            return;
        }
        // Guns count too: helicopters landing among them do not get through.
        let candidate = self.front(Side::Blue).into_iter().find(|&p| {
            !self.active_insertion(p) && self.threat_of(p, Some(view)) <= AUTO_INSERT_THREAT
        });
        let Some(place) = candidate else { return };
        if let Ok(e) = self.order_insertion(None, place) {
            self.auto_insert_at = self.clock;
            ev.extend(e);
        }
    }

    fn blue_places(&self) -> impl Iterator<Item = &Place> {
        self.places.iter().filter(|p| p.owner == Side::Blue)
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
        view.aaa
            .iter()
            .enumerate()
            .filter(|(_, a)| a.alive && a.side == side && dist(a.x - x, a.y - y) <= AAA_RANGE)
            .map(|(k, _)| k as u32)
            .collect()
    }

    /// Rebuild the mission board; true if it changed. Offers keep their id while they stay.
    pub(crate) fn rebuild_board(&mut self, view: &WorldView) -> bool {
        let side = Side::Blue;
        let mut offers: Vec<MissionOffer> = Vec::new();
        let now = self.clock;
        let mut add = |c: &mut CampaignState, kind, title: String, objective, x, y, threat| {
            let Some(base) = c.nearest_base(side, x, y) else { return };
            let key = objective_key(&objective);
            let (id, posted) = match c.board.iter().find(|o| objective_key(&o.objective) == key) {
                Some(o) => (o.id, o.posted),
                None => (c.id(), now),
            };
            offers.push(MissionOffer { id, kind, title, objective, x, y, base, threat, posted });
        };
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
        // Escorts for the insertions under way.
        for ins in self.insertions.clone() {
            if !matches!(ins.state, InsertionState::Pending { .. } | InsertionState::Enroute) {
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
        offers.truncate(BOARD_SIZE);
        let changed = offers != self.board;
        self.board = offers;
        changed
    }
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
    }
}
