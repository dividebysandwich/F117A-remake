//! The dedicated server's console: a terminal screen (ratatui) with the uptime, the places
//! each side holds, the players and the recent events, or plain log lines when the output is
//! not a terminal.

use std::collections::VecDeque;
use std::io::IsTerminal;
use std::time::{Duration, Instant};

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, List, ListItem, Paragraph, Row, Table};

use crate::protocol::{PlayerInfo, PlayerStatus};

/// Log lines kept for the screen.
const KEEP: usize = 1000;
/// The screen is drawn this often.
const REDRAW: Duration = Duration::from_millis(250);

/// What the screen shows of the server.
#[derive(Debug, Clone, Default)]
pub struct Status {
    pub name: String,
    pub port: u16,
    pub theater: String,
    /// Campaign day and time ("DAY 3 14:05").
    pub clock: String,
    /// Places held (cities and airfields) and airfields among them: blue, red, neutral.
    pub places: [(usize, usize); 3],
    pub players: Vec<PlayerInfo>,
    pub max_players: usize,
    pub rules: String,
    pub save: String,
}

pub struct Console {
    tui: Option<DefaultTerminal>,
    started: Instant,
    log: VecDeque<(String, String)>,
    drawn: Option<Instant>,
}

/// Time of day (UTC) for the log.
pub fn stamp() -> String {
    let s = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    format!("{:02}:{:02}:{:02}", s / 3600 % 24, s / 60 % 60, s % 60)
}

impl Console {
    /// The terminal screen when the output is a terminal (and `plain` is not asked for).
    pub fn new(plain: bool) -> Self {
        let tui = (!plain && std::io::stdout().is_terminal()).then(ratatui::init);
        Self { tui, started: Instant::now(), log: VecDeque::new(), drawn: None }
    }

    pub fn is_screen(&self) -> bool {
        self.tui.is_some()
    }

    /// A log line (timestamped).
    pub fn say(&mut self, text: impl Into<String>) {
        let text = text.into();
        let t = stamp();
        if self.tui.is_none() {
            println!("[{t}] {text}");
        }
        self.log.push_back((t, text));
        while self.log.len() > KEEP {
            self.log.pop_front();
        }
    }

    /// Keys since the last call: true when the operator asked to stop (q, Esc, Ctrl-C).
    pub fn stop_asked(&mut self) -> bool {
        if self.tui.is_none() {
            return false;
        }
        let mut stop = false;
        while event::poll(Duration::ZERO).unwrap_or(false) {
            if let Ok(Event::Key(k)) = event::read()
                && k.kind == KeyEventKind::Press
            {
                stop |= matches!(k.code, KeyCode::Char('q') | KeyCode::Esc)
                    || (k.code == KeyCode::Char('c')
                        && k.modifiers.contains(KeyModifiers::CONTROL));
            }
        }
        stop
    }

    /// Draws the screen now and then.
    pub fn draw(&mut self, s: &Status) {
        let now = Instant::now();
        if self.drawn.is_some_and(|t| now.duration_since(t) < REDRAW) {
            return;
        }
        self.drawn = Some(now);
        let uptime = uptime(now.duration_since(self.started));
        let log = &self.log;
        let Some(t) = self.tui.as_mut() else { return };
        let _ = t.draw(|f| render(f, s, log, &uptime));
    }

    /// Back to the plain terminal (the log so far printed on it).
    pub fn close(&mut self) {
        if self.tui.take().is_some() {
            ratatui::restore();
            for (t, text) in self.log.iter().skip(self.log.len().saturating_sub(20)) {
                println!("[{t}] {text}");
            }
        }
    }
}

impl Drop for Console {
    fn drop(&mut self) {
        self.close();
    }
}

