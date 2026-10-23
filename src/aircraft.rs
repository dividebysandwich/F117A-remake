//! Flyable aircraft: the exterior model, the cockpit(s), where the pilot's eye sits in the model
//! and the flight model constants of each type.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::cockpit3d::layout::PanelStyle;
use crate::flight::FlightConstants;
use crate::player::AIRCRAFT_MODEL_SCALE;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum AircraftKind {
    #[default]
    F117,
    F16,
    UH60,
}

/// How the aircraft's shadow is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shadow {
    /// A flat outline model of the STFLT file (the classic F-117A shadow is model 17).
    Model(u16),
    /// The exterior model squashed flat onto the ground.
    Flattened,
}

/// The exterior model drawn from the cockpit: without its canopy glass, and without the parts
/// reaching into the cockpit.
pub struct CockpitView {
    /// Palette colours of the canopy glass.
    pub canopy_colours: &'static [u8],
    /// Box around the eye (aircraft frame, metres from the eye point, -Z forward): triangles
    /// with a corner or their centre inside are left out.
    pub cut: (Vec3, Vec3),
    /// Also drawn from inside (back faces): a cabin without an interior model of its own.
    pub two_sided: bool,
    /// The rotors (the model's articulated parts) are drawn too, overhead.
    pub rotors: bool,
}

pub struct AircraftSpec {
    /// STFLT model node of the exterior.
    pub model: &'static str,
    /// Height of the model origin above the aircraft position (the ground contact point), m.
    pub model_lift: f32,
    /// Design eye point in the aircraft frame (-Z forward, +Y up), m.
    pub eye: Vec3,
    /// The exterior model as seen from the cockpit (the F-117A's cockpit model has its own canopy
    /// frame and shows none, the others look out over their nose and wings).
    pub cockpit_view: Option<CockpitView>,
    /// Cockpit models (glb stem under `models/`): one, or the modern and the classic one.
    pub cockpits: &'static [(PanelStyle, &'static str)],
    pub shadow: Shadow,
    /// VR cockpit size at 100% (rig scale: the fraction of life size the cockpit looks in the
    /// headset), what felt right in each.
    pub vr_scale: f32,
    /// Height of the real aircraft's eye above the runway (m), kept in VR: the rig scale makes
    /// the world look as much smaller as the cockpit, so the cockpit is raised until the ground
    /// looks this far below (None: never raised).
    pub vr_eye_height: Option<f32>,
    /// Modern HUD scale: canvas pixels per unit of the tangent plane (`hud::modern`), set by
    /// the combiner's size.
    pub hud_scale: f32,
    /// The modern HUD scale in VR, where the symbology filling the combiner on a screen looks
    /// far too large.
    pub vr_hud_scale: f32,
    /// Weapons in a bay (else on pylons: always "open", seen by radars as the F-117A's open bay).
    pub bomb_bay: bool,
    /// Flown by the helicopter flight model (`flight::heli`): lands anywhere flat, no gun and
    /// no ejection seat.
    pub helicopter: bool,
    flight: fn() -> FlightConstants,
}

impl AircraftSpec {
    pub fn flight(&self) -> FlightConstants {
        (self.flight)()
    }

    /// The cockpit flown with the cockpit style setting (aircraft with one cockpit ignore it).
    pub fn cockpit(&self, classic: bool) -> (PanelStyle, &'static str) {
        self.cockpits
            .iter()
            .copied()
            .find(|(s, _)| (*s == PanelStyle::Classic) == classic)
            .unwrap_or(self.cockpits[0])
    }
}

/// A point of the STFLT model (model units, -Z forward) in the aircraft frame, m.
const fn model_point(lift: f32, x: f32, y: f32, z: f32) -> Vec3 {
    let s = AIRCRAFT_MODEL_SCALE as f32;
    Vec3::new(x * s, lift + y * s, z * s)
}

/// The F-117A's model origin is at the belly (gear down: 10 model units below it); raised by
/// the gear and clear of the coplanar ground overlays (lifted up to ~0.7 m, world.rs) so the
/// flat wings are not hidden on the runway.
const F117_LIFT: f32 = (10.0 * AIRCRAFT_MODEL_SCALE) as f32 + 0.7;
/// The F-16's model has no landing gear: its lowest point (the intake lip, 21.6 units below
/// the origin) is held 0.8 m above the ground.
const F16_LIFT: f32 = (21.6 * AIRCRAFT_MODEL_SCALE) as f32 + 0.8;

