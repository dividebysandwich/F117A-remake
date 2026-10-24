//! Continuous time of day (the "Time of day" switch, `--continuous-time`): the sun and the moon
//! move with the mission clock, the palette blends between the theater's night, dusk and day
//! palettes by the sun's elevation, the moon shows the phase of the briefing date (or the one
//! chosen; in a campaign it moves on 30% of its cycle a day), stars come out, and moonlight sets how far enemy pilots see at night. Off, the classic palette drift
//! and fixed sun stay (`palette::step_time_of_day`).
//!
//! The sun keeps to the equinox (it rises at 06:00 and sets at 18:00 local time everywhere), so
//! day missions still start at dawn and night missions at dusk.

use std::f64::consts::TAU;

use bevy::prelude::*;

use crate::palette::{Palette, TheaterPalettes, TimeOfDay, VgaPalette};
use crate::settings::{AppState, MissionSetup, Settings};

pub struct SkyPlugin;

impl Plugin for SkyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Sky>()
            .add_systems(OnEnter(AppState::Flight), reset_sky)
            .add_systems(Update, update_sky.run_if(in_state(AppState::Flight)));
    }
}

/// Synodic month (days).
const SYNODIC: f64 = 29.530_588;
/// A campaign's moon moves on this much of its cycle a day, so the nights differ and a full or
/// new moon comes round only every few cycles (`stealth_host::sky`).
pub const CAMPAIGN_PHASE_PER_DAY: f64 = 0.3;
/// Light level for visual identification under a full moon high in the sky (4 km, as the
/// classic night).
const FULL_MOON_LIGHT: f32 = 85.0;

/// Sun, moon and stars as drawn.
#[derive(Resource, Debug, Clone)]
pub struct Sky {
    /// Continuous time of day is on.
    pub on: bool,
    /// Directions (Bevy: x east, y up, z south) to the sun and the moon.
    pub sun: Vec3,
    pub moon: Vec3,
    /// Moonlight on the ground 0..1 (lit fraction, less near the horizon).
    pub moonlight: f32,
    /// Star visibility 0..1.
    pub stars: f32,
    /// Daylight from the sun's elevation 0 (night) .. 1 (day).
    pub day: f32,
    /// Darkness of a night without moonlight 0 .. 1 (black sky and ground, lights and stars
    /// only), and the strength of the red glow it leaves at the horizon.
    pub dark: f32,
    pub glow: f32,
    /// Mission date: days since 1970-01-01 (the moon's phase), and the theater's latitude.
    days: f64,
    lat: f64,
    /// The season 0 (midwinter) .. 1 (midsummer); summer without a mission date.
    summer: f64,
    /// The moon phase chosen for the mission (else the date's).
    chosen: Option<f64>,
    /// A campaign's sky (its moon moves on faster).
    campaign: bool,
    /// Without continuous time of day: the sun or the moon and the stars are drawn anyway
    /// (`Settings::sun_moon`), over the classic palette.
    pub classic_bodies: bool,
    /// The golden hour palette in use and its look.
    golden: Option<(Look, VgaPalette)>,
}

impl Default for Sky {
    fn default() -> Self {
        Self {
            on: false,
            sun: crate::lighting::sun_direction(),
            moon: Vec3::Y,
            moonlight: 0.0,
            stars: 0.0,
            day: 1.0,
            dark: 0.0,
            glow: 0.0,
            days: 0.0,
            lat: 45.0,
            summer: 1.0,
            chosen: None,
            campaign: false,
            classic_bodies: false,
            golden: None,
        }
    }
}

impl Sky {
    /// Light level for the simulation's visual identification ([`stealth_sim::Sim::visual_range`]):
    /// 0 moonless night .. 85 full moon .. 255 day.
    pub fn visual_light(&self) -> u8 {
        let night = self.moonlight * FULL_MOON_LIGHT;
        (night + (255.0 - night) * self.day).round().clamp(0.0, 255.0) as u8
    }

    /// The moon's phase `hours` into the mission date.
    pub fn phase_at(&self, hours: f64) -> f64 {
        match self.chosen {
            Some(p) => p,
            None if self.campaign => {
                (moon_phase(self.days) + CAMPAIGN_PHASE_PER_DAY * hours / 24.0).rem_euclid(1.0)
            }
            None => moon_phase(self.days + hours / 24.0),
        }
    }
}

