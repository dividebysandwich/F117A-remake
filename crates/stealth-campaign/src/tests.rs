use stealth_mission::TheaterData;

use super::*;

fn theater(code: &str) -> TheaterData {
    let path = format!("{}/../../assets/mission/theaters/{code}.json", env!("CARGO_MANIFEST_DIR"));
    TheaterData::from_json(&std::fs::read(path).expect("theater file")).expect("theater")
}

fn campaign(code: &str, index: usize) -> CampaignState {
    let t = theater(code);
    let setup = CampaignSetup {
        theater: index,
        theater_code: code.into(),
        roe: 1,
        opponents: 1,
        seed: 1234,
        sky_ratio: 24.0,
        start_hour: 8.0,
        dynamic_weather: true,
    };
    CampaignState::new(setup, &t.world)
}

const P0: PlayerId = PlayerId(0);

fn place_named(c: &CampaignState, name: &str) -> PlaceId {
    c.places.iter().position(|p| p.name == name).expect(name) as PlaceId
}

#[test]
fn places_and_sides_from_the_theater() {
    for (code, index) in [("ce", 3), ("ua", 9), ("pg", 1), ("vn", 4)] {
        let c = campaign(code, index);
        assert!(c.held(Side::Blue) > 0, "{code}: no friendly places");
        assert!(c.held(Side::Red) > 0, "{code}: no enemy places");
        assert!(!c.bases(Side::Blue).is_empty(), "{code}: no friendly bases");
        assert!(!c.front(Side::Blue).is_empty(), "{code}: no front");
        // Every SAM site that belongs to a place is one of its defences.
        for s in c.sites.iter().filter(|s| s.kind == SiteKind::Sam) {
            if let Some(p) = s.place {
                assert!(c.places[p as usize].defences.contains(&s.object));
            }
        }
    }
    // Central Europe: Tabor is a city with an airfield and a SAM site.
    let c = campaign("ce", 3);
    let tabor = &c.places[place_named(&c, "Tabor") as usize];
    assert_eq!(tabor.owner, Side::Red);
    assert!(tabor.airfield.is_some());
    assert_eq!(tabor.defences.len(), 1);
    assert_eq!(c.places[place_named(&c, "Ramstein") as usize].owner, Side::Blue);
    assert_eq!(c.places[place_named(&c, "Vienna") as usize].owner, Side::Neutral);
}

#[test]
fn the_board_offers_missions_on_the_front() {
    let mut c = campaign("ce", 3);
    let ev = c.tick(1.0, &WorldView::default(), &[]);
    assert!(ev.contains(&CampaignEvent::BoardChanged));
    let kinds: Vec<MissionKind> = c.board.iter().map(|o| o.kind).collect();
    for k in [MissionKind::Recon, MissionKind::Insertion, MissionKind::Cap] {
        assert!(kinds.contains(&k), "no {k:?} in {kinds:?}");
    }
    assert!(kinds.contains(&MissionKind::Sead) || kinds.contains(&MissionKind::Strike));
    assert!(c.board.len() <= tuning::BOARD_SIZE);
    // Offers keep their ids from one rebuild to the next.
    let ids: Vec<u32> = c.board.iter().map(|o| o.id).collect();
    let ev = c.tick(tuning::BOARD_REFRESH + 1.0, &WorldView::default(), &[]);
    assert!(!ev.contains(&CampaignEvent::BoardChanged));
    assert_eq!(ids, c.board.iter().map(|o| o.id).collect::<Vec<_>>());
}

/// Orders an insertion against the first front place and lifts the helicopters off.
fn launch(c: &mut CampaignState) -> (PlaceId, u32, ObjectId, Vec<u32>) {
    let place = c.front(Side::Blue)[0];
    c.apply(P0, Command::OrderInsertion { place }).expect("order");
    assert!(c.apply(P0, Command::OrderInsertion { place }).is_err(), "ordered twice");
    let ev = c.tick(tuning::INSERTION_LEAD + 1.0, &WorldView::default(), &[]);
    let (id, target, tags) = ev
        .iter()
        .find_map(|e| match e {
            CampaignEvent::InsertionLaunched { id, target, tags, .. } => {
                Some((*id, *target, tags.clone()))
            }
            _ => None,
        })
        .expect("launched");
    assert_eq!(tags.len(), tuning::INSERTION_HELOS);
    (place, id, target, tags)
}

