//! Instrument panel layouts: the faces drawn into the instrument atlas (lamps, readouts,
//! labels, bezel buttons), which of them are buttons, and the displays.
//!
//! The cockpit model (`model`, the Blender files) places the faces; their rectangles here set
//! each face's size in the atlas. The classic panel's (`classic`, the 256PIT arrangement
//! with the 77x60 MFDs) are in 320x200 screen pixels, where the faces were in its picture.
//!
//! The F-117A's modern panel (`modern`) and the F-16's (`f16`) are not on that plane: their
//! rectangles are millimetres on their own panels (x right, y down from the panel's centre).
//! Nor is the UH-60A's (`uh60`, the modern faces rearranged). Their models are generated from
//! these layouts (`tools/models/build_cockpit_f117.py`, `build_cockpit_f16.py` and
//! `build_cockpit_uh60.py` read `assets-src/blend/cockpit_modern_layout.json`,
//! `cockpit_f16_layout.json` and `cockpit_uh60_layout.json`, which a test keeps in step with the
//! layouts).

use crate::input::Action;
use crate::mfd::{LeftMfd, MfdSide, OSB_COUNT};
use crate::panel::{LampId, LedId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelStyle {
    Classic,
    Modern,
    F16,
    /// The UH-60A's: the modern faces after the UH-60M's panel, two MFDs.
    Uh60,
    /// The AH-1Z's: the UH-60A's narrowed to a tandem cockpit.
    Ah1z,
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
    /// A round needle instrument (F-16).
    Dial(Dial),
}

/// The F-16's needle instruments: flight instruments round the attitude indicator and HSI (the
/// AOA and vertical velocity indicators are vertical tapes), engine instruments and fuel on
/// the right, the clock on the right console.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dial {
    Airspeed,
    Altimeter,
    Aoa,
    Vvi,
    Rpm,
    Ftit,
    Oil,
    Nozzle,
    Fuel,
    /// Local time, 12-hour face.
    Clock,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    pub elem: Elem,
    pub rect: PxRect,
    /// Pushable: the face is a button cap.
    pub cap: Option<Effect>,
    /// Modelled round (the rectangle's inscribed circle shows): dials, round buttons.
    pub round: bool,
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
    /// The MFDs: left and right, the F-117A's third.
    pub mfds: Vec<MfdUnit>,
    /// Radar warning receiver scope and horizontal situation indicator (F-16).
    pub rwr: Option<PxRect>,
    pub hsi: Option<PxRect>,
    /// Data entry display (F-16, F-117A): the up-front controls' readout.
    pub ded: Option<PxRect>,
}

/// Bezel button cap size (millimetres).
const OSB_W: f32 = 12.0;
const OSB_H: f32 = 10.0;

impl PanelStyle {
    /// The cockpit has a third MFD (`MfdSide::Third`).
    pub fn third_mfd(self) -> bool {
        self == PanelStyle::Modern
    }
}

impl PanelLayout {
    pub fn for_style(style: PanelStyle) -> Self {
        match style {
            PanelStyle::Classic => Self::classic(),
            PanelStyle::Modern => Self::modern(),
            PanelStyle::F16 => Self::f16(),
            PanelStyle::Uh60 => Self::uh60(),
            PanelStyle::Ah1z => Self::ah1z(),
        }
    }

    /// Bezel buttons of an MFD: (slot, cap rectangle); 0..5 above the screen, 5..10 below.
    pub fn osbs(&self, unit: &MfdUnit) -> Vec<(u8, PxRect)> {
        if !unit.osbs {
            return Vec::new();
        }
        let s = unit.screen;
        let (w, h) = (OSB_W, OSB_H);
        (0..OSB_COUNT)
            .map(|slot| {
                let col = (slot % 5) as f32;
                let cx = s.x + s.w * (col + 0.5) / 5.0;
                let y = if slot < 5 { s.y - h - 1.5 } else { s.y + s.h + 1.5 };
                (slot, rect(cx - w / 2.0, y, w, h))
            })
            .collect()
    }