/// Name of a moon phase 0..1 ("Full moon").
pub fn phase_name(phase: f64) -> &'static str {
    const NAMES: [&str; 8] = [
        "New moon",
        "Waxing crescent",
        "First quarter",
        "Waxing gibbous",
        "Full moon",
        "Waning gibbous",
        "Last quarter",
        "Waning crescent",
    ];
    NAMES[((phase * 8.0 + 0.5) as usize) % 8]
}

/// Direction to the sun at the equinox, `hours` local solar time, at latitude `lat` (degrees),
/// in Bevy's frame (x east, y up, z south).
pub fn sun_dir(hours: f64, lat: f64) -> Vec3 {
    let h = (hours - 12.0) / 24.0 * TAU;
    let phi = lat.to_radians();
    let east = -h.sin();
    let north = -h.cos() * phi.sin();
    let up = h.cos() * phi.cos();
    Vec3::new(east as f32, up as f32, -north as f32)
}

/// Direction to the moon: it trails the sun by the phase (a full moon stands opposite).
pub fn moon_dir(hours: f64, lat: f64, phase: f64) -> Vec3 {
    sun_dir(hours - phase * 24.0, lat)
}

/// Moon phase 0 (new) .. 1 on a day (days since 1970-01-01, fractional), from the new moon of
/// 6 January 2000, 18:14 UTC.
pub fn moon_phase(days: f64) -> f64 {
    let new_moon = 10_962.0 + 18.23 / 24.0;
    ((days - new_moon) / SYNODIC).rem_euclid(1.0)
}

/// Lit fraction of the moon's disc at a phase.
pub fn illumination(phase: f64) -> f64 {
    (1.0 - (phase * TAU).cos()) / 2.0
}

fn smoothstep(e0: f64, e1: f64, x: f64) -> f64 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A golden hour look: the sky from the horizon up (8-bit sRGB: at the horizon, 8 degrees up,
/// from 30 degrees up) and the warm tint of everything else.
struct Golden {
    sky: [[f64; 3]; 3],
    tint: [f64; 3],
}

/// The golden hours' base looks.
const GOLDEN: [Golden; 4] = [
    // Gold.
    Golden {
        sky: [[255.0, 150.0, 30.0], [246.0, 176.0, 92.0], [66.0, 100.0, 168.0]],
        tint: [1.0, 0.78, 0.5],
    },
    // Deep orange.
    Golden {
        sky: [[255.0, 96.0, 18.0], [236.0, 126.0, 72.0], [46.0, 72.0, 146.0]],
        tint: [1.0, 0.7, 0.45],
    },
    // Pink and orange.
    Golden {
        sky: [[255.0, 118.0, 56.0], [236.0, 140.0, 140.0], [76.0, 88.0, 164.0]],
        tint: [1.0, 0.74, 0.6],
    },
    // Yellow gold.
    Golden {
        sky: [[255.0, 190.0, 70.0], [240.0, 200.0, 130.0], [86.0, 130.0, 196.0]],
        tint: [1.0, 0.86, 0.6],
    },
];

/// One dawn's or dusk's golden hour: a base look, how strong its colours are (0.75 .. 1.3)
/// and how far they lean to red (-1) or yellow (+1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    pub base: usize,
    pub intensity: f64,
    pub warmth: f64,
}

impl Look {
    /// The look of a mission's dawn or dusk, rolled from the date (each day has its own), or
    /// the test aid `--sky-golden <base>,<intensity>[,<warmth>]`.
    pub fn of(days: f64, evening: bool) -> Look {
        if let Some(l) = test_look() {
            return l;
        }
        let mut h = (days.floor() as i64 * 2 + evening as i64) as u64;
        let mut next = || {
            h = h.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = h;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64
        };
        let base = (next() * GOLDEN.len() as f64) as usize % GOLDEN.len();
        Look { base, intensity: 0.75 + 0.55 * next(), warmth: next() * 2.0 - 1.0 }
    }
}

