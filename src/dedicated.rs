//! `--dedicated`: the coop server without a window (`stealth_net::dedicated`), stopped with
//! Ctrl-C or a termination signal (it saves the campaign first).

use std::sync::atomic::{AtomicBool, Ordering};

static STOP: AtomicBool = AtomicBool::new(false);

const USAGE: &str = "\
stealth-mission-89 --dedicated [switches]

  --name <text>          server name (shown to the players)
  --port <n>             UDP port (default 28989)
  --max-players <n>      1..8 (default 8)
  --theater <code>       ce, cu, ko, ku, lb, me, nc, pg, ua, vn (default: the saved
                         campaign's, else ce; another theater than the saved one starts
                         a new campaign and keeps the old save aside)
  --roe <0..2>           cold war .. conventional war (default 1, new campaigns)
  --opponents <0..3>     enemy skill (default 1, new campaigns)
  --seed <n>             campaign seed (new campaigns)
  --sky-ratio <x>        the sky runs x times faster than real time (default 24)
  --start-hour <h>       hour of the day at the start (default 8, new campaigns)
  --save <file>          the campaign file (default: campaigns/dedicated.json in the
                         game's data directory), saved every minute and on stopping
  --realistic-damage     realistic missile damage
  --realistic-guidance   realistic missile guidance
  --no-crashes           collisions bounce, crashes do not end the flight
  --continuous-time      moving sun and moon; moonlight sets the enemy's visual range
  --realistic-weapons    the F-117A as it was: no cannon, two bays
  --plain                log lines instead of the console screen (UTC timestamps; the screen is shown
                         when the output is a terminal; q or Ctrl-C stops the server)
";

/// Runs the server and exits when the command line asks for one.
pub fn run_if_asked() {
    let args: Vec<String> = std::env::args().collect();
    if !args.iter().any(|a| a == "--dedicated") {
        return;
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{USAGE}");
        std::process::exit(0);
    }
    on_stop();
    let code = match stealth_net::dedicated::Config::from_args(&args)
        .and_then(|cfg| stealth_net::dedicated::run(cfg, &STOP))
    {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("dedicated server: {e:#}");
            1
        }
    };
    std::process::exit(code);
}

#[cfg(unix)]
fn on_stop() {
    extern "C" fn stop(_: libc::c_int) {
        STOP.store(true, Ordering::Relaxed);
    }
    // SAFETY: the handler only stores to an atomic, which is async-signal-safe.
    unsafe {
        libc::signal(libc::SIGINT, stop as *const () as libc::sighandler_t);
        libc::signal(libc::SIGTERM, stop as *const () as libc::sighandler_t);
    }
}

#[cfg(windows)]
fn on_stop() {
    unsafe extern "system" fn stop(_: u32) -> windows_sys::core::BOOL {
        STOP.store(true, Ordering::Relaxed);
        1
    }
    // SAFETY: the handler only stores to an atomic.
    unsafe {
        windows_sys::Win32::System::Console::SetConsoleCtrlHandler(Some(stop), 1);
    }
}

#[cfg(not(any(unix, windows)))]
fn on_stop() {}