    /// The classic arrangement (256PIT).
    pub fn classic() -> Self {
        use Effect as E;
        use LampId::*;
        let mut items = Vec::new();
        // Lamps at the classic light rectangles ; rect = (x,y)-(x+w,y+h).
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
            items.push(Placed {
                elem: Elem::Lamp(id, None),
                rect: rect(x, y, w + 1.0, h + 1.0),
                cap,
                round: false,
            });
        }
        // Countermeasure release buttons with their labels, LED readouts and the other texts
        // that were part of the picture.
        for (k, (label, action, led)) in [
            ("CHAFF", Action::Chaff, LedId::Chaff),
            ("FLARE", Action::Flare, LedId::Flares),
            ("DECOY", Action::Decoy, LedId::Decoys),
        ]
        .into_iter()
        .enumerate()
        {
            let y = 147.0 + 10.0 * k as f32;
            items.push(Placed {
                elem: Elem::Label(label, 47, 38),
                rect: rect(4.0, y, 25.0, 9.0),
                cap: Some(E::Action(action)),
                round: false,
            });
            items.push(Placed {
                elem: Elem::Led(led),
                rect: rect(33.0, y, 14.0, 10.0),
                cap: None,
                round: false,
            });
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
        items.extend(fixed.into_iter().map(|(elem, rect)| Placed {
            elem,
            rect,
            cap: None,
            round: false,
        }));

        let screens = [rect(74.0, 110.0, 77.0, 60.0), rect(169.0, 110.0, 77.0, 60.0)];
        // The top two squares outside each screen are buttons: map zoom on the left MFD,
        // waypoint selection on the right one.
        for (x, y, label, action) in [
            (65.0, 110.0, "+", Action::MapZoomIn),
            (65.0, 123.0, "-", Action::MapZoomOut),
            (249.0, 112.0, "+", Action::NextWaypoint),
            (249.0, 123.0, "-", Action::LastWaypoint),
        ] {
            items.push(Placed {
                elem: Elem::Label(label, 47, 22),
                rect: rect(x, y, 6.0, 6.0),
                cap: Some(E::Action(action)),
                round: false,
            });
        }
        Self {
            style: PanelStyle::Classic,
            items,
            adi: rect(265.0, 139.0, 32.0, 32.0),
            mfds: vec![
                MfdUnit { side: MfdSide::Left, screen: screens[0], osbs: false },
                MfdUnit { side: MfdSide::Right, screen: screens[1], osbs: false },
            ],
            rwr: None,
            hsi: None,
            ded: None,
        }
    }