fn test_look() -> Option<Look> {
    static L: std::sync::OnceLock<Option<Look>> = std::sync::OnceLock::new();
    *L.get_or_init(|| {
        let a: Vec<String> = std::env::args().collect();
        let v = a.windows(2).find(|w| w[0] == "--sky-golden")?[1].clone();
        let n: Vec<f64> = v.split(',').filter_map(|x| x.parse().ok()).collect();
        Some(Look {
            base: *n.first()? as usize,
            intensity: n.get(1).copied().unwrap_or(1.0),
            warmth: n.get(2).copied().unwrap_or(0.0),
        })
    })
}

/// A colour of a look (8-bit sRGB): its saturation scaled by the intensity, green and blue
/// shifted by the warmth.
fn vary(c: [f64; 3], l: &Look) -> [f64; 3] {
    let grey = (c[0] + c[1] + c[2]) / 3.0;
    let shift = [1.0, 1.0 + 0.14 * l.warmth, 1.0 + 0.1 * l.warmth];
    std::array::from_fn(|ch| ((grey + (c[ch] - grey) * l.intensity) * shift[ch]).clamp(0.0, 255.0))
}

/// The golden hour palette of a look: the day palette with a warm, low-sun sky (colours
/// 0x80.. 0x8F, the bands from the horizon up, each coloured for its elevation) and everything
/// else warmer and a little darker. The fixed low 16 colours (the displays) stay.
pub fn golden(p: &TheaterPalettes, l: &Look) -> VgaPalette {
    let g = &GOLDEN[l.base % GOLDEN.len()];
    let sky = g.sky.map(|c| vary(c, l));
    let tint = vary(g.tint.map(|t| t * 255.0), l).map(|t| t / 255.0);
    let six = |v: f64| (v / 4.0).round().clamp(0.0, 63.0) as u8;
    let colors = p
        .day
        .colors
        .iter()
        .enumerate()
        .map(|(i, c)| {
            if i < 16 {
                *c
            } else if (0x80..0x90).contains(&i) {
                // The band's lower edge (`SKY_RING`: 256 x the tangent of the elevation).
                let el = (crate::palette::SKY_RING[i - 0x80] as f64 / 256.0).atan().to_degrees();
                let (a, b, k) = if el < 8.0 {
                    (sky[0], sky[1], el / 8.0)
                } else {
                    (sky[1], sky[2], ((el - 8.0) / 22.0).min(1.0))
                };
                std::array::from_fn(|ch| six(a[ch] + (b[ch] - a[ch]) * k))
            } else {
                std::array::from_fn(|ch| six(c[ch] as f64 * 4.0 * tint[ch] * 0.86))
            }
        })
        .collect();
    VgaPalette { colors }
}

fn mix(a: &VgaPalette, b: &VgaPalette, t: f64) -> VgaPalette {
    let colors = a
        .colors
        .iter()
        .zip(&b.colors)
        .map(|(x, y)| {
            std::array::from_fn(|c| (x[c] as f64 + (y[c] as f64 - x[c] as f64) * t).round() as u8)
        })
        .collect();
    VgaPalette { colors }
}

/// The palette for a sun elevation (degrees): night below -14; the dusk palette's purple and
/// dark blue at -9 (about 19:00 at the equinox in Central Europe); the golden hour from about
/// -2 to +4 degrees; day above +14. Moonlight lifts the night towards dusk.
pub fn blend(p: &TheaterPalettes, golden: &VgaPalette, elev: f64, moonlight: f64) -> VgaPalette {
    if elev < -9.0 {
        let t = smoothstep(-14.0, -9.0, elev).max(moonlight * 0.25);
        mix(&p.night, &p.dusk, t)
    } else if elev < -2.0 {
        mix(&p.dusk, golden, smoothstep(-9.0, -2.0, elev))
    } else {
        mix(golden, &p.day, smoothstep(4.0, 14.0, elev))
    }
}

/// How dark the night is (0 .. 1) at a sun elevation (degrees): the palette's night under a
/// full moon high in the sky, black when the moon is new or down. It falls with the dusk.
/// The season on a day (days since 1970-01-01) at a latitude: 0 (midwinter) .. 1 (midsummer).
pub fn season(days: f64, lat: f64) -> f64 {
    // The March equinox of 1970 fell on day 78.
    let s = (TAU * (days - 78.0) / 365.2422).sin();
    // Full summer from May to mid August.
    smoothstep(-0.5, 0.6, if lat < 0.0 { -s } else { s })
}

