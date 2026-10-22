//! Tacview real-time telemetry (`--tacview-real-time`, the menu's "Tacview real-time
//! telemetry"): the game hosts a session on TCP port 42674 (`--tacview-address`) that Tacview
//! joins with *Record > Real-Time Telemetry*. The flight goes out while it is recorded, as the
//! ACMI text of the export ([`crate::tacview::AcmiWriter`]); a client joining in flight first
//! gets the flight so far. A session is one flight: its clients are disconnected when it ends,
//! and those connected between flights wait for the next one.
//!
//! The protocol (Tacview's "Real-Time Telemetry Public Protocol", UTF-8 text): the host greets
//! with `XtraLib.Stream.0\nTacview.RealTimeTelemetry.0\n<host name>\n\0`, the client answers
//! `XtraLib.Stream.0\nTacview.RealTimeTelemetry.0\n<client name>\n<password hash>\0` (CRC-64/WE of
//! the UTF-16 password, in hexadecimal), then the host sends ACMI 2.2 text, one packet per frame.
//! There is no keep-alive; either side may close the connection at any time.
// The browser build cannot listen for Tacview (the plugin only adds the resource there).
#![cfg_attr(target_arch = "wasm32", allow(dead_code, unused_imports))]

use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use bevy::prelude::*;

use crate::combat::{Combat, CombatSystems};
use crate::mission::ActiveMission;
use crate::settings::{AppState, Settings};
use crate::tacview::AcmiWriter;

/// Tacview's default port.
pub const DEFAULT_PORT: u16 = 42674;
/// Where the session is hosted by default: this computer only.
pub const DEFAULT_ADDRESS: SocketAddr =
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), DEFAULT_PORT);
const STREAM: &str = "XtraLib.Stream.0";
const PROTOCOL: &str = "Tacview.RealTimeTelemetry.0";
const HOST_NAME: &str = "Stealth Mission 89";
/// Time a client has to answer the greeting.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
/// A client that takes no data for this long is dropped.
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
/// How often the listener looks for new connections (and whether it should stop).
const ACCEPT_POLL: Duration = Duration::from_millis(200);
/// Wait before hosting again when the address was taken.
const RETRY: Duration = Duration::from_secs(5);
/// Longest client handshake read.
const MAX_REPLY: usize = 4096;

pub struct TacviewRealTimePlugin;

impl Plugin for TacviewRealTimePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Telemetry>();
        // The browser cannot listen for Tacview's connection.
        #[cfg(not(target_arch = "wasm32"))]
        app.add_systems(Update, (serve, stream.after(CombatSystems)).chain())
            .add_systems(OnExit(AppState::Flight), end_flight);
    }
}

/// `--tacview-address <ip[:port]|port>`: where the session is hosted (default 127.0.0.1:42674;
/// 0.0.0.0 lets Tacview join from another computer).
pub fn parse_address(s: &str) -> Option<SocketAddr> {
    s.parse::<SocketAddr>()
        .ok()
        .or_else(|| s.parse::<IpAddr>().ok().map(|ip| SocketAddr::new(ip, DEFAULT_PORT)))
        .or_else(|| s.parse::<u16>().ok().map(|p| SocketAddr::new(DEFAULT_ADDRESS.ip(), p)))
}

/// What the game tells the session thread.
enum Msg {
    /// A client that completed the handshake.
    Client(TcpStream, String),
    /// A flight starts: what follows is its ACMI text.
    Begin,
    /// ACMI text of the last frames.
    Data(String),
    /// The flight is over: its clients are disconnected.
    End,
}

/// A hosted session: a listener thread taking clients and a session thread sending them the
/// flight. Dropping it stops the listener, which frees the port; the session thread ends with
/// the last sender.
struct Server {
    tx: Sender<Msg>,
    stop: Arc<AtomicBool>,
}

impl Server {
    fn start(addr: SocketAddr, password: Option<String>) -> std::io::Result<Self> {
        let listener = TcpListener::bind(addr)?;
        listener.set_nonblocking(true)?;
        let (tx, rx) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        std::thread::Builder::new().name("tacview session".into()).spawn(move || session(rx))?;
        let (clients, quit) = (tx.clone(), stop.clone());
        std::thread::Builder::new()
            .name("tacview listener".into())
            .spawn(move || listen(listener, clients, quit, password))?;
        info!("tacview real-time: hosting on {addr}");
        Ok(Self { tx, stop })
    }

