//! Smooth motion between the server's frames.
//!
//! * [`Playout`]: a client shows the server's frames a little behind the newest, at a steady
//!   pace, so packets that come late or bunched up don't make the world stutter. The delay
//!   follows the gap between the frames (the server sends a few a second) and their jitter.
//! * [`RemoteTrack`]: the other pilots' aircraft as sampled at server frames; they are drawn
//!   at the playout time between the two samples around it (a pilot far away is in fewer
//!   frames), carried on a little when the samples run out.

use std::collections::{HashMap, VecDeque};

use stealth_sim::PlayerView;

/// Server frames at most a pilot is carried on past his last sample (about a quarter second).
const EXTRAPOLATE: f64 = 4.0;
/// Samples kept per pilot.
const SAMPLES: usize = 6;
/// A jump this far (world units) between samples is a new start: no blend.
const JUMP: i32 = 0x4000;

#[derive(Default)]
pub struct Playout {
    /// The server frame shown now (fractional); None until the first frame.
    clock: Option<f64>,
    /// Server frames of the last two frames applied.
    prev: u32,
    cur: u32,
    /// The newest frame received, and seconds since it came.
    newest: Option<u32>,
    since: f64,
    /// Typical frames between received frames, and the arrival jitter (frames).
    gap: f64,
    jitter: f64,
}

impl Playout {
    /// Frame `step` came in (each one once, in order).
    pub fn received(&mut self, step: u32, fs: f64) {
        if let Some(n) = self.newest {
            let d = step.wrapping_sub(n) as i32;
            if d <= 0 {
                return;
            }
            let late = d as f64 - self.since * fs;
            self.jitter = self.jitter * 0.9 + late.abs() * 0.1;
            self.gap = self.gap * 0.9 + d as f64 * 0.1;
        } else {
            self.gap = 2.0;
        }
        self.newest = Some(step);
        self.since = 0.0;
    }

    /// Frames behind the server's newest shown: a frame gap and the jitter's margin.
    fn delay(&self) -> f64 {
        (self.gap + 2.0 * self.jitter + 0.5).clamp(1.0, 15.0)
    }

    /// Time goes on: the clock runs at the server's pace, a little faster or slower to keep
    /// its distance behind the newest frame.
    pub fn advance(&mut self, secs: f64, fs: f64) {
        self.since += secs;
        let delay = self.delay();
        let (Some(c), Some(n)) = (self.clock.as_mut(), self.newest) else { return };
        let target = n as f64 + self.since * fs - delay;
        let err = target - *c;
        if err.abs() > 2.0 * fs {
            *c = target;
        } else {
            *c += secs * fs * (1.0 + (err * 0.1).clamp(-0.25, 0.25));
        }
    }

    /// May the next frame be applied (the clock has reached the current one)?
    pub fn due(&self) -> bool {
        self.clock.is_none_or(|c| c >= self.cur as f64)
    }

    /// Frame `step` was applied: the server frames it moves the world by.
    pub fn applied(&mut self, step: u32) -> u32 {
        let Some(c) = self.clock else {
            self.clock = Some(step as f64);
            (self.prev, self.cur) = (step, step);
            return 1;
        };
        let gap = step.wrapping_sub(self.cur).max(1);
        (self.prev, self.cur) = (self.cur, step);
        // Far behind (a pause): show the new frame at once.
        if c < self.prev as f64 - 1.0 {
            self.clock = Some(self.prev as f64);
        }
        gap
    }

    /// The server frame shown now.
    pub fn now(&self) -> Option<f64> {
        self.clock
    }

    /// How far the clock is from the frame before the current one to it (0..1).
    pub fn alpha(&self) -> f32 {
        let Some(c) = self.clock else { return 1.0 };
        let span = self.cur.wrapping_sub(self.prev);
        if span == 0 {
            return 1.0;
        }
        ((c - self.prev as f64) / span as f64).clamp(0.0, 1.0) as f32
    }

    /// Server frames between the last two frames applied.
    pub fn span(&self) -> u32 {
        self.cur.wrapping_sub(self.prev).max(1)
    }
}

/// The other pilots' aircraft at the server frames they were sampled at.
#[derive(Default)]
pub struct RemoteTrack {
    samples: HashMap<u8, VecDeque<(u32, PlayerView)>>,
}

impl RemoteTrack {
    /// Pilot `id` was at `view` at server frame `step`.
    pub fn sample(&mut self, id: u8, step: u32, view: PlayerView) {
        let s = self.samples.entry(id).or_default();
        if s.back().is_some_and(|&(t, _)| step.wrapping_sub(t) as i32 <= 0) {
            return;
        }
        s.push_back((step, view));
        while s.len() > SAMPLES {
            s.pop_front();
        }
    }

    /// Only these pilots are still about.
    pub fn retain(&mut self, ids: &[u8]) {
        self.samples.retain(|id, _| ids.contains(id));
    }

