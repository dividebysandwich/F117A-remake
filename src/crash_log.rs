//! Diagnostics for crashes that leave no trace (seen on the Steam Frame): a status line every
//! few seconds (memory, temperature, frame times, entity and asset counts, the player's flight
//! state), a line for every long frame followed by the last frames' history (head direction
//! relative to the nose, entity and mesh churn), and a last line when the process dies from a
//! signal or a panic. A log that ends without a "fatal" line means the process was killed with SIGKILL
//! (usually the out-of-memory killer; `journalctl -k` shows it).
//! Everything but the fatal line is logged at debug level: run with
//! `RUST_LOG=info,stealth_mission_89::crash_log=debug` to see it.

use std::collections::VecDeque;

use bevy::prelude::*;

use crate::camera::OffscreenCamera;
use crate::flight::{Aircraft, Player};
use crate::settings::AppState;

/// Seconds between status lines.
const INTERVAL: f64 = 5.0;
/// Frames longer than this are logged at once (GPU stalls before a device loss).
const HITCH: f64 = 0.25;
/// Frames of history printed after a long frame.
const HISTORY: usize = 40;

pub struct CrashLogPlugin;

impl Plugin for CrashLogPlugin {
    fn build(&self, app: &mut App) {
        debug!("diag: args {:?}", std::env::args().skip(1).collect::<Vec<_>>());
        install();
        app.init_resource::<Stats>().add_systems(Last, status);
    }
}

/// Installs the panic hook and the fatal signal handlers (after the default plugins, so neither
/// is replaced by theirs).
fn install() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        error!(
            "fatal: panic in thread {:?}: {info}\n{}",
            thread.name().unwrap_or("?"),
            std::backtrace::Backtrace::force_capture()
        );
        default(info);
    }));
    #[cfg(unix)]
    signals::install();
}

#[derive(Resource, Default)]
struct Stats {
    last: f64,
    frames: u32,
    worst: f64,
    state: Option<AppState>,
    history: VecDeque<Frame>,
    frame: u64,
}

/// One frame of the history.
struct Frame {
    n: u64,
    ms: f64,
    /// Head direction relative to the nose (degrees: right, up).
    look: Option<(f32, f32)>,
    entities: usize,
    meshes: usize,
    alt: f32,
    knots: f32,
}

impl Frame {
    fn line(&self, prev: Option<&Frame>) -> String {
        let delta = |a: usize, b: Option<usize>| {
            b.map_or(String::new(), |b| format!("{:+}", a as i64 - b as i64))
        };
        format!(
            "#{} {:.0} ms look {} | entities {}{} meshes {}{} | alt {:.0} ft {:.0} kt",
            self.n,
            self.ms,
            self.look.map_or("?".into(), |(r, u)| format!("right {r:.0} up {u:.0}")),
            self.entities,
            delta(self.entities, prev.map(|p| p.entities)),
            self.meshes,
            delta(self.meshes, prev.map(|p| p.meshes)),
            self.alt,
            self.knots,
        )
    }
}

/// Direction of the first active view camera relative to the aircraft's nose.
fn look(
    a: &Aircraft,
    cameras: &Query<(&Camera, &GlobalTransform), (With<Camera3d>, Without<OffscreenCamera>)>,
) -> Option<(f32, f32)> {
    let (_, tf) = cameras.iter().filter(|(c, _)| c.is_active).max_by_key(|(c, _)| c.order)?;
    let d = a.orientation.inverse() * tf.forward().as_vec3();
    Some((d.x.atan2(-d.z).to_degrees(), d.y.clamp(-1.0, 1.0).asin().to_degrees()))
}