    /// The F-117A's panel (millimetres as the F-16's, x right and y down, unfolded; the model
    /// is built at the F-117A cockpit's larger scale), after the real one's:
    /// - The centre column (|x| <= 100): the up-front display under the HUD, a data entry
    ///   display between two rows of seven caps (top: the HUD modes, LOCK, DCLT, NVG, ILS;
    ///   bottom: MENU, VOL, TIME, STPT, ORD and two blank ones), then the large centre display
    ///   (the right MFD: where the real one shows the infrared picture).
    /// - The wings (100 < |x| <= 358), their upper edges falling steeply outwards as the
    ///   canopy's (`tools/models/build_cockpit_f117.py`): the left MFD and the third one (the
    ///   HSI at first), the warning lights over their inner corners; the F-16's round
    ///   instruments down the outer edges (airspeed, altimeter, the AOA and vertical velocity
    ///   tapes and the clock left; RPM, FTIT, oil pressure and nozzle position right), the
    ///   attitude indicator and the BAY cap under the left MFD, fuel quantity, the engine
    ///   readouts and the EMV gauge under the right one.
    /// - As in the F-16 the sloped panels from the consoles up to the main panel: the gear
    ///   lights with FLAPS, BRAKE and AUTO left (x <= -600, centred on -700; the gear handle and
    ///   the target buttons are modelled), the caution lights and the countermeasures right
    ///   (x >= 1100, centred on 1200).
    pub fn modern() -> Self {
        use Effect as E;
        use LampId::*;
        let mut items = Vec::new();
        let mut put = |elem: Elem, rect: PxRect, cap: Option<Effect>, round: bool| {
            items.push(Placed { elem, rect, cap, round })
        };
        // Up-front display caps (black on grey as the F-16's keys).
        const KEY: (u8, u8) = (5, 14);
        let ufc = |k: usize, y: f32| rect(-69.0 + 20.0 * k as f32, y, 18.0, 12.0);
        for (k, (elem, cap)) in [
            (Elem::Lamp(Nav, None), Some(E::HudMode(0))),
            (Elem::Lamp(Air, None), Some(E::HudMode(1))),
            (Elem::Lamp(Gnd, None), Some(E::HudMode(2))),
            (Elem::Lamp(Lock, None), Some(E::Action(Action::DesignateTarget))),
            (Elem::Label("DCLT", KEY.0, KEY.1), Some(E::Action(Action::HudDeclutter))),
            (Elem::Label("NVG", KEY.0, KEY.1), Some(E::Action(Action::Nvg))),
            (Elem::Label("ILS", KEY.0, KEY.1), Some(E::Action(Action::Ils))),
        ]
        .into_iter()
        .enumerate()
        {
            put(elem, ufc(k, -135.0), cap, false);
        }
        for (k, (label, cap)) in [
            ("MENU", Some(E::Action(Action::Menu))),
            ("VOL", Some(E::Action(Action::Volume))),
            ("TIME", Some(E::TimeToggle)),
            ("STPT", Some(E::Action(Action::SelectWaypoint))),
            ("ORD", Some(E::Action(Action::MissionOrders))),
            ("COM", None),
            ("IFF", None),
        ]
        .into_iter()
        .enumerate()
        {
            put(Elem::Label(label, KEY.0, KEY.1), ufc(k, -69.0), cap, false);
        }
        // Warning lights over the side MFDs' inner corners: master caution, stall and the
        // threat lights left; fire, engine, flight controls and jammer / avionics damage right.
        put(Elem::Lamp(Mw, Some("MASTER\nCAUTION")), rect(-168.0, -96.0, 40.0, 14.0), None, false);
        put(Elem::Lamp(Stall, None), rect(-126.0, -96.0, 24.0, 14.0), None, false);
        put(Elem::Lamp(Trak, Some("TRACK")), rect(-168.0, -80.0, 20.0, 11.0), None, false);
        put(Elem::Lamp(Ir, None), rect(-146.0, -80.0, 20.0, 11.0), None, false);
        put(Elem::Lamp(Rad, Some("LAUNCH")), rect(-124.0, -80.0, 22.0, 11.0), None, false);
        put(Elem::Lamp(Fire, Some("ENG\nFIRE")), rect(102.0, -96.0, 24.0, 14.0), None, false);
        put(Elem::Lamp(Eng, Some("ENGINE")), rect(128.0, -96.0, 40.0, 14.0), None, false);
        put(Elem::Lamp(Fc, Some("FLCS")), rect(102.0, -80.0, 22.0, 11.0), None, false);
        put(Elem::Lamp(Jam, None), rect(126.0, -80.0, 20.0, 11.0), None, false);
        put(Elem::Lamp(Av, None), rect(148.0, -80.0, 20.0, 11.0), None, false);
        // Flight instruments down the left edge, engine instruments down the right one.
        for (k, dial) in [Dial::Airspeed, Dial::Altimeter].into_iter().enumerate() {
            put(Elem::Dial(dial), rect(-321.0, -36.0 + 46.0 * k as f32, 42.0, 42.0), None, true);
        }
        put(Elem::Dial(Dial::Aoa), rect(-320.0, 56.0, 18.0, 52.0), None, false);
        put(Elem::Dial(Dial::Vvi), rect(-298.0, 56.0, 18.0, 52.0), None, false);
        put(Elem::Dial(Dial::Clock), rect(-321.0, 114.0, 42.0, 42.0), None, true);
        for (k, dial) in [Dial::Rpm, Dial::Ftit, Dial::Oil, Dial::Nozzle].into_iter().enumerate() {
            put(Elem::Dial(dial), rect(279.0, -36.0 + 46.0 * k as f32, 42.0, 42.0), None, true);
        }
        // Under the left MFD right of the attitude indicator: the bay doors (the master arm
        // panel, its switches modelled).
        put(
            Elem::Lamp(Bay, None),
            rect(-160.0, 92.0, 38.0, 14.0),
            Some(E::Action(Action::BayDoors)),
            false,
        );
        // Under the right MFD: fuel quantity with the fuel and thrust readouts, the EMV gauge.
        put(Elem::Dial(Dial::Fuel), rect(110.0, 86.0, 42.0, 42.0), None, true);
        put(Elem::Led(LedId::Fuel), rect(158.0, 90.0, 30.0, 13.0), None, false);
        put(Elem::Label("FUEL", 47, 79), rect(190.0, 90.0, 34.0, 13.0), None, false);
        put(Elem::Lamp(LowFuel, Some("LOW\nFUEL")), rect(226.0, 90.0, 30.0, 13.0), None, false);
        put(Elem::Led(LedId::Thrust), rect(158.0, 108.0, 30.0, 13.0), None, false);
        put(Elem::Label("THRUST", 47, 79), rect(190.0, 108.0, 34.0, 13.0), None, false);
        put(Elem::Label("EGR\nEAR", 47, 33), rect(110.0, 138.0, 20.0, 13.0), None, false);
        put(Elem::Gauge, rect(132.0, 138.0, 84.0, 13.0), None, false);
        put(Elem::Label("EMV", 47, 33), rect(218.0, 138.0, 20.0, 13.0), None, false);
        put(Elem::Lamp(EmvWarn, None), rect(240.0, 138.0, 18.0, 13.0), None, false);
        // Left slope: the gear lights and under them FLAPS, BRAKE and AUTO, a column clear of
        // the slope's outer edge (it comes in towards the top) and of the gear handle right of it.
        put(Elem::GearLights, rect(-732.0, -110.0, 40.0, 32.0), None, false);
        for (k, (id, action)) in
            [(Flaps, Action::Flaps), (Brake, Action::Brakes), (Auto, Action::Autopilot)]
                .into_iter()
                .enumerate()
        {
            put(
                Elem::Lamp(id, None),
                rect(-731.0, -68.0 + 18.0 * k as f32, 38.0, 14.0),
                Some(E::Action(action)),
                false,
            );
        }
        // Right slope: the caution lights, then the countermeasures (release caps, counts,
        // released lights; the jammers).
        const CAUTION: (u8, u8) = (16, 43);
        for (k, elem) in [
            Elem::Lamp(Fuel, None),
            Elem::Lamp(Bd, None),
            Elem::Lamp(Blank, None),
            Elem::Label("ELEC\nSYS", CAUTION.0, CAUTION.1),
            Elem::Label("HYD\nPRESS", CAUTION.0, CAUTION.1),
            Elem::Label("OXY\nLOW", CAUTION.0, CAUTION.1),
        ]
        .into_iter()
        .enumerate()
        {
            put(
                elem,
                rect(1132.0 + 35.0 * (k % 3) as f32, -118.0 + 17.0 * (k / 3) as f32, 32.0, 14.0),
                None,
                false,
            );
        }
        for (k, (label, action, led, act)) in [
            ("CHAFF", Action::Chaff, LedId::Chaff, ChaffAct),
            ("FLARE", Action::Flare, LedId::Flares, FlareAct),
            ("DECOY", Action::Decoy, LedId::Decoys, DecoyAct),
        ]
        .into_iter()
        .enumerate()
        {
            let y = -70.0 + 17.0 * k as f32;
            put(
                Elem::Label(label, 47, 38),
                rect(1150.0, y, 30.0, 13.0),
                Some(E::Action(action)),
                false,
            );
            put(Elem::Led(led), rect(1183.0, y, 20.0, 13.0), None, false);
            put(Elem::Lamp(act, None), rect(1206.0, y, 16.0, 13.0), None, false);
        }
        put(
            Elem::Lamp(Ecm, None),
            rect(1150.0, -16.0, 34.0, 13.0),
            Some(E::Action(Action::Ecm)),
            false,
        );
        put(
            Elem::Lamp(Irj, None),
            rect(1188.0, -16.0, 34.0, 13.0),
            Some(E::Action(Action::IrJammer)),
            false,
        );

        // The HIRES canvas' shape: the side MFDs 120 mm wide, the centre display 180.
        let hires = crate::mfd::HIRES.1 as f32 / crate::mfd::HIRES.0 as f32;
        let left = rect(-245.0, -38.0, 120.0, 120.0 * hires);
        let centre = rect(-90.0, -12.0, 180.0, 180.0 * hires);
        let third = rect(125.0, -38.0, 120.0, 120.0 * hires);
        Self {
            style: PanelStyle::Modern,
            items,
            adi: rect(-250.0, 92.0, 68.0, 68.0),
            mfds: vec![
                MfdUnit { side: MfdSide::Left, screen: left, osbs: true },
                MfdUnit { side: MfdSide::Right, screen: centre, osbs: true },
                MfdUnit { side: MfdSide::Third, screen: third, osbs: true },
            ],
            rwr: None,
            hsi: None,
            ded: Some(rect(
                -60.0,
                -121.5,
                DED_W,
                DED_W * DED_CANVAS.1 as f32 / DED_CANVAS.0 as f32,
            )),
        }
    }

