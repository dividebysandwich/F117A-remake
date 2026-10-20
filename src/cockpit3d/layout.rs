//! Instrument panel layouts: the faces drawn into the instrument atlas (lamps, readouts,
//! labels, bezel buttons), which of them are buttons, and the displays, in the panel's pixel
//! frame (the original 320x200 screen projected onto the panel plane, `geometry::panel_point`).
//!
//! The cockpit model (`model`, the Blender files) places the faces; their rectangles here set
//! each face's size in the atlas, so a face modelled at its rectangle's size shows its texels
//! square. The rectangles are where the faces were when the models were made:
//! - `classic`: the original 256PIT arrangement, with the 77x60 MFDs.
//! - `modern`: larger high-resolution MFDs with labelled bezel buttons, lamp blocks either side.
//!
//! The F-16's panel (`f16`) is not on that plane: its rectangles are millimetres on its own flat
//! panel (x right, y down from the panel's centre). Its model is generated from this layout
//! (`tools/models/build_cockpit_f16.py` reads `assets-src/blend/cockpit_f16_layout.json`, which
//! a test keeps in step with `f16`).

use crate::input::Action;
use crate::mfd::{LeftMfd, MfdSide, OSB_COUNT};
use crate::panel::{LampId, LedId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelStyle {
    Classic,
    Modern,
    F16,
}

/// A rectangle in panel pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PxRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

pub const fn rect(x: f32, y: f32, w: f32, h: f32) -> PxRect {
    PxRect { x, y, w, h }
}

impl PxRect {
    #[cfg(test)]
    fn overlaps(&self, o: &PxRect) -> bool {
        self.x < o.x + o.w && o.x < self.x + self.w && self.y < o.y + o.h && o.y < self.y + self.h
    }
}

/// What a pushable control does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    Action(Action),
    /// Select a HUD mode (0 NAV, 1 AIR, 2 GND) by cycling.
    HudMode(u8),
    /// Select a left MFD page.
    LeftPage(LeftMfd),
    /// Next tracking camera direction.
    CamCycle,
    /// Time acceleration on / off.
    TimeToggle,
    /// An MFD bezel button (its function depends on the page, `mfd::osb`).
    Osb(MfdSide, u8),
}