#[allow(clippy::too_many_arguments)]
fn status(
    mut stats: ResMut<Stats>,
    time: Res<Time<Real>>,
    state: Option<Res<State<AppState>>>,
    entities: Query<()>,
    meshes: Res<Assets<Mesh>>,
    images: Res<Assets<Image>>,
    materials: Res<Assets<StandardMaterial>>,
    cockpit_materials: Res<Assets<crate::cockpit3d::CockpitMaterial>>,
    player: Query<&Aircraft, With<Player>>,
    cameras: Query<(&Camera, &GlobalTransform), (With<Camera3d>, Without<OffscreenCamera>)>,
    lamps: Query<(&crate::scenery::LampBatch, &InheritedVisibility)>,
) {
    let now = time.elapsed_secs_f64();
    let dt = time.delta_secs_f64();
    stats.frames += 1;
    stats.frame += 1;
    stats.worst = stats.worst.max(dt);
    let a = player.single().ok();
    let frame = Frame {
        n: stats.frame,
        ms: dt * 1000.0,
        look: a.and_then(|a| look(a, &cameras)),
        entities: entities.iter().count(),
        meshes: meshes.len(),
        alt: a.map_or(0.0, |a| a.altitude),
        knots: a.map_or(0.0, |a| a.velocity / 27.0),
    };
    if stats.history.len() == HISTORY {
        stats.history.pop_front();
    }
    stats.history.push_back(frame);
    if dt > HITCH && stats.frames > 1 {
        debug!("diag: long frame {:.0} ms; the frames before it:", dt * 1000.0);
        let mut prev = None;
        for f in &stats.history {
            debug!("diag:   {}", f.line(prev));
            prev = Some(f);
        }
    }
    let state = state.map(|s| *s.get());
    if state != stats.state {
        debug!("diag: state {:?}", state);
        stats.state = state;
    }
    if now - stats.last < INTERVAL {
        return;
    }
    let fps = stats.frames as f64 / (now - stats.last);
    let flight = a.map_or(String::new(), |a| {
        let look = look(a, &cameras).map_or("?".into(), |(r, u)| format!("right {r:.0} up {u:.0}"));
        format!(
            " | alt {:.0} ft speed {:.0} kt ground {} look {look}",
            a.altitude,
            a.velocity / 27.0,
            a.on_ground
        )
    });
    // Night: every lit scenery lamp is a quad (two triangles) in every view.
    let (batches, dots) = lamps
        .iter()
        .filter(|(_, v)| v.get())
        .fold((0, 0), |(b, d), (l, _)| (b + 1, d + l.points.len()));
    debug!(
        "diag: {:.0} fps worst {:.0} ms | rss {} MB avail {} MB swap free {} MB | temp {} | entities {} meshes {} images {} materials {} | lit lamps {dots} in {batches} batches{}",
        fps,
        stats.worst * 1000.0,
        status_kb("VmRSS").map_or(-1, |k| k / 1024),
        meminfo_kb("MemAvailable").map_or(-1, |k| k / 1024),
        meminfo_kb("SwapFree").map_or(-1, |k| k / 1024),
        max_temp().map_or("?".into(), |t| format!("{t:.0} C")),
        stats.history.back().map_or(0, |f| f.entities),
        meshes.len(),
        images.len(),
        materials.len() + cockpit_materials.len(),
        flight,
    );
    stats.last = now;
    stats.frames = 0;
    stats.worst = 0.0;
}

fn field_kb(file: &str, key: &str) -> Option<i64> {
    let text = std::fs::read_to_string(file).ok()?;
    let line = text.lines().find(|l| l.starts_with(key) && l[key.len()..].starts_with(':'))?;
    line[key.len() + 1..].split_whitespace().next()?.parse().ok()
}

fn status_kb(key: &str) -> Option<i64> {
    field_kb("/proc/self/status", key)
}

fn meminfo_kb(key: &str) -> Option<i64> {
    field_kb("/proc/meminfo", key)
}

/// Hottest thermal zone (degrees C).
fn max_temp() -> Option<f64> {
    std::fs::read_dir("/sys/class/thermal")
        .ok()?
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("thermal_zone"))
        .filter_map(|e| {
            std::fs::read_to_string(e.path().join("temp")).ok()?.trim().parse::<f64>().ok()
        })
        .map(|m| m / 1000.0)
        .reduce(f64::max)
}