/// Strength of the red glow a dark night leaves at the horizon, by the sun's elevation (degrees).
/// Its last trace lingers: gone about two hours after dusk in winter, at midnight (the sun's
/// lowest, the latitude less 90) in summer, back again for the dawn.
pub fn horizon_glow(elev: f64, lat: f64, summer: f64) -> f64 {
    let end = (-27.0 + (lat - 90.0 + 27.0) * summer).min(-12.0);
    // The sun's height over its lowest grows with the square of the time, so the root of it
    // fades evenly in time.
    2.0 * smoothstep(-18.0, -9.0, elev) + ((elev - end) / (-9.0 - end)).clamp(0.0, 1.0).sqrt()
}

pub fn darkness(elev: f64, moonlight: f64) -> f64 {
    (1.0 - moonlight) * (1.0 - smoothstep(-14.0, -5.0, elev))
}

fn test_arg(name: &str) -> Option<f64> {
    let a: Vec<String> = std::env::args().collect();
    a.windows(2).find(|w| w[0] == name).and_then(|w| w[1].parse().ok())
}

fn test_hours() -> Option<f64> {
    static H: std::sync::OnceLock<Option<f64>> = std::sync::OnceLock::new();
    *H.get_or_init(|| test_arg("--sky-hours"))
}

fn test_phase() -> Option<f64> {
    static P: std::sync::OnceLock<Option<f64>> = std::sync::OnceLock::new();
    *P.get_or_init(|| test_arg("--sky-phase"))
}

fn reset_sky(
    mut sky: ResMut<Sky>,
    setup: Res<MissionSetup>,
    mission: Option<Res<crate::mission::ActiveMission>>,
) {
    let (_, lat0) = crate::world::geo_origin(&setup.theater);
    let date =
        mission.as_ref().and_then(|m| crate::tacview::parse_briefing_date(&m.0.briefing.date));
    let days = date.map_or(0.0, |(y, mo, d)| crate::tacview::days_from_civil(y, mo, d) as f64);
    let chosen = mission.and_then(|m| m.0.settings.moon).map(|e| (e % 8) as f64 / 8.0);
    // The middle of the theater, 256 km north of its corner.
    let lat = lat0 + 256.0 / 111.2;
    let summer = date.map_or(1.0, |_| season(days, lat));
    *sky = Sky { lat, days, summer, chosen, ..default() };
}

/// The sun and the moon in the classic time of day: by day the sun at the classic light
/// direction; by night the moon of the mission's phase where it stands at 23:00, and the stars.
fn classic_bodies(sky: &mut Sky, on: bool, night: bool) {
    sky.classic_bodies = on;
    // The classic palette is the sky: no darkness or glow over it.
    (sky.dark, sky.glow) = (0.0, 0.0);
    if !night {
        (sky.sun, sky.moon, sky.stars) = (crate::lighting::sun_direction(), Vec3::NEG_Y, 0.0);
        return;
    }
    let hours = test_hours().unwrap_or(23.0);
    let phase = test_phase().unwrap_or_else(|| sky.phase_at(hours));
    sky.sun = sun_dir(hours, sky.lat);
    sky.moon = moon_dir(hours, sky.lat, phase);
    let moonlight = illumination(phase) * smoothstep(0.0, 0.25, sky.moon.y as f64);
    sky.stars = (1.0 - 0.6 * moonlight) as f32;
}

