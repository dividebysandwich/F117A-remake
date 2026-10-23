//! Helicopter missions: the generated targets become landing zones. The helicopter lands near
//! the primary (and the secondary) target and lets its troops out (mission type
//! [`INSERT`], judged by the simulation); landing at a secret airstrip (type 4) stays as it is.

use crate::Mission;
use crate::text::OrdersLine;

/// Mission type of an insertion: land within the simulation's insertion radius of the target.
pub const INSERT: i16 = 9;

/// Turns a generated mission into a helicopter's: insertions at its targets, no stores. Done
/// once (a mission already turned is left alone).
pub fn for_helicopter(m: &mut Mission) {
    if m.primary.mission_type == INSERT {
        return;
    }
    let mut places = Vec::new();
    for t in [&mut m.primary, &mut m.secondary] {
        if t.object_index < 0 || t.mission_type == 4 {
            places.push(None);
            continue;
        }
        let place = if t.place_name.is_empty() { t.onc.clone() } else { t.place_name.clone() };
        let old = std::mem::take(&mut t.objective);
        t.mission_type = INSERT;
        t.type_name = "INSERT".into();
        t.objective = format!(
            "Land the troops within 500 m of the target at {place} and stay on the ground until \
             they are out."
        );
        let b = &mut m.briefing;
        for line in b.lines.iter_mut().filter(|l| **l == old) {
            line.clone_from(&t.objective);
        }
        places.push(Some(place));
    }
    let b = &mut m.briefing;
    b.primary_objective.clone_from(&m.primary.objective);
    b.secondary_objective.clone_from(&m.secondary.objective);
    b.armament.clear();
    m.loadout.usable_bays = 0;
    let o = &mut m.orders;
    let back: Vec<OrdersLine> = o.lines.iter().rev().take(2).rev().cloned().collect();
    let mut lines = Vec::new();
    for (place, colour, order) in
        [(&places[0], 0x11, &mut o.primary), (&places[1], 0x29, &mut o.secondary)]
    {
        if let Some(p) = place {
            lines.push(OrdersLine { text: "Insert troops at".into(), color: colour });
            lines.push(OrdersLine { text: p.clone(), color: colour });
            *order = format!("INSERT TROOPS AT {}", p.to_uppercase());
        }
    }
    if !lines.is_empty() {
        lines.extend(back);
        o.lines = lines;
    }
}
