//! The WebSocket-to-UDP relay for the browser build (docs/web.md): a browser opens
//! `/relay/<server id>` and each binary message goes on as one UDP datagram to that listed
//! game server, from a socket of its own, and each datagram back as one message. The game
//! server sees an ordinary client at this machine's address.
//!
//! Only to servers on the list, only the game's packets (`SM89` first), a few per client
//! address and a cap on the rate, so it can't be used to send UDP anywhere else.

use std::collections::HashMap;
use std::io::ErrorKind;
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tungstenite::protocol::CloseFrame;
use tungstenite::protocol::frame::coding::CloseCode;
use tungstenite::{Message, WebSocket};

use crate::{List, client_ip, is_internal};

/// Relays open at once per client address, and in all.
pub const MAX_PER_IP: usize = 4;
pub const MAX_RELAYS: usize = 512;
/// The game's packets start so (stealth-net's transport); nothing else is passed on.
const MAGIC: &[u8] = b"SM89";
/// Largest datagram passed on (the game's are at most 1200 bytes).
const MAX_DATAGRAM: usize = 1500;
/// Datagrams a browser may send per second, and in a burst.
const RATE: f64 = 300.0;
const BURST: f64 = 600.0;
/// Closed after this long without a packet either way (the game times out after 10 s).
const IDLE: Duration = Duration::from_secs(15);
/// How long a relay waits for the browser before it looks for the server's datagrams.
const TICK: Duration = Duration::from_millis(1);

/// The relays open, by client address.
#[derive(Default)]
pub struct Relays {
    open: HashMap<IpAddr, usize>,
    total: usize,
}

impl Relays {
    /// One more for `ip`, if it is under the caps.
    pub fn admit(&mut self, ip: IpAddr) -> bool {
        let n = self.open.entry(ip).or_default();
        if *n >= MAX_PER_IP || self.total >= MAX_RELAYS {
            return false;
        }
        *n += 1;
        self.total += 1;
        true
    }

    pub fn leave(&mut self, ip: IpAddr) {
        if let Some(n) = self.open.get_mut(&ip) {
            *n -= 1;
            self.total -= 1;
            if *n == 0 {
                self.open.remove(&ip);
            }
        }
    }
}

/// A place in [`Relays`], given back when the relay ends.
struct Admitted {
    relays: Arc<Mutex<Relays>>,
    ip: IpAddr,
}

impl Drop for Admitted {
    fn drop(&mut self) {
        if let Ok(mut r) = self.relays.lock() {
            r.leave(self.ip);
        }
    }
}

/// Where `/relay/<id>` goes: the listed server's address, or why not.
fn target(
    path: &str,
    list: &Mutex<List>,
    allow_internal: bool,
) -> Result<SocketAddr, (u16, &'static str)> {
    let id = path.strip_prefix("/relay/").ok_or((404, "not found"))?.trim_end_matches('/');
    let addr = list
        .lock()
        .map_err(|_| (500, "unavailable"))?
        .addr_of(id, Instant::now())
        .ok_or((404, "not on the list"))?;
    if is_internal(addr.ip()) && !allow_internal {
        return Err((403, "not reachable from here"));
    }
    Ok(addr)
}

/// Takes browsers on `listener` until it fails. `allow_internal` lets it reach servers on
/// this machine and its LAN (for testing).
pub fn serve(listener: TcpListener, list: Arc<Mutex<List>>, allow_internal: bool) {
    let relays = Arc::new(Mutex::new(Relays::default()));
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let (list, relays) = (list.clone(), relays.clone());
        let _ = std::thread::Builder::new()
            .name("relay".into())
            .spawn(move || relay(stream, &list, relays, allow_internal));
    }
}

fn relay(stream: TcpStream, list: &Mutex<List>, relays: Arc<Mutex<Relays>>, allow_internal: bool) {
    let Ok(peer) = stream.peer_addr() else { return };
    // A browser that never finishes its handshake doesn't keep the thread.
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_nodelay(true);
    let mut admitted = None;
    let mut to = None;
    // The error type is tungstenite's.
    #[allow(clippy::result_large_err)]
    let callback = |req: &Request, resp: Response| -> Result<Response, ErrorResponse> {
        let header = |n: &str| req.headers().get(n).and_then(|v| v.to_str().ok());
        let from = client_ip(peer.ip(), header);
        let refuse = |status: u16, why: &str| {
            eprintln!("{from} relay {}: {status} {why}", req.uri().path());
            let mut e = ErrorResponse::new(Some(why.to_string()));
            *e.status_mut() = tungstenite::http::StatusCode::from_u16(status)
                .unwrap_or(tungstenite::http::StatusCode::BAD_REQUEST);
            e
        };
        let addr = target(req.uri().path(), list, allow_internal).map_err(|(s, w)| refuse(s, w))?;
        if !relays.lock().map(|mut r| r.admit(from)).unwrap_or(false) {
            return Err(refuse(429, "too many relays"));
        }
        admitted = Some(Admitted { relays: relays.clone(), ip: from });
        to = Some(addr);
        Ok(resp)
    };
    let Ok(mut ws) = tungstenite::accept_hdr(stream, callback) else { return };
    let (Some(admitted), Some(to)) = (admitted, to) else { return };
    let from = admitted.ip;
    eprintln!("{from} relay to {to} opens");
    let why = match pass(&mut ws, to) {
        Ok(why) => {
            let frame = CloseFrame { code: CloseCode::Away, reason: why.into() };
            let _ = ws.close(Some(frame));
            let _ = ws.flush();
            why
        }
        Err(why) => why,
    };
    eprintln!("{from} relay to {to} closes: {why}");
}

