//! Replication for networked coop: the host's simulation in pieces a client's copy (a
//! *mirror*, never stepped) is kept up to date with.
//!
//! * [`WorldFast`]: what moves, every frame for everybody (aircraft, missiles, enemy rounds,
//!   smoke, busy guns, the pilots' aircraft), complete each time, so a lost one costs nothing.
//! * [`WorldSlow`]: what changes now and then (objects, the air defence units, campaign
//!   flight records, new shells), as changes against what the client has been sent: it goes
//!   on a reliable channel. [`SlowBase`] is the host's record of a client's copy.
//! * [`PilotSnap`]: a pilot's own state for his client (damage, stores, locks, lights,
//!   objectives); the client keeps its own flight model's position, attitude and views.
//!
//! The shells fly on on the mirror ([`Sim::mirror_frame`]); hits are the host's business.

use serde::{Deserialize, Serialize};

use crate::aaa::{AaaUnit, Shell};
use crate::campaign::{AiMissile, CampaignFlight};
use crate::types::*;
use crate::{ENEMY_SLOTS, PILOT_SLOTS, Pilot, PlayerView, Sim};

/// Another pilot's aircraft as everyone sees it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemotePilot {
    pub id: u8,
    pub view: PlayerView,
    pub gear_up: bool,
    pub on_ground: bool,
    /// Bay doors 0 (closed) .. 2 (open).
    pub door_anim: i32,
    /// The aircraft type: the real F-117A (no cannon), and weapons outside (F-16).
    pub lockheed: bool,
    pub external_stores: bool,
    pub gun_firing: bool,
    pub burning: bool,
    /// Ejected: the parachute (map x, map y, alt).
    pub chute: Option<(i32, i32, i32)>,
    /// His weapons in flight (projectile records).
    pub missiles: Vec<Projectile>,
    /// His countermeasures.
    pub cms: [Countermeasure; 4],
}

/// What moves, as of one frame.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WorldFast {
    pub frame: i32,
    pub mission_tick: i32,
    pub tick_frames: i32,
    pub alarm_timer: i32,
    pub alarm_pilot: u8,
    pub reference: (i32, i32, i32),
    pub flights_len: u16,
    /// Existing flight units with the pilot each goes for.
    pub flights: Vec<(u16, FlightUnit, u8)>,
    /// Live enemy missiles with the pilot each goes for.
    pub enemy: Vec<(u8, Projectile, u8)>,
    pub enemy_rounds: Vec<Bullet>,
    pub smoke: Vec<(u8, Smoke)>,
    pub smoke_src: i32,
    pub wreck: Wreck,
    pub flash: Flash,
    /// Air defence units aiming or firing: index, azimuth, elevation, burst, tracking.
    pub aaa_busy: Vec<(u16, f32, f32, i32, bool)>,
    pub ai_missiles: Vec<AiMissile>,
    pub pilots: Vec<RemotePilot>,
}

/// What changed now and then.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WorldSlow {
    /// Changed objects: index, record, shape drawn, pilot it is bound to, recon intelligence,
    /// engaging AI aircraft.
    pub objects: Vec<(u16, GroundObject, i32, u8, bool, bool)>,
    /// The air defence list's length (units come and go with captures).
    pub aaa_len: u32,
    /// Changed units (without their aim): index, record, friendly.
    pub aaa: Vec<(u16, AaaUnit, bool)>,
    /// Campaign flight records that changed: unit, record (None: freed).
    pub campaign_flights: Vec<(u16, Option<CampaignFlight>)>,
    /// The tile damage list (only when a client joins; the events keep it after that).
    pub tiles: Option<Vec<TileDamage>>,
    /// Shells fired since the last message (air defences at the pilots, the AI at the AI).
    pub shells: Vec<Shell>,
    pub ai_shells: Vec<Shell>,
}

/// The host's record of what a client has been sent of the slow state.
#[derive(Debug, Clone, Default)]
pub struct SlowBase {
    objects: Vec<(GroundObject, i32, u8, bool, bool)>,
    aaa: Vec<(AaaUnit, bool)>,
    campaign_flights: Vec<Option<CampaignFlight>>,
    joined: bool,
}

/// A pilot's own state for his client.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PilotSnap {
    pub pilot: Pilot,
}