    /// The UH-60A's panel: the F-117A's faces rearranged after the UH-60M's (millimetres, x
    /// right and y down, life size; `tools/models/build_cockpit_uh60.py`):
    /// - The main panel, its origin in front of the pilot (in the right seat): two large MFDs
    ///   side by side (no third), the warning lights over them; under the left one the gear
    ///   lights with FLAPS, BRAKE and AUTO and the BAY cap, under the right one fuel quantity,
    ///   the engine readouts and the EMV gauge. Between the seats (x about -520, the cabin's
    ///   centre line) the standby attitude indicator over the airspeed indicator and the
    ///   altimeter, the clock and the AOA and vertical velocity tapes; the engine gauges between
    ///   them and the left MFD.
    /// - The centre console between the seats (x >= 1500, centred on x 2000, y growing aft down
    ///   the console): the up-front display with its two rows of caps as the CDU, the caution
    ///   lights and the countermeasures behind it.
    pub fn uh60() -> Self {
        let m = Self::modern();
        // A face's new place: alone, or moved with its group of the F-117A's panel.
        let place = |p: &Placed| {
            let r = p.rect;
            let at = |x: f32, y: f32| rect(x, y, r.w, r.h);
            let by = |dx: f32, dy: f32| rect(r.x + dx, r.y + dy, r.w, r.h);
            let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
            match p.elem {
                Elem::Dial(Dial::Airspeed) => at(-567.0, -45.0),
                Elem::Dial(Dial::Altimeter) => at(-515.0, -45.0),
                Elem::Dial(Dial::Clock) => at(-567.0, 5.0),
                Elem::Dial(Dial::Aoa) => at(-515.0, 0.0),
                Elem::Dial(Dial::Vvi) => at(-493.0, 0.0),
                Elem::Dial(Dial::Rpm) => at(-415.0, -45.0),
                Elem::Dial(Dial::Ftit) => at(-365.0, -45.0),
                Elem::Dial(Dial::Oil) => at(-415.0, 5.0),
                Elem::Dial(Dial::Nozzle) => at(-365.0, 5.0),
                Elem::Lamp(LampId::Bay, _) => at(-180.0, 110.0),
                // The caution lights and countermeasures to the console, behind the CDU.
                _ if cx >= 1100.0 => by(818.0, 150.0),
                // The gear lights, FLAPS, BRAKE and AUTO under the left MFD.
                _ if cx <= -600.0 => by(492.0, 210.0),
                // The up-front display's caps to the console.
                _ if cx.abs() < 80.0 && cy < -50.0 => by(2000.0, 0.0),
                // The warning lights over the MFDs.
                _ if cy < -60.0 => by(10f32.copysign(cx), -29.0),
                // Fuel, the engine readouts and the EMV gauge under the right MFD.
                _ => by(-40.0, 20.0),
            }
        };
        let items = m.items.iter().map(|p| Placed { rect: place(p), ..*p }).collect();
        let hires = crate::mfd::HIRES.1 as f32 / crate::mfd::HIRES.0 as f32;
        let ded = m.ded.map(|r| rect(r.x + 2000.0, r.y, r.w, r.h));
        Self {
            style: PanelStyle::Uh60,
            items,
            adi: rect(-554.0, -125.0, 68.0, 68.0),
            mfds: vec![
                MfdUnit {
                    side: MfdSide::Left,
                    screen: rect(-240.0, -70.0, 190.0, 190.0 * hires),
                    osbs: true,
                },
                MfdUnit {
                    side: MfdSide::Right,
                    screen: rect(50.0, -70.0, 190.0, 190.0 * hires),
                    osbs: true,
                },
            ],
            rwr: None,
            hsi: None,
            ded,
        }
    }