#[test]
fn a_landed_helicopter_captures_the_place() {
    let mut c = campaign("ce", 3);
    c.tick(1.0, &WorldView::default(), &[]);
    let (place, _, target, tags) = launch(&mut c);
    let defences = c.places[place as usize].defences.clone();
    let t = c.site(target).unwrap().clone();
    // Three shot down, the last one lands.
    let mut reports: Vec<Report> = tags[..3]
        .iter()
        .map(|&tag| Report::FlightDestroyed { tag, side: Side::Blue, x: 0, y: 0, by: None })
        .collect();
    reports.push(Report::FlightLanded { tag: tags[3], x: t.x + 10, y: t.y - 10 });
    let ev = c.tick(1.0, &WorldView::default(), &reports);
    let captured = ev.iter().find_map(|e| match e {
        CampaignEvent::PlaceCaptured { place: p, side, removed, .. } => {
            Some((*p, *side, removed.clone()))
        }
        _ => None,
    });
    assert_eq!(captured, Some((place, Side::Blue, defences.clone())));
    assert_eq!(c.places[place as usize].owner, Side::Blue);
    assert_eq!(c.owner(target), Some(Side::Blue));
    for d in defences {
        assert!(c.site(d).unwrap().removed);
    }
    // The airfield is a friendly base now, and the board moved on.
    if c.places[place as usize].airfield.is_some() {
        assert!(c.bases(Side::Blue).contains(&target));
    }
    assert!(!c.front(Side::Blue).contains(&place));
}

#[test]
fn an_insertion_fails_when_every_helicopter_is_lost() {
    let mut c = campaign("ce", 3);
    let (place, id, _, tags) = launch(&mut c);
    let reports: Vec<Report> = tags
        .iter()
        .map(|&tag| Report::FlightDestroyed { tag, side: Side::Blue, x: 0, y: 0, by: None })
        .collect();
    let ev = c.tick(1.0, &WorldView::default(), &reports);
    assert!(ev.contains(&CampaignEvent::InsertionFailed { id, place }));
    assert_eq!(c.places[place as usize].owner, Side::Red);
    // A landing somewhere else takes nothing either.
    assert_eq!(c.insertion(id).unwrap().state, InsertionState::Failed);
}

#[test]
fn a_recon_sortie_gives_intelligence() {
    let mut c = campaign("ce", 3);
    c.tick(1.0, &WorldView::default(), &[]);
    let offer = c.board.iter().find(|o| o.kind == MissionKind::Recon).unwrap().clone();
    let Objective::Recon { place, object } = offer.objective else { unreachable!() };
    c.apply(P0, Command::TakeMission { offer: offer.id, base: None, aircraft: Aircraft::F117 })
        .expect("take");
    assert!(
        c.apply(P0, Command::TakeMission { offer: offer.id, base: None, aircraft: Aircraft::F117 })
            .is_err()
    );
    let ev = c.tick(1.0, &WorldView::default(), &[Report::PhotoTaken { player: P0, object }]);
    assert!(ev.iter().any(|e| matches!(e, CampaignEvent::ObjectiveAchieved { .. })));
    assert!(
        ev.iter().any(|e| matches!(e, CampaignEvent::IntelGained { place: p, .. } if *p == place))
    );
    assert!(c.has_intel(object));
    let ev = c.apply(P0, Command::EndSortie { end: SortieEnd::Crashed }).unwrap();
    let CampaignEvent::SortieFinished { record, .. } = &ev[0] else { panic!() };
    assert_eq!(record.result, SortieResult::Success);
    // The intelligence goes stale.
    let ev = c.tick(tuning::INTEL_SECONDS, &WorldView::default(), &[]);
    assert!(ev.iter().any(|e| matches!(e, CampaignEvent::IntelExpired { .. })));
    assert!(!c.has_intel(object));
}

