//! The dedicated server: the campaign and its simulation without a player of its own or a
//! renderer, for players who join it.
//!
//! The campaign is saved every minute and on stopping, and goes on from the save when the
//! server starts again, unless the theater asked for is another one: then a new campaign
//! starts (the old save is kept aside).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use stealth_campaign::{CampaignEvent, CampaignSetup};
use stealth_host::{Host, HostEvent, Rules, save, theater};

use crate::console::{Console, Status};
use crate::protocol::{DEFAULT_PORT, MAX_PLAYERS};
use crate::server::{SEND_EVERY, Server};

/// Seconds between saves.
const SAVE_EVERY: f32 = 60.0;
/// Frames per second of the simulation.
const FS: u8 = 15;

/// The server's settings (`--dedicated` and the switches after it).
#[derive(Debug, Clone)]
pub struct Config {
    pub name: String,
    pub port: u16,
    pub max_players: usize,
    /// None: the saved campaign's, else Central Europe.
    pub theater: Option<String>,
    pub roe: u8,
    pub opponents: u8,
    pub seed: Option<u32>,
    pub sky_ratio: f32,
    pub start_hour: f32,
    pub rules: Rules,
    pub save: PathBuf,
    /// Simulation frames between the moving world's messages (`--send-rate`, a second).
    pub send_every: u32,
    /// The server list's master server (None: not listed, `--no-master`).
    pub master: Option<String>,
    /// Plain log lines instead of the terminal screen.
    pub plain: bool,
}

impl Config {
    /// From the command line: `--name`, `--port`, `--max-players`, `--theater`, `--roe`,
    /// `--opponents`, `--seed`, `--sky-ratio`, `--start-hour`, `--save <file>` and the rules
    /// `--realistic-damage`, `--realistic-guidance`, `--no-crashes`, `--continuous-time`,
    /// `--realistic-weapons`.
    pub fn from_args(args: &[String]) -> Result<Self> {
        let get = |f: &str| args.iter().position(|a| a == f).and_then(|i| args.get(i + 1)).cloned();
        let has = |f: &str| args.iter().any(|a| a == f);
        fn num<T: std::str::FromStr>(v: Option<String>, flag: &str, default: T) -> Result<T> {
            match v {
                Some(s) => s.parse().map_err(|_| anyhow::anyhow!("{flag}: not a number: {s}")),
                None => Ok(default),
            }
        }
        let theater = get("--theater").map(|t| t.to_ascii_lowercase());
        if let Some(t) = &theater
            && theater::theater_index(t).is_none()
        {
            anyhow::bail!("--theater: no theater {t} (one of {})", theater_list());
        }
        let save = match get("--save") {
            Some(p) => PathBuf::from(p),
            None => save::path_of("dedicated").context("no data directory: give --save <file>")?,
        };
        Ok(Self {
            name: get("--name").unwrap_or_else(|| "Stealth Mission 89 server".into()),
            port: num(get("--port"), "--port", DEFAULT_PORT)?,
            max_players: num(get("--max-players"), "--max-players", MAX_PLAYERS)?
                .clamp(1, MAX_PLAYERS),
            theater,
            roe: num(get("--roe"), "--roe", 1u8)?.min(2),
            opponents: num(get("--opponents"), "--opponents", 1u8)?.min(3),
            seed: get("--seed").map(|s| s.parse()).transpose().ok().flatten(),
            sky_ratio: num(get("--sky-ratio"), "--sky-ratio", 24.0f32)?.max(0.0),
            start_hour: num(get("--start-hour"), "--start-hour", 8.0f32)?.rem_euclid(24.0),
            rules: Rules {
                realistic_damage: has("--realistic-damage"),
                realistic_guidance: has("--realistic-guidance"),
                no_crashes: has("--no-crashes"),
                continuous_time: has("--continuous-time"),
                realistic_weapons: has("--realistic-weapons"),
            },
            save,
            send_every: {
                let hz = num(get("--send-rate"), "--send-rate", FS as f32 / SEND_EVERY as f32)?;
                (FS as f32 / hz.max(1.0)).round().clamp(1.0, FS as f32) as u32
            },
            master: (!has("--no-master"))
                .then(|| get("--master").unwrap_or_else(|| crate::master::DEFAULT_MASTER.into())),
            plain: has("--plain"),
        })
    }
}

fn theater_list() -> String {
    theater::codes().join(", ")
}

/// The campaign to run: the save's if it is of the theater asked for, else a new one (the
/// old save kept aside as `<file>.<theater>.bak.json`).
fn campaign(cfg: &Config, data: &stealth_mission::GameData, con: &mut Console) -> Result<Host> {
    let saved = save::load_from(&cfg.save)?;
    match saved {
        Some(state)
            if cfg
                .theater
                .as_ref()
                .is_none_or(|t| t.eq_ignore_ascii_case(&state.setup.theater_code)) =>
        {
            con.say(format!(
                "resuming the campaign in {} (day {}, {} players known)",
                state.setup.theater_code.to_ascii_uppercase(),
                (state.sky_hours() / 24.0).floor() as i64 + 1,
                state.players.len()
            ));
            Host::resume(state, data, cfg.rules)
        }
        saved => {
            if let Some(old) = saved {
                let mut aside = cfg.save.clone().into_os_string();
                aside.push(format!(".{}.bak.json", old.setup.theater_code.to_ascii_lowercase()));
                std::fs::rename(&cfg.save, &aside).with_context(|| {
                    format!("keeping the old save as {}", aside.to_string_lossy())
                })?;
                con.say(format!(
                    "the theater changed ({} to {}): a new campaign; the old one is kept as {}",
                    old.setup.theater_code.to_ascii_uppercase(),
                    cfg.theater.as_deref().unwrap_or("ce").to_ascii_uppercase(),
                    aside.to_string_lossy()
                ));
            }
            let code = cfg.theater.clone().unwrap_or_else(|| "ce".into());
            let setup = CampaignSetup {
                theater: theater::theater_index(&code).context("theater")?,
                theater_code: code.clone(),
                roe: cfg.roe,
                opponents: cfg.opponents,
                seed: cfg.seed.unwrap_or_else(|| {
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(1, |d| d.subsec_nanos())
                }),
                sky_ratio: cfg.sky_ratio,
                start_hour: cfg.start_hour,
            };
            con.say(format!("a new campaign in {}", code.to_ascii_uppercase()));
            let host = Host::new(setup, data, cfg.rules)?;
            save::save_to(&cfg.save, &host.state)?;
            Ok(host)
        }
    }
}

