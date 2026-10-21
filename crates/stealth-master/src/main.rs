//! The master server: game servers report to it, games list them (`stealth-master-proto`).
//! It serves plain HTTP; HTTPS comes from a reverse proxy in front of it (`deploy/`), whose
//! `X-Forwarded-For` it believes when the request comes from this machine.
//!
//! `stealth-master [--listen <addr:port>] [--threads <n>]` (default 127.0.0.1:8089, 4).

use std::io::Read;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use stealth_master::{List, MAX_BODY};
use tiny_http::{Header, Request, Response, Server};

const USAGE: &str = "stealth-master [--listen <addr:port>] [--threads <n>]";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return;
    }
    let get = |f: &str| args.iter().position(|a| a == f).and_then(|i| args.get(i + 1)).cloned();
    let listen = get("--listen").unwrap_or_else(|| "127.0.0.1:8089".into());
    let threads: usize = get("--threads").and_then(|t| t.parse().ok()).unwrap_or(4).max(1);
    let server = match Server::http(&listen) {
        Ok(s) => Arc::new(s),
        Err(e) => {
            eprintln!("listening on {listen}: {e}");
            std::process::exit(1);
        }
    };
    eprintln!("stealth-master listening on http://{listen}");
    let list = Arc::new(Mutex::new(List::default()));
    let workers: Vec<_> = (0..threads)
        .map(|_| {
            let (server, list) = (server.clone(), list.clone());
            std::thread::spawn(move || {
                for req in server.incoming_requests() {
                    serve(req, &list);
                }
            })
        })
        .collect();
    for w in workers {
        let _ = w.join();
    }
}

/// The client's address: the peer's, or behind a proxy on this machine the one it forwards.
fn client_ip(req: &Request) -> Option<IpAddr> {
    let peer = req.remote_addr()?.ip();
    if !peer.is_loopback() {
        return Some(peer);
    }
    let forwarded = req
        .headers()
        .iter()
        .find(|h| h.field.equiv("X-Forwarded-For"))
        .and_then(|h| h.value.as_str().split(',').next_back())
        .and_then(|ip| ip.trim().parse().ok());
    Some(forwarded.unwrap_or(peer))
}

fn serve(mut req: Request, list: &Mutex<List>) {
    let Some(from) = client_ip(&req) else { return };
    let mut body = Vec::new();
    let ok = req.as_reader().take(MAX_BODY as u64 + 1).read_to_end(&mut body).is_ok();
    let method = req.method().as_str().to_string();
    let url = req.url().to_string();
    let r = if ok {
        match list.lock() {
            Ok(mut l) => l.handle(&method, &url, &body, from, Instant::now()),
            Err(_) => return,
        }
    } else {
        stealth_master::Response {
            status: 400,
            content_type: "application/json",
            body: "{\"error\":\"unreadable\"}".into(),
        }
    };
    if r.status >= 400 && r.status != 404 {
        eprintln!("{from} {method} {url}: {} {}", r.status, r.body);
    }
    let header = |k: &str, v: &str| Header::from_bytes(k.as_bytes(), v.as_bytes()).ok();
    let mut resp = Response::from_string(r.body).with_status_code(r.status);
    for h in [
        header("Content-Type", r.content_type),
        header("Cache-Control", "no-store"),
        header("Access-Control-Allow-Origin", "*"),
        header("X-Content-Type-Options", "nosniff"),
    ]
    .into_iter()
    .flatten()
    {
        resp.add_header(h);
    }
    let _ = req.respond(resp);
}