#[test]
fn strikes_stop_and_repairs_restore_an_airfield() {
    let mut c = campaign("ce", 3);
    c.tick(1.0, &WorldView::default(), &[]);
    let offer = c.board.iter().find(|o| o.kind == MissionKind::Strike).unwrap().clone();
    let Objective::Strike { airfield, .. } = offer.objective else { unreachable!() };
    c.apply(P0, Command::TakeMission { offer: offer.id, base: None, aircraft: Aircraft::F117 })
        .unwrap();
    let ev = c.tick(
        1.0,
        &WorldView::default(),
        &[Report::ObjectDestroyed { object: airfield, by: Some(P0) }],
    );
    assert!(ev.iter().any(|e| matches!(e, CampaignEvent::ObjectiveAchieved { .. })));
    assert!(!c.bases(Side::Red).contains(&airfield));
    let ev = c.tick(tuning::REPAIR_AIRFIELD, &WorldView::default(), &[]);
    assert!(ev.contains(&CampaignEvent::Repaired { object: airfield }));
    assert!(c.bases(Side::Red).contains(&airfield));
}

#[test]
fn an_escort_is_settled_by_its_insertion() {
    let mut c = campaign("ce", 3);
    let place = c.front(Side::Blue)[0];
    c.apply(P0, Command::OrderInsertion { place }).unwrap();
    c.tick(1.0, &WorldView::default(), &[]);
    let escort = c.board.iter().find(|o| o.kind == MissionKind::Escort).unwrap().clone();
    c.apply(P0, Command::TakeMission { offer: escort.id, base: None, aircraft: Aircraft::F117 })
        .unwrap();
    // The escort lands before the helicopters get there: pending.
    let ev = c.apply(P0, Command::EndSortie { end: SortieEnd::Landed }).unwrap();
    let CampaignEvent::SortieFinished { record, .. } = &ev[0] else { panic!() };
    assert_eq!(record.result, SortieResult::Pending);
    let ev = c.tick(tuning::INSERTION_LEAD, &WorldView::default(), &[]);
    let tags = ev
        .iter()
        .find_map(|e| match e {
            CampaignEvent::InsertionLaunched { tags, target, .. } => Some((tags.clone(), *target)),
            _ => None,
        })
        .unwrap();
    let t = c.site(tags.1).unwrap().clone();
    c.tick(1.0, &WorldView::default(), &[Report::FlightLanded { tag: tags.0[0], x: t.x, y: t.y }]);
    assert_eq!(c.history.last().unwrap().result, SortieResult::Success);
}

#[test]
fn enemy_patrols_and_scrambles() {
    let mut c = campaign("ce", 3);
    let ev = c.tick(tuning::PATROL_EVERY + 1.0, &WorldView::default(), &[]);
    let patrol = ev.iter().find_map(|e| match e {
        CampaignEvent::LaunchFighters { tag, side: Side::Red, from, .. } => Some((*tag, *from)),
        _ => None,
    });
    let (tag, from) = patrol.expect("a patrol");
    assert_eq!(c.owner(from), Some(Side::Red));
    // The patrol stays tracked while the simulation has it.
    let view = WorldView {
        flights: vec![FlightView { x: 0, y: 0, side: Side::Red, tag }],
        ..Default::default()
    };
    c.tick(60.0, &view, &[]);
    assert!(c.patrols.iter().any(|p| p.0 == tag));
    c.tick(60.0, &WorldView::default(), &[]);
    assert!(c.patrols.is_empty());
}

#[test]
fn the_campaign_saves_and_loads() {
    let mut c = campaign("ua", 9);
    c.tick(1.0, &WorldView::default(), &[]);
    let (_, id, _, _) = launch(&mut c);
    let json = serde_json::to_string(&c).unwrap();
    let mut d: CampaignState = serde_json::from_str(&json).unwrap();
    assert_eq!(c, d);
    d.after_load();
    assert!(matches!(d.insertion(id).unwrap().state, InsertionState::Pending { .. }));
}

#[test]
fn the_same_inputs_give_the_same_campaign() {
    let run = || {
        let mut c = campaign("ce", 3);
        for k in 0..200 {
            c.tick(30.0, &WorldView::default(), &[]);
            if k == 10 {
                let place = c.front(Side::Blue)[0];
                let _ = c.apply(P0, Command::OrderInsertion { place });
            }
        }
        serde_json::to_string(&c).unwrap()
    };
    assert_eq!(run(), run());
}

