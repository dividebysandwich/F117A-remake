//! One player's part of the simulation. The simulation holds the active pilot in `Sim::pl`
//! and the others parked; [`Sim::activate`] swaps a parked pilot in. Code that runs for "the
//! player" works on whichever pilot is active, so with one pilot the simulation is the
//! classic one.

use serde::{Deserialize, Serialize};

use crate::types::*;
use crate::{ENEMY_SLOTS, N_PROJ, PILOT_SLOTS, PlayerView, Sim, tiles};

fn none_unloading(v: &[i32; 2]) -> bool {
    *v == [0, 0]
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Pilot {
    /// Player id (index into the pilots).
    pub(crate) id: u8,
    /// Not in the world (campaign: between sorties, or not joined).
    #[serde(default)]
    pub(crate) absent: bool,
    /// The aircraft carries its weapons outside (`SimOptions::external_stores` for pilot 0).
    #[serde(default)]
    pub(crate) external_stores: bool,
    /// The aircraft is a helicopter (`SimOptions::helicopter` for pilot 0).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) helicopter: bool,
    /// The helicopter is a gunship (`SimOptions::gunship` for pilot 0): a turret cannon.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) gunship: bool,
    /// Frames landed near each insertion target (mission type 9).
    #[serde(default, skip_serializing_if = "none_unloading")]
    pub(crate) unload: [i32; 2],
    /// World object 0 while this pilot is parked: the tile object his tile lock points at.
    #[serde(default)]
    pub(crate) scratch0: GroundObject,
    /// The ground point designated with the targeting pod away from any object (map units):
    /// the ground lock is object 0 on it, plain ground, while the lock holds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) spi: Option<(i32, i32)>,
    /// His gun rounds and his weapons in flight while parked (the active pilot's are in the
    /// world's first gun-round slots and projectile slots 8..11).
    #[serde(default)]
    pub(crate) rounds: Vec<Bullet>,
    #[serde(default)]
    pub(crate) missiles: [Projectile; PILOT_SLOTS],
    /// His last report from the flight model (used again when none comes this frame).
    #[serde(default)]
    pub(crate) last_input: PlayerState,
    pub(crate) lockheed: bool,
    pub(crate) p: PlayerView,
    /// Player flags (`pflags`).
    pub(crate) pflags: i32,
    pub(crate) emv: i32,
    pub(crate) damage: u8,
    pub(crate) hits: i32,
    /// Realistic damage: damaged control surfaces; frames until a fire is decided (burns out
    /// or blows the aircraft up) and until the destroyed airframe breaks up (0 = none).
    #[serde(default)]
    pub(crate) control_fault: ControlFault,
    #[serde(default)]
    pub(crate) fire_frames: i32,
    #[serde(default)]
    pub(crate) breakup_frames: i32,
    pub(crate) irj_heat: i32,
    pub(crate) door_anim: i32,
    pub(crate) bay_req: bool,
    /// Bays or pylon stations (weapon type, count; type -1 empty).
    pub(crate) bays: Vec<(i32, i32)>,
    /// Selected station: the first one carrying the selected weapon.
    pub(crate) cur_bay: usize,
    /// The cannon is the selected weapon (Space steps to it after the
    /// stores, and the fire button fires it).
    #[serde(default)]
    pub(crate) gun_selected: bool,
    /// A gunship's turret is slaved to the targeting pod, else to the helmet sight: the cannon
    /// is selected slaved to the helmet, Space then slaves it to the pod.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) gun_pod: bool,
    /// Countermeasure stores ([1] flare, [2] chaff, [3] decoy).
    pub(crate) stores: [i32; 4],
    pub(crate) gun_ammo: i32,
    pub(crate) gun_firing: bool,
    /// Where the turret points while it fires (BAM heading, pitch), else along the nose.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) gun_aim: Option<(i32, i32)>,
    pub(crate) fire_cooldown: i32,
    pub(crate) prev_fire: bool,
    /// CCRP: ground distance (ft) the bomb's impact point still had to go to the target last
    /// frame (None: no CCRP solution).
    #[serde(default)]
    pub(crate) ccrp_last: Option<f64>,
    pub(crate) prev_hud_mode: HudMode,
    pub(crate) hud_mode: HudMode,
    pub(crate) camera_mfd: bool,
    pub(crate) external_view: bool,
    pub(crate) prev_on_ground: bool,
    pub(crate) on_ground: bool,
    pub(crate) cms: [Countermeasure; 4],
    /// Result of the last tile search.
    pub(crate) last_tile: Option<tiles::TileHit>,
    /// Frames left on the radar sweep animation, -1 idle.
    pub(crate) sweep_anim: i32,
    /// Aircraft engaging the player: last frame / this frame.
    pub(crate) threat_count_last: i32,
    pub(crate) threat_count: i32,
    /// TRAK count, radar IDs and visual IDs held against the player.
    pub(crate) tracking: i32,
    pub(crate) radar_ids: i32,
    pub(crate) visual_ids: i32,
    /// Nearest base object and its distance; base used for traffic.
    pub(crate) nearest_base: i32,
    pub(crate) nearest_base_dist: i32,
    /// The player's last position and smoothed velocity as the air defences track him.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) aaa_track: Option<([f64; 3], [f64; 3])>,
    /// Lamp timers: chaff / flare / EMV.
    pub(crate) chaff_lamp: i32,
    pub(crate) flare_lamp: i32,
    pub(crate) emv_lamp: i32,
    pub(crate) egr: Option<SignalBar>,
    pub(crate) ear: Option<SignalBar>,
    pub(crate) arc: Option<RadarArc>,
    pub(crate) lights: Lights,
    /// Missile launches against the player.
    pub(crate) launches: i32,
    /// Air / ground target locks (bit 0x80 = reselect, -1 none).
    pub(crate) air_lock: i32,
    pub(crate) gnd_lock: i32,
    /// Air lock on another pilot (`Commands::lock_pilot`), while `air_lock` is none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) pilot_lock: Option<u8>,
    pub(crate) targeting: TargetingState,
    /// Where the Sidewinder seeker following the helmet sight looks (`Sim::helmet_seeker`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) helmet_seeker: Option<[f64; 3]>,
    pub(crate) gnd_cooldown: i32,
    /// Frames left for a nearest-target request to wait for the HUD mode (`Commands::nearest_target`).
    pub(crate) nearest_req: u8,
    /// Locks cleared on request (`Commands::clear_target`): no automatic acquisition.
    pub(crate) target_hold: bool,
    /// Camera view direction relative to the nose.
    pub(crate) cam_dir: i32,
    pub(crate) last_missile_slot: i32,
    pub(crate) maverick_view: bool,
    pub(crate) targets: [TargetSlot; 2],
    pub(crate) waypoint: i32,
    pub(crate) deadline: i32,
    pub(crate) cue_timers: [i32; 2],
    pub(crate) photo_timer: i32,
    pub(crate) photo_frames: i32,
    pub(crate) deadline_done: [bool; 2],
    pub(crate) time_expired_shown: bool,
    pub(crate) target_escaped: bool,
    pub(crate) corridor: bool,
    /// Landing state: 0 until airborne, 1 once airborne, then counts frames on a friendly runway.
    pub(crate) armed_landing: i32,
    /// Frames since ejection (counting from 1), 0 = not ejected.
    pub(crate) eject: i32,
    pub(crate) chute: (i32, i32, i32),
    pub(crate) landing_type: u8,
    pub(crate) log: Vec<LogEvent>,
    /// A coop client flies his own gun rounds and reports their hits (`claims`): the host's
    /// rounds of his only fly (they don't hit). The step he last fired in, and his reports in
    /// the current step.
    #[serde(skip)]
    pub(crate) client_rounds: bool,
    #[serde(skip)]
    pub(crate) fired_step: u32,
    #[serde(skip)]
    pub(crate) claims: (u32, u8),
}

/// An event of a step and the pilot it happened to (whose turn it was): private events
/// ([`SimEvent::is_private`]) are his alone, the others everybody's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PilotEvent {
    pub pilot: u8,
    pub event: SimEvent,
}