    /// The AH-1Z's panel: the UH-60A's (millimetres, life size; `tools/models/build_cockpit_ah1z.py`)
    /// narrowed to the pilot's cockpit, after the AH-1Z's: the two MFDs side by side under the
    /// warning lights, the standby airspeed indicator, altimeter and clock with the AOA and
    /// vertical velocity tapes left of them, the engine gauges right of them, the standby
    /// attitude indicator between the groups under them. The CDU, the cautions and the
    /// countermeasures stay on the console (here the pedestal between the knees). No gear
    /// lights (skids) and no AUTO cap.
    pub fn ah1z() -> Self {
        let u = Self::uh60();
        let items = u
            .items
            .iter()
            .filter(|p| !matches!(p.elem, Elem::GearLights | Elem::Lamp(LampId::Auto, _)))
            .map(|p| {
                let r = p.rect;
                let at = |x: f32, y: f32| rect(x, y, r.w, r.h);
                let rect = match p.elem {
                    Elem::Dial(Dial::Aoa) => at(-372.0, -60.0),
                    Elem::Dial(Dial::Vvi) => at(-372.0, -2.0),
                    Elem::Dial(Dial::Airspeed) => at(-322.0, -60.0),
                    Elem::Dial(Dial::Altimeter) => at(-322.0, -10.0),
                    Elem::Dial(Dial::Clock) => at(-322.0, 40.0),
                    Elem::Dial(Dial::Rpm) => at(280.0, -60.0),
                    Elem::Dial(Dial::Ftit) => at(280.0, -10.0),
                    Elem::Dial(Dial::Oil) => at(330.0, -60.0),
                    Elem::Dial(Dial::Nozzle) => at(330.0, -10.0),
                    _ => r,
                };
                Placed { rect, ..*p }
            })
            .collect();
        Self { style: PanelStyle::Ah1z, items, adi: rect(-34.0, 110.0, 68.0, 68.0), ..u }
    }

