//! Campaign weather: a sequence of phases of 10-22 hours of the
//! sky's clock, each a weather preset rolled from the campaign seed alone. Every computer of
//! a session derives the same weather from the setup and the campaign clock, so none of it is
//! sent or saved.
//!
//! Between two phases the weather changes over a few minutes. A layer present in both (at
//! about the same altitude) keeps its clouds and only grows or thins; otherwise the old layer
//! thins out before the new one builds up. Cumulus and cumulonimbus come and go in their rank
//! order ([`Cloud::rank`](stealth_mission::weather::Cloud)), stratus by its coverage
//! threshold, so at every moment one [`Weather`] holds for the lines of sight.

use stealth_mission::weather::{Layer, Sky, Weather, odds};

use crate::CampaignSetup;

/// Cover points over which a cumulus or cumulonimbus fades in or out.
pub const FADE: f32 = 5.0;
/// Length of a phase in hours of the sky: this plus up to 12.
const PHASE_HOURS: f64 = 10.0;
/// Change between phases: real seconds, this plus up to 240.
const CHANGE_SECS: f64 = 240.0;

#[derive(Debug, Clone)]
struct Phase {
    /// Campaign clock at the start, length and the change from the previous phase (s).
    start: f64,
    len: f64,
    change: f64,
    weather: Weather,
    /// Cumulus, stratus, cumulonimbus layer kept from the previous phase.
    kept: [bool; 3],
}

/// The weather at a moment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Now {
    /// What the lines of sight see: the clouds at least half built up.
    pub weather: Weather,
    /// Cumulus, stratus and cumulonimbus coverage (percent, unrounded) for drawing; a cloud of
    /// rank r is drawn with the opacity [`cloud_fade`].
    pub cover: [f32; 3],
    /// Stratus field threshold 0..1 of the drawn deck.
    pub stratus_threshold: f32,
    /// Storm cells built up, 0..1.
    pub storm: f32,
    pub vis_km: f32,
    /// The next phase's preset and the campaign clock when it begins to build up.
    pub next: (Sky, f64),
}

/// Opacity 0..1 of a cumulus or cumulonimbus of rank `rank` at layer coverage `cover`.
pub fn cloud_fade(cover: f32, rank: u8) -> f32 {
    ((cover * (100.0 + FADE) / 100.0 - rank as f32) / FADE).clamp(0.0, 1.0)
}

/// The layer coverage the lines of sight see: the clouds more than half faded in.
fn seen_cover(cover: f32) -> i32 {
    (cover * (100.0 + FADE) / 100.0 - FADE / 2.0).ceil() as i32
}

fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The theater's odds of [`Sky::ALL`] made milder: overcast at most 15 %, storms 4-10 %; the
/// rest goes to scattered and broken clouds.
fn campaign_odds(theater: [u8; 5]) -> [u8; 5] {
    let [mut clear, mut scattered, mut broken, mut overcast, mut storm] = theater.map(i32::from);
    if overcast > 15 {
        scattered += overcast - 15;
        overcast = 15;
    }
    if storm > 10 {
        broken += storm - 10;
        storm = 10;
    }
    if storm < 4 {
        clear -= 4 - storm;
        storm = 4;
    }
    [clear, scattered, broken, overcast, storm].map(|v| v.max(0) as u8)
}

fn layers(w: &mut Weather) -> [&mut Layer; 3] {
    [&mut w.cumulus, &mut w.stratus, &mut w.cumulonimbus]
}

fn layer(w: &Weather, i: usize) -> Layer {
    [w.cumulus, w.stratus, w.cumulonimbus][i]
}

/// The campaign's weather over time.
#[derive(Debug, Clone)]
pub struct Forecast {
    seed: u64,
    odds: [u8; 5],
    /// Campaign seconds per hour of the sky.
    hour: f64,
    phases: Vec<Phase>,
    /// Stratus thresholds of the last layer seeds.
    tables: Vec<(u32, [u16; 101])>,
}

impl Forecast {
    /// The setup's weather; none for a campaign under a clear sky.
    pub fn new(setup: &CampaignSetup) -> Option<Self> {
        setup.dynamic_weather.then(|| Self {
            seed: mix((setup.seed as u64) << 8 ^ setup.theater as u64 ^ 0x5745_4154_4845_5200),
            odds: campaign_odds(odds(setup.theater)),
            hour: 3600.0 / setup.sky_ratio.max(0.01) as f64,
            phases: Vec::new(),
            tables: Vec::new(),
        })
    }

    fn hash(&self, k: usize, i: u64) -> u64 {
        mix(mix(self.seed ^ k as u64) ^ i)
    }

