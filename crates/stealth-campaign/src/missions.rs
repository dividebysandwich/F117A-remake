//! The players' sorties: taking a mission, following its objective, ending it.

use crate::record::{flight_points, settle};
use crate::state::*;
use crate::tuning::*;
use crate::{CampaignEvent, Report, WorldView};

impl CampaignState {
    pub(crate) fn take_mission(
        &mut self,
        player: PlayerId,
        offer: u32,
        base: Option<ObjectId>,
        aircraft: Aircraft,
    ) -> Result<Vec<CampaignEvent>, String> {
        let Some(o) = self.board.iter().find(|o| o.id == offer).cloned() else {
            return Err("That mission is no longer on the board".into());
        };
        if !o.kind.flyable_by(aircraft) {
            return Err(o.kind.not_flyable(aircraft).into());
        }
        if self.sortie(player).is_some() {
            return Err("Already flying a sortie".into());
        }
        if self.objective_met(&o.objective, &Progress::default()) {
            return Err("That target is already destroyed".into());
        }
        let base = base.unwrap_or(o.base);
        if !self.player_bases().contains(&base) {
            return Err("Not a friendly base".into());
        }
        let mut progress = Progress { heli: aircraft.transport(), ..Progress::default() };
        let mut ev = Vec::new();
        if let Objective::Insertion { place } = o.objective {
            let (id, e) = self.fly_insertion(player, place, base)?;
            progress.insertion = Some(id);
            ev = e;
        }
        let sortie = Sortie {
            id: self.id(),
            offer: o.clone(),
            base,
            aircraft,
            started: self.clock,
            progress,
        };
        let from = self.site(base).map(|s| s.name.clone()).unwrap_or_default();
        self.note(format!("{} from {from}", o.title));
        self.slot_mut(player).sortie = Some(sortie.clone());
        self.leave_plans(player);
        match self.plans.iter_mut().find(|p| p.offer == offer) {
            Some(p) => p.flying.push(player),
            None => self.plans.push(MissionPlan {
                offer,
                base,
                route: Vec::new(),
                crew: Vec::new(),
                flying: vec![player],
            }),
        }
        ev.extend([CampaignEvent::SortieStarted { player, sortie }, self.plans_changed()]);
        Ok(ev)
    }

    pub(crate) fn end_sortie(&mut self, player: PlayerId, end: SortieEnd) -> Vec<CampaignEvent> {
        let Some(s) = self.slot_mut(player).sortie.take() else { return Vec::new() };
        self.leave_plans(player);
        let mut ev = Vec::new();
        // His own insertion goes on with the wingmen (if any are left); a pilot aboard is lost
        // with the helicopter.
        if let Some(i) = s.progress.insertion {
            self.left_insertion(player, i, &mut ev);
        }
        if let Objective::Rescue { downed } = s.offer.objective
            && s.progress.aboard
            && !s.progress.delivered
        {
            self.lost_aboard(downed, &mut ev);
        }
        let (insertion, raid, downed) = match s.offer.objective {
            Objective::Escort { insertion } | Objective::Intercept { insertion } => {
                (Some(insertion), None, None)
            }
            Objective::Insertion { .. } => (s.progress.insertion, None, None),
            Objective::Defend { raid } => (None, Some(raid), None),
            Objective::Rescue { downed } => (None, None, Some(downed)),
            _ => (None, None, None),
        };
        let open = insertion.and_then(|i| self.insertion(i)).is_some_and(|i| {
            matches!(i.state, InsertionState::Pending { .. } | InsertionState::Enroute)
        }) || raid.and_then(|r| self.raid(r)).is_some_and(Raid::open)
            || downed
                .and_then(|d| self.downed_pilot(d))
                .is_some_and(|d| d.state == DownedState::Waiting);
        // A player's insertion is his whatever he did once it lifted off.
        let present = s.progress.present() || s.progress.insertion.is_some();
        let result = if s.progress.achieved {
            SortieResult::Success
        } else if open && present {
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
            raid,
            downed,
            aircraft: s.aircraft,
            pilot: self.player_name(player).unwrap_or("").to_string(),
            points: flight_points(
                s.progress.air_kills,
                (s.progress.sites_destroyed.len() + s.progress.aaa_destroyed.len()) as u32,
                end,
                s.progress.bonus,
            ) + if result == SortieResult::Success { s.offer.kind.points() } else { 0 },
            bonus: s.progress.bonus,
            place: self.objective_place(&s.offer.objective),
            achieved_at: s.progress.achieved_at,
        };
        let verdict = match result {
            SortieResult::Success => "objective achieved",
            SortieResult::Failed => "objective not achieved",
            SortieResult::Pending if raid.is_some() => "waiting for the raid",
            SortieResult::Pending if downed.is_some() => "waiting for the rescue",
            SortieResult::Pending => "waiting for the insertion",
        };
        self.note(format!("{}: {} - {verdict}", s.offer.title, end.name()));
        self.history.push(record.clone());
        ev.extend([CampaignEvent::SortieFinished { player, record }, self.plans_changed()]);
        if end == SortieEnd::Ejected
            && let Some((x, y)) = s.progress.last
        {
            self.ejected(player, x, y, &mut ev);
        }
        ev
    }