    fn send(&self, msg: Msg) {
        let _ = self.tx.send(msg);
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Takes connections until stopped; each one shakes hands on its own thread.
fn listen(listener: TcpListener, tx: Sender<Msg>, stop: Arc<AtomicBool>, password: Option<String>) {
    let password: Option<Arc<str>> = password.map(Into::into);
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, peer)) => {
                let (tx, password) = (tx.clone(), password.clone());
                let _ =
                    std::thread::Builder::new().name("tacview handshake".into()).spawn(move || {
                        match handshake(stream, password.as_deref()) {
                            Ok(Some((stream, name))) => {
                                let _ = tx.send(Msg::Client(stream, name));
                            }
                            Ok(None) => info!("tacview real-time: {peer} refused"),
                            Err(e) => info!("tacview real-time: {peer}: {e}"),
                        }
                    });
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(ACCEPT_POLL),
            Err(e) => {
                warn!("tacview real-time: {e}");
                std::thread::sleep(ACCEPT_POLL);
            }
        }
    }
}

/// Greets a client and checks its answer: the stream and the client's name, or None if it is
/// not Tacview or has the wrong password.
fn handshake(
    mut s: TcpStream,
    password: Option<&str>,
) -> std::io::Result<Option<(TcpStream, String)>> {
    // An accepted socket may inherit the listener's non-blocking mode.
    s.set_nonblocking(false)?;
    s.set_nodelay(true)?;
    s.set_read_timeout(Some(HANDSHAKE_TIMEOUT))?;
    s.set_write_timeout(Some(WRITE_TIMEOUT))?;
    s.write_all(format!("{STREAM}\n{PROTOCOL}\n{HOST_NAME}\n\0").as_bytes())?;
    let mut reply = Vec::new();
    let mut buf = [0u8; 256];
    let end = loop {
        if let Some(end) = reply.iter().position(|&b| b == 0) {
            break end;
        }
        let n = s.read(&mut buf)?;
        if n == 0 || reply.len() > MAX_REPLY {
            return Ok(None);
        }
        reply.extend_from_slice(&buf[..n]);
    };
    s.set_read_timeout(None)?;
    Ok(client_name(&String::from_utf8_lossy(&reply[..end]), password).map(|name| (s, name)))
}

/// The client's name from its handshake (without the final `\0`), if it is accepted.
fn client_name(reply: &str, password: Option<&str>) -> Option<String> {
    let mut lines = reply.split('\n').map(|l| l.trim_end_matches('\r'));
    if lines.next()? != STREAM || lines.next()? != PROTOCOL {
        return None;
    }
    let name = lines.next()?.to_string();
    if let Some(p) = password {
        let hash = u64::from_str_radix(lines.next()?.trim(), 16).ok()?;
        if hash != password_hash(p) {
            return None;
        }
    }
    Some(name)
}

/// Tacview's password hash: CRC-64/WE of the UTF-16 (little endian) text.
fn password_hash(password: &str) -> u64 {
    let bytes: Vec<u8> = password.encode_utf16().flat_map(u16::to_le_bytes).collect();
    crc64_we(&bytes)
}

/// CRC-64/WE: polynomial 0x42F0E1EBA9EA3693, initial value and final XOR all ones, not reflected.
fn crc64_we(bytes: &[u8]) -> u64 {
    const POLY: u64 = 0x42F0_E1EB_A9EA_3693;
    let mut crc = !0u64;
    for &b in bytes {
        crc ^= (b as u64) << 56;
        for _ in 0..8 {
            crc = if crc & (1 << 63) != 0 { (crc << 1) ^ POLY } else { crc << 1 };
        }
    }
    !crc
}

/// Sends the flight to the clients until the game and the listener are gone.
fn session(rx: Receiver<Msg>) {
    let mut clients: Vec<(TcpStream, String)> = Vec::new();
    // The flight so far (None between flights), for the clients joining late.
    let mut flight: Option<String> = None;
    let send = |clients: &mut Vec<(TcpStream, String)>, text: &str| {
        clients.retain_mut(|(s, name)| {
            let ok = s.write_all(text.as_bytes()).is_ok();
            if !ok {
                info!("tacview real-time: {name} left");
            }
            ok
        });
    };
    let close = |clients: &mut Vec<(TcpStream, String)>| {
        for (s, _) in clients.drain(..) {
            let _ = s.shutdown(Shutdown::Both);
        }
    };
    for msg in rx {
        match msg {
            Msg::Client(s, name) => {
                info!("tacview real-time: {name} joined");
                let mut joined = vec![(s, name)];
                if let Some(f) = &flight {
                    send(&mut joined, f);
                }
                clients.append(&mut joined);
            }
            Msg::Begin => flight = Some(String::new()),
            Msg::Data(text) => {
                send(&mut clients, &text);
                if let Some(f) = &mut flight {
                    f.push_str(&text);
                }
            }
            Msg::End => {
                flight = None;
                close(&mut clients);
            }
        }
    }
    close(&mut clients);
}

#[derive(Resource, Default)]
struct Telemetry {
    server: Option<Server>,
    /// The session could not be hosted (the port is taken, e.g. by the game just relaunched in
    /// VR): when to try again.
    retry: Option<Instant>,
    /// The flight being sent.
    writer: Option<AcmiWriter>,
}