    /// Phase `k` after the last one rolled.
    fn roll(&self, k: usize) -> Phase {
        let prev = self.phases.last();
        let h = |i| self.hash(k, i);
        let mut len = (PHASE_HOURS + (h(1) % 1200) as f64 / 100.0) * self.hour;
        let start = match prev {
            Some(p) => p.start + p.len,
            // The campaign starts somewhere in the first phase.
            None => {
                len *= 0.3 + 0.7 * (h(2) % 1000) as f64 / 1000.0;
                0.0
            }
        };
        let mut n = (h(3) % 100) as i32;
        let mut sky = Sky::ALL
            .into_iter()
            .zip(self.odds)
            .find(|&(_, p)| {
                n -= p as i32;
                n < 0
            })
            .map_or(Sky::Clear, |(s, _)| s);
        if sky == Sky::Storm && prev.is_some_and(|p| p.weather.sky == Sky::Storm) {
            sky = if h(4) % 2 == 0 { Sky::Scattered } else { Sky::Broken };
        }
        let v = std::array::from_fn(|i| (h(10 + i as u64) % 1000) as i32);
        let mut weather = Weather::from_sky(sky, h(5) as u32, v);
        let mut kept = [false; 3];
        for (i, l) in layers(&mut weather).into_iter().enumerate() {
            if !l.present() {
                continue;
            }
            l.seed = h(20 + i as u64) as u32 | 1;
            // A layer at about the same altitude carries on.
            if let Some(p) = prev.map(|p| layer(&p.weather, i))
                && p.present()
                && (i == 2 || (p.base_ft - l.base_ft).abs() <= 2000)
            {
                (l.base_ft, l.top_ft, l.seed) = (p.base_ft, p.top_ft, p.seed);
                kept[i] = true;
            }
        }
        let change = prev
            .map_or(0.0, |p| (CHANGE_SECS + (h(6) % 241) as f64).min(0.4 * len).min(0.4 * p.len));
        Phase { start, len, change, weather, kept }
    }

    /// Index of the phase at `clock`, rolling phases up to the one after it.
    fn phase(&mut self, clock: f64) -> usize {
        while self.phases.last().is_none_or(|p| p.start <= clock) {
            let p = self.roll(self.phases.len());
            self.phases.push(p);
        }
        self.phases.iter().rposition(|p| p.start <= clock).unwrap_or(0)
    }

    fn thresholds(&mut self, w: &Weather) -> [u16; 101] {
        let seed = w.layer_seed(stealth_mission::CloudKind::Stratus);
        if let Some((_, t)) = self.tables.iter().find(|(s, _)| *s == seed) {
            return *t;
        }
        let t = w.stratus_thresholds();
        if self.tables.len() >= 4 {
            self.tables.remove(0);
        }
        self.tables.push((seed, t));
        t
    }

    /// The weather at campaign second `clock`.
    pub fn at(&mut self, clock: f64) -> Now {
        let k = self.phase(clock);
        let cur = self.phases[k].clone();
        let next = &self.phases[k + 1];
        let next = (next.weather.sky, next.start);
        let a = if k > 0 { self.phases[k - 1].weather } else { cur.weather };
        let b = cur.weather;
        let t =
            if cur.change > 0.0 { ((clock - cur.start) / cur.change).clamp(0.0, 1.0) } else { 1.0 };
        let p = (t * t * (3.0 - 2.0 * t)) as f32;
        let mut w = b;
        if p < 0.5 {
            w.sky = a.sky;
        }
        let mut cover = [0.0; 3];
        for (i, l) in layers(&mut w).into_iter().enumerate() {
            let (la, lb) = (layer(&a, i), layer(&b, i));
            let (from, to) = (la.cover as f32, lb.cover as f32);
            let (src, c) = if cur.kept[i] {
                (lb, from + (to - from) * p)
            } else if !la.present() {
                (lb, to * p)
            } else if !lb.present() {
                (la, from * (1.0 - p))
            } else if p < 0.5 {
                (la, from * (1.0 - 2.0 * p))
            } else {
                (lb, to * (2.0 * p - 1.0))
            };
            let seen = if i == 1 { c.round() as i32 } else { seen_cover(c) };
            *l = Layer { cover: seen.clamp(0, 100) as u8, ..src };
            cover[i] = c;
        }
        let table = self.thresholds(&w);
        w.stratus_threshold = table[w.stratus.cover as usize];
        let c = cover[1].clamp(0.0, 100.0);
        let (lo, hi) = (table[c.floor() as usize] as f32, table[c.ceil() as usize] as f32);
        let stratus_threshold = (lo + (hi - lo) * c.fract()) / 65535.0;
        let storms = a.cumulonimbus.cover.max(b.cumulonimbus.cover);
        let storm = if storms > 0 { (cover[2] / storms as f32).min(1.0) } else { 0.0 };
        let vis_km = a.vis_km as f32 + (b.vis_km - a.vis_km) as f32 * p;
        w.vis_km = vis_km.round() as i32;
        Now { weather: w, cover, stratus_threshold, storm, vis_km, next }
    }
}

