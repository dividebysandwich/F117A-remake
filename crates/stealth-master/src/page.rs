//! The server list as a web page: a row per server with its name and players, opened for the
//! theater, the campaign's clock and settings, the places each side holds, who plays and the
//! live map (the places and the flying players over the theater's picture). A server's own
//! page (`/server/<id>`) shows it opened with a bigger map. A little script fetches the page
//! again every few seconds and swaps in what changed (without it, the page reloads).

use std::fmt::Write;

use stealth_master_proto::{MapPos, ServerEntry};

/// The theaters' map pictures (`--export-maps` in the game), 0..0x8000 map units both ways.
const MAPS: [(&str, &[u8]); 10] = [
    ("lb", include_bytes!("../maps/lb.png")),
    ("pg", include_bytes!("../maps/pg.png")),
    ("nc", include_bytes!("../maps/nc.png")),
    ("ce", include_bytes!("../maps/ce.png")),
    ("vn", include_bytes!("../maps/vn.png")),
    ("me", include_bytes!("../maps/me.png")),
    ("ku", include_bytes!("../maps/ku.png")),
    ("cu", include_bytes!("../maps/cu.png")),
    ("ko", include_bytes!("../maps/ko.png")),
    ("ua", include_bytes!("../maps/ua.png")),
];
/// Map units across the theater.
const THEATER: f32 = 32768.0;
/// Seconds between the page's looks for news.
const POLL_MS: u32 = 5000;

/// Theater `id`'s map picture.
pub fn map_picture(id: &str) -> Option<&'static [u8]> {
    MAPS.iter().find(|m| m.0 == id).map(|m| m.1)
}

/// Text for HTML.
fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

const STYLE: &str = r#"
:root {
  --bg: #f4f2ec; --panel: #fffdf8; --ink: #1d221c; --dim: #5d665a; --line: #d9d5c8;
  --accent: #2f6b3a; --amber: #8a5a00; --blue: #2b5c9e; --red: #a3352b; --neutral: #6d6a60;
  --row: #f9f7f1;
}
@media (prefers-color-scheme: dark) {
  :root {
    --bg: #0b100c; --panel: #111a13; --ink: #d7e8d2; --dim: #84967f; --line: #22322a;
    --accent: #6fd27f; --amber: #e0a63a; --blue: #6fa4ef; --red: #ef7a6c; --neutral: #a8a597;
    --row: #0e1610;
  }
}
* { box-sizing: border-box; }
body { margin: 0; background: var(--bg); color: var(--ink);
  font: 15px/1.45 ui-sans-serif, system-ui, -apple-system, "Segoe UI", sans-serif; }
main { max-width: 960px; margin: 0 auto; padding: 28px 16px 48px; }
header { display: flex; align-items: baseline; justify-content: space-between; gap: 12px;
  flex-wrap: wrap; border-bottom: 2px solid var(--accent); padding-bottom: 10px; margin-bottom: 18px; }
h1 { margin: 0; font: 700 22px/1.2 ui-monospace, "SFMono-Regular", Menlo, Consolas, monospace;
  letter-spacing: .06em; text-transform: uppercase; }
h1 span { color: var(--accent); }
.summary { color: var(--dim); font-size: 14px; }
.summary b { color: var(--ink); font-variant-numeric: tabular-nums; }
.head, details > summary { display: grid; grid-template-columns: 1fr 9ch 16ch; gap: 12px; align-items: center; }
.head { color: var(--dim); font-size: 12px; text-transform: uppercase; letter-spacing: .08em;
  padding: 0 14px 6px; }
.list { display: flex; flex-direction: column; gap: 8px; }
details { background: var(--panel); border: 1px solid var(--line); border-radius: 6px; }
details[open] { border-color: var(--accent); }
summary { cursor: pointer; list-style: none; padding: 12px 14px; }
summary::-webkit-details-marker { display: none; }
summary:hover { background: var(--row); }
.name { font-weight: 600; overflow-wrap: anywhere; }
.name::before { content: "\25B8"; display: inline-block; width: 1.1em; text-align: center;
  margin-right: 2px; color: var(--accent); transition: transform .15s; }