fn update_sky(
    settings: Res<Settings>,
    clock: Res<crate::mfd::MissionClock>,
    tod: Res<TimeOfDay>,
    mut sky: ResMut<Sky>,
    palette: Option<ResMut<Palette>>,
    combat: Option<ResMut<crate::combat::Combat>>,
) {
    let on = settings.continuous_time;
    sky.campaign = combat.as_ref().is_some_and(|c| c.sky_seconds.is_some());
    if !on {
        if sky.on {
            sky.on = false;
            sky.sun = crate::lighting::sun_direction();
            sky.stars = 0.0;
        }
        // A campaign's light is its host's (`stealth_host`), by its rules.
        if let Some(mut c) = combat.filter(|c| c.sim.campaign().is_none()) {
            c.sim.set_visual_light(None);
        }
        classic_bodies(&mut sky, settings.sun_moon, tod.night_mission);
        return;
    }
    sky.classic_bodies = false;
    let seconds = clock.start_seconds as f64 + clock.elapsed.unwrap_or(0.0) as f64;
    // Test aids: `--sky-hours <h>` sets the local time, `--sky-phase <0..1>` the moon's phase.
    let hours = test_hours().unwrap_or(seconds / 3600.0);
    let phase = test_phase().unwrap_or_else(|| sky.phase_at(hours));
    let sun = sun_dir(hours, sky.lat);
    let moon = moon_dir(hours, sky.lat, phase);
    let elev = (sun.y as f64).asin().to_degrees();
    let lit = illumination(phase);
    let moonlight = lit * smoothstep(0.0, 0.25, moon.y as f64);
    let day = smoothstep(-12.0, 6.0, elev);
    *sky = Sky {
        on,
        sun,
        moon,
        moonlight: moonlight as f32,
        stars: ((1.0 - smoothstep(-14.0, -4.0, elev)) * (1.0 - 0.6 * moonlight)) as f32,
        day: day as f32,
        dark: darkness(elev, moonlight) as f32,
        glow: horizon_glow(elev, sky.lat, sky.summer) as f32,
        ..sky.clone()
    };
    // Alt-N's jump to the target palette wins.
    if let Some(mut palette) = palette
        && !tod.frozen
    {
        let look = Look::of(sky.days, hours.rem_euclid(24.0) >= 12.0);
        if sky.golden.as_ref().is_none_or(|(l, _)| *l != look) {
            sky.golden = Some((look, golden(&palette.theater, &look)));
        }
        let Some((_, gold)) = &sky.golden else { return };
        let p = blend(&palette.theater, gold, elev, moonlight);
        if p.colors != palette.active.colors {
            palette.active = p;
        }
    }
    if let Some(mut c) = combat.filter(|c| c.sim.campaign().is_none()) {
        let light = sky.visual_light();
        c.sim.set_visual_light(Some(light));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn horizon_glow_lingers_in_summer() {
        let glow = |hours: f64, summer: f64| {
            let elev = (sun_dir(hours, 50.0).y as f64).asin().to_degrees();
            horizon_glow(elev, 50.0, summer)
        };
        for h in [19.0, 20.0, 21.0, 22.0, 23.0, 23.5, 24.0] {
            eprintln!("{h}: winter {:.4} summer {:.4}", glow(h, 0.0), glow(h, 1.0));
        }
        assert!(glow(21.0, 0.0) == 0.0 && glow(23.0, 1.0) > 0.0);
        assert!(glow(24.0, 1.0) < 1e-6);
        // Midwinter and midsummer dates in the northern hemisphere.
        let day = |m, d| crate::tacview::days_from_civil(1991, m, d) as f64;
        assert!(season(day(12, 21), 50.0) < 0.01 && season(day(6, 21), 50.0) > 0.99);
    }

    #[test]
    fn equinox_sun() {
        // Noon: due south, 90 - latitude high; midnight: as far below; 06:00 rises in the east.
        let noon = sun_dir(12.0, 50.0);
        assert!((noon.y - 40f32.to_radians().sin()).abs() < 1e-5);
        assert!(noon.z > 0.0 && noon.x.abs() < 1e-6);
        assert!((sun_dir(0.0, 50.0).y + noon.y).abs() < 1e-5);
        let rise = sun_dir(6.0, 50.0);
        assert!(rise.y.abs() < 1e-6 && rise.x > 0.99);
        assert!(sun_dir(18.0, 50.0).x < -0.99);
    }

    #[test]
    fn moon_phases() {
        // Full moon of 21 January 2000 (04:40 UTC); new moon 6 days after 21 Oct 2026's.
        let full = crate::tacview::days_from_civil(2000, 1, 21) as f64 + 4.67 / 24.0;
        assert!((moon_phase(full) - 0.5).abs() < 0.02);
        assert!(illumination(0.5) > 0.999 && illumination(0.0) < 1e-9);
        // A full moon is up at midnight, high in the south.
        let m = moon_dir(0.0, 50.0, 0.5);
        assert!(m.y > 0.6 && m.z > 0.0);
        assert_eq!(phase_name(0.5), "Full moon");
        assert_eq!(phase_name(0.98), "New moon");
    }

    #[test]
    fn campaign_moon() {
        // 30% of the cycle a night; full and new only after more than a cycle; the client's
        // 30-day wrap of the sky clock is 9 whole cycles.
        let sky = Sky { days: 10_962.76, campaign: true, ..default() };
        let nights: Vec<f64> = (0..11).map(|n| sky.phase_at(24.0 * n as f64)).collect();
        assert!(nights[0] < 1e-3);
        assert!((nights[1] - 0.3).abs() < 1e-3 && (nights[5] - 0.5).abs() < 1e-3);
        assert!(nights[1..5].iter().all(|p| phase_name(*p) != "Full moon"));
        assert!(nights[1..10].iter().all(|p| phase_name(*p) != "New moon"));
        assert!((sky.phase_at(30.0 * 24.0 + 5.0) - sky.phase_at(5.0)).abs() < 1e-9);
        let chosen = Sky { chosen: Some(0.5), campaign: true, ..default() };
        assert_eq!(chosen.phase_at(100.0), 0.5);
    }

    #[test]
    fn golden_hours() {
        let pal = |v: u8| VgaPalette { colors: vec![[v; 3]; 256] };
        let p = TheaterPalettes { day: pal(60), dusk: pal(20), night: pal(2) };
        let g = golden(&p, &Look { base: 0, intensity: 1.0, warmth: 0.0 });
        // A warm horizon, a blue zenith, the displays' colours untouched.
        assert!(g.colors[0x80][0] > g.colors[0x80][2]);
        assert!(g.colors[0x8F][2] > g.colors[0x8F][0]);
        assert_eq!(g.colors[3], [60; 3]);
        assert_eq!(blend(&p, &g, -30.0, 0.0).colors, p.night.colors);
        assert_eq!(blend(&p, &g, -9.0, 0.0).colors, p.dusk.colors);
        assert_eq!(blend(&p, &g, 0.0, 0.0).colors, g.colors);
        assert_eq!(blend(&p, &g, 30.0, 0.0).colors, p.day.colors);
        // 19:00 at the equinox in Central Europe is near full dusk.
        let e19 = (sun_dir(19.0, 52.6).y as f64).asin().to_degrees();
        assert!((-10.0..-8.0).contains(&e19), "{e19}");
        let looks: Vec<Look> = (0..40).map(|d| Look::of(d as f64 + 9000.0, d % 2 == 0)).collect();
        let bases: std::collections::HashSet<_> = looks.iter().map(|l| l.base).collect();
        assert!(bases.len() > 2);
        assert!(looks.iter().all(|l| (0.75..=1.3).contains(&l.intensity)));
        assert!(looks.iter().any(|l| l.warmth < -0.5) && looks.iter().any(|l| l.warmth > 0.5));
        // A stronger look is more saturated at the horizon.
        let strong = golden(&p, &Look { base: 0, intensity: 1.3, warmth: 0.0 });
        let h = |p: &VgaPalette| p.colors[0x80][0] as i32 - p.colors[0x80][2] as i32;
        assert!(h(&strong) >= h(&g));
    }

    #[test]
    fn moonless_darkness() {
        assert_eq!(darkness(-30.0, 0.0), 1.0);
        assert_eq!(darkness(-30.0, 1.0), 0.0);
        assert!((darkness(-30.0, 0.5) - 0.5).abs() < 1e-9);
        assert_eq!(darkness(0.0, 0.0), 0.0);
        assert!(darkness(-9.0, 0.0) > 0.2 && darkness(-9.0, 0.0) < 0.8);
    }

    #[test]
    fn light_levels() {
        let mut sky = Sky { day: 0.0, moonlight: 0.0, ..default() };
        assert_eq!(sky.visual_light(), 0);
        sky.moonlight = 1.0;
        assert_eq!(sky.visual_light(), 85);
        sky.day = 1.0;
        assert_eq!(sky.visual_light(), 255);
    }
}