impl Sim {
    /// The moving world as of now.
    pub fn world_fast(&self) -> WorldFast {
        let nb = self.n_bullets;
        WorldFast {
            frame: self.frame,
            mission_tick: self.mission_tick,
            tick_frames: self.tick_frames,
            alarm_timer: self.alarm_timer,
            alarm_pilot: self.alarm_pilot,
            reference: (self.ref_x, self.ref_y, self.ref_alt),
            flights_len: self.flights.len() as u16,
            flights: self
                .flights
                .iter()
                .enumerate()
                .filter(|(_, u)| u.flags != 0)
                .map(|(i, u)| (i as u16, *u, self.flight_pilot.get(i).copied().unwrap_or(0)))
                .collect(),
            enemy: (0..ENEMY_SLOTS)
                .filter(|&s| self.proj[s].ttl != 0)
                .map(|s| (s as u8, self.proj[s], self.proj_pilot[s]))
                .collect(),
            enemy_rounds: self.bullets.get(nb..).map_or_else(Vec::new, <[Bullet]>::to_vec),
            smoke: self
                .smoke
                .iter()
                .enumerate()
                .filter(|(_, s)| *s != &Smoke::default())
                .map(|(i, s)| (i as u8, *s))
                .collect(),
            smoke_src: self.smoke_src,
            wreck: self.wreck,
            flash: self.flash,
            aaa_busy: self
                .aaa
                .iter()
                .enumerate()
                .filter(|(_, u)| !u.destroyed && (u.tracking || u.burst > 0))
                .map(|(i, u)| (i as u16, u.aim_az, u.aim_el, u.burst, u.tracking))
                .collect(),
            ai_missiles: self.ai_missiles().to_vec(),
            pilots: self.remote_pilots(),
        }
    }

    /// The pilots in the world as everyone sees them.
    pub fn remote_pilots(&self) -> Vec<RemotePilot> {
        if self.mirror {
            return self.remote.clone();
        }
        self.present()
            .into_iter()
            .map(|k| {
                let p = self.pilot_ref(k);
                let missiles = if k == self.pl.id as usize {
                    self.proj[ENEMY_SLOTS..].to_vec()
                } else {
                    p.missiles.to_vec()
                };
                RemotePilot {
                    id: k as u8,
                    view: p.p,
                    gear_up: p.pflags & pflags::GEAR_UP != 0,
                    on_ground: p.on_ground,
                    door_anim: p.door_anim,
                    lockheed: p.lockheed,
                    external_stores: p.external_stores,
                    gun_firing: p.gun_firing,
                    burning: p.fire_frames > 0 || p.breakup_frames > 0,
                    chute: (p.eject != 0).then_some(p.chute),
                    missiles: missiles.into_iter().filter(|m| m.ttl != 0).collect(),
                    cms: p.cms,
                }
            })
            .collect()
    }

    /// The slow state's changes since `base` (everything the first time), and `base` brought
    /// up to date. The caller adds the new shells ([`Sim::take_new_shells`]).
    pub fn world_slow(&self, base: &mut SlowBase) -> WorldSlow {
        let mut out = WorldSlow::default();
        let c = self.campaign.as_deref();
        let objects = self.objects.iter().enumerate().map(|(i, o)| {
            (
                *o,
                self.display_shape.get(i).copied().unwrap_or(0),
                self.site_pilot.get(i).copied().unwrap_or(0),
                c.is_some_and(|c| c.intel.get(i).copied().unwrap_or(false)),
                self.site_engaging(i),
            )
        });
        base.objects.resize(self.objects.len(), Default::default());
        for (i, now) in objects.enumerate() {
            // Object 0 is each pilot's scratch tile object.
            if i > 0 && (!base.joined || base.objects[i] != now) {
                base.objects[i] = now;
                out.objects.push((i as u16, now.0, now.1, now.2, now.3, now.4));
            }
        }
        out.aaa_len = self.aaa.len() as u32;
        base.aaa.truncate(self.aaa.len());
        for (i, u) in self.aaa.iter().enumerate() {
            // The aim, the burst and the tracking go with the fast state.
            let still =
                AaaUnit { aim_az: 0.0, aim_el: 0.0, burst: 0, cooldown: 0, tracking: false, ..*u };
            let now = (still, self.aaa_blue(i));
            if base.aaa.get(i) != Some(&now) {
                if i < base.aaa.len() {
                    base.aaa[i] = now;
                } else {
                    base.aaa.push(now);
                }
                out.aaa.push((i as u16, now.0, now.1));
            }
        }
        let flights: Vec<Option<CampaignFlight>> = c.map_or_else(Vec::new, |c| c.flights.clone());
        base.campaign_flights.resize(flights.len(), None);
        for (i, f) in flights.iter().enumerate() {
            if base.campaign_flights[i] != *f || (!base.joined && f.is_some()) {
                base.campaign_flights[i] = *f;
                out.campaign_flights.push((i as u16, *f));
            }
        }
        if !base.joined {
            out.tiles = Some(self.tile_damage.clone());
            out.shells = self.aaa_shells.clone();
            out.ai_shells = self.ai_shells().copied().collect();
        }
        base.joined = true;
        out
    }