/// Passes datagrams both ways until one side stops: the reason to tell the browser (Ok), or
/// why the browser's side is gone (Err).
fn pass(ws: &mut WebSocket<TcpStream>, to: SocketAddr) -> Result<&'static str, &'static str> {
    let bind = if to.is_ipv6() { "[::]:0" } else { "0.0.0.0:0" };
    let udp = UdpSocket::bind(bind).map_err(|_| "no socket")?;
    // Connected: only the server's datagrams come back.
    udp.connect(to).map_err(|_| "no route")?;
    udp.set_nonblocking(true).map_err(|_| "no socket")?;
    let _ = ws.get_mut().set_read_timeout(Some(TICK));
    let mut buf = [0u8; 2048];
    let (mut heard_browser, mut heard_server) = (Instant::now(), Instant::now());
    let mut tokens = BURST;
    let mut last = Instant::now();
    loop {
        let now = Instant::now();
        tokens = (tokens + now.duration_since(last).as_secs_f64() * RATE).min(BURST);
        last = now;
        match ws.read() {
            Ok(Message::Binary(b)) => {
                heard_browser = now;
                if b.len() <= MAX_DATAGRAM && b.starts_with(MAGIC) && tokens >= 1.0 {
                    tokens -= 1.0;
                    let _ = udp.send(&b);
                }
            }
            Ok(_) => heard_browser = now,
            Err(tungstenite::Error::Io(e))
                if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => {
                return Err("the browser left");
            }
            Err(_) => return Err("the browser's connection broke"),
        }
        loop {
            match udp.recv(&mut buf) {
                Ok(n) => {
                    heard_server = now;
                    if ws.send(Message::Binary(buf[..n].to_vec().into())).is_err() {
                        return Err("the browser's connection broke");
                    }
                }
                // ICMP "port unreachable" from an earlier datagram: the server is down, which
                // the idle timeout tells.
                Err(e) if e.kind() == ErrorKind::ConnectionRefused => continue,
                Err(_) => break,
            }
        }
        if now.duration_since(heard_server) > IDLE {
            return Ok("the server doesn't answer");
        }
        if now.duration_since(heard_browser) > IDLE {
            return Ok("idle");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stealth_master_proto::{ServerReport, server_id};

    #[test]
    fn relays_are_capped_per_address() {
        let mut r = Relays::default();
        let a: IpAddr = "203.0.113.5".parse().unwrap();
        let b: IpAddr = "203.0.113.6".parse().unwrap();
        for _ in 0..MAX_PER_IP {
            assert!(r.admit(a));
        }
        assert!(!r.admit(a));
        assert!(r.admit(b));
        r.leave(a);
        assert!(r.admit(a));
    }

    /// A game server on loopback that answers each datagram with itself.
    fn echo() -> u16 {
        let s = UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = s.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let mut buf = [0u8; 2048];
            while let Ok((n, from)) = s.recv_from(&mut buf) {
                let _ = s.send_to(&buf[..n], from);
            }
        });
        port
    }

    fn start(allow_internal: bool) -> (SocketAddr, String) {
        let port = echo();
        let mut list = List::default();
        let report = ServerReport { name: "Echo".into(), port, ..Default::default() };
        let from: IpAddr = "127.0.0.1".parse().unwrap();
        let r = list.handle(
            "POST",
            "/api/servers",
            &serde_json::to_vec(&report).unwrap(),
            from,
            Instant::now(),
        );
        assert_eq!(r.status, 200);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let list = Arc::new(Mutex::new(list));
        std::thread::spawn(move || serve(listener, list, allow_internal));
        (addr, server_id("Echo", "127.0.0.1"))
    }

    fn open(addr: SocketAddr, path: &str) -> Result<WebSocket<TcpStream>, tungstenite::Error> {
        let stream = TcpStream::connect(addr).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        tungstenite::client(format!("ws://{addr}{path}"), stream).map(|(ws, _)| ws).map_err(|e| {
            match e {
                tungstenite::HandshakeError::Failure(e) => e,
                tungstenite::HandshakeError::Interrupted(_) => tungstenite::Error::ConnectionClosed,
            }
        })
    }

    fn status(r: Result<WebSocket<TcpStream>, tungstenite::Error>) -> u16 {
        match r {
            Err(tungstenite::Error::Http(resp)) => resp.status().as_u16(),
            Err(e) => panic!("{e}"),
            Ok(_) => 101,
        }
    }

    #[test]
    fn a_browser_reaches_a_listed_server() {
        let (addr, id) = start(true);
        let mut ws = open(addr, &format!("/relay/{id}")).unwrap();
        // Not the game's: dropped. The game's: there and back.
        ws.send(Message::Binary(b"hello".to_vec().into())).unwrap();
        ws.send(Message::Binary(b"SM89 ping".to_vec().into())).unwrap();
        match ws.read().unwrap() {
            Message::Binary(b) => assert_eq!(&b[..], b"SM89 ping"),
            m => panic!("{m:?}"),
        }
        assert_eq!(status(open(addr, "/relay/0123456789abcdef")), 404);
        // A few per address.
        let more: Vec<_> =
            (1..MAX_PER_IP).map(|_| open(addr, &format!("/relay/{id}")).unwrap()).collect();
        assert_eq!(status(open(addr, &format!("/relay/{id}"))), 429);
        drop(more);
    }

    #[test]
    fn servers_on_this_network_are_not_relayed_to() {
        let (addr, id) = start(false);
        assert_eq!(status(open(addr, &format!("/relay/{id}"))), 403);
    }
}