/// The screen.
fn render(f: &mut ratatui::Frame, s: &Status, log: &VecDeque<(String, String)>, uptime: &str) {
    let [head, sides, players, events] = Layout::vertical([
        Constraint::Length(4),
        Constraint::Length(5),
        Constraint::Length(s.players.len().clamp(1, 24) as u16 + 3),
        Constraint::Min(5),
    ])
    .areas(f.area());
    let title = Line::from(vec![
        Span::styled(format!(" {} ", s.name), Style::new().bold().fg(Color::LightGreen)),
        Span::raw(format!(" UDP {}  ", s.port)),
    ]);
    let info = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("Uptime ", Style::new().fg(Color::DarkGray)),
            Span::styled(uptime.to_string(), Style::new().bold()),
            Span::styled("   Theater ", Style::new().fg(Color::DarkGray)),
            Span::raw(s.theater.clone()),
            Span::styled("   Campaign ", Style::new().fg(Color::DarkGray)),
            Span::raw(s.clock.clone()),
        ]),
        Line::from(vec![
            Span::styled("Rules ", Style::new().fg(Color::DarkGray)),
            Span::raw(s.rules.clone()),
            Span::styled("   Save ", Style::new().fg(Color::DarkGray)),
            Span::raw(s.save.clone()),
        ]),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .title_bottom(Line::from(" q: stop and save ").right_aligned()),
    );
    f.render_widget(info, head);
    let side = |name: &str, (n, a): (usize, usize), c: Color| {
        Row::new(vec![
            Cell::from(name.to_string()).style(Style::new().fg(c).bold()),
            Cell::from(n.to_string()),
            Cell::from(a.to_string()),
            Cell::from((n - a.min(n)).to_string()),
        ])
    };
    let table = Table::new(
        vec![
            side("BLUE", s.places[0], Color::LightBlue),
            side("RED", s.places[1], Color::LightRed),
            side("NEUTRAL", s.places[2], Color::Gray),
        ],
        [
            Constraint::Length(10),
            Constraint::Length(8),
            Constraint::Length(10),
            Constraint::Length(8),
        ],
    )
    .header(
        Row::new(vec!["Side", "Places", "Airfields", "Cities"])
            .style(Style::new().fg(Color::DarkGray)),
    )
    .block(Block::default().borders(Borders::ALL).title(" Bases "));
    f.render_widget(table, sides);
    let rows: Vec<Row> = if s.players.is_empty() {
        vec![Row::new(vec![Cell::from("Nobody connected").style(Style::new().fg(Color::DarkGray))])]
    } else {
        s.players
            .iter()
            .map(|p| {
                let (status, c) = match &p.status {
                    PlayerStatus::Ops => ("On the Ops screen".to_string(), Color::Gray),
                    PlayerStatus::Flying { title, aircraft } => {
                        (format!("Flying {} ({})", title, aircraft.name()), Color::LightGreen)
                    }
                    PlayerStatus::Reconnecting { seconds } => {
                        (format!("Dropped out - aircraft kept {seconds} s"), Color::Yellow)
                    }
                };
                Row::new(vec![
                    Cell::from(format!("{}", p.id.0 + 1)),
                    Cell::from(p.name.clone()).style(Style::new().bold()),
                    Cell::from(status).style(Style::new().fg(c)),
                    Cell::from(if p.ping > 0 { format!("{} ms", p.ping) } else { String::new() }),
                    Cell::from(if p.rate > 0 {
                        format!("{:.1} kB/s", p.rate as f32 / 1000.0)
                    } else {
                        String::new()
                    }),
                ])
            })
            .collect()
    };
    let players_table = Table::new(
        rows,
        [
            Constraint::Length(4),
            Constraint::Length(26),
            Constraint::Min(30),
            Constraint::Length(8),
            Constraint::Length(11),
        ],
    )
    .header(
        Row::new(vec!["#", "Name", "Status", "Ping", "Sent"])
            .style(Style::new().fg(Color::DarkGray)),
    )
    .block(Block::default().borders(Borders::ALL).title(format!(
        " Players {}/{} ",
        s.players.len(),
        s.max_players
    )));
    f.render_widget(players_table, players);
    let room = events.height.saturating_sub(2) as usize;
    let items: Vec<ListItem> = log
        .iter()
        .skip(log.len().saturating_sub(room))
        .map(|(t, text)| {
            ListItem::new(Line::from(vec![
                Span::styled(format!("{t} "), Style::new().fg(Color::DarkGray)),
                Span::raw(text.clone()),
            ]))
        })
        .collect();
    let list = List::new(items).block(Block::default().borders(Borders::ALL).title(" Events "));
    f.render_widget(list, events);
}

fn uptime(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 86_400 {
        format!("{}d {:02}:{:02}:{:02}", s / 86_400, s / 3600 % 24, s / 60 % 60, s % 60)
    } else {
        format!("{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use stealth_campaign::{Aircraft, PlayerId};

    #[test]
    fn the_screen_shows_the_server() {
        let mut t = ratatui::Terminal::new(TestBackend::new(100, 30)).unwrap();
        let s = Status {
            name: "Test server".into(),
            port: 28989,
            theater: "CE".into(),
            clock: "day 2 14:05".into(),
            places: [(12, 4), (9, 3), (2, 0)],
            players: vec![
                PlayerInfo {
                    id: PlayerId(1),
                    name: "Maverick".into(),
                    status: PlayerStatus::Flying {
                        title: "Strike Tabor".into(),
                        aircraft: Aircraft::F16,
                    },
                    ping: 42,
                    host: false,
                    rate: 6400,
                },
                PlayerInfo {
                    id: PlayerId(2),
                    name: "Goose".into(),
                    status: PlayerStatus::Reconnecting { seconds: 17 },
                    ping: 0,
                    host: false,
                    rate: 0,
                },
            ],
            max_players: 8,
            rules: "classic".into(),
            save: "/srv/dedicated.json".into(),
        };
        let log: VecDeque<(String, String)> =
            (0..40).map(|i| ("12:00:00".to_string(), format!("event {i}"))).collect();
        t.draw(|f| render(f, &s, &log, "01:02:03")).unwrap();
        let text: String = t
            .backend()
            .buffer()
            .content()
            .chunks(100)
            .map(|r| r.iter().map(|c| c.symbol()).collect::<String>() + "\n")
            .collect();
        eprintln!("{text}");
        for want in [
            "Test server",
            "01:02:03",
            "BLUE",
            "12",
            "Maverick",
            "Strike Tabor (F-16)",
            "42 ms",
            "kept 17 s",
            "event 39",
            "Players 2/8",
        ] {
            assert!(text.contains(want), "{want} missing");
        }
        assert!(!text.contains("event 0 "), "old events scroll off");
    }
}