impl SimEvent {
    /// What only the pilot it happened to hears or sees (messages, sounds, warnings, his
    /// damage and his sortie's course); the rest is the world's.
    pub fn is_private(&self) -> bool {
        matches!(
            self,
            SimEvent::Message(_)
                | SimEvent::Sound { .. }
                | SimEvent::Rwr(_)
                | SimEvent::CancelTimeAccel
                | SimEvent::PlayerHit { .. }
                | SimEvent::DamageFlash
                | SimEvent::ObjectiveDone { .. }
                | SimEvent::PhotoTaken { .. }
                | SimEvent::FuelTransfer { .. }
                | SimEvent::Resupplied
                | SimEvent::Ejected { .. }
                | SimEvent::CollisionBounce
                | SimEvent::MissionEnd(_)
                | SimEvent::Log(_)
                | SimEvent::SortieEnded(_)
                | SimEvent::LandedAtBase { .. }
        )
    }
}

impl Sim {
    /// Number of pilot slots (joined or not).
    pub fn pilot_count(&self) -> usize {
        self.parked.len().max(1)
    }

    /// Id of the active pilot (the one the accessors show).
    pub fn active_pilot(&self) -> u8 {
        self.pl.id
    }

    /// Pilots in the world.
    pub(crate) fn present(&self) -> Vec<usize> {
        (0..self.pilot_count()).filter(|&k| !self.pilot_ref(k).absent).collect()
    }