#[cfg(unix)]
mod signals {
    /// Writes one line to stderr and re-raises the signal with the default action. Only
    /// async-signal-safe calls: no allocation, no locks.
    extern "C" fn handler(sig: libc::c_int, info: *mut libc::siginfo_t, _: *mut libc::c_void) {
        let name: &[u8] = match sig {
            libc::SIGSEGV => b"SIGSEGV (invalid memory access)",
            libc::SIGBUS => b"SIGBUS (bus error)",
            libc::SIGABRT => b"SIGABRT (abort, often from a driver or the allocator)",
            libc::SIGILL => b"SIGILL (illegal instruction)",
            libc::SIGFPE => b"SIGFPE (arithmetic error)",
            libc::SIGTERM => b"SIGTERM (terminated by another process)",
            libc::SIGHUP => b"SIGHUP (hangup)",
            _ => b"signal",
        };
        let mut buf = [0u8; 256];
        let mut n = 0;
        for part in [b"fatal: ".as_slice(), name, b" in thread "] {
            n += copy(&mut buf[n..], part);
        }
        let mut thread = [0 as libc::c_char; 32];
        // SAFETY: the buffer is valid for its length; pthread_getname_np only reads the kernel's comm.
        if unsafe {
            libc::pthread_getname_np(libc::pthread_self(), thread.as_mut_ptr(), thread.len())
        } == 0
        {
            let len = thread.iter().position(|&c| c == 0).unwrap_or(thread.len());
            // SAFETY: c_char and u8 have the same layout.
            n += copy(&mut buf[n..], unsafe {
                std::slice::from_raw_parts(thread.as_ptr().cast(), len)
            });
        }
        if matches!(sig, libc::SIGSEGV | libc::SIGBUS) && !info.is_null() {
            n += copy(&mut buf[n..], b" at 0x");
            // SAFETY: the kernel passes a valid siginfo for SA_SIGINFO handlers.
            n += hex(&mut buf[n..], unsafe { (*info).si_addr() } as usize);
        }
        n += copy(&mut buf[n..], b"\n");
        // SAFETY: write, signal and raise are async-signal-safe.
        unsafe {
            libc::write(2, buf.as_ptr().cast(), n);
            libc::signal(sig, libc::SIG_DFL);
            libc::raise(sig);
        }
    }

    fn copy(dst: &mut [u8], src: &[u8]) -> usize {
        let n = src.len().min(dst.len());
        dst[..n].copy_from_slice(&src[..n]);
        n
    }

    fn hex(dst: &mut [u8], v: usize) -> usize {
        let mut digits = [0u8; 16];
        let mut n = 0;
        let mut v = v;
        loop {
            digits[n] = b"0123456789abcdef"[v & 15];
            n += 1;
            v >>= 4;
            if v == 0 {
                break;
            }
        }
        digits[..n].reverse();
        copy(dst, &digits[..n])
    }

    pub fn install() {
        for sig in [
            libc::SIGSEGV,
            libc::SIGBUS,
            libc::SIGABRT,
            libc::SIGILL,
            libc::SIGFPE,
            libc::SIGTERM,
            libc::SIGHUP,
        ] {
            // SAFETY: a zeroed sigaction with our handler; SA_ONSTACK uses the alternate stacks
            // std sets up for its threads, so stack overflows still get their line.
            unsafe {
                let mut action: libc::sigaction = std::mem::zeroed();
                action.sa_sigaction = handler as *const () as usize;
                action.sa_flags = libc::SA_SIGINFO | libc::SA_ONSTACK;
                libc::sigemptyset(&mut action.sa_mask);
                libc::sigaction(sig, &action, std::ptr::null_mut());
            }
        }
    }
}