    /// Pilot `id` at server frame `t`: the samples before and after it and how far between
    /// (beyond 1 carried on past the last one); `fallback` without samples.
    pub fn at(&self, id: u8, t: f64, fallback: PlayerView) -> (PlayerView, PlayerView, f64) {
        let Some(s) = self.samples.get(&id).filter(|s| !s.is_empty()) else {
            return (fallback, fallback, 1.0);
        };
        let rel = |step: u32| step as f64;
        let i = s.iter().position(|&(step, _)| rel(step) > t);
        let (a, b) = match i {
            Some(0) => return (s[0].1, s[0].1, 1.0),
            Some(i) => (s[i - 1], s[i]),
            None if s.len() >= 2 => (s[s.len() - 2], s[s.len() - 1]),
            None => return (s[0].1, s[0].1, 1.0),
        };
        let span = b.0.wrapping_sub(a.0).max(1) as f64;
        let jump = (a.1.wx - b.1.wx).abs() > JUMP || (a.1.wy - b.1.wy).abs() > JUMP;
        if jump {
            return (b.1, b.1, 1.0);
        }
        let k = ((t - rel(a.0)) / span).clamp(0.0, 1.0 + EXTRAPOLATE / span);
        (a.1, b.1, k)
    }

    /// Pilot `id` at server frame `t` as one view (positions blended, angles the short way).
    pub fn view_at(&self, id: u8, t: f64, fallback: PlayerView) -> PlayerView {
        let (a, b, k) = self.at(id, t, fallback);
        blend(&a, &b, k)
    }
}

/// A view between two (k 0..1, beyond 1 carried on).
pub fn blend(a: &PlayerView, b: &PlayerView, k: f64) -> PlayerView {
    let lerp = |x: i32, y: i32| (x as f64 + (y - x) as f64 * k).round() as i32;
    let angle = |x: i32, y: i32| {
        let d = (y as i16).wrapping_sub(x as i16) as f64;
        (x as f64 + d * k).round() as i32 & 0xFFFF
    };
    PlayerView {
        wx: lerp(a.wx, b.wx),
        wy: lerp(a.wy, b.wy),
        mx: lerp(a.mx, b.mx),
        my: lerp(a.my, b.my),
        alt: lerp(a.alt, b.alt),
        view_z: lerp(a.view_z, b.view_z),
        heading: angle(a.heading, b.heading),
        pitch: (angle(a.pitch, b.pitch) as u16) as i16 as i32,
        roll: (angle(a.roll, b.roll) as u16) as i16 as i32,
        velocity: lerp(a.velocity, b.velocity),
        knots: lerp(a.knots, b.knots),
        climb: lerp(a.climb, b.climb),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(wx: i32) -> PlayerView {
        PlayerView { wx, ..Default::default() }
    }

    #[test]
    fn the_track_blends_and_carries_on() {
        let mut t = RemoteTrack::default();
        t.sample(1, 10, at(100));
        t.sample(1, 12, at(200));
        assert_eq!(t.view_at(1, 11.0, at(0)).wx, 150);
        // Past the last sample: carried on, not further than the limit.
        assert_eq!(t.view_at(1, 13.0, at(0)).wx, 250);
        assert_eq!(t.view_at(1, 100.0, at(0)).wx, 400);
        // Before the first: held.
        assert_eq!(t.view_at(1, 5.0, at(0)).wx, 100);
        assert_eq!(t.view_at(2, 5.0, at(7)).wx, 7);
    }

    #[test]
    fn angles_blend_the_short_way() {
        let a = PlayerView { heading: 0xFF00, ..Default::default() };
        let b = PlayerView { heading: 0x0100, ..Default::default() };
        assert_eq!(blend(&a, &b, 0.5).heading, 0);
    }

    #[test]
    fn the_playout_keeps_a_steady_distance() {
        let mut p = Playout::default();
        let fs = 15.0;
        let mut step = 0u32;
        let mut applied = Vec::new();
        let mut pending = std::collections::VecDeque::new();
        // Frames every second server frame, every other pair arriving together.
        for f in 0..300 {
            let secs = 1.0 / fs;
            if f % 4 == 3 {
                for _ in 0..2 {
                    step += 2;
                    p.received(step, fs);
                    pending.push_back(step);
                }
            }
            p.advance(secs, fs);
            while p.due()
                && let Some(s) = pending.pop_front()
            {
                p.applied(s);
                applied.push((f, s));
            }
        }
        // Once settled, a frame is applied every other server frame, never two at once.
        let late: Vec<_> = applied.iter().filter(|(f, _)| *f > 100).collect();
        for w in late.windows(2) {
            assert!(w[1].0 > w[0].0, "{late:?}");
        }
        assert!(p.alpha() >= 0.0 && p.alpha() <= 1.0);
    }
}