    /// The F-16C's panel (millimetres, x right and y down, unfolded), after the Block 50's:
    /// - The centre column (|x| <= 85): the up-front controls on a pedestal under the HUD
    ///   (y < -46): the round override buttons on top (COM1 = volume, MENU = menu, A-A, A-G),
    ///   the keypad with its page labels (T-ILS, STPT, TIME and MARK work) and the declutter
    ///   and NVG keys; under it the airspeed indicator, attitude indicator and altimeter, then
    ///   the AOA indicator, HSI and vertical velocity indicator, and the clock under the HSI.
    /// - The wings (85 < |x| <= 335): the MFDs in wells; top left the flap, brake and
    ///   autopilot caps, the threat lights with the master caution and stall light, and the
    ///   RWR scope next to the centre; top right the data entry display with the oil and
    ///   nozzle gauges right of it; RPM, FTIT and fuel down the right edge.
    /// - The advisory light panel on the right console ahead of the stick (600 <= x < 1100,
    ///   centred on x 700, y 0); the countermeasure panel on the right slope from the console
    ///   up to the panel (x >= 1100, centred on x 1200; y grows down the slope). The left slope
    ///   (x <= -600, centred on x -700) holds the gear lights left of the gear handle, and the
    ///   target buttons (modelled, not faces) below them.
    ///
    /// No EMV gauge: the RWR shows the radars.
    pub fn f16() -> Self {
        use Effect as E;
        use LampId::*;
        let mut items = Vec::new();
        let mut put = |elem: Elem, rect: PxRect, cap: Option<Effect>, round: bool| {
            items.push(Placed { elem, rect, cap, round })
        };
        // Up-front controls: the override buttons (round, black on light grey), the keypad and
        // the keys right of it (black on grey).
        const OVERRIDE: (u8, u8) = (5, 7);
        const KEY: (u8, u8) = (5, 14);
        for (k, (label, cap)) in [
            ("COM1", Some(E::Action(Action::Volume))),
            ("COM2", None),
            ("IFF", None),
            ("MENU", Some(E::Action(Action::Menu))),
            ("A-A", Some(E::HudMode(1))),
            ("A-G", Some(E::HudMode(2))),
        ]
        .into_iter()
        .enumerate()
        {
            let r = rect(-69.0 + 24.0 * k as f32, -157.0, 18.0, 18.0);
            match label {
                "A-A" => put(Elem::Lamp(Air, Some("A-A")), r, cap, true),
                "A-G" => put(Elem::Lamp(Gnd, Some("A-G")), r, cap, true),
                _ => put(Elem::Label(label, OVERRIDE.0, OVERRIDE.1), r, cap, true),
            }
        }
        const KEYS: [(&str, Option<Effect>); 12] = [
            ("1\nT-ILS", Some(E::Action(Action::Ils))),
            ("2\nALOW", None),
            ("3", None),
            ("4\nSTPT", Some(E::Action(Action::SelectWaypoint))),
            ("5\nCRUS", None),
            ("6\nTIME", Some(E::TimeToggle)),
            ("7\nMARK", Some(E::Action(Action::DesignateTarget))),
            ("8\nFIX", None),
            ("9\nA-CAL", None),
            ("RCL", None),
            ("0\nM-SEL", None),
            ("ENTR", None),
        ];
        for (k, (label, cap)) in KEYS.into_iter().enumerate() {
            put(
                Elem::Label(label, KEY.0, KEY.1),
                rect(-63.0 + 24.0 * (k % 3) as f32, -132.0 + 20.0 * (k / 3) as f32, 22.0, 16.0),
                cap,
                false,
            );
        }
        put(
            Elem::Label("DCLT", KEY.0, KEY.1),
            rect(15.0, -132.0, 20.0, 16.0),
            Some(E::Action(Action::HudDeclutter)),
            false,
        );
        put(
            Elem::Label("NVG", KEY.0, KEY.1),
            rect(15.0, -112.0, 20.0, 16.0),
            Some(E::Action(Action::Nvg)),
            false,
        );
        put(Elem::Label("WX", KEY.0, KEY.1), rect(15.0, -92.0, 20.0, 16.0), None, false);
        put(Elem::Label("FLIR", KEY.0, KEY.1), rect(15.0, -72.0, 20.0, 16.0), None, false);
        // Flight instruments as in the F-16C: airspeed and altimeter over the attitude
        // indicator's upper corners, the AOA and vertical velocity tapes either side of it, the
        // HSI under it.
        put(Elem::Dial(Dial::Airspeed), rect(-84.0, -44.0, 42.0, 42.0), None, true);
        put(Elem::Dial(Dial::Altimeter), rect(42.0, -44.0, 42.0, 42.0), None, true);
        put(Elem::Dial(Dial::Aoa), rect(-58.0, 2.0, 18.0, 52.0), None, false);
        put(Elem::Dial(Dial::Vvi), rect(40.0, 2.0, 18.0, 52.0), None, false);
        // Unlit legends of the warning lights the game doesn't drive (red and amber).
        const WARN: (u8, u8) = (54, 55);
        const CAUTION: (u8, u8) = (16, 43);
        // Left eyebrow under the glare shield: master caution, the stall warning and the
        // game's radar track and IR launch warnings.
        put(Elem::Lamp(Mw, Some("MASTER\nCAUTION")), rect(-262.0, -154.0, 50.0, 20.0), None, false);
        put(Elem::Lamp(Stall, None), rect(-262.0, -131.0, 50.0, 11.0), None, false);
        put(Elem::Lamp(Trak, Some("TRACK")), rect(-262.0, -117.0, 24.0, 11.0), None, false);
        put(Elem::Lamp(Ir, Some("IR")), rect(-236.0, -117.0, 24.0, 11.0), None, false);
        // Threat warning prime: a column of six push-lights left of the RWR (LAUNCH lights
        // with a radar missile launch).
        for (k, elem) in [
            Elem::Label("HAND\nOFF", WARN.0, WARN.1),
            Elem::Lamp(Rad, Some("LAUNCH")),
            Elem::Label("PRI\nMODE", WARN.0, WARN.1),
            Elem::Label("UNK", WARN.0, WARN.1),
            Elem::Label("SYS\nTEST", WARN.0, WARN.1),
            Elem::Label("TGT\nSEP", WARN.0, WARN.1),
        ]
        .into_iter()
        .enumerate()
        {
            put(elem, rect(-206.0, -154.0 + 12.8 * k as f32, 22.0, 11.0), None, false);
        }
        // Gear lights left of the gear handle on the left slope (the landing gear panel), the
        // flaps, brakes and autopilot under them.
        put(Elem::GearLights, rect(-746.0, -104.0, 40.0, 32.0), None, false);
        for (k, (id, action)) in
            [(Flaps, Action::Flaps), (Brake, Action::Brakes), (Auto, Action::Autopilot)]
                .into_iter()
                .enumerate()
        {
            put(
                Elem::Lamp(id, None),
                rect(-764.0, -62.0 + 18.0 * k as f32, 38.0, 14.0),
                Some(E::Action(action)),
                false,
            );
        }
        // Clock at the top of the right slope (the right auxiliary console).
        put(Elem::Dial(Dial::Clock), rect(1150.0, -136.0, 44.0, 44.0), None, true);
        // Right eyebrow under the glare shield: the engine fire, engine and flight control
        // warnings among the F-16's others.
        for (k, elem) in [
            Elem::Lamp(Fire, Some("ENG\nFIRE")),
            Elem::Lamp(Eng, Some("ENGINE")),
            Elem::Label("HYD/OIL\nPRESS", WARN.0, WARN.1),
            Elem::Lamp(Fc, Some("FLCS")),
            Elem::Label("DBU\nON", WARN.0, WARN.1),
            Elem::Label("TO/LDG\nCONFIG", WARN.0, WARN.1),
            Elem::Label("CANOPY", WARN.0, WARN.1),
            Elem::Label("OXY\nLOW", WARN.0, WARN.1),
        ]
        .into_iter()
        .enumerate()
        {
            put(
                elem,
                rect(220.0 + 25.0 * (k % 2) as f32, -154.0 + 13.0 * (k / 2) as f32, 23.0, 11.5),
                None,
                false,
            );
        }
        // Engine instruments down the right edge (oil pressure, nozzle position, RPM, FTIT),
        // fuel quantity under them.
        for (k, dial) in
            [Dial::Oil, Dial::Nozzle, Dial::Rpm, Dial::Ftit, Dial::Fuel].into_iter().enumerate()
        {
            put(Elem::Dial(dial), rect(273.0, -127.0 + 45.0 * k as f32, 42.0, 42.0), None, true);
        }
        // The caution light panel on the right console (layout x >= 600, see the doc).
        for (k, elem) in [
            Elem::Label("ELEC\nSYS", CAUTION.0, CAUTION.1),
            Elem::Label("SEAT NOT\nARMED", CAUTION.0, CAUTION.1),
            Elem::Label("ANTI\nSKID", CAUTION.0, CAUTION.1),
            Elem::Lamp(Fuel, None),
            Elem::Lamp(LowFuel, Some("LOW\nFUEL")),
            Elem::Lamp(Jam, None),
            Elem::Lamp(Av, None),
            Elem::Lamp(EmvWarn, Some("EMV")),
            Elem::Lamp(Blank, None),
        ]
        .into_iter()
        .enumerate()
        {
            put(
                elem,
                rect(640.0 + 42.0 * (k % 3) as f32, -38.0 + 19.0 * (k / 3) as f32, 36.0, 14.0),
                None,
                false,
            );
        }
        // Countermeasures on the right slope: release caps, counts, released lights; jammers.
        for (k, (label, action, led, act)) in [
            ("CHAFF", Action::Chaff, LedId::Chaff, ChaffAct),
            ("FLARE", Action::Flare, LedId::Flares, FlareAct),
            ("DECOY", Action::Decoy, LedId::Decoys, DecoyAct),
        ]
        .into_iter()
        .enumerate()
        {
            let y = -70.0 + 17.0 * k as f32;
            put(
                Elem::Label(label, 47, 38),
                rect(1150.0, y, 30.0, 13.0),
                Some(E::Action(action)),
                false,
            );
            put(Elem::Led(led), rect(1183.0, y, 20.0, 13.0), None, false);
            put(Elem::Lamp(act, None), rect(1206.0, y, 16.0, 13.0), None, false);
        }
        put(
            Elem::Lamp(Ecm, None),
            rect(1150.0, -16.0, 34.0, 13.0),
            Some(E::Action(Action::Ecm)),
            false,
        );
        put(
            Elem::Lamp(Irj, None),
            rect(1188.0, -16.0, 34.0, 13.0),
            Some(E::Action(Action::IrJammer)),
            false,
        );

        // The HIRES canvas' shape (231x180), 130 mm wide.
        let (w, h) = (130.0, 130.0 * crate::mfd::HIRES.1 as f32 / crate::mfd::HIRES.0 as f32);
        let screens = [rect(-240.0, -10.0, w, h), rect(110.0, -10.0, w, h)];
        Self {
            style: PanelStyle::F16,
            items,
            adi: rect(-34.0, -30.0, 68.0, 68.0),
            mfds: vec![
                MfdUnit { side: MfdSide::Left, screen: screens[0], osbs: true },
                MfdUnit { side: MfdSide::Right, screen: screens[1], osbs: true },
            ],
            rwr: Some(rect(-176.0, -154.0, 76.0, 76.0)),
            hsi: Some(rect(-34.0, 44.0, 68.0, 68.0)),
            ded: Some(rect(95.0, -152.0, DED_W, DED_W * DED_CANVAS.1 as f32 / DED_CANVAS.0 as f32)),
        }
    }
}