    /// Pilot `k`, active or parked.
    pub(crate) fn pilot_ref(&self, k: usize) -> &Pilot {
        if k == self.pl.id as usize || k >= self.parked.len() { &self.pl } else { &self.parked[k] }
    }

    /// Projectile slots of the active pilot's weapons.
    pub(crate) fn own_slots() -> std::ops::Range<usize> {
        ENEMY_SLOTS..N_PROJ
    }

    /// Makes pilot `k` the active one: his state into `pl`, his tile object into object 0,
    /// his rounds and weapons into their slots.
    pub(crate) fn activate(&mut self, k: usize) {
        let cur = self.pl.id as usize;
        if k == cur || k >= self.parked.len() {
            return;
        }
        let nb = self.n_bullets.min(self.bullets.len());
        if let Some(o) = self.objects.first() {
            self.pl.scratch0 = *o;
        }
        self.pl.rounds.clear();
        self.pl.rounds.extend_from_slice(&self.bullets[..nb]);
        self.pl.missiles.copy_from_slice(&self.proj[ENEMY_SLOTS..]);
        let incoming = std::mem::take(&mut self.parked[k]);
        self.parked[cur] = std::mem::replace(&mut self.pl, incoming);
        if let Some(o) = self.objects.first_mut() {
            *o = self.pl.scratch0;
        }
        self.pl.rounds.resize(nb, Bullet::default());
        self.bullets[..nb].copy_from_slice(&self.pl.rounds);
        self.proj[ENEMY_SLOTS..].copy_from_slice(&self.pl.missiles);
    }

    /// Adds pilot `id` (campaign): absent until his first sortie, flying the mission's aircraft.
    /// A pilot already there (a returning player) stays as he is.
    pub fn add_pilot(&mut self, id: u8) {
        let id = id as usize;
        if self.parked.len() <= id {
            self.parked.resize_with(id + 1, Pilot::default);
        }
        if id == self.pl.id as usize || self.parked[id].id as usize == id {
            return;
        }
        let mut p = self.template_pilot();
        p.id = id as u8;
        p.absent = true;
        self.parked[id] = p;
    }

    /// A fresh pilot as the mission starts one (stores, aircraft), from pilot 0's settings.
    fn template_pilot(&self) -> Pilot {
        let p0 = self.pilot_ref(0);
        let mut p = Pilot {
            lockheed: p0.lockheed,
            external_stores: p0.external_stores,
            helicopter: p0.helicopter,
            gunship: p0.gunship,
            hits: 2,
            air_lock: -1,
            gnd_lock: -1,
            last_missile_slot: -1,
            sweep_anim: -1,
            nearest_base: -1,
            nearest_base_dist: 0x7FFF,
            corridor: true,
            landing_type: 1,
            prev_on_ground: true,
            on_ground: true,
            scratch0: self.objects.first().copied().unwrap_or_default(),
            targets: [TargetSlot { mission_type: 0, object: -1, base: p0.targets[0].base, code: 0 };
                2],
            ..Default::default()
        };
        if let Some(c) = &self.campaign {
            p.stores = c.stores_init;
            p.gun_ammo = c.gun_init;
        }
        p
    }

