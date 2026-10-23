//! Replication for networked coop: the host's simulation in pieces a client's copy (a
//! *mirror*, never stepped) is kept up to date with.
//!
//! * [`WorldFast`]: what moves, every frame for everybody (aircraft, missiles, enemy rounds,
//!   smoke, busy guns, the pilots' aircraft), complete each time, so a lost one costs nothing.
//! * [`WorldSlow`]: what changes now and then (objects, the air defence units, campaign
//!   flight records, new shells), as changes against what the client has been sent: it goes
//!   on a reliable channel. [`SlowBase`] is the host's record of a client's copy.
//! * [`PilotSnap`]: a pilot's own state for his client (damage, stores, locks, lights,
//!   objectives); the client keeps its own flight model's position, attitude and views. It
//!   only changes now and then, so the server sends it when it does ([`PilotTick`] is the
//!   little of it that changes all the time).
//!
//! A server sends the moving world a few times a second, not every frame; a client shows it a
//! little behind and interpolates. Far away pilots may be left out of a frame
//! ([`WorldFast::present`] lists them all): the mirror keeps them as they were.
//!
//! The shells fly on on the mirror ([`Sim::mirror_frame`]); hits are the host's business,
//! but for the player's own gun rounds: those the mirror flies, and reports their hits
//! (`claims`).

use serde::{Deserialize, Serialize};

use crate::aaa::{AaaUnit, Shell};
use crate::campaign::{AiMissile, CampaignFlight};
use crate::types::*;
use crate::{ENEMY_SLOTS, PILOT_SLOTS, Pilot, PlayerView, Sim};

/// Another pilot's aircraft as everyone sees it (on the wire as a compact [`RemoteWire`]:
/// there may be 64 of them in every frame).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(into = "RemoteWire", from = "RemoteWire")]
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
    /// A helicopter (UH-60A), an attack helicopter (AH-1Z).
    pub helicopter: bool,
    pub gunship: bool,
    pub gun_firing: bool,
    /// Where his turret points while it fires (BAM heading, pitch), else along the nose.
    pub gun_aim: Option<(i32, i32)>,
    pub burning: bool,
    /// Damage smoke: 0 none, 1 light, 2 heavy (badly hit), 3 on fire.
    pub smoke: u8,
    /// Ejected: the parachute (map x, map y, alt).
    pub chute: Option<(i32, i32, i32)>,
    /// His weapons in flight: slot, projectile record.
    pub missiles: Vec<(u8, Projectile)>,
    /// His countermeasures.
    pub cms: [Countermeasure; 4],
}

/// [`RemotePilot`] as a sequence without field names: id, view (wx, wy, mx, my, alt, view_z,
/// heading, pitch, roll, velocity, knots, climb), flags (gear up, on the ground, F-117A as it
/// was, external stores, gun firing, burning, damage smoke in two bits, helicopter, gunship),
/// bay doors, parachute, missiles by slot, the live countermeasures by slot, the turret's aim.

#[derive(Serialize, Deserialize)]
struct RemoteWire(
    u8,
    [i32; 12],
    u16,
    i32,
    Option<(i32, i32, i32)>,
    Vec<(u8, Projectile)>,
    Vec<(u8, Countermeasure)>,
    #[serde(default)] Option<(i32, i32)>,
);

impl From<RemotePilot> for RemoteWire {
    fn from(p: RemotePilot) -> Self {
        let v = p.view;
        let flags =
            [p.gear_up, p.on_ground, p.lockheed, p.external_stores, p.gun_firing, p.burning]
                .iter()
                .enumerate()
                .fold(0u16, |f, (k, &b)| f | (b as u16) << k)
                | (p.smoke.min(3) as u16) << 6
                | (p.helicopter as u16) << 8
                | (p.gunship as u16) << 9;
        RemoteWire(
            p.id,
            [
                v.wx, v.wy, v.mx, v.my, v.alt, v.view_z, v.heading, v.pitch, v.roll, v.velocity,
                v.knots, v.climb,
            ],
            flags,
            p.door_anim,
            p.chute,
            p.missiles,
            (0..4u8)
                .filter(|&k| p.cms[k as usize].ttl != 0)
                .map(|k| (k, p.cms[k as usize]))
                .collect(),
            p.gun_aim.filter(|_| p.gun_firing),
        )
    }
}

