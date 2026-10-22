//! A small connection layer on UDP: connections with timeouts, a reliable ordered channel
//! (acks, resends, fragments of any size) and an unreliable one (fragmented; a message with a
//! lost fragment is dropped).
//!
//! Every data packet carries its sequence number and acknowledges the peer's latest packets
//! (the latest number and a bitfield of the 32 before it). Reliable messages ride in packets
//! until a packet carrying them is acknowledged.
//!
//! The packets go through [`Datagrams`]: a UDP socket, or in the browser build a WebSocket to
//! the master server's relay, which passes each message on as one datagram (`websocket.rs`).

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::io::ErrorKind;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::time::Duration;

use web_time::Instant;

/// Bytes of payload in one packet (stays under common path MTUs).
const MTU: usize = 1200;
const MAGIC: [u8; 4] = *b"SM89";
/// No packet for this long: the connection is gone.
pub const TIMEOUT: Duration = Duration::from_secs(10);
/// A packet at least this often (it carries the acks).
const KEEPALIVE: Duration = Duration::from_millis(100);
/// Connection requests are repeated this often.
const CONNECT_EVERY: Duration = Duration::from_millis(250);
/// Unacknowledged reliable data in flight per connection at most (bytes).
const WINDOW: usize = 256 * 1024;
/// An unreliable message larger than this is not sent.
const MAX_UNRELIABLE: usize = 255 * (MTU - 32);

const CONNECT: u8 = 0;
const ACCEPT: u8 = 1;
const DENY: u8 = 2;
const DATA: u8 = 3;
const CLOSE: u8 = 4;

const RELIABLE: u8 = 0;
const UNRELIABLE: u8 = 1;

/// A connection's handle.
pub type ConnId = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    /// Arrives, in order.
    Reliable,
    /// May not arrive; the latest is all that matters.
    Unreliable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetEvent {
    Connected(ConnId),
    /// Gone (timed out, closed, or denied: the reason).
    Disconnected(ConnId, String),
    Message(ConnId, Channel, Vec<u8>),
}

/// A reliable message part waiting for its acknowledgement.
struct Pending {
    id: u16,
    last: bool,
    data: Vec<u8>,
    sent: Option<Instant>,
}

struct Conn {
    id: ConnId,
    addr: SocketAddr,
    salt: u32,
    connected: bool,
    last_recv: Instant,
    last_send: Option<Instant>,
    /// Packets: next to send, the peer's latest and the ones before it.
    seq: u16,
    remote_seq: u16,
    remote_bits: u32,
    got_any: bool,
    /// Sent packets not yet acknowledged: the reliable parts they carried and when.
    sent: HashMap<u16, (Instant, Vec<u16>)>,
    /// Reliable parts to send and resend.
    out: VecDeque<Pending>,
    next_out: u16,
    /// Reliable parts received out of order, the next one expected, the message so far.
    inbox: BTreeMap<u16, (bool, Vec<u8>)>,
    next_in: u16,
    assembling: Vec<u8>,
    /// Unreliable messages to send this flush, the next number, and a message being put
    /// together.
    unreliable: Vec<Vec<u8>>,
    next_unreliable: u16,
    fragments: Option<(u16, Vec<Option<Vec<u8>>>)>,
    rtt: f32,
    closing: Option<String>,
}

impl Conn {
    fn new(id: ConnId, addr: SocketAddr, salt: u32, now: Instant) -> Self {
        Self {
            id,
            addr,
            salt,
            connected: false,
            last_recv: now,
            last_send: None,
            seq: 0,
            remote_seq: 0,
            remote_bits: 0,
            got_any: false,
            sent: HashMap::new(),
            out: VecDeque::new(),
            next_out: 0,
            inbox: BTreeMap::new(),
            next_in: 0,
            assembling: Vec::new(),
            unreliable: Vec::new(),
            next_unreliable: 0,
            fragments: None,
            rtt: 0.1,
            closing: None,
        }
    }

    fn in_flight(&self) -> usize {
        self.out.iter().filter(|p| p.sent.is_some()).map(|p| p.data.len()).sum()
    }

