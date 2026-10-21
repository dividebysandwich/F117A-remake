//! The light enemy pilots see by (the renderer's continuous time of day, `src/sky.rs`), for a
//! host without one.

use std::f64::consts::TAU;

/// Synodic month (days).
const SYNODIC: f64 = 29.530_588;
/// Light level under a full moon high in the sky.
const FULL_MOON_LIGHT: f64 = 85.0;

/// Latitude (degrees) of the middle of a theater.
pub fn latitude(code: &str) -> f64 {
    let lat0 = match code {
        "ce" => 50.34,
        "cu" => 19.15,
        "ko" => 38.26,
        "ku" => 30.05,
        "lb" => 30.24,
        "me" => 32.62,
        "nc" => 66.10,
        "pg" => 26.48,
        "ua" => 44.78,
        "vn" => 17.04,
        _ => 45.0,
    };
    lat0 + 256.0 / 111.2
}

/// Days since 1970-01-01 of a civil date.
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// A briefing date ("Jun 14, 1989") as (year, month, day).
pub fn briefing_date(text: &str) -> (i64, i64, i64) {
    const MONTHS: [&str; 12] =
        ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
    let lower = text.to_ascii_lowercase();
    let words: Vec<&str> = lower.split([' ', ',']).filter(|w| !w.is_empty()).collect();
    let parsed = (|| {
        let first = words.first()?;
        let m = MONTHS.iter().position(|m| first.starts_with(m))?;
        Some((words.get(2)?.parse().ok()?, m as i64 + 1, words.get(1)?.parse().ok()?))
    })();
    parsed.unwrap_or((1990, 1, 1))
}

fn smoothstep(e0: f64, e1: f64, x: f64) -> f64 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Height of a body over the horizon (sine), `hours` local solar time at the equinox.
fn sun_up(hours: f64, lat: f64) -> f64 {
    let h = (hours - 12.0) / 24.0 * TAU;
    h.cos() * lat.to_radians().cos()
}

/// The light for the simulation's visual identification (0 moonless night .. 255 day) at
/// `hours` of day `days` (since 1970) at latitude `lat`.
pub fn visual_light(days: f64, hours: f64, lat: f64) -> u8 {
    let new_moon = 10_962.0 + 18.23 / 24.0;
    let phase = ((days + hours / 24.0 - new_moon) / SYNODIC).rem_euclid(1.0);
    let elev = sun_up(hours, lat).asin().to_degrees();
    let lit = (1.0 - (phase * TAU).cos()) / 2.0;
    let moonlight = lit * smoothstep(0.0, 0.25, sun_up(hours - phase * 24.0, lat));
    let day = smoothstep(-12.0, 6.0, elev);
    let night = moonlight * FULL_MOON_LIGHT;
    (night + (255.0 - night) * day).round().clamp(0.0, 255.0) as u8
}