#[cfg(test)]
mod tests {
    use stealth_mission::CloudKind;

    use super::*;

    fn forecast(seed: u32, theater: usize) -> Forecast {
        Forecast::new(&CampaignSetup {
            theater,
            theater_code: String::new(),
            roe: 1,
            opponents: 1,
            seed,
            sky_ratio: 24.0,
            start_hour: 8.0,
            dynamic_weather: true,
        })
        .unwrap()
    }

    /// One day of the sky in campaign seconds at sky ratio 24.
    const DAY: f64 = 3600.0;

    #[test]
    fn same_setup_same_weather() {
        let (mut a, mut b) = (forecast(77, 3), forecast(77, 3));
        // Asked in a different order.
        let clocks: Vec<f64> = (0..200).map(|i| i as f64 * 913.7).collect();
        let first: Vec<Now> = clocks.iter().map(|&c| a.at(c)).collect();
        for (i, &c) in clocks.iter().enumerate().rev() {
            assert_eq!(b.at(c), first[i]);
        }
    }

    #[test]
    fn changes_once_or_twice_a_day_mostly_fair() {
        for theater in 0..10 {
            let mut f = forecast(4242 + theater as u32, theater);
            f.at(1000.0 * DAY);
            let phases = &f.phases;
            let n = phases.iter().filter(|p| p.start < 1000.0 * DAY).count();
            assert!((1000..2000).contains(&n), "{n} phases in 1000 days");
            let storms = phases.iter().filter(|p| p.weather.sky == Sky::Storm).count();
            assert!(storms * 100 <= n * 11, "{storms} storms of {n}");
            assert!(storms > 0);
            let fair = phases
                .iter()
                .filter(|p| matches!(p.weather.sky, Sky::Clear | Sky::Scattered | Sky::Broken))
                .count();
            assert!(fair * 2 > n, "{fair} fair of {n}");
            assert!(
                phases
                    .windows(2)
                    .all(|w| w[0].weather.sky != Sky::Storm || w[1].weather.sky != Sky::Storm)
            );
        }
    }

    #[test]
    fn changes_are_gradual() {
        let mut f = forecast(99, 4);
        let mut prev = f.at(0.0);
        for s in 1..(30.0 * DAY) as i32 {
            let now = f.at(s as f64);
            for i in 0..3 {
                assert!((now.cover[i] - prev.cover[i]).abs() < 2.0, "{s}: {i}");
            }
            // (The field's upper tail: the first percent covers few, high spots.)
            if now.cover[1].min(prev.cover[1]) >= 1.0 {
                assert!((now.stratus_threshold - prev.stratus_threshold).abs() < 0.05, "{s}");
            }
            prev = now;
        }
        let k = f.phase(30.0 * DAY);
        let changes = f.phases[1..k].iter().map(|p| p.change);
        assert!(changes.clone().all(|c| (60.0..=480.0).contains(&c)));
    }

    #[test]
    fn seen_clouds_are_half_faded_in() {
        let mut f = forecast(5, 4);
        for s in (0..(10.0 * DAY) as i32).step_by(37) {
            let now = f.at(s as f64);
            for (i, kind) in [(0, CloudKind::Cumulus), (2, CloudKind::Cumulonimbus)] {
                for g in 0..400 {
                    let Some(c) = now.weather.cloud(kind, g % 20, g / 20) else { continue };
                    assert!(cloud_fade(now.cover[i], c.rank) >= 0.5);
                }
            }
        }
        // Full and no coverage.
        assert!(seen_cover(100.0) >= 100);
        assert!(seen_cover(0.0) <= 0);
        assert_eq!(cloud_fade(100.0, 99), 1.0);
    }

    #[test]
    fn clear_sky_campaign_has_no_forecast() {
        let s = CampaignSetup {
            theater: 0,
            theater_code: String::new(),
            roe: 1,
            opponents: 1,
            seed: 1,
            sky_ratio: 24.0,
            start_hour: 8.0,
            dynamic_weather: false,
        };
        assert!(Forecast::new(&s).is_none());
    }
}