details[open] .name::before { transform: rotate(90deg); }
.tag { font: 600 11px/1 ui-monospace, Menlo, Consolas, monospace; color: var(--dim);
  border: 1px solid var(--line); border-radius: 3px; padding: 2px 5px; margin-left: 6px;
  vertical-align: 2px; }
.count { font: 600 15px ui-monospace, Menlo, Consolas, monospace; font-variant-numeric: tabular-nums; }
.count.full { color: var(--amber); }
.theater { color: var(--dim); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.body { border-top: 1px solid var(--line); padding: 14px; display: grid;
  grid-template-columns: repeat(auto-fit, minmax(230px, 1fr)); gap: 16px 24px; }
h2 { margin: 0 0 6px; font-size: 12px; color: var(--dim); text-transform: uppercase; letter-spacing: .08em; font-weight: 600; }
dl { margin: 0; display: grid; grid-template-columns: max-content 1fr; gap: 3px 12px; }
dt { color: var(--dim); }
dd { margin: 0; overflow-wrap: anywhere; }
table { border-collapse: collapse; width: 100%; font-variant-numeric: tabular-nums; }
td, th { text-align: left; padding: 3px 8px 3px 0; }
th { color: var(--dim); font-weight: 500; font-size: 13px; }
td.n { text-align: right; padding-right: 12px; }
.blue { color: var(--blue); } .red { color: var(--red); } .neutral { color: var(--neutral); }
ul { margin: 0; padding: 0; list-style: none; }
li { padding: 2px 0; display: flex; gap: 8px; flex-wrap: wrap; }
li .who { font-weight: 600; }
li .what { color: var(--dim); }
.addr { font: 13px ui-monospace, Menlo, Consolas, monospace; color: var(--dim); }
.empty { background: var(--panel); border: 1px dashed var(--line); border-radius: 6px;
  padding: 28px 16px; text-align: center; color: var(--dim); }
.map { grid-column: 1 / -1; }
.map svg { display: block; width: 100%; height: auto; aspect-ratio: 1; background: #06160b;
  border: 1px solid var(--line); border-radius: 4px; }
.map svg text { font-family: ui-monospace, Menlo, Consolas, monospace; paint-order: stroke;
  stroke: #000; stroke-linejoin: round; }
.map .caption { display: flex; justify-content: space-between; gap: 12px; flex-wrap: wrap;
  margin-top: 6px; color: var(--dim); font-size: 13px; }
.map .caption a { color: var(--accent); }
.key { display: inline-flex; gap: 12px; flex-wrap: wrap; }
.key i { font-style: normal; }
.single main { max-width: 1100px; }
.back { color: var(--accent); text-decoration: none; font-size: 14px; }
footer { margin-top: 24px; color: var(--dim); font-size: 13px; }
footer code { font-size: 12px; }
@media (max-width: 560px) {
  .head, details > summary { grid-template-columns: 1fr 7ch; }
  .head .t, .theater { display: none; }
}
"#;

/// The page for these servers.
pub fn render(servers: &[ServerEntry]) -> String {
    let players: usize = servers.iter().map(|s| s.report.players.len()).sum();
    let mut h = String::new();
    let _ = write!(
        h,
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <noscript><meta http-equiv=\"refresh\" content=\"30\"></noscript>\
         <title>Stealth Mission 89 servers</title><style>{STYLE}</style></head><body><main>\
         <div id=\"live\"><header><h1>Stealth Mission <span>89</span> servers</h1>\
         <div class=\"summary\"><b>{}</b> server{} &middot; <b>{players}</b> pilot{} flying coop</div>\
         </header>",
        servers.len(),
        if servers.len() == 1 { "" } else { "s" },
        if players == 1 { "" } else { "s" },
    );
    if servers.is_empty() {
        h.push_str("<div class=\"empty\">No servers are up right now.</div>");
    } else {
        h.push_str(
            "<div class=\"head\"><span>Server</span><span>Players</span><span class=\"t\">Theater</span></div>\
             <div class=\"list\">",
        );
        for s in servers {
            server(&mut h, s, false);
        }
        h.push_str("</div>");
    }
    h.push_str(
        "</div><footer>Join from the game: MULTIPLAYER, JOIN A GAME. The list and the maps \
         follow the servers' reports live; game servers report to <code>/api/servers</code>.\
         </footer></main>",
    );
    script(&mut h);
    h.push_str("</body></html>");
    h
}

/// A server's own page: it opened, with a big live map (or a note while it is not listed).
pub fn server_page(entry: Option<&ServerEntry>) -> String {
    let title = entry.map_or("Server".to_string(), |e| esc(&e.report.name));
    let mut h = String::new();
    let _ = write!(
        h,
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <noscript><meta http-equiv=\"refresh\" content=\"30\"></noscript>\
         <title>{title} - Stealth Mission 89</title><style>{STYLE}</style></head>\
         <body class=\"single\"><main><a class=\"back\" href=\"/\">&larr; All servers</a>\
         <div id=\"live\"><header><h1>{title}</h1>",
    );
    match entry {
        Some(e) => {
            let n = e.report.players.len();
            let flying = e.report.players.iter().filter(|p| p.pos.is_some()).count();
            let _ = write!(
                h,
                "<div class=\"summary\"><b>{n}</b>/{} pilot{} &middot; <b>{flying}</b> in the air\
                 </div></header><div class=\"list\">",
                e.report.max_players,
                if n == 1 { "" } else { "s" },
            );
            server(&mut h, e, true);
            h.push_str("</div>");
        }
        None => h.push_str(
            "</header><div class=\"empty\">This server is not on the list right now. The page \
             shows it again as soon as it reports.</div>",
        ),
    }
    h.push_str("</div></main>");
    script(&mut h);
    h.push_str("</body></html>");
    h
}

/// Fetches the page again now and then and swaps in its live part, keeping the opened servers
/// open.
fn script(h: &mut String) {
    let _ = write!(
        h,
        "<script>(()=>{{const tick=async()=>{{try{{\
         const r=await fetch(location.pathname,{{cache:'no-store'}});\
         const doc=new DOMParser().parseFromString(await r.text(),'text/html');\
         const fresh=doc.getElementById('live'),live=document.getElementById('live');\
         if(!fresh||!live)return;\
         const open=new Set([...live.querySelectorAll('details')].filter(d=>d.open).map(d=>d.dataset.id));\
         const known=new Set([...live.querySelectorAll('details')].map(d=>d.dataset.id));\
         fresh.querySelectorAll('details').forEach(d=>{{if(known.has(d.dataset.id))d.open=open.has(d.dataset.id);}});\
         live.replaceWith(document.adoptNode(fresh));}}catch(e){{}}}};\
         setInterval(tick,{POLL_MS});}})();</script>"
    );
}

fn server(h: &mut String, s: &ServerEntry, open: bool) {
    let r = &s.report;
    let n = r.players.len();
    let full = r.max_players > 0 && n >= r.max_players as usize;
    let _ = write!(
        h,
        "<details data-id=\"{}\"{}><summary><span class=\"name\">{}{}</span>\
         <span class=\"count{}\">{n}/{}</span><span class=\"theater\">{}</span></summary>\
         <div class=\"body\">",
        esc(&s.id),
        if open { " open" } else { "" },
        esc(&r.name),
        if r.dedicated { "<span class=\"tag\">DEDICATED</span>" } else { "" },
        if full { " full" } else { "" },
        r.max_players,
        esc(&r.theater),
    );
    // The campaign.
    let rules = if r.rules.is_empty() { "Classic".to_string() } else { r.rules.join(", ") };
    let _ = write!(
        h,
        "<section><h2>Campaign</h2><dl>\
         <dt>Theater</dt><dd>{}</dd><dt>Clock</dt><dd>{}</dd>\
         <dt>Situation</dt><dd>{}</dd><dt>Enemy</dt><dd>{}</dd>\
         <dt>Rules</dt><dd>{}</dd><dt>Address</dt><dd class=\"addr\">{}</dd></dl></section>",
        esc(&r.theater),
        esc(&r.clock),
        esc(&r.situation),
        esc(&r.opponents),
        esc(&rules),
        esc(&s.addr),
    );
    // The front.
    h.push_str(
        "<section><h2>Places held</h2><table><tr><th>Side</th><th>Places</th><th>Airfields</th></tr>",
    );
    for side in &r.sides {
        let class = match side.side.as_str() {
            "Friendly" => "blue",
            "Enemy" => "red",
            _ => "neutral",
        };
        let _ = write!(
            h,
            "<tr><td class=\"{class}\">{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td></tr>",
            esc(&side.side),
            side.places,
            side.airfields
        );
    }
    h.push_str("</table></section>");
    // Who plays.
    h.push_str("<section><h2>Pilots</h2>");
    if r.players.is_empty() {
        h.push_str("<p class=\"what\" style=\"margin:0;color:var(--dim)\">Nobody yet</p>");
    } else {
        h.push_str("<ul>");
        for p in &r.players {
            let _ = write!(
                h,
                "<li><span class=\"who\">{}</span>{}<span class=\"what\">{}</span></li>",
                esc(&p.name),
                if p.host { "<span class=\"tag\">HOST</span>" } else { "" },
                esc(&p.status),
            );
        }
        h.push_str("</ul>");
    }
    h.push_str("</section>");
    map(h, s, open);
    h.push_str("</div></details>");
}

/// A side's colour on the map.
fn side_colour(side: &str) -> &'static str {
    match side {
        "Friendly" => "#5aa0ff",
        "Enemy" => "#ff4632",
        _ => "#b4b0a0",
    }
}

/// The live map: the theater's picture around the places and the flying players.
fn map(h: &mut String, s: &ServerEntry, own_page: bool) {
    let Some(m) = &s.report.map else { return };
    let Some(theater) = MAPS.iter().find(|t| t.0 == m.theater).map(|t| t.0) else { return };
    let pilots: Vec<(&str, MapPos)> =
        s.report.players.iter().filter_map(|p| Some((p.name.as_str(), p.pos?))).collect();
    // The view: a square around everything, inside the theater.
    let pts = m.places.iter().map(|p| (p.x, p.y)).chain(pilots.iter().map(|p| (p.1.x, p.1.y)));
    let (mut lo, mut hi) = ((f32::MAX, f32::MAX), (f32::MIN, f32::MIN));
    for (x, y) in pts {
        let (x, y) = (x as f32, y as f32);
        lo = (lo.0.min(x), lo.1.min(y));
        hi = (hi.0.max(x), hi.1.max(y));
    }
    let (side, x0, y0) = if lo.0 > hi.0 {
        (THEATER, 0.0, 0.0)
    } else {
        let side = ((hi.0 - lo.0).max(hi.1 - lo.1) * 1.2).clamp(4096.0, THEATER);
        let at = |lo: f32, hi: f32| ((lo + hi - side) / 2.0).clamp(0.0, THEATER - side);
        (side, at(lo.0, hi.0), at(lo.1, hi.1))
    };
    let u = side / 100.0;
    let _ = write!(
        h,
        "<section class=\"map\"><h2>Live map</h2>\
         <svg viewBox=\"{x0:.0} {y0:.0} {side:.0} {side:.0}\" role=\"img\" \
         aria-label=\"Map of the campaign with the flying players\">\
         <image href=\"/maps/{theater}.png\" x=\"0\" y=\"0\" width=\"{THEATER}\" \
         height=\"{THEATER}\" style=\"image-rendering:pixelated\"/>\
         <rect x=\"0\" y=\"0\" width=\"{THEATER}\" height=\"{THEATER}\" fill=\"#000\" \
         opacity=\"0.3\"/>",
    );
    // The places: airfields squares, the others rings.
    let font = u * 1.6;
    for p in &m.places {
        let col = side_colour(&p.side);
        let (x, y) = (p.x as f32, p.y as f32);
        if p.airfield {
            let a = u * 0.9;
            let _ = write!(
                h,
                "<rect x=\"{:.0}\" y=\"{:.0}\" width=\"{:.0}\" height=\"{:.0}\" fill=\"{col}\" \
                 fill-opacity=\"0.45\" stroke=\"{col}\" stroke-width=\"{:.0}\"/>",
                x - a,
                y - a,
                2.0 * a,
                2.0 * a,
                u * 0.3,
            );
        } else {
            let _ = write!(
                h,
                "<circle cx=\"{x:.0}\" cy=\"{y:.0}\" r=\"{:.0}\" fill=\"{col}\" \
                 fill-opacity=\"0.45\" stroke=\"{col}\" stroke-width=\"{:.0}\"/>",
                u * 0.8,
                u * 0.3,
            );
        }
        let _ = write!(
            h,
            "<text x=\"{:.0}\" y=\"{:.0}\" font-size=\"{font:.0}\" fill=\"{col}\" \
             stroke-width=\"{:.0}\">{}</text>",
            x + u * 1.3,
            y - u * 0.6,
            font * 0.25,
            esc(&p.name),
        );
    }
    // The players in the air: an arrowhead along the heading, name and altitude.
    for (name, p) in &pilots {
        let a = u * 1.6;
        let _ = write!(
            h,
            "<g transform=\"translate({} {}) rotate({})\"><polygon points=\"0,{:.0} {:.0},{:.0} \
             0,{:.0} {:.0},{:.0}\" fill=\"#ffd34d\" stroke=\"#000\" stroke-width=\"{:.0}\"/></g>\
             <text x=\"{:.0}\" y=\"{:.0}\" font-size=\"{:.0}\" font-weight=\"700\" \
             fill=\"#ffd34d\" stroke-width=\"{:.0}\">{}<tspan font-weight=\"400\" \
             fill=\"#fff3c4\"> {} ft</tspan></text>",
            p.x,
            p.y,
            p.heading,
            -a,
            a * 0.7,
            a * 0.8,
            a * 0.35,
            -a * 0.7,
            a * 0.8,
            u * 0.25,
            p.x as f32 + a,
            p.y as f32 + a * 0.4,
            font * 1.15,
            font * 0.3,
            esc(name),
            thousands(p.alt.max(0)),
        );
    }
    h.push_str(
        "</svg><div class=\"caption\"><span class=\"key\">\
        <i style=\"color:#5aa0ff\">&#9632; friendly</i><i style=\"color:#ff4632\">&#9632; enemy</i>\
        <i style=\"color:#b4b0a0\">&#9632; neutral</i><i>&#9632; airfield &#9679; town</i>\
        <i style=\"color:#e0a63a\">&#9650; player in the air</i></span>",
    );
    let _ = write!(h, "<span>Reported {}</span>", ago(s.age));
    if !own_page {
        let _ = write!(h, "<a href=\"/server/{}\">Open the live map page</a>", esc(&s.id));
    }
    h.push_str("</div></section>");
}

/// "12,400".
fn thousands(n: i32) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn ago(secs: u64) -> String {
    match secs {
        0..=1 => "just now".into(),
        s => format!("{s} s ago"),
    }
}