    /// The peer's packet `seq` arrived: false if it is a duplicate.
    fn note_received(&mut self, seq: u16) -> bool {
        if !self.got_any {
            self.got_any = true;
            self.remote_seq = seq;
            self.remote_bits = 0;
            return true;
        }
        let d = seq.wrapping_sub(self.remote_seq) as i16;
        if d > 0 {
            let d = d as u32;
            self.remote_bits = if d >= 32 { 0 } else { (self.remote_bits << d) | (1 << (d - 1)) };
            self.remote_seq = seq;
            true
        } else if d == 0 {
            false
        } else {
            let back = (-d) as u32;
            if back > 32 {
                return false;
            }
            let bit = 1 << (back - 1);
            let new = self.remote_bits & bit == 0;
            self.remote_bits |= bit;
            new
        }
    }

    /// The peer acknowledged our packets up to `ack` (and `bits` before it).
    fn note_acks(&mut self, ack: u16, bits: u32, now: Instant) {
        let mut acked = Vec::new();
        for (k, seq) in
            std::iter::once(ack).chain((1..=32).map(|i| ack.wrapping_sub(i))).enumerate()
        {
            if k > 0 && bits & (1 << (k - 1)) == 0 {
                continue;
            }
            if let Some((at, ids)) = self.sent.remove(&seq) {
                if k == 0 {
                    let sample = now.duration_since(at).as_secs_f32();
                    self.rtt = self.rtt * 0.9 + sample * 0.1;
                }
                acked.extend(ids);
            }
        }
        if !acked.is_empty() {
            self.out.retain(|p| !acked.contains(&p.id));
        }
        // Forget packets too old to be acknowledged any more (their parts get resent).
        let horizon = Duration::from_secs(5);
        self.sent.retain(|_, (at, _)| now.duration_since(*at) < horizon);
    }

    /// A reliable part arrived: the whole messages it completes.
    fn receive_reliable(&mut self, id: u16, last: bool, data: Vec<u8>) -> Vec<Vec<u8>> {
        let ahead = id.wrapping_sub(self.next_in);
        if ahead >= 0x8000 {
            return Vec::new();
        }
        self.inbox.entry(id).or_insert((last, data));
        let mut done = Vec::new();
        while let Some((last, data)) = self.inbox.remove(&self.next_in) {
            self.next_in = self.next_in.wrapping_add(1);
            self.assembling.extend_from_slice(&data);
            if last {
                done.push(std::mem::take(&mut self.assembling));
            }
        }
        done
    }

    /// An unreliable fragment arrived: the message if it is complete.
    fn receive_fragment(
        &mut self,
        msg: u16,
        frag: u8,
        count: u8,
        data: Vec<u8>,
    ) -> Option<Vec<u8>> {
        if count <= 1 {
            return Some(data);
        }
        match &self.fragments {
            Some((m, _)) if *m == msg => {}
            Some((m, _)) if (msg.wrapping_sub(*m) as i16) < 0 => return None,
            _ => self.fragments = Some((msg, vec![None; count as usize])),
        }
        let (_, parts) = self.fragments.as_mut()?;
        if let Some(p) = parts.get_mut(frag as usize) {
            *p = Some(data);
        }
        if parts.iter().all(Option::is_some) {
            let (_, parts) = self.fragments.take()?;
            return Some(parts.into_iter().flatten().flatten().collect());
        }
        None
    }
}

/// What an [`Endpoint`]'s packets go through (Send and Sync: the game keeps it in a resource).
pub trait Datagrams: Send + Sync {
    /// The next datagram that came in and where from; `WouldBlock` when there is none.
    fn recv_from(&mut self, buf: &mut [u8]) -> std::io::Result<(usize, SocketAddr)>;
    fn send_to(&mut self, p: &[u8], to: SocketAddr);
    fn local_addr(&self) -> Option<SocketAddr> {
        None
    }
    /// Why nothing goes through any more, once that is so (a relay closed).
    fn closed(&self) -> Option<String> {
        None
    }
}

impl Datagrams for UdpSocket {
    fn recv_from(&mut self, buf: &mut [u8]) -> std::io::Result<(usize, SocketAddr)> {
        UdpSocket::recv_from(self, buf)
    }