    /// Records the shells fired from now on for [`Sim::take_new_shells`] (a host's simulation).
    pub fn record_shells(&mut self, on: bool) {
        self.record_shells = on;
        self.new_shells.clear();
        self.new_ai_shells.clear();
    }

    /// The shells fired since the last call: air defences', the AI's.
    pub fn take_new_shells(&mut self) -> (Vec<Shell>, Vec<Shell>) {
        (std::mem::take(&mut self.new_shells), std::mem::take(&mut self.new_ai_shells))
    }

    /// Pilot `id`'s own state for his client.
    pub fn pilot_snap(&self, id: u8) -> PilotSnap {
        let k = id as usize;
        let mut pilot = self.pilot_ref(k).clone();
        if k == self.pl.id as usize {
            let nb = self.n_bullets.min(self.bullets.len());
            pilot.rounds = self.bullets[..nb].to_vec();
            pilot.missiles.copy_from_slice(&self.proj[ENEMY_SLOTS..]);
            if let Some(o) = self.objects.first() {
                pilot.scratch0 = *o;
            }
        }
        pilot.log.clear();
        pilot.last_input = PlayerState::default();
        PilotSnap { pilot }
    }

    // ------------------------------------------------------------------ the mirror

    /// Makes this simulation a client's mirror of a host's, for pilot `me`: it is never
    /// stepped, only fed with the host's state.
    pub fn make_mirror(&mut self, me: u8) {
        self.mirror = true;
        self.parked = vec![Pilot::default(); me as usize + 1];
        self.pl.id = me;
        self.pl.absent = true;
        self.events.clear();
        self.event_pilot.clear();
        self.event_credit.clear();
    }

    pub fn is_mirror(&self) -> bool {
        self.mirror
    }

    /// The client's own flight model this frame (position, attitude, views) without any of
    /// the simulation's consequences.
    pub fn set_local_player(&mut self, ps: &PlayerState) {
        self.ingest(ps);
    }

    pub fn apply_world_fast(&mut self, w: &WorldFast) {
        self.frame = w.frame;
        self.mission_tick = w.mission_tick;
        self.tick_frames = w.tick_frames;
        self.alarm_timer = w.alarm_timer;
        self.alarm_pilot = w.alarm_pilot;
        (self.ref_x, self.ref_y, self.ref_alt) = w.reference;
        let n = w.flights_len as usize;
        self.flights.clear();
        self.flights.resize(n, FlightUnit::default());
        self.flight_pilot.clear();
        self.flight_pilot.resize(n, 0);
        for &(i, u, k) in &w.flights {
            if let Some(f) = self.flights.get_mut(i as usize) {
                *f = u;
                self.flight_pilot[i as usize] = k;
            }
        }
        for s in 0..ENEMY_SLOTS {
            self.proj[s] = Projectile::default();
        }
        for &(s, m, k) in &w.enemy {
            if (s as usize) < ENEMY_SLOTS {
                self.proj[s as usize] = m;
                self.proj_pilot[s as usize] = k;
            }
        }
        let nb = self.n_bullets;
        for (i, b) in w.enemy_rounds.iter().enumerate() {
            if let Some(slot) = self.bullets.get_mut(nb + i) {
                *slot = *b;
            }
        }
        self.smoke = Default::default();
        for &(i, s) in &w.smoke {
            if let Some(slot) = self.smoke.get_mut(i as usize) {
                *slot = s;
            }
        }
        self.smoke_src = w.smoke_src;
        self.wreck = w.wreck;
        self.flash = w.flash;
        for u in self.aaa.iter_mut() {
            u.tracking = false;
            u.burst = 0;
        }
        for &(i, az, el, burst, tracking) in &w.aaa_busy {
            if let Some(u) = self.aaa.get_mut(i as usize) {
                u.aim_az = az;
                u.aim_el = el;
                u.burst = burst;
                u.tracking = tracking;
            }
        }
        if let Some(c) = self.campaign.as_mut() {
            c.missiles = w.ai_missiles.clone();
        }
        self.remote = w.pilots.iter().filter(|p| p.id != self.pl.id).cloned().collect();
    }