/// Width of the data entry display on the F-16 and F-117A panels (mm); its height keeps its canvas' shape.
const DED_W: f32 = 120.0;
/// The data entry display's canvas: 24 columns of 5 lines of the flight font at
/// `DED_SCALE`, with margins.
pub const DED_COLS: i32 = 24;
pub const DED_LINES: i32 = 5;
pub const DED_SCALE: i32 = 3;
pub const DED_CANVAS: (usize, usize) =
    ((DED_COLS * 4 * DED_SCALE + 8) as usize, (DED_LINES * 8 * DED_SCALE + 3) as usize);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mfd::{MfdState, RightMfd, osb};

    fn all() -> [PanelLayout; 5] {
        [
            PanelLayout::classic(),
            PanelLayout::modern(),
            PanelLayout::f16(),
            PanelLayout::uh60(),
            PanelLayout::ah1z(),
        ]
    }

    /// Faces and caps do not overlap each other or the screens.
    #[test]
    fn no_overlaps() {
        for l in all() {
            let mut rects: Vec<PxRect> = l.items.iter().map(|p| p.rect).collect();
            rects.push(l.adi);
            rects.extend(l.rwr.iter().chain(&l.hsi).chain(&l.ded));
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

    #[test]
    fn lamps() {
        let lamps = |l: &PanelLayout| -> Vec<LampId> {
            l.items
                .iter()
                .filter_map(|p| if let Elem::Lamp(id, _) = p.elem { Some(id) } else { None })
                .collect()
        };
        assert_eq!(lamps(&PanelLayout::classic()).len(), crate::panel::LAMP_COUNT);
        let modern = lamps(&PanelLayout::modern());
        for id in [
            LampId::Map,
            LampId::Tac,
            LampId::Flir,
            LampId::Wpn,
            LampId::CamDir,
            LampId::Way,
            LampId::Ord,
            LampId::Tc,
        ] {
            assert!(!modern.contains(&id), "{id:?} is a bezel button in the modern panel");
        }
    }

    /// Every bezel function of every page has a button.
    #[test]
    fn bezel_buttons() {
        for l in
            [PanelLayout::modern(), PanelLayout::f16(), PanelLayout::uh60(), PanelLayout::ah1z()]
        {
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