static F117: AircraftSpec = AircraftSpec {
    model: "stflt_024",
    model_lift: F117_LIFT,
    eye: Vec3::new(0.0, 1.6, -3.0),
    cockpit_view: None,
    cockpits: &[(PanelStyle::Modern, "cockpit_modern"), (PanelStyle::Classic, "cockpit_classic")],
    shadow: Shadow::Model(17),
    vr_scale: 0.85 * 0.7,
    vr_eye_height: Some(2.7),
    hud_scale: crate::hud::modern::SCALE,
    // About the F-16's angular size: the symbology in the middle half of the combiner.
    vr_hud_scale: 2.0 * crate::hud::modern::SCALE,
    bomb_bay: true,
    helicopter: false,
    flight: FlightConstants::default,
};

static F16: AircraftSpec = AircraftSpec {
    model: "stflt_023",
    model_lift: F16_LIFT,
    // Under the canopy bubble (x +-6, y 11..30, z -95..-40 model units), eyes a little forward
    // of its middle and level with its upper part.
    eye: model_point(F16_LIFT, 0.0, 24.0, -72.0),
    cockpit_view: Some(CockpitView {
        canopy_colours: &[144, 145],
        cut: (Vec3::new(-0.40, -1.15, -0.85), Vec3::new(0.40, 0.40, 0.80)),
        two_sided: false,
        rotors: false,
    }),
    cockpits: &[(PanelStyle::F16, "cockpit_f16")],
    shadow: Shadow::Flattened,
    // The symbology (about 920 x 470 canvas pixels) inside the combiner (x +-0.1675, z -0.12 ..
    // 0.095 at 0.70 m in front of the eye, tools/models/build_cockpit_f16.py).
    vr_scale: 1.25 * 0.7,
    vr_eye_height: None,
    hud_scale: 2080.0,
    vr_hud_scale: 2080.0,
    bomb_bay: false,
    helicopter: false,
    flight: FlightConstants::f16,
};

/// A point of the UH-60A model (tools/models/uh60.py: metres, x right, y forward, z up, origin
/// on the ground under the mast, `K` model units per metre) in the aircraft frame.
const fn uh60_point(x: f32, y: f32, z: f32) -> Vec3 {
    const K: f32 = 16.38;
    model_point(UH60_LIFT, x * K, z * K, -y * K)
}

/// The UH-60A's model origin is on the ground under its wheels; clear of the ground overlays.
const UH60_LIFT: f32 = 0.05;

static UH60: AircraftSpec = AircraftSpec {
    model: "stflt_025",
    model_lift: UH60_LIFT,
    // The pilot's (right) seat, behind the windshield top (y 3.05) and ahead of the door post
    // (y 2.40), head under the roof.
    eye: uh60_point(0.52, 2.62, 1.90),
    cockpit_view: Some(CockpitView {
        // The windshield, the door and the cabin windows: open from inside.
        canopy_colours: &[144, 145],
        cut: (Vec3::ZERO, Vec3::ZERO),
        two_sided: true,
        rotors: true,
    }),
    cockpits: &[(PanelStyle::Uh60, "cockpit_uh60")],
    shadow: Shadow::Flattened,
    // Life size, as the F-16's.
    vr_scale: 1.25 * 0.7,
    vr_eye_height: None,
    hud_scale: crate::hud::modern::SCALE,
    vr_hud_scale: 2.0 * crate::hud::modern::SCALE,
    bomb_bay: false,
    helicopter: true,
    flight: FlightConstants::uh60,
};

impl AircraftKind {
    pub const ALL: [AircraftKind; 3] = [AircraftKind::F117, AircraftKind::F16, AircraftKind::UH60];

    pub fn spec(self) -> &'static AircraftSpec {
        match self {
            AircraftKind::F117 => &F117,
            AircraftKind::F16 => &F16,
            AircraftKind::UH60 => &UH60,
        }
    }

    /// Command-line name (`--aircraft`).
    pub fn id(self) -> &'static str {
        match self {
            AircraftKind::F117 => "f117",
            AircraftKind::F16 => "f16",
            AircraftKind::UH60 => "uh60",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        let s = s.to_ascii_lowercase().replace(['-', '_'], "");
        Self::ALL.into_iter().find(|k| {
            k.id() == s
                || (s == "f117a" && *k == AircraftKind::F117)
                || (s == "f16c" && *k == AircraftKind::F16)
                || (matches!(s.as_str(), "uh60a" | "blackhawk") && *k == AircraftKind::UH60)
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            AircraftKind::F117 => "F-117A Nighthawk",
            AircraftKind::F16 => "F-16C Fighting Falcon",
            AircraftKind::UH60 => "UH-60A Black Hawk",
        }
    }
}