    pub fn apply_world_slow(&mut self, w: &WorldSlow) {
        let n = self.objects.len();
        self.site_pilot.resize(n, 0);
        for &(i, o, shape, k, intel, engaging) in &w.objects {
            let i = i as usize;
            if i >= n {
                continue;
            }
            self.objects[i] = o;
            if let Some(s) = self.display_shape.get_mut(i) {
                *s = shape;
            }
            if let Some(s) = self.site_pilot.get_mut(i) {
                *s = k;
            }
            if let Some(c) = self.campaign.as_mut() {
                if let Some(v) = c.intel.get_mut(i) {
                    *v = intel;
                }
                if let Some(v) = c.site_cool.get_mut(i) {
                    *v = engaging as i32;
                }
            }
        }
        let len = w.aaa_len as usize;
        self.aaa.resize(len, AaaUnit::default());
        if let Some(c) = self.campaign.as_mut() {
            c.aaa_blue.resize(len, false);
            c.aaa_target.resize(len, None);
        }
        for &(i, u, blue) in &w.aaa {
            let i = i as usize;
            if i < len {
                let old = self.aaa[i];
                self.aaa[i] = AaaUnit {
                    aim_az: old.aim_az,
                    aim_el: old.aim_el,
                    burst: old.burst,
                    tracking: old.tracking,
                    ..u
                };
                if let Some(c) = self.campaign.as_mut() {
                    c.aaa_blue[i] = blue;
                }
            }
        }
        if let Some(c) = self.campaign.as_mut() {
            for &(i, f) in &w.campaign_flights {
                let i = i as usize;
                if c.flights.len() <= i {
                    c.flights.resize(i + 1, None);
                }
                c.flights[i] = f;
            }
            for s in &w.ai_shells {
                c.shells.push(crate::campaign::AiShell { shell: *s, target: 0 });
            }
        }
        if let Some(t) = &w.tiles {
            self.tile_damage = t.clone();
        }
        self.aaa_shells.extend_from_slice(&w.shells);
    }

    /// The pilot's own state from the host; his flight model's part stays the client's.
    pub fn apply_pilot_snap(&mut self, s: &PilotSnap) {
        let me = self.pl.id;
        let mut p = s.pilot.clone();
        let own = &self.pl;
        p.id = me;
        p.p = own.p;
        p.hud_mode = own.hud_mode;
        p.prev_hud_mode = own.prev_hud_mode;
        p.camera_mfd = own.camera_mfd;
        p.external_view = own.external_view;
        p.on_ground = own.on_ground;
        p.prev_on_ground = own.prev_on_ground;
        p.log = std::mem::take(&mut self.pl.log);
        let nb = self.n_bullets.min(self.bullets.len());
        if p.rounds.len() == nb {
            self.bullets[..nb].copy_from_slice(&p.rounds);
        }
        self.proj[ENEMY_SLOTS..].copy_from_slice(&p.missiles[..PILOT_SLOTS]);
        if let Some(o) = self.objects.first_mut() {
            *o = p.scratch0;
        }
        self.pl = p;
    }

    /// One frame of the mirror between the host's: the shells fly on.
    pub fn mirror_frame(&mut self) {
        let dt = 1.0 / self.fs.max(1) as f64;
        let fly = |s: &mut Shell| {
            s.pos = [s.pos[0] + s.vel[0] * dt, s.pos[1] + s.vel[1] * dt, s.pos[2] + s.vel[2] * dt];
            s.vel[2] -= 9.81 * dt;
            s.ttl -= 1;
            s.pos[2] >= 0.0 && s.ttl > 0
        };
        self.aaa_shells.retain_mut(fly);
        if let Some(c) = self.campaign.as_mut() {
            c.shells.retain_mut(|s| fly(&mut s.shell));
        }
    }
}