    /// A player joins the plan of an offer (None: leaves the plans). The plan's base stands
    /// unless nobody is planning or flying it.
    pub(crate) fn select(
        &mut self,
        player: PlayerId,
        offer: Option<u32>,
        base: ObjectId,
    ) -> Result<Vec<CampaignEvent>, String> {
        if let Some(offer) = offer {
            if !self.board.iter().any(|o| o.id == offer) {
                return Err("That mission is no longer on the board".into());
            }
            if self.sortie(player).is_some() {
                return Err("Already flying a sortie".into());
            }
            if !self.player_bases().contains(&base) {
                return Err("Not a friendly base".into());
            }
        }
        for p in &mut self.plans {
            p.crew.retain(|&c| c != player);
        }
        if let Some(offer) = offer {
            match self.plans.iter_mut().find(|p| p.offer == offer) {
                Some(p) => {
                    if p.crew.is_empty() && p.flying.is_empty() && p.base != base {
                        p.base = base;
                        p.route.clear();
                    }
                    p.crew.push(player);
                }
                None => self.plans.push(MissionPlan {
                    offer,
                    base,
                    route: Vec::new(),
                    crew: vec![player],
                    flying: Vec::new(),
                }),
            }
        }
        Ok(vec![self.plans_changed()])
    }

    /// A player on a plan changes its base (which resets the route) and route; not while
    /// anyone flies it.
    pub(crate) fn edit_plan(
        &mut self,
        player: PlayerId,
        offer: u32,
        base: ObjectId,
        route: Vec<RoutePoint>,
    ) -> Result<Vec<CampaignEvent>, String> {
        if route.len() > ROUTE_POINTS {
            return Err("Too many route points".into());
        }
        if !self.player_bases().contains(&base) {
            return Err("Not a friendly base".into());
        }
        let Some(p) = self.plans.iter_mut().find(|p| p.offer == offer) else {
            return Err("That mission is no longer on the board".into());
        };
        if !p.crew.contains(&player) {
            return Err("Not planning that mission".into());
        }
        if !p.flying.is_empty() {
            return Err("The route is locked while the mission is flown".into());
        }
        if p.base != base {
            p.base = base;
            p.route.clear();
        } else {
            p.route = route;
        }
        Ok(vec![self.plans_changed()])
    }

    fn leave_plans(&mut self, player: PlayerId) {
        for p in &mut self.plans {
            p.crew.retain(|&c| c != player);
            p.flying.retain(|&c| c != player);
        }
    }

    /// Drops the plans of offers gone from the board nobody flies. True: some went.
    pub(crate) fn prune_plans(&mut self) -> bool {
        let n = self.plans.len();
        let board = &self.board;
        self.plans.retain(|p| !p.flying.is_empty() || board.iter().any(|o| o.id == p.offer));
        self.plans.len() != n
    }