/// Every cockpit model file of the flyable aircraft.
pub fn cockpit_files() -> Vec<&'static str> {
    let mut v: Vec<&str> =
        AircraftKind::ALL.iter().flat_map(|k| k.spec().cockpits.iter().map(|(_, f)| *f)).collect();
    v.sort_unstable();
    v.dedup();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Accessor bounds of the primitives of `node` drawn in the given palette colours (glTF JSON
    /// chunk only).
    fn primitive_bounds(node: &str, colours: &[u8]) -> (Vec3, Vec3) {
        let path = crate::asset_path::asset("models/stflt.glb");
        let data = std::fs::read(&path).unwrap();
        let len = u32::from_le_bytes(data[12..16].try_into().unwrap()) as usize;
        let j: serde_json::Value = serde_json::from_slice(&data[20..20 + len]).unwrap();
        let n = j["nodes"].as_array().unwrap().iter().find(|n| n["name"] == node).unwrap();
        let mesh = &j["meshes"][n["mesh"].as_u64().unwrap() as usize];
        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        let mut found = 0;
        for p in mesh["primitives"].as_array().unwrap() {
            let mat =
                j["materials"][p["material"].as_u64().unwrap() as usize]["name"].as_str().unwrap();
            if !colours.iter().any(|c| mat == format!("pal_{c:03}")) {
                continue;
            }
            found += 1;
            let acc = &j["accessors"][p["attributes"]["POSITION"].as_u64().unwrap() as usize];
            let v = |k: &str| {
                let a = acc[k].as_array().unwrap();
                Vec3::new(
                    a[0].as_f64().unwrap() as f32,
                    a[1].as_f64().unwrap() as f32,
                    a[2].as_f64().unwrap() as f32,
                )
            };
            lo = lo.min(v("min"));
            hi = hi.max(v("max"));
        }
        assert_eq!(found, colours.len(), "canopy primitives of {node}");
        (lo, hi)
    }

    #[test]
    fn f16_eye_under_the_canopy() {
        let spec = AircraftKind::F16.spec();
        let (lo, hi) =
            primitive_bounds("stflt_023_lod0", spec.cockpit_view.as_ref().unwrap().canopy_colours);
        let s = AIRCRAFT_MODEL_SCALE as f32;
        let eye = (spec.eye - Vec3::Y * spec.model_lift) / s;
        assert!(
            eye.cmpgt(lo).all() && eye.cmplt(hi).all(),
            "eye {eye} outside the canopy {lo}..{hi}"
        );
    }

    /// The UH-60A's eye: in the cockpit between the windshield top and the door post
    /// (tools/models/uh60.py STATIONS), under the roof, inside the side.
    #[test]
    fn uh60_eye_in_the_cockpit() {
        let spec = AircraftKind::UH60.spec();
        let s = AIRCRAFT_MODEL_SCALE as f32 * 16.38;
        let eye = (spec.eye - Vec3::Y * spec.model_lift) / s;
        let (x, y, z) = (eye.x, -eye.z, eye.y);
        assert!((2.40..3.05).contains(&y), "eye {y} m from the mast");
        assert!((1.40..2.20).contains(&z), "eye {z} m high");
        assert!(x.abs() < 1.0, "eye {x} m to the side");
    }

    #[test]
    fn names_round_trip() {
        for k in AircraftKind::ALL {
            assert_eq!(AircraftKind::parse(k.id()), Some(k));
            assert!(!k.spec().cockpits.is_empty());
        }
        assert_eq!(AircraftKind::parse("F-16"), Some(AircraftKind::F16));
        assert_eq!(AircraftKind::parse("UH-60"), Some(AircraftKind::UH60));
        assert_eq!(AircraftKind::parse("blackhawk"), Some(AircraftKind::UH60));
        assert_eq!(AircraftKind::F117.spec().eye, Vec3::new(0.0, 1.6, -3.0));
    }
}