/// Hosts the session while the switch is on (also in the menu, so Tacview can join before the
/// flight).
fn serve(mut telemetry: ResMut<Telemetry>, settings: Res<Settings>) {
    if !settings.tacview_real_time {
        if telemetry.server.is_some() {
            info!("tacview real-time: stopped");
        }
        *telemetry = Telemetry::default();
        return;
    }
    let now = Instant::now();
    if telemetry.server.is_none() && telemetry.retry.is_none_or(|t| now >= t) {
        match Server::start(settings.tacview_address, settings.tacview_password.clone()) {
            Ok(server) => {
                telemetry.server = Some(server);
                telemetry.retry = None;
            }
            Err(e) => {
                // Said once, then tried again quietly.
                if telemetry.retry.is_none() {
                    warn!("tacview real-time: {}: {e}", settings.tacview_address);
                }
                telemetry.retry = Some(now + RETRY);
            }
        }
    }
}

/// Sends the frames the flight recorder took since the last frame.
fn stream(
    mut telemetry: ResMut<Telemetry>,
    settings: Res<Settings>,
    combat: Option<Res<Combat>>,
    mission: Option<Res<ActiveMission>>,
) {
    let (Some(combat), Some(mission)) = (combat, mission) else { return };
    let telemetry = &mut *telemetry;
    let Some(server) = &telemetry.server else { return };
    // A new campaign sortie: its clients are let go like at the end of a flight.
    if telemetry.writer.take_if(|w| w.restarted(&combat.record)).is_some() {
        server.send(Msg::End);
    }
    let writer = telemetry.writer.get_or_insert_with(|| {
        server.send(Msg::Begin);
        AcmiWriter::new(&mission.0)
    });
    let mut text = String::new();
    writer.advance(&combat.record, &mission.0, &combat.data, settings.aircraft.name(), &mut text);
    if !text.is_empty() {
        server.send(Msg::Data(text));
    }
}

fn end_flight(mut telemetry: ResMut<Telemetry>) {
    if telemetry.writer.take().is_some()
        && let Some(server) = &telemetry.server
    {
        server.send(Msg::End);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc64_we_check() {
        assert_eq!(crc64_we(b"123456789"), 0x62EC_59E3_F1A4_F00A);
    }

    #[test]
    fn handshake_replies() {
        let ok = "XtraLib.Stream.0\nTacview.RealTimeTelemetry.0\nTacview\n0";
        assert_eq!(client_name(ok, None).as_deref(), Some("Tacview"));
        assert_eq!(client_name("XtraLib.Stream.0\nOther.0\nTacview\n0", None), None);
        let hash = format!("{:x}", password_hash("secret"));
        let with = format!("XtraLib.Stream.0\nTacview.RealTimeTelemetry.0\nTacview\n{hash}");
        assert_eq!(client_name(&with, Some("secret")).as_deref(), Some("Tacview"));
        assert_eq!(client_name(&with, Some("other")), None);
        assert_eq!(client_name(ok, Some("secret")), None);
    }

    #[test]
    fn addresses() {
        assert_eq!(parse_address("0.0.0.0:42675"), "0.0.0.0:42675".parse().ok());
        assert_eq!(parse_address("0.0.0.0"), "0.0.0.0:42674".parse().ok());
        assert_eq!(parse_address("42675"), "127.0.0.1:42675".parse().ok());
        assert_eq!(parse_address("nowhere"), None);
    }

    /// A client joining in flight is greeted, accepted, sent the flight from its start and
    /// disconnected when it ends; dropping the server frees the port.
    #[test]
    fn session_streams_to_a_client() {
        // A free port.
        let addr = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
        let server = Server::start(addr, None).unwrap();
        server.send(Msg::Begin);
        server.send(Msg::Data("FileType=text/acmi/tacview\n".into()));

        let mut c = TcpStream::connect(addr).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut greeting = [0u8; 64];
        let mut got = Vec::new();
        while !got.contains(&0) {
            let n = c.read(&mut greeting).unwrap();
            got.extend_from_slice(&greeting[..n]);
        }
        assert!(got.starts_with(b"XtraLib.Stream.0\nTacview.RealTimeTelemetry.0\n"));
        c.write_all(b"XtraLib.Stream.0\nTacview.RealTimeTelemetry.0\nTest\n0\0").unwrap();
        server.send(Msg::Data("#1\n".into()));
        let mut text = String::new();
        let mut buf = [0u8; 256];
        while !text.contains("#1") {
            let n = c.read(&mut buf).unwrap();
            text.push_str(std::str::from_utf8(&buf[..n]).unwrap());
        }
        assert_eq!(text, "FileType=text/acmi/tacview\n#1\n");
        server.send(Msg::End);
        assert_eq!(c.read(&mut buf).unwrap(), 0);

        drop(server);
        std::thread::sleep(ACCEPT_POLL * 2);
        assert!(TcpListener::bind(addr).is_ok());
    }
}