    pub(crate) fn plans_changed(&self) -> CampaignEvent {
        CampaignEvent::PlansChanged(self.plans.clone())
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
                        s.progress.cap_kills += 1;
                        s.progress.on_station += CAP_SECONDS / CAP_KILLS as f64;
                    }
                }
                // A friendly aircraft lost there: the time on station starts again (the kills
                // there stand).
                (
                    Report::FlightDestroyed { side: Side::Blue, x, y, .. },
                    Objective::Cap { x: cx, y: cy },
                ) if dist(x - cx, y - cy) < CAP_RADIUS => {
                    s.progress.losses += 1;
                    s.progress.on_station =
                        s.progress.cap_kills as f64 * CAP_SECONDS / CAP_KILLS as f64;
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
        let mut captures = Vec::new();
        let mut boarded = Vec::new();
        for k in 0..self.players.len() {
            let id = self.players[k].id;
            let Some(s) = self.players[k].sortie.as_ref() else { continue };
            let mut p = s.progress.clone();
            let me = view.players.iter().find(|p| p.id == id && p.airborne);
            if let Some(me) = me {
                p.last = Some((me.x, me.y));
            }
            // The helicopter landed where it counts: the troops get out, the downed pilot
            // boards; with him aboard, landed at a friendly base he is home.
            let landed = view.players.iter().find(|p| p.id == id && p.landed);
            if p.heli {
                let zone = match s.offer.objective {
                    Objective::Insertion { .. } => p
                        .insertion
                        .and_then(|i| self.insertion(i))
                        .filter(|i| i.state == InsertionState::Enroute)
                        .and_then(|i| self.site(i.target))
                        .map(|t| (t.x, t.y, CAPTURE_RADIUS, UNLOAD_SECONDS)),
                    Objective::Rescue { downed } if !p.aboard => self
                        .downed_pilot(downed)
                        .filter(|d| d.state == DownedState::Waiting)
                        .map(|d| (d.x, d.y, BOARD_RADIUS, BOARD_SECONDS)),
                    _ => None,
                };
                match (zone, landed) {
                    (Some((x, y, r, secs)), Some(me)) if dist(me.x - x, me.y - y) <= r => {
                        p.landed_secs += dt;
                        if p.landed_secs >= secs && p.landed_secs - dt < secs {
                            match s.offer.objective {
                                Objective::Rescue { downed } => {
                                    p.aboard = true;
                                    boarded.push(downed);
                                }
                                _ => captures.extend(p.insertion),
                            }
                        }
                    }
                    _ => p.landed_secs = 0.0,
                }
                if p.aboard
                    && !p.delivered
                    && let Some(me) = landed
                    && self.player_bases().iter().any(|&b| {
                        self.site(b).is_some_and(|s| dist(me.x - s.x, me.y - s.y) <= HOME_RADIUS)
                    })
                {
                    p.delivered = true;
                }
            }
            // On station: over the CAP point, or over the downed pilot.
            let station = match s.offer.objective {
                Objective::Cap { x, y } => Some((x, y, CAP_RADIUS)),
                Objective::Rescue { downed } => self
                    .downed_pilot(downed)
                    .filter(|d| d.state == DownedState::Waiting)
                    .map(|d| (d.x, d.y, RESCUE_RADIUS)),
                _ => None,
            };
            if let (Some((x, y, r)), Some(me)) = (station, me)
                && dist(me.x - x, me.y - y) < r
            {
                p.on_station += dt;
            }
            if let Some(me) = me
                && self
                    .presence_points(&s.offer.objective, view)
                    .iter()
                    .any(|&(x, y)| dist(me.x - x, me.y - y) < PRESENCE_RADIUS)
            {
                p.near_secs += dt;
            }
            self.players[k].sortie.as_mut().expect("checked").progress = p;
        }
        for id in captures {
            if let Some(k) = self.insertions.iter().position(|i| i.id == id) {
                let name = self.places[self.insertions[k].place as usize].name.clone();
                ev.push(CampaignEvent::Message(format!("Troops are out at {name}")));
                self.capture(k, ev);
            }
        }
        for downed in boarded {
            if let Some(d) = self.downed.iter_mut().find(|d| d.id == downed) {
                d.state = DownedState::Aboard;
                let text = format!("{} is aboard: bring him home", d.name);
                self.note(text.clone());
                ev.push(CampaignEvent::Message(text));
            }
        }
        for k in 0..self.players.len() {
            let id = self.players[k].id;
            let Some(s) = self.players[k].sortie.as_ref() else { continue };
            if !s.progress.achieved && self.objective_met(&s.offer.objective, &s.progress) {
                achieved.push((id, s.offer.title.clone(), s.offer.objective.clone()));
                let p = &mut self.players[k].sortie.as_mut().expect("checked").progress;
                p.achieved = true;
                p.achieved_at = Some(self.clock);
            }
        }
        for (player, title, objective) in achieved {
            if let Objective::Recon { place, .. } = objective {
                self.gain_intel(place, ev);
            }
            if let Objective::Rescue { downed } = objective {
                self.rescued(downed, ev);
            }
            self.note(format!("{title}: objective achieved"));
            ev.push(CampaignEvent::ObjectiveAchieved { player, title });
            self.package(player, ev);
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
            Objective::Cas { aaa, .. } | Objective::Battery { aaa, .. } => {
                share(aaa.len(), aaa.iter().filter(|a| p.aaa_destroyed.contains(a)).count())
            }
            Objective::Strike { airfield, .. } => !self.site(*airfield).is_some_and(Site::alive),
            Objective::AntiShip { ship } => !self.site(*ship).is_some_and(Site::alive),
            Objective::Cap { .. } => p.on_station >= CAP_SECONDS,
            Objective::Escort { insertion } => {
                p.present()
                    && self
                        .insertion(*insertion)
                        .is_some_and(|i| i.state == InsertionState::Captured)
            }
            Objective::Insertion { .. } => p
                .insertion
                .and_then(|i| self.insertion(i))
                .is_some_and(|i| i.state == InsertionState::Captured),
            Objective::Intercept { insertion } => {
                p.present()
                    && self.insertion(*insertion).is_some_and(|i| i.state == InsertionState::Failed)
            }
            Objective::Defend { raid } => {
                p.present() && self.raid(*raid).is_some_and(|r| r.state == RaidState::Repelled)
            }
            Objective::Interdict { object, .. } => !self.site(*object).is_some_and(Site::alive),
            Objective::Awacs { tag } => self.awacs_down.contains(tag),
            Objective::Rescue { .. } if p.heli => p.delivered,
            Objective::Rescue { downed } => {
                p.on_station >= RESCUE_SECONDS
                    || p.present()
                        && self
                            .downed_pilot(*downed)
                            .is_some_and(|d| d.state == DownedState::Rescued)
            }
        }
    }

    /// Where the pilot has to be for an objective that depends on others (helicopters, raiders,
    /// a downed pilot) while it goes on; empty when it does not, or is over.
    fn presence_points(&self, o: &Objective, view: &WorldView) -> Vec<(i32, i32)> {
        let flying = |tags: &[u32]| -> Vec<(i32, i32)> {
            view.flights.iter().filter(|f| tags.contains(&f.tag)).map(|f| (f.x, f.y)).collect()
        };
        let at = |k: ObjectId| self.site(k).map(|s| (s.x, s.y));
        match *o {
            Objective::Escort { insertion } | Objective::Intercept { insertion } => {
                let Some(i) = self.insertion(insertion).filter(|i| {
                    matches!(i.state, InsertionState::Pending { .. } | InsertionState::Enroute)
                }) else {
                    return Vec::new();
                };
                let mut v = flying(&i.helicopters);
                v.extend(at(i.target));
                if matches!(o, Objective::Intercept { .. }) {
                    v.push(self.intercept_point(i));
                } else {
                    v.extend(at(i.from));
                }
                v
            }
            Objective::Defend { raid } => {
                let Some(r) = self.raid(raid).filter(|r| r.open()) else { return Vec::new() };
                let mut v = flying(&r.aircraft);
                v.extend(at(r.target));
                v
            }
            Objective::Rescue { downed } => self
                .downed_pilot(downed)
                .filter(|d| d.state == DownedState::Waiting)
                .map(|d| vec![(d.x, d.y)])
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    /// An insertion is over (captured, failed or called off): settles the escorts and
    /// intercepts against it that already ended their sortie.
    pub(crate) fn settle_insertion(&mut self, insertion: u32, state: InsertionState) {
        for r in &mut self.history {
            if r.insertion == Some(insertion) && r.result == SortieResult::Pending {
                let won = match r.kind {
                    MissionKind::Intercept => state == InsertionState::Failed,
                    _ => state == InsertionState::Captured,
                };
                settle(r, won);
            }
        }
    }

    /// A raid is over: settles the airfield defences against it that already ended.
    pub(crate) fn settle_raid(&mut self, raid: u32, repelled: bool) {
        for r in &mut self.history {
            if r.raid == Some(raid) {
                settle(r, repelled);
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