impl From<RemoteWire> for RemotePilot {
    fn from(w: RemoteWire) -> Self {
        let RemoteWire(id, v, flags, door_anim, chute, missiles, live, gun_aim) = w;
        let [wx, wy, mx, my, alt, view_z, heading, pitch, roll, velocity, knots, climb] = v;
        let bit = |k: u8| flags & (1 << k) != 0;
        let mut cms = [Countermeasure::default(); 4];
        for (k, c) in live {
            if let Some(slot) = cms.get_mut(k as usize) {
                *slot = c;
            }
        }
        RemotePilot {
            id,
            view: PlayerView {
                wx,
                wy,
                mx,
                my,
                alt,
                view_z,
                heading,
                pitch,
                roll,
                velocity,
                knots,
                climb,
            },
            gear_up: bit(0),
            on_ground: bit(1),
            door_anim,
            lockheed: bit(2),
            external_stores: bit(3),
            gun_firing: bit(4),
            burning: bit(5),
            smoke: (flags >> 6) as u8 & 3,
            helicopter: bit(8),
            gunship: bit(9),
            gun_aim,
            chute,
            missiles,
            cms,
        }
    }
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
    /// Every pilot in the world; those not in `pilots` this time are as they were.
    #[serde(default)]
    pub present: Vec<u8>,
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

/// A pilot's own state for his client, without what changes every frame (the same as long as
/// nothing happens).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PilotSnap {
    pub pilot: Pilot,
}

/// What of a pilot's own state changes every frame: the radar sweep, the distance to the
/// nearest base.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct PilotTick {
    pub sweep_anim: i32,
    pub nearest_base_dist: i32,
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
            present: Vec::new(),
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
                    helicopter: p.helicopter,
                    gunship: p.gunship,
                    gun_firing: p.gun_firing,
                    gun_aim: p.gun_aim,
                    burning: p.fire_frames > 0 || p.breakup_frames > 0,
                    smoke: if p.damage & dmg::FIRE != 0 {
                        3
                    } else if p.hits >= 6 {
                        2
                    } else {
                        (p.damage != 0) as u8
                    },
                    chute: (p.eject != 0).then_some(p.chute),
                    missiles: missiles
                        .into_iter()
                        .enumerate()
                        .filter(|(_, m)| m.ttl != 0)
                        .map(|(s, m)| (s as u8, m))
                        .collect(),
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
            let still = AaaUnit {
                aim_az: 0.0,
                aim_el: 0.0,
                burst: 0,
                cooldown: 0,
                tracking: false,
                rounds: 0,
                ..*u
            };
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
            pilot.missiles.copy_from_slice(&self.proj[ENEMY_SLOTS..]);
            if let Some(o) = self.objects.first() {
                pilot.scratch0 = *o;
            }
        }
        pilot.log.clear();
        pilot.last_input = PlayerState::default();
        // His gun rounds fly on his client (`claims`).
        pilot.rounds.clear();
        // His own flight model's, the host's own tracking, and the ticking part.
        pilot.p = PlayerView::default();
        pilot.aaa_track = None;
        pilot.sweep_anim = 0;
        pilot.nearest_base_dist = 0;
        PilotSnap { pilot }
    }

    /// Pilot `id`'s state that changes every frame.
    pub fn pilot_tick(&self, id: u8) -> PilotTick {
        let p = self.pilot_ref(id as usize);
        PilotTick { sweep_anim: p.sweep_anim, nearest_base_dist: p.nearest_base_dist }
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
        let me = self.pl.id;
        let mut remote: Vec<RemotePilot> =
            w.pilots.iter().filter(|p| p.id != me).cloned().collect();
        // Those left out this time (far away) stay as they were.
        for p in std::mem::take(&mut self.remote) {
            if w.present.contains(&p.id) && remote.iter().all(|q| q.id != p.id) {
                remote.push(p);
            }
        }
        remote.sort_by_key(|p| p.id);
        self.remote = remote;
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
        p.sweep_anim = own.sweep_anim;
        p.nearest_base_dist = own.nearest_base_dist;
        p.log = std::mem::take(&mut self.pl.log);
        self.proj[ENEMY_SLOTS..].copy_from_slice(&p.missiles[..PILOT_SLOTS]);
        if let Some(o) = self.objects.first_mut() {
            *o = p.scratch0;
        }
        self.pl = p;
    }

    pub fn apply_pilot_tick(&mut self, t: &PilotTick) {
        self.pl.sweep_anim = t.sweep_anim;
        self.pl.nearest_base_dist = t.nearest_base_dist;
    }

    /// One frame of the mirror between the host's: the shells fly on, and the other pilots'
    /// rounds from their trigger (only the trigger crosses the network).
    pub fn mirror_frame(&mut self) {
        let fs = self.fs.max(1);
        let nb = self.n_bullets as u16;
        let g = Self::gun_round_gravity(fs);
        self.remote_rounds.retain_mut(|(b, age)| {
            b.x += b.vx;
            b.y += b.vy;
            b.alt += b.vz;
            b.vz -= g;
            *age += 1;
            *age < nb && b.alt > 0
        });
        for p in self.remote.iter().filter(|p| p.gun_firing && !p.lockheed && p.chute.is_none()) {
            self.remote_rounds.push((Self::gun_round(&p.view, p.gun_aim, fs), 0));
        }
        let dt = 1.0 / fs as f64;
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