/// A face drawn into the instrument atlas.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Elem {
    /// A lamp, with an optional label replacing the standard one.
    Lamp(LampId, Option<&'static str>),
    Led(LedId),
    /// EMV / EGR / EAR gauge.
    Gauge,
    /// Text (lines separated by '\n'), foreground and background colour.
    Label(&'static str, u8, u8),
    /// Yellow and black warning stripes.
    Hazard,
    /// Landing gear lights: an aircraft outline with a green lamp at each wheel, lit with the
    /// gear down.
    GearLights,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    pub elem: Elem,
    pub rect: PxRect,
    /// Pushable: the face is a button cap.
    pub cap: Option<Effect>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MfdUnit {
    pub side: MfdSide,
    pub screen: PxRect,
    /// Labelled bezel buttons around the screen.
    pub osbs: bool,
}

pub struct PanelLayout {
    pub style: PanelStyle,
    pub items: Vec<Placed>,
    /// Attitude ball (square around the disc).
    pub adi: PxRect,
    pub mfds: [MfdUnit; 2],
    /// Radar warning receiver scope and horizontal situation indicator (F-16).
    pub rwr: Option<PxRect>,
    pub hsi: Option<PxRect>,
}

/// Bezel button cap size (panel pixels; millimetres on the F-16 panel).
const OSB_W: f32 = 16.0;
const OSB_H: f32 = 8.0;
const F16_OSB_W: f32 = 14.0;
const F16_OSB_H: f32 = 9.0;

impl PanelLayout {
    pub fn for_style(style: PanelStyle) -> Self {
        match style {
            PanelStyle::Classic => Self::classic(),
            PanelStyle::Modern => Self::modern(),
            PanelStyle::F16 => Self::f16(),
        }
    }

    /// Bezel buttons of an MFD: (slot, cap rectangle); 0..5 above the screen, 5..10 below.
    pub fn osbs(&self, unit: &MfdUnit) -> Vec<(u8, PxRect)> {
        if !unit.osbs {
            return Vec::new();
        }
        let s = unit.screen;
        let (w, h) = if self.style == PanelStyle::F16 { (F16_OSB_W, F16_OSB_H) } else { (OSB_W, OSB_H) };
        (0..OSB_COUNT)
            .map(|slot| {
                let col = (slot % 5) as f32;
                let cx = s.x + s.w * (col + 0.5) / 5.0;
                let y = if slot < 5 { s.y - h - 1.5 } else { s.y + s.h + 1.5 };
                (slot, rect(cx - w / 2.0, y, w, h))
            })
            .collect()
    }

    /// The original arrangement (256PIT).
    pub fn classic() -> Self {
        use Effect as E;
        use LampId::*;
        let mut items = Vec::new();
        // Lamps at the original light rectangles ; rect = (x,y)-(x+w,y+h).
        const LIGHTS: [(f32, f32, f32, f32); 37] = [
            (119.0, 94.0, 18.0, 8.0),
            (139.0, 94.0, 18.0, 8.0),
            (159.0, 94.0, 18.0, 8.0),
            (184.0, 94.0, 21.0, 8.0),
            (38.0, 105.0, 24.0, 8.0),
            (4.0, 117.0, 18.0, 8.0),
            (24.0, 117.0, 18.0, 8.0),
            (44.0, 117.0, 18.0, 8.0),
            (4.0, 127.0, 18.0, 8.0),
            (24.0, 127.0, 18.0, 8.0),
            (44.0, 127.0, 18.0, 8.0),
            (4.0, 137.0, 18.0, 8.0),
            (24.0, 137.0, 18.0, 8.0),
            (44.0, 137.0, 18.0, 8.0),
            (51.0, 149.0, 11.0, 6.0),
            (51.0, 159.0, 11.0, 6.0),
            (51.0, 169.0, 11.0, 6.0),
            (4.0, 179.0, 28.0, 8.0),
            (36.0, 179.0, 26.0, 8.0),
            (4.0, 191.0, 18.0, 8.0),
            (24.0, 191.0, 18.0, 8.0),
            (44.0, 191.0, 18.0, 8.0),
            (80.0, 173.0, 18.0, 8.0),
            (100.0, 173.0, 18.0, 8.0),
            (142.0, 193.0, 14.0, 5.0),
            (232.0, 184.0, 17.0, 8.0),
            (190.0, 181.0, 18.0, 8.0),
            (190.0, 191.0, 38.0, 8.0),
            (170.0, 181.0, 18.0, 8.0),
            (210.0, 181.0, 18.0, 8.0),
            (170.0, 191.0, 18.0, 8.0),
            (257.0, 103.0, 25.0, 8.0),
            (257.0, 113.0, 25.0, 8.0),
            (284.0, 113.0, 25.0, 8.0),
            (257.0, 123.0, 25.0, 8.0),
            (284.0, 123.0, 25.0, 8.0),
            (302.0, 179.0, 13.0, 7.0),
        ];
        let caps: [(LampId, Effect); 19] = [
            (Nav, E::HudMode(0)),
            (Air, E::HudMode(1)),
            (Gnd, E::HudMode(2)),
            (Lock, E::Action(Action::DesignateTarget)),
            (Ecm, E::Action(Action::Ecm)),
            (Irj, E::Action(Action::IrJammer)),
            (Map, E::LeftPage(LeftMfd::Map)),
            (Tac, E::LeftPage(LeftMfd::Tac)),
            (Flir, E::Action(Action::Flir)),
            (Wpn, E::Action(Action::Ordnance)),
            (CamDir, E::CamCycle),
            (Way, E::Action(Action::SelectWaypoint)),
            (Ord, E::Action(Action::MissionOrders)),
            (Tc, E::Action(Action::CamAhead)),
            (Bay, E::Action(Action::BayDoors)),
            (Auto, E::Action(Action::Autopilot)),
            (Flaps, E::Action(Action::Flaps)),
            (Gear, E::Action(Action::Gear)),
            (Brake, E::Action(Action::Brakes)),
        ];
        for (i, &(x, y, w, h)) in LIGHTS.iter().enumerate() {
            let id = crate::panel::LAMPS[i].0;
            let cap = caps.iter().find(|(l, _)| *l == id).map(|&(_, e)| e);
            items.push(Placed { elem: Elem::Lamp(id, None), rect: rect(x, y, w + 1.0, h + 1.0), cap });
        }
        // Countermeasure release buttons with their labels, LED readouts and the other texts
        // that were part of the picture.
        for (k, (label, action, led)) in
            [("CHAFF", Action::Chaff, LedId::Chaff), ("FLARE", Action::Flare, LedId::Flares), ("DECOY", Action::Decoy, LedId::Decoys)].into_iter().enumerate()
        {
            let y = 147.0 + 10.0 * k as f32;
            items.push(Placed { elem: Elem::Label(label, 47, 38), rect: rect(4.0, y, 25.0, 9.0), cap: Some(E::Action(action)) });
            items.push(Placed { elem: Elem::Led(led), rect: rect(33.0, y, 14.0, 10.0), cap: None });
        }
        let fixed = [
            (Elem::Led(LedId::Fuel), rect(259.0, 178.0, 18.0, 9.0)),
            (Elem::Led(LedId::Thrust), rect(259.0, 190.0, 18.0, 9.0)),
            (Elem::Gauge, rect(86.0, 187.0, 51.0, 11.0)),
            (Elem::Label("EGR\nEAR", 47, 33), rect(67.0, 187.0, 16.0, 12.0)),
            (Elem::Label("EMV", 47, 33), rect(141.0, 185.0, 17.0, 7.0)),
            (Elem::Label("FUEL", 47, 79), rect(279.0, 178.0, 20.0, 9.0)),
            (Elem::Label("THRUST", 47, 79), rect(279.0, 190.0, 30.0, 9.0)),
            (Elem::Hazard, rect(307.0, 136.0, 12.0, 40.0)),
        ];
        items.extend(fixed.into_iter().map(|(elem, rect)| Placed { elem, rect, cap: None }));

        let screens = [rect(74.0, 110.0, 77.0, 60.0), rect(169.0, 110.0, 77.0, 60.0)];
        // The top two squares outside each screen are buttons (not in the original): map zoom
        // on the left MFD, waypoint selection on the right one.
        for (x, y, label, action) in [
            (65.0, 110.0, "+", Action::MapZoomIn),
            (65.0, 123.0, "-", Action::MapZoomOut),
            (249.0, 112.0, "+", Action::NextWaypoint),
            (249.0, 123.0, "-", Action::LastWaypoint),
        ] {
            items.push(Placed { elem: Elem::Label(label, 47, 22), rect: rect(x, y, 6.0, 6.0), cap: Some(E::Action(action)) });
        }
        Self {
            style: PanelStyle::Classic,
            items,
            adi: rect(265.0, 139.0, 32.0, 32.0),
            mfds: [MfdUnit { side: MfdSide::Left, screen: screens[0], osbs: false }, MfdUnit { side: MfdSide::Right, screen: screens[1], osbs: false }],
            rwr: None,
            hsi: None,
        }
    }

    /// The modern arrangement: two large MFDs with bezel buttons in the middle, warning and
    /// countermeasure lamps on the left, attitude ball and aircraft systems on the right, the
    /// EMV gauge and engine readouts along the bottom.
    pub fn modern() -> Self {
        use Effect as E;
        use LampId::*;
        let mut items = Vec::new();
        let mut lamp = |id: LampId, r: PxRect, cap: Option<Effect>| items.push(Placed { elem: Elem::Lamp(id, None), rect: r, cap });
        // HUD mode and lock caps under the glare shield (as in the original).
        lamp(Nav, rect(119.0, 93.0, 19.0, 8.0), Some(E::HudMode(0)));
        lamp(Air, rect(139.0, 93.0, 19.0, 8.0), Some(E::HudMode(1)));
        lamp(Gnd, rect(159.0, 93.0, 19.0, 8.0), Some(E::HudMode(2)));
        lamp(Lock, rect(184.0, 93.0, 22.0, 8.0), Some(E::Action(Action::DesignateTarget)));
        // Left block: stall, warnings, threats.
        lamp(Stall, rect(2.0, 109.0, 52.0, 8.0), None);
        for (row, ids) in [[Mw, Eng, Fc], [Jam, Fire, Fuel], [Bd, Av, Blank], [Rad, Ir, Trak]].into_iter().enumerate() {
            let y = if row < 3 { 119.0 + 9.0 * row as f32 } else { 147.0 };
            for (col, id) in ids.into_iter().enumerate() {
                lamp(id, rect(2.0 + 18.0 * col as f32, y, 16.0, 8.0), None);
            }
        }
        // Countermeasures: release button, count, released light; jammer switches.
        for (k, (label, action, led, act)) in [
            ("CHAFF", Action::Chaff, LedId::Chaff, ChaffAct),
            ("FLARE", Action::Flare, LedId::Flares, FlareAct),
            ("DECOY", Action::Decoy, LedId::Decoys, DecoyAct),
        ]
        .into_iter()
        .enumerate()
        {
            let y = 157.0 + 9.0 * k as f32;
            items.push(Placed { elem: Elem::Label(label, 47, 38), rect: rect(2.0, y, 22.0, 8.0), cap: Some(E::Action(action)) });
            items.push(Placed { elem: Elem::Led(led), rect: rect(25.0, y, 13.0, 8.0), cap: None });
            items.push(Placed { elem: Elem::Lamp(act, None), rect: rect(39.0, y, 15.0, 8.0), cap: None });
        }
        items.push(Placed { elem: Elem::Lamp(Ecm, None), rect: rect(2.0, 185.0, 25.0, 9.0), cap: Some(E::Action(Action::Ecm)) });
        items.push(Placed { elem: Elem::Lamp(Irj, None), rect: rect(29.0, 185.0, 25.0, 9.0), cap: Some(E::Action(Action::IrJammer)) });
        // Right block: aircraft systems under the attitude ball.
        for (id, x, y, action) in [
            (Bay, 268.0, 158.0, Action::BayDoors),
            (Auto, 294.0, 158.0, Action::Autopilot),
            (Flaps, 268.0, 169.0, Action::Flaps),
            (Gear, 294.0, 169.0, Action::Gear),
            (Brake, 268.0, 180.0, Action::Brakes),
        ] {
            items.push(Placed { elem: Elem::Lamp(id, None), rect: rect(x, y, 24.0, 9.0), cap: Some(E::Action(action)) });
        }
        items.push(Placed { elem: Elem::Lamp(LowFuel, Some("LOW\nFUEL")), rect: rect(294.0, 180.0, 24.0, 9.0), cap: None });
        // Bottom strip: EMV gauge, engine readouts.
        let fixed = [
            (Elem::Label("EGR\nEAR", 47, 33), rect(58.0, 190.0, 14.0, 9.0)),
            (Elem::Gauge, rect(73.0, 190.0, 70.0, 9.0)),
            (Elem::Label("EMV", 47, 33), rect(144.0, 190.0, 16.0, 9.0)),
            (Elem::Lamp(EmvWarn, None), rect(161.0, 190.0, 14.0, 9.0)),
            (Elem::Led(LedId::Fuel), rect(180.0, 190.0, 18.0, 9.0)),
            (Elem::Label("FUEL", 47, 79), rect(199.0, 190.0, 20.0, 9.0)),
            (Elem::Led(LedId::Thrust), rect(222.0, 190.0, 18.0, 9.0)),
            (Elem::Label("THRUST", 47, 79), rect(241.0, 190.0, 25.0, 9.0)),
            (Elem::Hazard, rect(268.0, 191.0, 50.0, 7.0)),
        ];
        items.extend(fixed.into_iter().map(|(elem, rect)| Placed { elem, rect, cap: None }));

        // Shaped like the HIRES canvas on the panel (panel pixels are not square), so the
        // screen's pixels are.
        let screens = [rect(61.5, 116.0, 94.0, 60.0), rect(166.5, 116.0, 94.0, 60.0)];
        Self {
            style: PanelStyle::Modern,
            items,
            adi: rect(273.0, 113.0, 40.0, 40.0),
            mfds: [MfdUnit { side: MfdSide::Left, screen: screens[0], osbs: true }, MfdUnit { side: MfdSide::Right, screen: screens[1], osbs: true }],
            rwr: None,
            hsi: None,
        }
    }

    /// The F-16's panel (millimetres, x right and y down, unfolded): the centre column
    /// (|x| <= 75) with the up-front controls on a pedestal under the HUD (y < -80: master modes,
    /// lock and the keypad) and the attitude indicator and HSI side by side under a hood below
    /// it; the wings either side (75 < |x| <= 305, angled towards the pilot) with the MFDs,
    /// the RWR scope top left with the threat lights left of it, over the aircraft system caps,
    /// the countermeasure panel top right over the fuel and thrust readouts; the advisory light
    /// panel on the right console ahead of the stick (600 <= x < 1100, centred on x 700, y 0);
    /// the gear lights on the left slope from the console up to the panel (x <= -600, centred on
    /// x -700, y 0; y grows down the slope). No EMV gauge: the RWR shows the radars.
    pub fn f16() -> Self {
        use Effect as E;
        use LampId::*;
        let mut items = Vec::new();
        let mut lamp = |id: LampId, label: Option<&'static str>, r: PxRect, cap: Option<Effect>| items.push(Placed { elem: Elem::Lamp(id, label), rect: r, cap });
        // Up-front controls: master modes and lock, the keypad under them.
        lamp(Nav, None, rect(-66.0, -150.0, 30.0, 12.0), Some(E::HudMode(0)));
        lamp(Air, Some("A-A"), rect(-32.0, -150.0, 30.0, 12.0), Some(E::HudMode(1)));
        lamp(Gnd, Some("A-G"), rect(2.0, -150.0, 30.0, 12.0), Some(E::HudMode(2)));
        lamp(Lock, None, rect(36.0, -150.0, 30.0, 12.0), Some(E::Action(Action::DesignateTarget)));
        // Threat warnings: square lights left of the RWR (tracking, radar and IR missile launch).
        for (k, id) in [Trak, Rad, Ir].into_iter().enumerate() {
            lamp(id, None, rect(-298.0, -110.0 + 25.0 * k as f32, 24.0, 22.0), None);
        }
        // The other warnings on the advisory light panel on the right console (layout x >= 600,
        // see `f16`'s doc).
        for (k, (id, label)) in [
            (Mw, None),
            (Stall, None),
            (LowFuel, Some("LOW\nFUEL")),
            (Fire, None),
            (Eng, None),
            (Fc, None),
            (Fuel, None),
            (Jam, None),
            (Av, None),
            (EmvWarn, Some("EMV")),
        ]
        .into_iter()
        .enumerate()
        {
            lamp(id, label, rect(640.0 + 42.0 * (k % 3) as f32, -38.0 + 19.0 * (k / 3) as f32, 36.0, 14.0), None);
        }
        // Countermeasures, top right: release caps, counts, released lights; jammers.
        for (k, (label, action, led, act)) in [
            ("CHAFF", Action::Chaff, LedId::Chaff, ChaffAct),
            ("FLARE", Action::Flare, LedId::Flares, FlareAct),
            ("DECOY", Action::Decoy, LedId::Decoys, DecoyAct),
        ]
        .into_iter()
        .enumerate()
        {
            let y = -146.0 + 17.0 * k as f32;
            items.push(Placed { elem: Elem::Label(label, 47, 38), rect: rect(120.0, y, 30.0, 13.0), cap: Some(E::Action(action)) });
            items.push(Placed { elem: Elem::Led(led), rect: rect(153.0, y, 20.0, 13.0), cap: None });
            items.push(Placed { elem: Elem::Lamp(act, None), rect: rect(176.0, y, 16.0, 13.0), cap: None });
        }
        items.push(Placed { elem: Elem::Lamp(Ecm, None), rect: rect(120.0, -93.0, 34.0, 13.0), cap: Some(E::Action(Action::Ecm)) });
        items.push(Placed { elem: Elem::Lamp(Irj, None), rect: rect(158.0, -93.0, 34.0, 13.0), cap: Some(E::Action(Action::IrJammer)) });
        // Aircraft systems down the left edge.
        // (No bay: the stores hang outside; the gear has its handle and lights on the left slope.)
        for (k, (id, action)) in [(Flaps, Action::Flaps), (Brake, Action::Brakes), (Auto, Action::Autopilot)].into_iter().enumerate() {
            items.push(Placed { elem: Elem::Lamp(id, None), rect: rect(-296.0, -24.0 + 18.0 * k as f32, 40.0, 14.0), cap: Some(E::Action(action)) });
        }
        // Keypad (not working, as in the original's cockpit art) and the readouts.
        const KEYS: [&str; 12] = ["1", "2", "3", "RCL", "4", "5", "6", "ENTR", "7", "8", "9", "0"];
        let mut fixed: Vec<(Elem, PxRect)> =
            KEYS.iter().enumerate().map(|(k, key)| (Elem::Label(key, 7, 38), rect(-56.0 + 30.0 * (k % 4) as f32, -131.0 + 16.0 * (k / 4) as f32, 22.0, 11.0))).collect();
        fixed.extend([
            (Elem::Label("FUEL", 47, 79), rect(254.0, -40.0, 40.0, 10.0)),
            (Elem::Led(LedId::Fuel), rect(254.0, -28.0, 40.0, 14.0)),
            (Elem::Label("THRUST", 47, 79), rect(254.0, -8.0, 40.0, 10.0)),
            (Elem::Led(LedId::Thrust), rect(254.0, 4.0, 40.0, 14.0)),
            (Elem::Hazard, rect(254.0, 26.0, 40.0, 8.0)),
            // Left slope, beside the gear handle.
            (Elem::GearLights, rect(-766.0, -54.0, 36.0, 28.0)),
        ]);
        items.extend(fixed.into_iter().map(|(elem, rect)| Placed { elem, rect, cap: None }));

        // The HIRES canvas' shape (231x180), 115 mm wide.
        let (w, h) = (115.0, 115.0 * crate::mfd::HIRES.1 as f32 / crate::mfd::HIRES.0 as f32);
        let screens = [rect(-210.0, -12.0, w, h), rect(95.0, -12.0, w, h)];
        Self {
            style: PanelStyle::F16,
            items,
            adi: rect(-74.0, -66.0, 70.0, 70.0),
            mfds: [MfdUnit { side: MfdSide::Left, screen: screens[0], osbs: true }, MfdUnit { side: MfdSide::Right, screen: screens[1], osbs: true }],
            rwr: Some(rect(-246.0, -150.0, 84.0, 84.0)),
            hsi: Some(rect(4.0, -66.0, 70.0, 70.0)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mfd::{MfdState, RightMfd, osb};

    fn all() -> [PanelLayout; 3] {
        [PanelLayout::classic(), PanelLayout::modern(), PanelLayout::f16()]
    }

    /// Faces and caps do not overlap each other or the screens.
    #[test]
    fn no_overlaps() {
        for l in all() {
            let mut rects: Vec<PxRect> = l.items.iter().map(|p| p.rect).collect();
            rects.push(l.adi);
            rects.extend(l.rwr.iter().chain(&l.hsi));
            for m in &l.mfds {
                rects.push(m.screen);
                rects.extend(l.osbs(m).into_iter().map(|(_, r)| r));
            }
            for (i, a) in rects.iter().enumerate() {
                for b in &rects[i + 1..] {
                    assert!(!a.overlaps(b), "{:?}: {a:?} overlaps {b:?}", l.style);
                }
            }
        }
    }

    /// The modern screens have the HIRES canvas' shape on the panel (square pixels).
    #[test]
    fn modern_screens_square_pixels() {
        let canvas = crate::mfd::HIRES.0 as f32 / crate::mfd::HIRES.1 as f32;
        for m in &PanelLayout::modern().mfds {
            let s = m.screen;
            let (_, size) = super::super::geometry::panel_rect(s.x, s.y, s.w, s.h, 0.0);
            assert!((size.x / size.y / canvas - 1.0).abs() < 0.01, "{:?}: {}", m.side, size.x / size.y);
        }
    }

    #[test]
    fn lamps() {
        let lamps = |l: &PanelLayout| -> Vec<LampId> {
            l.items.iter().filter_map(|p| if let Elem::Lamp(id, _) = p.elem { Some(id) } else { None }).collect()
        };
        assert_eq!(lamps(&PanelLayout::classic()).len(), crate::panel::LAMP_COUNT);
        let modern = lamps(&PanelLayout::modern());
        for id in [LampId::Map, LampId::Tac, LampId::Flir, LampId::Wpn, LampId::CamDir, LampId::Way, LampId::Ord, LampId::Tc] {
            assert!(!modern.contains(&id), "{id:?} is a bezel button in the modern panel");
        }
    }

    /// Every bezel function of every page has a button.
    #[test]
    fn bezel_buttons() {
        for l in [PanelLayout::modern(), PanelLayout::f16()] {
            bezel_buttons_of(&l);
        }
    }

    fn bezel_buttons_of(l: &PanelLayout) {
        for right in [RightMfd::Camera, RightMfd::Way, RightMfd::Wpn, RightMfd::Ord] {
            for left in [LeftMfd::Map, LeftMfd::Tac] {
                let s = MfdState { left, right, ..Default::default() };
                for m in &l.mfds {
                    let slots: Vec<u8> = l.osbs(m).into_iter().map(|(s, _)| s).collect();
                    for slot in 0..OSB_COUNT {
                        if osb(m.side, slot, &s).is_some() {
                            assert!(slots.contains(&slot));
                        }
                    }
                }
            }
        }
    }
}