#[test]
fn sites_are_spotted_by_rwr_and_recon() {
    let mut c = campaign("ce", 3);
    let wittstock = place_named(&c, "Wittstock");
    let sam = c.places[wittstock as usize].defences[0];
    assert!(!c.site(sam).unwrap().spotted);
    c.tick(1.0, &WorldView::default(), &[Report::SiteSpotted { object: sam }]);
    assert!(c.site(sam).unwrap().spotted);
    // Recon photos map a place's defences.
    let tabor = place_named(&c, "Tabor");
    let other = c.places[tabor as usize].defences[0];
    let mut ev = Vec::new();
    c.gain_intel(tabor, &mut ev);
    assert!(c.site(other).unwrap().spotted);
}

#[test]
fn a_full_campaign_hands_an_idle_slot_to_a_new_name() {
    // Slot 0 is the local player's.
    let mut c = campaign("ce", 3);
    let b = PlayerId(1);
    assert_eq!(c.join("Alpha", 3, |_| false), Some(b));
    assert_eq!(c.join("Bravo", 3, |_| false), Some(PlayerId(2)));
    assert_eq!(c.join("alpha", 3, |_| false), Some(b), "the same name, the same slot");
    // Everybody connected: no room.
    assert_eq!(c.join("Charlie", 3, |_| true), None);
    // Alpha left: Charlie gets his slot.
    assert_eq!(c.join("Charlie", 3, |id| id != b), Some(b));
    assert_eq!(c.player_name(b), Some("Charlie"));
}

#[test]
fn players_share_a_plan_locked_while_it_is_flown() {
    let mut c = campaign("ce", 3);
    c.tick(1.0, &WorldView::default(), &[]);
    let b = PlayerId(1);
    c.join("Bravo", 3, |_| false);
    let offer = c.board.iter().find(|o| !o.kind.ai_flown()).unwrap().clone();
    let bases = c.bases(Side::Blue);
    let other = *bases.iter().find(|&&x| x != offer.base).expect("two bases");
    let select = |base| Command::Select { offer: Some(offer.id), base };
    c.apply(P0, select(offer.base)).expect("select");
    // The second player joins on the plan's base, whatever he chose.
    let ev = c.apply(b, select(other)).expect("join");
    let CampaignEvent::PlansChanged(plans) = &ev[0] else { panic!() };
    assert_eq!(plans[0].crew, vec![P0, b]);
    assert_eq!(plans[0].base, offer.base);
    let route = vec![
        RoutePoint { x: 100, y: 100, kind: RouteKind::Nav },
        RoutePoint { x: offer.x, y: offer.y, kind: RouteKind::Primary },
    ];
    let plan = |route: Vec<RoutePoint>| Command::Plan { offer: offer.id, base: offer.base, route };
    c.apply(b, plan(route.clone())).expect("edit");
    assert_eq!(c.plan(offer.id).unwrap().route, route);
    // Flown: locked; the pilot leaves the crew.
    c.apply(P0, Command::TakeMission { offer: offer.id, base: None, aircraft: Aircraft::F117 })
        .expect("take");
    assert_eq!(c.plan(offer.id).unwrap().flying, vec![P0]);
    assert_eq!(c.plan(offer.id).unwrap().crew, vec![b]);
    assert!(c.apply(b, plan(Vec::new())).is_err());
    // Nobody flying any more: editable again, the route kept.
    c.apply(P0, Command::EndSortie { end: SortieEnd::Aborted }).expect("end");
    assert_eq!(c.plan(offer.id).unwrap().route, route);
    c.apply(b, plan(Vec::new())).expect("edit again");
    // A new base resets the route.
    c.apply(b, plan(route.clone())).unwrap();
    c.apply(b, Command::Plan { offer: offer.id, base: other, route: route.clone() }).unwrap();
    assert_eq!(c.plan(offer.id).unwrap().base, other);
    assert!(c.plan(offer.id).unwrap().route.is_empty());
    // Leaving; the plan stays while its offer is on the board.
    c.apply(b, Command::Select { offer: None, base: 0 }).unwrap();
    assert!(c.plan(offer.id).unwrap().crew.is_empty());
    c.board.retain(|o| o.id != offer.id);
    assert!(c.prune_plans());
    assert!(c.plan(offer.id).is_none());
}