/// Runs the server until `stop` is set.
pub fn run(cfg: Config, stop: &AtomicBool) -> Result<()> {
    let data = theater::game_data().context("the mission data (assets/mission)")?;
    let mut con = Console::new(cfg.plain);
    let mut host = campaign(&cfg, &data, &mut con)?;
    let setup = host.state.setup.clone();
    let sim_data = theater::sim_data(setup.theater, &setup.theater_code)
        .context("the simulation data (assets/sim)")?;
    let mut sim = host.new_sim(&sim_data);
    let mut server = Server::new(&cfg.name, cfg.port, cfg.max_players, None)
        .with_context(|| format!("listening on UDP port {}", cfg.port))?;
    server.send_every = cfg.send_every;
    if let Some(m) = &cfg.master {
        server.list_on(m);
    }
    con.say(format!(
        "{} listening on UDP port {} for {} players, saving to {}",
        cfg.name,
        server.port().unwrap_or(cfg.port),
        cfg.max_players,
        cfg.save.display()
    ));
    con.say(format!("rules: {}", rules_text(&host.rules)));
    con.say(format!("sending the world {:.1} times a second", FS as f32 / cfg.send_every as f32));
    let ev = host.begin(&mut sim);
    log_events(&mut con, &ev);
    let frame = Duration::from_secs_f64(1.0 / FS as f64);
    let mut next = Instant::now();
    let mut last = Instant::now();
    let mut save_timer = 0.0f32;

    while !stop.load(Ordering::Relaxed) && !con.stop_asked() {
        let now = Instant::now();
        if now < next {
            std::thread::sleep(next - now);
            continue;
        }
        // Far behind (the machine was suspended): start afresh rather than catch up.
        next = if now.duration_since(next) > frame * 30 { now + frame } else { next + frame };
        let dt = now.duration_since(last).as_secs_f64();
        last = now;
        let ev = server.pump(&mut host, &mut sim, now);
        log_events(&mut con, &ev);
        let inputs = server.inputs();
        let mut events = sim.step_pilots(&inputs, FS);
        events.extend(host.take_sim_events(&mut sim));
        server.after_step(&events);
        let ev = host.tick(&mut sim, dt, &events);
        server.route(&ev);
        log_events(&mut con, &ev);
        let after = host.take_sim_events(&mut sim);
        server.after_step(&after);
        server.send_frame(&host, &mut sim, now);
        for line in server.log.drain(..) {
            con.say(line);
        }
        con.draw(&status(&cfg, &host, &sim, &server));

        save_timer += dt as f32;
        if save_timer >= SAVE_EVERY {
            save_timer = 0.0;
            if let Err(e) = save::save_to(&cfg.save, &host.state) {
                con.say(format!("saving: {e:#}"));
            }
        }
    }
    con.close();
    con.say("stopping");
    server.shutdown("The server is shutting down");
    save::save_to(&cfg.save, &host.state)?;
    con.say(format!("saved to {}", cfg.save.display()));
    Ok(())
}

/// The campaign's news for the server's log.
fn log_events(con: &mut Console, events: &[HostEvent]) {
    for e in events {
        match e {
            HostEvent::Campaign(CampaignEvent::Message(m)) => con.say(m.clone()),
            HostEvent::SortieStarted { player, sortie, .. } => {
                con.say(format!("player {} took off: {}", player.0 + 1, sortie.offer.title))
            }
            HostEvent::SortieFinished { player, debrief } => con.say(format!(
                "player {} back: {} ({})",
                player.0 + 1,
                debrief.record.title,
                debrief.title
            )),
            _ => {}
        }
    }
}

/// What the console shows.
fn status(cfg: &Config, host: &Host, sim: &stealth_sim::Sim, server: &Server) -> Status {
    use crate::master::held;
    use stealth_campaign::Side;
    Status {
        name: cfg.name.clone(),
        port: server.port().unwrap_or(cfg.port),
        theater: host.state.setup.theater_code.to_ascii_uppercase(),
        clock: crate::master::clock_text(host),
        places: [held(host, Side::Blue), held(host, Side::Red), held(host, Side::Neutral)],
        players: server.roster(host, sim),
        max_players: cfg.max_players,
        rules: rules_text(&host.rules),
        save: cfg.save.display().to_string(),
    }
}

fn rules_text(r: &Rules) -> String {
    let on = crate::master::rules_list(r);
    if on.is_empty() { "classic".into() } else { on.join(", ") }
}
