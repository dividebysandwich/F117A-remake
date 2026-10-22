//! The master server: game servers report to it, games list them (`stealth-master-proto`).
//! It serves plain HTTP; HTTPS comes from a reverse proxy in front of it (`deploy/`, or
//! Cloudflare), whose `CF-Connecting-IP` or `X-Forwarded-For` it believes when the request
//! comes from this machine or its LAN.
//!
//! The relay for the browser build (`relay.rs`) takes WebSockets on a port of its own, to
//! which the proxy sends `/relay/`.
//!
//! `stealth-master [--listen <addr:port>] [--threads <n>] [--relay-listen <addr:port>|off]
//! [--relay-internal]` (default 127.0.0.1:8089, 4, 127.0.0.1:8090).

use std::io::Read;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use stealth_master::{List, MAX_BODY, is_internal, relay};
use tiny_http::{Header, Method, Request, Response, Server};

const USAGE: &str = "stealth-master [--listen <addr:port>] [--threads <n>] \
                     [--relay-listen <addr:port>|off] [--relay-internal]";

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
    let relay_listen = get("--relay-listen").unwrap_or_else(|| "127.0.0.1:8090".into());
    if relay_listen != "off" {
        let listener = match std::net::TcpListener::bind(&relay_listen) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("relay listening on {relay_listen}: {e}");
                std::process::exit(1);
            }
        };
        eprintln!("relay listening on ws://{relay_listen}/relay/<server id>");
        let allow_internal = args.iter().any(|a| a == "--relay-internal");
        let list = list.clone();
        std::thread::spawn(move || relay::serve(listener, list, allow_internal));
    }
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

/// The client's address: the peer's, or behind proxies the one they forward.
fn client_ip(req: &Request) -> Option<IpAddr> {
    let peer = req.remote_addr()?.ip();
    Some(stealth_master::client_ip(peer, |name| header(req, name)))
}

fn header<'r>(req: &'r Request, name: &str) -> Option<&'r str> {
    req.headers()
        .iter()
        .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str())
}

fn serve(mut req: Request, list: &Mutex<List>) {
    let Some(from) = client_ip(&req) else { return };
    if is_internal(from) && req.method() == &Method::Post {
        eprintln!("{from} reports from a LAN address: a server not joinable from the internet");
    }
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
            body: b"{\"error\":\"unreadable\"}".to_vec(),
        }
    };
    if r.status >= 400 && r.status != 404 {
        eprintln!("{from} {method} {url}: {} {}", r.status, r.text());
    }
    let header = |k: &str, v: &str| Header::from_bytes(k.as_bytes(), v.as_bytes()).ok();
    // The map pictures never change while the master server runs.
    let cache = if r.content_type == "image/png" { "public, max-age=86400" } else { "no-store" };
    let content_type = r.content_type;
    let mut resp = Response::from_data(r.body).with_status_code(r.status);
    for h in [
        header("Content-Type", content_type),
        header("Cache-Control", cache),
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