    /// The pilot leaves for good (campaign: he left the game); his sortie ends as aborted.
    pub fn remove_pilot(&mut self, id: u8) {
        self.end_sortie_pilot(id, 7);
    }

    /// `f` on the simulation as pilot `id` sees it (his lights, locks, warnings).
    pub fn as_pilot<R>(&mut self, id: u8, f: impl FnOnce(&Self) -> R) -> R {
        let back = self.pl.id as usize;
        self.activate(id as usize);
        let r = f(self);
        self.activate(back);
        r
    }

    /// Pilot `k` is another one, flying in the world (not landed or ejected).
    pub(crate) fn pilot_airborne(&self, k: u8) -> bool {
        let p = self.pilot_ref(k as usize);
        k != self.pl.id && p.id == k && !p.absent && !p.on_ground && p.eject == 0
    }

    /// The other pilots flying in the world and where they are: a host's pilots, the ones a
    /// mirror shows.
    pub(crate) fn other_pilots(&self) -> Vec<(u8, PlayerView)> {
        if self.mirror {
            return self
                .remote
                .iter()
                .filter(|p| p.id != self.pl.id && !p.on_ground && p.chute.is_none())
                .map(|p| (p.id, p.view))
                .collect();
        }
        self.present()
            .into_iter()
            .map(|k| k as u8)
            .filter(|&k| self.pilot_airborne(k))
            .map(|k| (k, self.pilot_ref(k as usize).p))
            .collect()
    }

    /// Pilot `id`'s view of the world (position, attitude) while he is in it.
    pub fn pilot_view(&self, id: u8) -> Option<PlayerView> {
        let p = self.pilot_ref(id as usize);
        (p.id == id && !p.absent).then_some(p.p)
    }

    /// Pilot `id` flies a helicopter, stopped on the ground (landed anywhere).
    pub fn pilot_landed(&self, id: u8) -> bool {
        let p = self.pilot_ref(id as usize);
        p.id == id && !p.absent && p.helicopter && p.on_ground && p.p.knots <= 2
    }

    /// Pilot `id` flies a helicopter.
    pub fn pilot_helicopter(&self, id: u8) -> bool {
        let p = self.pilot_ref(id as usize);
        p.id == id && p.helicopter
    }

    /// Pilot `id`'s last report from his flight model.
    pub fn pilot_input(&self, id: u8) -> PlayerState {
        let p = self.pilot_ref(id as usize);
        if p.id == id { p.last_input.clone() } else { PlayerState::default() }
    }

    /// Sets pilot `id`'s report (where his flight model starts him).
    pub fn set_pilot_input(&mut self, id: u8, ps: &PlayerState) {
        let back = self.pl.id as usize;
        self.activate(id as usize);
        if self.pl.id == id {
            self.ingest(ps);
            self.pl.last_input = ps.clone();
        }
        self.activate(back);
    }

    /// Which pilot an enemy missile in slot `s` (0..7) goes for.
    pub fn enemy_slot_target(&self, s: usize) -> u8 {
        self.proj_pilot.get(s).copied().unwrap_or(0)
    }

    /// Which pilot site, flight or AAA binding is: kept while he is in the world, else the
    /// one nearest (x, y). Without others it is always the active pilot.
    pub(crate) fn nearest_pilot(&self, current: u8, keep: bool, x: i32, y: i32) -> u8 {
        let present = self.present();
        if present.len() <= 1 {
            return present.first().map_or(self.pl.id, |&k| k as u8);
        }
        if keep && present.contains(&(current as usize)) {
            return current;
        }
        present
            .iter()
            .map(|&k| {
                let p = &self.pilot_ref(k).p;
                (crate::math::approx(crate::math::s16(p.mx - x), crate::math::s16(p.my - y)), k)
            })
            .min()
            .map_or(current, |(_, k)| k as u8)
    }
}