    fn send_to(&mut self, p: &[u8], to: SocketAddr) {
        let _ = UdpSocket::send_to(self, p, to);
    }

    fn local_addr(&self) -> Option<SocketAddr> {
        UdpSocket::local_addr(self).ok()
    }
}

/// One end: a server with many connections, or a client with one.
pub struct Endpoint {
    socket: Box<dyn Datagrams>,
    server: bool,
    conns: Vec<Conn>,
    next_id: ConnId,
    events: Vec<NetEvent>,
    /// Test aid: drop this share of the packets sent (0..1) with a simple random stream.
    pub loss: f32,
    rng: u32,
    max_conns: usize,
    /// A client's last connection request.
    connect_at: Option<Instant>,
}

impl Endpoint {
    /// A server listening on `addr` (e.g. "0.0.0.0:28989") for at most `max_conns` clients.
    pub fn server(addr: &str, max_conns: usize) -> std::io::Result<Self> {
        let socket = UdpSocket::bind(addr)?;
        socket.set_nonblocking(true)?;
        Ok(Self::with_socket(Box::new(socket), true, max_conns))
    }

    /// A client connecting to `addr` ("host:port").
    pub fn client(addr: &str) -> std::io::Result<Self> {
        let target = addr
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| std::io::Error::new(ErrorKind::NotFound, "no such host"))?;
        let bind = if target.is_ipv6() { "[::]:0" } else { "0.0.0.0:0" };
        let socket = UdpSocket::bind(bind)?;
        socket.set_nonblocking(true)?;
        Ok(Self::client_over(Box::new(socket), target))
    }

    /// A client connecting through the master server's relay at `url`
    /// ("wss://.../relay/<server id>"), which knows the server's address.
    #[cfg(target_arch = "wasm32")]
    pub fn relay(url: &str) -> std::io::Result<Self> {
        let socket = crate::websocket::WsDatagrams::open(url)?;
        Ok(Self::client_over(Box::new(socket), crate::websocket::PEER))
    }

    /// A client connecting to `target` through any [`Datagrams`].
    pub fn client_over(socket: Box<dyn Datagrams>, target: SocketAddr) -> Self {
        let mut e = Self::with_socket(socket, false, 1);
        let salt = e.random() | 1;
        e.conns.push(Conn::new(0, target, salt, Instant::now()));
        e.next_id = 1;
        e
    }

    fn with_socket(socket: Box<dyn Datagrams>, server: bool, max_conns: usize) -> Self {
        let seed = web_time::SystemTime::now()
            .duration_since(web_time::SystemTime::UNIX_EPOCH)
            .map_or(1, |d| d.subsec_nanos() ^ d.as_secs() as u32)
            | 1;
        Self {
            socket,
            server,
            conns: Vec::new(),
            next_id: 0,
            events: Vec::new(),
            loss: 0.0,
            rng: seed,
            max_conns,
            connect_at: None,
        }
    }

    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.socket.local_addr()
    }

    fn random(&mut self) -> u32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        self.rng
    }

    fn conn(&mut self, id: ConnId) -> Option<&mut Conn> {
        self.conns.iter_mut().find(|c| c.id == id)
    }

    /// Connected connections.
    pub fn connections(&self) -> Vec<ConnId> {
        self.conns.iter().filter(|c| c.connected).map(|c| c.id).collect()
    }

    pub fn addr_of(&self, id: ConnId) -> Option<SocketAddr> {
        self.conns.iter().find(|c| c.id == id).map(|c| c.addr)
    }

    /// Round trip time (seconds).
    pub fn rtt(&self, id: ConnId) -> f32 {
        self.conns.iter().find(|c| c.id == id).map_or(0.0, |c| c.rtt)
    }

    /// Reliable data not yet acknowledged (bytes).
    pub fn backlog(&self, id: ConnId) -> usize {
        self.conns
            .iter()
            .find(|c| c.id == id)
            .map_or(0, |c| c.out.iter().map(|p| p.data.len()).sum())
    }

    /// Queues a message for the next flush.
    pub fn send(&mut self, id: ConnId, channel: Channel, data: &[u8]) {
        let Some(c) = self.conn(id) else { return };
        match channel {
            Channel::Reliable => {
                let part = MTU - 40;
                let n = data.len().div_ceil(part).max(1);
                for k in 0..n {
                    let chunk = &data[(k * part).min(data.len())..((k + 1) * part).min(data.len())];
                    let id = c.next_out;
                    c.next_out = c.next_out.wrapping_add(1);
                    c.out.push_back(Pending {
                        id,
                        last: k + 1 == n,
                        data: chunk.to_vec(),
                        sent: None,
                    });
                }
            }
            Channel::Unreliable => {
                if data.len() <= MAX_UNRELIABLE {
                    c.unreliable.push(data.to_vec());
                }
            }
        }
    }

    /// Closes a connection (the peer is told).
    pub fn close(&mut self, id: ConnId, reason: &str) {
        if let Some(c) = self.conn(id) {
            c.closing = Some(reason.to_string());
        }
    }

    /// Receives what came in; the events since the last call.
    pub fn poll(&mut self, now: Instant) -> Vec<NetEvent> {
        let mut buf = [0u8; 2048];
        loop {
            match self.socket.recv_from(&mut buf) {
                Ok((n, from)) => self.receive(&buf[..n], from, now),
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                // Windows reports an unreachable peer of an earlier send here.
                Err(e) if e.kind() == ErrorKind::ConnectionReset => continue,
                Err(_) => break,
            }
        }
        if let Some(why) = self.socket.closed() {
            for c in self.conns.drain(..) {
                self.events.push(NetEvent::Disconnected(c.id, why.clone()));
            }
        }
        let mut gone = Vec::new();
        for c in &self.conns {
            if now.duration_since(c.last_recv) > TIMEOUT {
                gone.push((c.id, if c.connected { "Timed out" } else { "No answer" }.to_string()));
            }
        }
        for (id, why) in gone {
            self.conns.retain(|c| c.id != id);
            self.events.push(NetEvent::Disconnected(id, why));
        }
        std::mem::take(&mut self.events)
    }

    fn receive(&mut self, p: &[u8], from: SocketAddr, now: Instant) {
        if p.len() < 9 || p[..4] != MAGIC {
            return;
        }
        let kind = p[4];
        let salt = u32::from_le_bytes([p[5], p[6], p[7], p[8]]);
        let body = &p[9..];
        let known = self.conns.iter().position(|c| c.addr == from);
        match kind {
            CONNECT if self.server => {
                let i = match known {
                    Some(i) if self.conns[i].salt == salt => i,
                    Some(i) => {
                        // The same address with a new salt: a new client there.
                        let id = self.conns[i].id;
                        self.conns.remove(i);
                        self.events.push(NetEvent::Disconnected(id, "Reconnected".into()));
                        return self.receive(p, from, now);
                    }
                    None => {
                        if self.conns.len() >= self.max_conns {
                            self.raw(from, DENY, salt, b"The server is full");
                            return;
                        }
                        let id = self.next_id;
                        self.next_id += 1;
                        self.conns.push(Conn::new(id, from, salt, now));
                        self.conns.len() - 1
                    }
                };
                let c = &mut self.conns[i];
                c.last_recv = now;
                if !c.connected {
                    c.connected = true;
                    let id = c.id;
                    self.events.push(NetEvent::Connected(id));
                }
                self.raw(from, ACCEPT, salt, &[]);
            }
            ACCEPT if !self.server => {
                if let Some(i) = known.filter(|&i| self.conns[i].salt == salt) {
                    let c = &mut self.conns[i];
                    c.last_recv = now;
                    if !c.connected {
                        c.connected = true;
                        let id = c.id;
                        self.events.push(NetEvent::Connected(id));
                    }
                }
            }
            DENY if !self.server => {
                if let Some(i) = known.filter(|&i| self.conns[i].salt == salt) {
                    let id = self.conns[i].id;
                    self.conns.remove(i);
                    let why = String::from_utf8_lossy(body).into_owned();
                    self.events.push(NetEvent::Disconnected(id, why));
                }
            }
            CLOSE => {
                if let Some(i) = known.filter(|&i| self.conns[i].salt == salt) {
                    let id = self.conns[i].id;
                    self.conns.remove(i);
                    let why = String::from_utf8_lossy(body).into_owned();
                    self.events.push(NetEvent::Disconnected(id, why));
                }
            }
            DATA => {
                let Some(i) = known.filter(|&i| self.conns[i].salt == salt) else { return };
                if !self.conns[i].connected {
                    return;
                }
                let mut events = Vec::new();
                self.data(i, body, now, &mut events);
                self.events.extend(events);
            }
            _ => {}
        }
    }

    fn data(&mut self, i: usize, b: &[u8], now: Instant, events: &mut Vec<NetEvent>) {
        let c = &mut self.conns[i];
        if b.len() < 10 {
            return;
        }
        c.last_recv = now;
        let seq = u16::from_le_bytes([b[0], b[1]]);
        let ack = u16::from_le_bytes([b[2], b[3]]);
        let bits = u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
        // The peer has received a packet of ours (else its acks mean nothing yet).
        if b[8] != 0 {
            c.note_acks(ack, bits, now);
        }
        let n = b[9];
        let fresh = c.note_received(seq);
        let mut at = 10;
        for _ in 0..n {
            let Some(&ty) = b.get(at) else { return };
            match ty {
                RELIABLE => {
                    if b.len() < at + 6 {
                        return;
                    }
                    let id = u16::from_le_bytes([b[at + 1], b[at + 2]]);
                    let last = b[at + 3] != 0;
                    let len = u16::from_le_bytes([b[at + 4], b[at + 5]]) as usize;
                    at += 6;
                    let Some(data) = b.get(at..at + len) else { return };
                    at += len;
                    for m in c.receive_reliable(id, last, data.to_vec()) {
                        events.push(NetEvent::Message(c.id, Channel::Reliable, m));
                    }
                }
                UNRELIABLE => {
                    if b.len() < at + 7 {
                        return;
                    }
                    let msg = u16::from_le_bytes([b[at + 1], b[at + 2]]);
                    let frag = b[at + 3];
                    let count = b[at + 4];
                    let len = u16::from_le_bytes([b[at + 5], b[at + 6]]) as usize;
                    at += 7;
                    let Some(data) = b.get(at..at + len) else { return };
                    at += len;
                    if fresh && let Some(m) = c.receive_fragment(msg, frag, count, data.to_vec()) {
                        events.push(NetEvent::Message(c.id, Channel::Unreliable, m));
                    }
                }
                _ => return,
            }
        }
    }

    fn raw(&mut self, to: SocketAddr, kind: u8, salt: u32, body: &[u8]) {
        let mut p = Vec::with_capacity(9 + body.len());
        p.extend_from_slice(&MAGIC);
        p.push(kind);
        p.extend_from_slice(&salt.to_le_bytes());
        p.extend_from_slice(body);
        self.transmit(to, &p);
    }

    fn transmit(&mut self, to: SocketAddr, p: &[u8]) {
        if self.loss > 0.0 && (self.random() % 10_000) as f32 / 10_000.0 < self.loss {
            return;
        }
        self.socket.send_to(p, to);
    }

    /// Sends what is queued, resends what is due, keeps the connections alive.
    pub fn flush(&mut self, now: Instant) {
        if !self.server
            && self.connect_at.is_none_or(|at| now.duration_since(at) >= CONNECT_EVERY)
            && let Some(c) = self.conns.first_mut()
            && !c.connected
        {
            // The answer is awaited from the first request on.
            if self.connect_at.is_none() {
                c.last_recv = now;
            }
            let (addr, salt) = (c.addr, c.salt);
            self.connect_at = Some(now);
            self.raw(addr, CONNECT, salt, &[]);
        }
        let mut closed = Vec::new();
        for i in 0..self.conns.len() {
            if !self.conns[i].connected {
                continue;
            }
            if let Some(why) = self.conns[i].closing.clone() {
                let (addr, salt, id) = (self.conns[i].addr, self.conns[i].salt, self.conns[i].id);
                for _ in 0..3 {
                    self.raw(addr, CLOSE, salt, why.as_bytes());
                }
                closed.push(id);
                continue;
            }
            let packets = self.pack(i, now);
            let addr = self.conns[i].addr;
            for p in packets {
                self.transmit(addr, &p);
            }
        }
        self.conns.retain(|c| !closed.contains(&c.id));
    }

    /// The packets due on connection `i`.
    fn pack(&mut self, i: usize, now: Instant) -> Vec<Vec<u8>> {
        let c = &mut self.conns[i];
        // Chunks: (reliable id or none, bytes).
        let mut chunks: Vec<(Option<u16>, Vec<u8>)> = Vec::new();
        let resend = Duration::from_secs_f32((c.rtt * 1.5).clamp(0.05, 1.0));
        let mut budget = WINDOW.saturating_sub(c.in_flight());
        for p in c.out.iter_mut() {
            let due = p.sent.is_none_or(|t| now.duration_since(t) >= resend);
            if !due {
                continue;
            }
            if p.sent.is_none() {
                if budget < p.data.len() {
                    break;
                }
                budget -= p.data.len();
            }
            let mut b = vec![RELIABLE];
            b.extend_from_slice(&p.id.to_le_bytes());
            b.push(p.last as u8);
            b.extend_from_slice(&(p.data.len() as u16).to_le_bytes());
            b.extend_from_slice(&p.data);
            p.sent = Some(now);
            chunks.push((Some(p.id), b));
        }
        for m in std::mem::take(&mut c.unreliable) {
            let part = MTU - 40;
            let n = m.len().div_ceil(part).max(1);
            let msg = c.next_unreliable;
            c.next_unreliable = c.next_unreliable.wrapping_add(1);
            for k in 0..n {
                let d = &m[(k * part).min(m.len())..((k + 1) * part).min(m.len())];
                let mut b = vec![UNRELIABLE];
                b.extend_from_slice(&msg.to_le_bytes());
                b.push(k as u8);
                b.push(n as u8);
                b.extend_from_slice(&(d.len() as u16).to_le_bytes());
                b.extend_from_slice(d);
                chunks.push((None, b));
            }
        }
        let quiet = c.last_send.is_none_or(|t| now.duration_since(t) >= KEEPALIVE);
        if chunks.is_empty() && !quiet {
            return Vec::new();
        }
        let mut packets = Vec::new();
        let mut chunks = chunks.into_iter().peekable();
        loop {
            let mut body = Vec::new();
            let mut ids = Vec::new();
            let mut n = 0u8;
            while let Some((_, b)) = chunks.peek() {
                if n == u8::MAX || (n > 0 && body.len() + b.len() > MTU) {
                    break;
                }
                let (id, b) = chunks.next().expect("peeked");
                body.extend_from_slice(&b);
                ids.extend(id);
                n += 1;
            }
            let seq = c.seq;
            c.seq = c.seq.wrapping_add(1);
            let mut p = Vec::with_capacity(19 + body.len());
            p.extend_from_slice(&MAGIC);
            p.push(DATA);
            p.extend_from_slice(&c.salt.to_le_bytes());
            p.extend_from_slice(&seq.to_le_bytes());
            p.extend_from_slice(&c.remote_seq.to_le_bytes());
            p.extend_from_slice(&c.remote_bits.to_le_bytes());
            p.push(c.got_any as u8);
            p.push(n);
            p.extend_from_slice(&body);
            c.sent.insert(seq, (now, ids));
            packets.push(p);
            if chunks.peek().is_none() {
                break;
            }
        }
        c.last_send = Some(now);
        packets
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pump(a: &mut Endpoint, b: &mut Endpoint, now: Instant) -> (Vec<NetEvent>, Vec<NetEvent>) {
        a.flush(now);
        b.flush(now);
        std::thread::sleep(Duration::from_millis(2));
        (a.poll(now), b.poll(now))
    }

    fn pair(loss: f32) -> (Endpoint, Endpoint, ConnId, ConnId) {
        let mut s = Endpoint::server("127.0.0.1:0", 4).unwrap();
        let addr = s.local_addr().unwrap();
        let mut c = Endpoint::client(&addr.to_string()).unwrap();
        s.loss = loss;
        c.loss = loss;
        let (mut sid, mut cid) = (None, None);
        let mut now = Instant::now();
        for _ in 0..400 {
            now += Duration::from_millis(30);
            let (se, ce) = pump(&mut s, &mut c, now);
            for e in se {
                if let NetEvent::Connected(id) = e {
                    sid = Some(id);
                }
            }
            for e in ce {
                if let NetEvent::Connected(id) = e {
                    cid = Some(id);
                }
            }
            if sid.is_some() && cid.is_some() {
                break;
            }
        }
        (s, c, sid.expect("server side"), cid.expect("client side"))
    }

    #[test]
    fn reliable_messages_arrive_in_order_despite_loss() {
        let (mut s, mut c, sid, cid) = pair(0.3);
        let msgs: Vec<Vec<u8>> =
            (0..60u32).map(|i| (0..(i * 97 % 5000)).map(|k| (k ^ i) as u8).collect()).collect();
        for m in &msgs {
            s.send(sid, Channel::Reliable, m);
        }
        let mut got = Vec::new();
        let mut now = Instant::now();
        for _ in 0..3000 {
            now += Duration::from_millis(20);
            let (_, ce) = pump(&mut s, &mut c, now);
            for e in ce {
                if let NetEvent::Message(id, Channel::Reliable, m) = e {
                    assert_eq!(id, cid);
                    got.push(m);
                }
            }
            if got.len() == msgs.len() {
                break;
            }
        }
        assert_eq!(got, msgs);
    }

    #[test]
    fn unreliable_messages_are_whole_or_lost() {
        let (mut s, mut c, sid, _) = pair(0.1);
        let mut now = Instant::now();
        let mut got = 0;
        for i in 0..200u32 {
            let m: Vec<u8> = (0..4000).map(|k| (k as u32 ^ i) as u8).collect();
            c.send(0, Channel::Unreliable, &m);
            now += Duration::from_millis(20);
            let (se, _) = pump(&mut s, &mut c, now);
            for e in se {
                if let NetEvent::Message(id, Channel::Unreliable, data) = e {
                    assert_eq!(id, sid);
                    assert_eq!(data.len(), 4000);
                    let j = data[1] ^ 1;
                    assert!(data.iter().enumerate().all(|(k, &b)| b == (k as u8 ^ j)));
                    got += 1;
                }
            }
        }
        assert!(got > 50 && got < 200, "{got}");
    }

    #[test]
    fn a_silent_peer_times_out() {
        let (mut s, _c, sid, _) = pair(0.0);
        let later = Instant::now() + TIMEOUT + Duration::from_secs(1);
        let ev = s.poll(later);
        assert!(ev.iter().any(|e| matches!(e, NetEvent::Disconnected(id, _) if *id == sid)));
    }

    #[test]
    fn a_full_server_says_so() {
        let mut s = Endpoint::server("127.0.0.1:0", 0).unwrap();
        let addr = s.local_addr().unwrap();
        let mut c = Endpoint::client(&addr.to_string()).unwrap();
        let mut now = Instant::now();
        let mut why = None;
        for _ in 0..50 {
            now += Duration::from_millis(300);
            let (_, ce) = pump(&mut s, &mut c, now);
            for e in ce {
                if let NetEvent::Disconnected(_, w) = e {
                    why = Some(w);
                }
            }
            if why.is_some() {
                break;
            }
        }
        assert_eq!(why.as_deref(), Some("The server is full"));
    }

    /// A relay that has closed.
    struct Closed;

    impl Datagrams for Closed {
        fn recv_from(&mut self, _: &mut [u8]) -> std::io::Result<(usize, SocketAddr)> {
            Err(ErrorKind::WouldBlock.into())
        }
        fn send_to(&mut self, _: &[u8], _: SocketAddr) {}
        fn closed(&self) -> Option<String> {
            Some("Relay: idle".into())
        }
    }

    #[test]
    fn a_closed_relay_ends_the_connection_with_its_reason() {
        let mut c = Endpoint::client_over(Box::new(Closed), "0.0.0.0:0".parse().unwrap());
        let now = Instant::now();
        c.flush(now);
        let events = c.poll(now);
        assert!(
            matches!(&events[..], [NetEvent::Disconnected(0, why)] if why == "Relay: idle"),
            "{events:?}"
        );
        assert!(c.poll(now).is_empty());
    }
}
