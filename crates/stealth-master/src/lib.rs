//! The master server's list and its answers, apart from the HTTP server around them
//! (`main.rs`): [`List::handle`] takes a request and gives the response.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

use stealth_master_proto::{
    EXPIRE_AFTER, REPORT_EVERY, Registered, ServerEntry, ServerReport, server_id,
};

mod page;

/// Seconds a game server waits at least between two reports (more are turned down).
const MIN_REPORT_GAP: f64 = 5.0;
/// Servers listed at most, and per address.
const MAX_SERVERS: usize = 2000;
const MAX_PER_IP: usize = 16;
/// Largest request body taken.
pub const MAX_BODY: usize = 64 * 1024;

pub struct Response {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

impl Response {
    fn json<T: serde::Serialize>(status: u16, v: &T) -> Self {
        let body = serde_json::to_vec(v).unwrap_or_else(|_| b"{}".to_vec());
        Response { status, content_type: "application/json", body }
    }

    fn html(status: u16, body: String) -> Self {
        Response { status, content_type: "text/html; charset=utf-8", body: body.into_bytes() }
    }

    /// The body as text.
    pub fn text(&self) -> std::borrow::Cow<'_, str> {
        String::from_utf8_lossy(&self.body)
    }

    fn error(status: u16, why: &str) -> Self {
        Response::json(status, &serde_json::json!({ "error": why }))
    }
}

struct Listed {
    report: ServerReport,
    seen: Instant,
}

/// The servers there now, by address ("ip:port").
#[derive(Default)]
pub struct List {
    servers: HashMap<SocketAddr, Listed>,
}

impl List {
    /// One request: method, path (with the query), body, and the address it came from (after
    /// a reverse proxy: the client's).
    pub fn handle(
        &mut self,
        method: &str,
        path: &str,
        body: &[u8],
        from: IpAddr,
        now: Instant,
    ) -> Response {
        self.expire(now);
        let path = path.split('?').next().unwrap_or("");
        match (method, path.trim_end_matches('/')) {
            ("GET", "" | "/index.html") => Response::html(200, page::render(&self.entries(now))),
            ("GET", "/api/servers") => Response::json(200, &self.entries(now)),
            ("GET", p) if p.starts_with("/api/server/") => {
                match self.entry(&p["/api/server/".len()..], now) {
                    Some(e) => Response::json(200, &e),
                    None => Response::error(404, "not on the list"),
                }
            }
            ("GET", p) if p.starts_with("/server/") => {
                let entry = self.entry(&p["/server/".len()..], now);
                Response::html(
                    if entry.is_some() { 200 } else { 404 },
                    page::server_page(entry.as_ref()),
                )
            }
            ("GET", p) if p.starts_with("/maps/") => {
                let id = p["/maps/".len()..].trim_end_matches(".png");
                match page::map_picture(id) {
                    Some(png) => {
                        Response { status: 200, content_type: "image/png", body: png.to_vec() }
                    }
                    None => Response::error(404, "no such map"),
                }
            }
            ("POST", "/api/servers") => self.report(body, from, now),
            ("DELETE", p) if p.starts_with("/api/servers/") => {
                match p["/api/servers/".len()..].parse::<u16>() {
                    Ok(port) => {
                        let gone = self.servers.remove(&SocketAddr::new(from, port)).is_some();
                        Response::json(200, &serde_json::json!({ "removed": gone }))
                    }
                    Err(_) => Response::error(400, "not a port"),
                }
            }
            ("GET", "/health") => Response::json(200, &serde_json::json!({ "ok": true })),
            (_, "/api/servers") => Response::error(405, "method not allowed"),
            _ => Response::error(404, "not found"),
        }
    }

    fn report(&mut self, body: &[u8], from: IpAddr, now: Instant) -> Response {
        if body.len() > MAX_BODY {
            return Response::error(413, "too large");
        }
        let report: ServerReport = match serde_json::from_slice(body) {
            Ok(r) => r,
            Err(e) => return Response::error(400, &format!("bad report: {e}")),
        };
        if report.port == 0 || report.name.trim().is_empty() {
            return Response::error(400, "a report needs a name and a port");
        }
        let addr = SocketAddr::new(from, report.port);
        if let Some(l) = self.servers.get(&addr)
            && now.duration_since(l.seen).as_secs_f64() < MIN_REPORT_GAP
        {
            return Response::error(429, "too often");
        }
        if !self.servers.contains_key(&addr) {
            let same_ip = self.servers.keys().filter(|a| a.ip() == from).count();
            if same_ip >= MAX_PER_IP || self.servers.len() >= MAX_SERVERS {
                return Response::error(429, "too many servers");
            }
        }
        self.servers.insert(addr, Listed { report: report.trimmed(), seen: now });
        Response::json(200, &Registered { every: REPORT_EVERY })
    }

    fn expire(&mut self, now: Instant) {
        let after = Duration::from_secs(EXPIRE_AFTER);
        self.servers.retain(|_, l| now.duration_since(l.seen) < after);
    }

    /// The server with this id.
    fn entry(&self, id: &str, now: Instant) -> Option<ServerEntry> {
        self.entries(now).into_iter().find(|e| e.id == id)
    }

