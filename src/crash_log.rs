//! Diagnostics for crashes that leave no trace (seen on the Steam Frame): a status line every
//! few seconds (memory, temperature, frame times, entity and asset counts, the player's flight
//! state), a line for every long frame, and a last line when the process dies from a signal or
//! a panic. A log that ends without a "fatal" line means the process was killed with SIGKILL
//! (usually the out-of-memory killer; `journalctl -k` shows it).

use bevy::prelude::*;

use crate::flight::{Aircraft, Player};
use crate::settings::AppState;

/// Seconds between status lines.
const INTERVAL: f64 = 5.0;
/// Frames longer than this are logged at once (GPU stalls before a device loss).
const HITCH: f64 = 0.25;

pub struct CrashLogPlugin;

impl Plugin for CrashLogPlugin {
    fn build(&self, app: &mut App) {
        info!("diag: args {:?}", std::env::args().skip(1).collect::<Vec<_>>());
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
        error!("fatal: panic in thread {:?}: {info}\n{}", thread.name().unwrap_or("?"), std::backtrace::Backtrace::force_capture());
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
    player: Query<&Aircraft, With<Player>>,
) {
    let now = time.elapsed_secs_f64();
    let dt = time.delta_secs_f64();
    stats.frames += 1;
    stats.worst = stats.worst.max(dt);
    if dt > HITCH && stats.frames > 1 {
        warn!("diag: long frame {:.0} ms", dt * 1000.0);
    }
    let state = state.map(|s| *s.get());
    if state != stats.state {
        info!("diag: state {:?}", state);
        stats.state = state;
    }
    if now - stats.last < INTERVAL {
        return;
    }
    let fps = stats.frames as f64 / (now - stats.last);
    let flight = player.single().map_or(String::new(), |a| {
        format!(" | alt {:.0} ft speed {:.0} kt ground {}", a.altitude, a.velocity / 27.0, a.on_ground)
    });
    info!(
        "diag: {:.0} fps worst {:.0} ms | rss {} MB avail {} MB swap free {} MB | temp {} | entities {} meshes {} images {} materials {}{}",
        fps,
        stats.worst * 1000.0,
        status_kb("VmRSS").map_or(-1, |k| k / 1024),
        meminfo_kb("MemAvailable").map_or(-1, |k| k / 1024),
        meminfo_kb("SwapFree").map_or(-1, |k| k / 1024),
        max_temp().map_or("?".into(), |t| format!("{t:.0} C")),
        entities.iter().count(),
        meshes.len(),
        images.len(),
        materials.len(),
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
        .filter_map(|e| std::fs::read_to_string(e.path().join("temp")).ok()?.trim().parse::<f64>().ok())
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
        if unsafe { libc::pthread_getname_np(libc::pthread_self(), thread.as_mut_ptr(), thread.len()) } == 0 {
            let len = thread.iter().position(|&c| c == 0).unwrap_or(thread.len());
            // SAFETY: c_char and u8 have the same layout.
            n += copy(&mut buf[n..], unsafe { std::slice::from_raw_parts(thread.as_ptr().cast(), len) });
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
        for sig in [libc::SIGSEGV, libc::SIGBUS, libc::SIGABRT, libc::SIGILL, libc::SIGFPE, libc::SIGTERM, libc::SIGHUP] {
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
