//! Continuous time of day (the "Time of day" switch, `--continuous-time`): the sun and the moon
//! move with the mission clock, the palette blends between the theater's night, dusk and day
//! palettes by the sun's elevation, the moon shows the phase of the briefing date, stars come
//! out, and moonlight sets how far enemy pilots see at night. Off, the original's palette drift
//! and fixed sun stay (`palette::step_time_of_day`).
//!
//! The sun keeps to the equinox (it rises at 06:00 and sets at 18:00 local time everywhere), so
//! day missions still start at dawn and night missions at dusk, as in the original.

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
/// Light level for visual identification under a full moon high in the sky (4 km, as the
/// original's night).
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
    /// Mission date: days since 1970-01-01 (the moon's phase), and the theater's latitude.
    days: f64,
    lat: f64,
    /// The golden hour palette in use and its variant.
    golden: Option<(usize, VgaPalette)>,
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
            days: 0.0,
            lat: 45.0,
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

/// The golden hours' looks; a mission's dawn and dusk each pick one by the date.
const GOLDEN: [Golden; 4] = [
    // Gold.
    Golden { sky: [[255.0, 172.0, 64.0], [236.0, 188.0, 124.0], [72.0, 112.0, 172.0]], tint: [1.0, 0.84, 0.62] },
    // Deep orange.
    Golden { sky: [[255.0, 118.0, 36.0], [222.0, 142.0, 104.0], [52.0, 82.0, 150.0]], tint: [1.0, 0.76, 0.55] },
    // Pink and gold.
    Golden { sky: [[255.0, 150.0, 92.0], [228.0, 158.0, 168.0], [82.0, 100.0, 170.0]], tint: [1.0, 0.8, 0.7] },
    // Pale yellow haze.
    Golden { sky: [[255.0, 212.0, 124.0], [222.0, 212.0, 172.0], [92.0, 140.0, 200.0]], tint: [1.0, 0.9, 0.72] },
];

/// The golden hour palette `variant` of a theater: the day palette with a warm, low-sun sky
/// (colours 0x80.. 0x8F, the bands from the horizon up, each coloured for its elevation) and
/// everything else warmer and a little darker.
/// The fixed low 16 colours (the displays) stay.
pub fn golden(p: &TheaterPalettes, variant: usize) -> VgaPalette {
    let g = &GOLDEN[variant % GOLDEN.len()];
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
                    (g.sky[0], g.sky[1], el / 8.0)
                } else {
                    (g.sky[1], g.sky[2], ((el - 8.0) / 22.0).min(1.0))
                };
                std::array::from_fn(|ch| six(a[ch] + (b[ch] - a[ch]) * k))
            } else {
                std::array::from_fn(|ch| six(c[ch] as f64 * 4.0 * g.tint[ch] * 0.88))
            }
        })
        .collect();
    VgaPalette { colors }
}

/// Golden hour look of a mission's dawn or dusk (by the date, so each day has its own).
pub fn golden_variant(days: f64, evening: bool) -> usize {
    let d = days.floor() as i64 * 2 + evening as i64;
    (d.wrapping_mul(0x9E37_79B9).rem_euclid(1 << 30) >> 7) as usize % GOLDEN.len()
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

fn test_hours() -> Option<f64> {
    static H: std::sync::OnceLock<Option<f64>> = std::sync::OnceLock::new();
    *H.get_or_init(|| {
        let a: Vec<String> = std::env::args().collect();
        a.windows(2).find(|w| w[0] == "--sky-hours").and_then(|w| w[1].parse().ok())
    })
}

fn reset_sky(
    mut sky: ResMut<Sky>,
    setup: Res<MissionSetup>,
    mission: Option<Res<crate::mission::ActiveMission>>,
) {
    let (_, lat0) = crate::world::geo_origin(&setup.theater);
    let days = mission.map_or(0.0, |m| {
        let (y, mo, d) = crate::tacview::briefing_date(&m.0.briefing.date);
        crate::tacview::days_from_civil(y, mo, d) as f64
    });
    // The middle of the theater, 256 km north of its corner.
    *sky = Sky { lat: lat0 + 256.0 / 111.2, days, ..default() };
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
    if !on {
        if sky.on {
            sky.on = false;
            sky.sun = crate::lighting::sun_direction();
            sky.stars = 0.0;
        }
        if let Some(mut c) = combat {
            c.sim.set_visual_light(None);
        }
        return;
    }
    let seconds = clock.start_seconds as f64 + clock.elapsed.unwrap_or(0.0) as f64;
    // Test aid: `--sky-hours <h>` sets the local time.
    let hours = test_hours().unwrap_or(seconds / 3600.0);
    let phase = moon_phase(sky.days + hours / 24.0);
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
        ..sky.clone()
    };
    // Alt-N's jump to the target palette wins.
    if let Some(mut palette) = palette
        && !tod.frozen
    {
        let variant = golden_variant(sky.days, hours.rem_euclid(24.0) >= 12.0);
        if sky.golden.as_ref().is_none_or(|(v, _)| *v != variant) {
            sky.golden = Some((variant, golden(&palette.theater, variant)));
        }
        let Some((_, gold)) = &sky.golden else { return };
        let p = blend(&palette.theater, gold, elev, moonlight);
        if p.colors != palette.active.colors {
            palette.active = p;
        }
    }
    if let Some(mut c) = combat {
        let light = sky.visual_light();
        c.sim.set_visual_light(Some(light));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn golden_hours() {
        let pal = |v: u8| VgaPalette { colors: vec![[v; 3]; 256] };
        let p = TheaterPalettes { day: pal(60), dusk: pal(20), night: pal(2) };
        let g = golden(&p, 0);
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
        let used: std::collections::HashSet<_> =
            (0..40).map(|d| golden_variant(d as f64 + 9000.0, d % 2 == 0)).collect();
        assert!(used.len() > 2);
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