    /// The servers, the most players first.
    pub fn entries(&self, now: Instant) -> Vec<ServerEntry> {
        let mut out: Vec<ServerEntry> = self
            .servers
            .iter()
            .map(|(addr, l)| ServerEntry {
                id: server_id(&l.report.name, &addr.ip().to_string()),
                addr: addr.to_string(),
                age: now.duration_since(l.seen).as_secs(),
                report: l.report.clone(),
            })
            .collect();
        out.sort_by(|a, b| {
            b.report
                .players
                .len()
                .cmp(&a.report.players.len())
                .then_with(|| a.report.name.to_lowercase().cmp(&b.report.name.to_lowercase()))
                .then_with(|| a.addr.cmp(&b.addr))
        });
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stealth_master_proto::ReportPlayer;

    fn report(name: &str, port: u16, players: usize) -> Vec<u8> {
        let r = ServerReport {
            name: name.into(),
            port,
            max_players: 16,
            theater: "Central Europe".into(),
            players: (0..players)
                .map(|k| ReportPlayer { name: format!("P{k}"), ..Default::default() })
                .collect(),
            ..Default::default()
        };
        serde_json::to_vec(&r).unwrap()
    }

    const A: IpAddr = IpAddr::V4(std::net::Ipv4Addr::new(203, 0, 113, 5));
    const B: IpAddr = IpAddr::V6(std::net::Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1));

    #[test]
    fn servers_report_are_listed_and_expire() {
        let mut l = List::default();
        let t = Instant::now();
        let r = l.handle("POST", "/api/servers", &report("Alpha", 28989, 1), A, t);
        assert_eq!(r.status, 200, "{}", r.text());
        let r = l.handle("POST", "/api/servers", &report("Bravo", 29000, 3), B, t);
        assert_eq!(r.status, 200);
        let r = l.handle("GET", "/api/servers", b"", A, t);
        let list: Vec<ServerEntry> = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].report.name, "Bravo", "the most players first");
        assert_eq!(list[0].addr, "[2001:db8::1]:29000");
        assert_eq!(list[1].addr, "203.0.113.5:28989");
        // Too soon again; later it refreshes.
        let r = l.handle("POST", "/api/servers", &report("Alpha", 28989, 2), A, t);
        assert_eq!(r.status, 429);
        let later = t + Duration::from_secs(60);
        let r = l.handle("POST", "/api/servers", &report("Alpha", 28989, 2), A, later);
        assert_eq!(r.status, 200);
        // Bravo was not heard of for 90 s: gone.
        let r = l.handle("GET", "/api/servers", b"", A, t + Duration::from_secs(95));
        let list: Vec<ServerEntry> = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].report.players.len(), 2);
    }

    #[test]
    fn a_server_leaves_and_bad_reports_are_turned_down() {
        let mut l = List::default();
        let t = Instant::now();
        l.handle("POST", "/api/servers", &report("Alpha", 28989, 1), A, t);
        // Only from its own address.
        l.handle("DELETE", "/api/servers/28989", b"", B, t);
        assert_eq!(l.entries(t).len(), 1);
        l.handle("DELETE", "/api/servers/28989", b"", A, t);
        assert!(l.entries(t).is_empty());
        assert_eq!(l.handle("POST", "/api/servers", b"{nope", A, t).status, 400);
        assert_eq!(l.handle("POST", "/api/servers", &report("", 1, 0), A, t).status, 400);
        assert_eq!(l.handle("PUT", "/api/servers", b"", A, t).status, 405);
        assert_eq!(l.handle("GET", "/nothing", b"", A, t).status, 404);
    }

    #[test]
    fn the_page_lists_the_servers_escaped() {
        let mut l = List::default();
        let t = Instant::now();
        l.handle("POST", "/api/servers", &report("<b>Evil</b> & co", 28989, 2), A, t);
        let r = l.handle("GET", "/", b"", A, t);
        assert_eq!(r.content_type, "text/html; charset=utf-8");
        assert!(r.text().contains("&lt;b&gt;Evil&lt;/b&gt; &amp; co"));
        assert!(!r.text().contains("<b>Evil"));
        assert!(r.text().contains("P1"));
    }

    #[test]
    fn a_server_has_a_page_with_its_live_map() {
        let mut l = List::default();
        let t = Instant::now();
        let mut r: ServerReport = serde_json::from_slice(&report("Alpha", 28989, 2)).unwrap();
        r.map = Some(stealth_master_proto::LiveMap {
            theater: "ce".into(),
            places: vec![stealth_master_proto::MapPlace {
                name: "Fulda".into(),
                x: 12000,
                y: 15000,
                side: "Enemy".into(),
                airfield: true,
            }],
        });
        r.players[0].pos =
            Some(stealth_master_proto::MapPos { x: 11000, y: 16000, heading: 45, alt: 500 });
        l.handle("POST", "/api/servers", &serde_json::to_vec(&r).unwrap(), A, t);
        let id = l.entries(t)[0].id.clone();
        assert_eq!(id, server_id("Alpha", "203.0.113.5"));
        let page = l.handle("GET", &format!("/server/{id}"), b"", A, t);
        assert_eq!(page.status, 200);
        assert!(page.text().contains("/maps/ce.png"));
        assert!(page.text().contains("Fulda"));
        assert!(page.text().contains("rotate(45"));
        let one = l.handle("GET", &format!("/api/server/{id}"), b"", A, t);
        let e: ServerEntry = serde_json::from_slice(&one.body).unwrap();
        assert_eq!(e.report.players[0].pos.unwrap().x, 11000);
        assert!(l.handle("GET", "/", b"", A, t).text().contains(&format!("/server/{id}")));
        assert_eq!(l.handle("GET", "/server/0000", b"", A, t).status, 404);
        let png = l.handle("GET", "/maps/ce.png", b"", A, t);
        assert_eq!((png.status, png.content_type), (200, "image/png"));
        assert_eq!(l.handle("GET", "/maps/xx.png", b"", A, t).status, 404);
    }
}
