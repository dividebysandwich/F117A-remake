//! The server list as a web page (no scripts): a row per server with its name and players,
//! opened for the theater, the campaign's clock and settings, the places each side holds and
//! who plays.

use std::fmt::Write;

use stealth_master_proto::ServerEntry;

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
         <meta http-equiv=\"refresh\" content=\"30\">\
         <title>Stealth Mission 89 servers</title><style>{STYLE}</style></head><body><main>\
         <header><h1>Stealth Mission <span>89</span> servers</h1>\
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
            server(&mut h, s);
        }
        h.push_str("</div>");
    }
    h.push_str(
        "<footer>Join from the game: MULTIPLAYER, JOIN A GAME. The list refreshes every 30 \
         seconds; game servers report to <code>/api/servers</code>.</footer></main></body></html>",
    );
    h
}

fn server(h: &mut String, s: &ServerEntry) {
    let r = &s.report;
    let n = r.players.len();
    let full = r.max_players > 0 && n >= r.max_players as usize;
    let _ = write!(
        h,
        "<details><summary><span class=\"name\">{}{}</span>\
         <span class=\"count{}\">{n}/{}</span><span class=\"theater\">{}</span></summary>\
         <div class=\"body\">",
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
    h.push_str("</section></div></details>");
}
