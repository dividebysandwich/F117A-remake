//! Coop players joining and leaving, and their chat: one panel in the cockpit, next to the
//! HUD's lower right corner (on the screen and in the headset alike), with the last few
//! messages, the newest at the bottom; it grows with them, fades in with the first and out
//! when the last one is old. While a chat message is typed (`chat.rs`) its field is under them
//! and the panel stays up. The whole history is on the Ops screen's CHAT tab.

use bevy::camera::primitives::MeshAabb;
use bevy::image::ImageSampler;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use stealth_campaign::PlayerId;
use stealth_net::protocol::{PlayerInfo, PlayerStatus};

use crate::canvas::Canvas;
use crate::cockpit3d::{CockpitRoot, HudGlass};
use crate::font::stroke_text;

/// Seconds a message fades in, stays (and a little more for each character) and fades out.
const FADE_IN: f32 = 0.3;
const HOLD: f32 = 4.0;
const HOLD_PER_CHAR: f32 = 0.06;
const FADE_OUT: f32 = 0.6;
/// Messages on the panel at most (the oldest go), and its lines at most (the oldest go).
const MAX_NOTICES: usize = 6;
const MAX_LINES: usize = 12;
/// Messages kept for the CHAT tab.
const MAX_LOG: usize = 500;

/// Cockpit panel canvas width, glyph height, line pitch and margin (pixels), its angular
/// width (radians) and its gap to the glass.
const W: usize = 640;
const GLYPH: f32 = 22.0;
const PITCH: usize = 32;
const PAD: usize = 14;
const WIDTH: f32 = 0.3;
const GAP: f32 = 0.015;
/// The panel is pulled this far towards the eye from the glass (in front of the glareshield).
const PULL: f32 = 0.8;
/// Canvas inks and their colours (sRGB; not the palette's, which dims at night).
const TEXT: u8 = 1;
const BORDER: u8 = 2;
const NAME: u8 = 3;
const OWN: u8 = 4;
const NEWS: u8 = 5;
const COLOURS: [[u8; 4]; 6] = [
    [0, 0, 0, 0],
    [255, 255, 255, 255],
    [215, 215, 215, 255],
    [255, 200, 80, 255],
    [120, 255, 140, 255],
    [175, 205, 255, 255],
];
/// Background opacity.
const BACK: f32 = 0.6;

pub struct PlayerNoticesPlugin;

impl Plugin for PlayerNoticesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PlayerNotices>()
            .add_systems(Update, (age, cockpit_panel).chain())
            .add_systems(
                OnExit(crate::settings::AppState::Flight),
                |mut n: ResMut<PlayerNotices>| {
                    *n = PlayerNotices::default();
                },
            );
    }
}

/// A message: a player's chat (`from`), or news of who joined and left.
#[derive(Debug, Clone)]
pub struct Notice {
    pub from: Option<String>,
    pub text: String,
    /// Said by this player.
    pub own: bool,
    /// Seconds since it came.
    age: f32,
}

impl Notice {
    /// Seconds it is on the panel.
    fn life(&self) -> f32 {
        FADE_IN + HOLD + self.text.chars().count() as f32 * HOLD_PER_CHAR + FADE_OUT
    }
}

/// The messages, oldest first, and the chat being typed.
#[derive(Resource, Default)]
pub struct PlayerNotices {
    log: Vec<Notice>,
    /// The cockpit's chat field while a message is typed in it.
    pub typing: Option<String>,
    /// Messages to send (`chat.rs` sends them and adds them here).
    pub outbox: Vec<String>,
    /// The Ops screen's chat field has the keyboard this frame (no other key does anything).
    pub field_focus: bool,
}

impl PlayerNotices {
    pub fn push(&mut self, text: String) {
        self.add(Notice { from: None, text, own: false, age: 0.0 });
    }

    /// A chat message (`own`: this player's).
    pub fn chat(&mut self, from: &str, text: &str, own: bool) {
        self.add(Notice { from: Some(from.to_string()), text: text.to_string(), own, age: 0.0 });
    }

    fn add(&mut self, n: Notice) {
        self.log.push(n);
        let extra = self.log.len().saturating_sub(MAX_LOG);
        self.log.drain(..extra);
    }

    /// Every message kept, oldest first.
    pub fn log(&self) -> &[Notice] {
        &self.log
    }

    /// The messages on the panel: the last few, those still up (all of them while typing).
    fn shown(&self) -> &[Notice] {
        let last = &self.log[self.log.len().saturating_sub(MAX_NOTICES)..];
        if self.typing.is_some() {
            return last;
        }
        let first = last.iter().position(|n| n.age < n.life()).unwrap_or(last.len());
        // An older message outlasting a newer one (it is longer) keeps those after it.
        &last[first..]
    }

    /// Who joined and who left between two rosters (not this player himself; nothing for
    /// the first roster, which is everybody already there).
    pub fn roster_changed(&mut self, old: &[PlayerInfo], new: &[PlayerInfo], me: PlayerId) {
        if old.is_empty() {
            return;
        }
        let here = |r: &[PlayerInfo], id| {
            r.iter().any(|p| p.id == id && !matches!(p.status, PlayerStatus::Reconnecting { .. }))
        };
        for p in new.iter().filter(|p| p.id != me) {
            if here(new, p.id) && !here(old, p.id) {
                self.push(format!("{} joined", p.name));
            }
        }
        for p in old.iter().filter(|p| p.id != me) {
            if here(old, p.id) && !here(new, p.id) {
                let dropped = new.iter().any(|q| q.id == p.id);
                self.push(format!(
                    "{} {}",
                    p.name,
                    if dropped { "lost connection" } else { "left" }
                ));
            }
        }
    }
}

/// The panel's opacity: fading in for `shown` seconds after it came up, fading out with the
/// last of its messages.
fn alpha(shown: f32, notices: &[Notice], typing: bool) -> f32 {
    let out = if typing {
        1.0
    } else {
        notices.iter().map(|n| (n.life() - n.age) / FADE_OUT).fold(0.0, f32::max)
    };
    (shown / FADE_IN).min(out).clamp(0.0, 1.0)
}

fn age(time: Res<Time<Real>>, mut notices: ResMut<PlayerNotices>) {
    let dt = time.delta_secs();
    let n = notices.log.len();
    for m in &mut notices.log[n.saturating_sub(MAX_NOTICES)..] {
        m.age += dt;
    }
}

/// A line of the panel: its first `name` characters in `name_ink`, the rest in `ink`.
#[derive(Debug, Clone, PartialEq)]
struct Line {
    text: String,
    name: usize,
    name_ink: u8,
    ink: u8,
}

/// Characters on a line of the panel.
fn line_chars() -> usize {
    // A glyph advances by its height (stroke_width).
    (((W - 2 * PAD) as f32 + GLYPH / 3.0) / GLYPH).floor() as usize
}

/// `s` broken into lines of `n` characters at most, between words where it can.
fn wrap(s: &str, n: usize) -> Vec<String> {
    let n = n.max(1);
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in s.split_whitespace() {
        let mut word: Vec<char> = word.chars().collect();
        loop {
            let len = line.chars().count();
            let room = if len == 0 { n } else { n.saturating_sub(len + 1) };
            if word.len() <= room {
                if len > 0 {
                    line.push(' ');
                }
                line.extend(&word);
                break;
            }
            if len > 0 {
                lines.push(std::mem::take(&mut line));
                continue;
            }
            // Longer than a line.
            lines.push(word.drain(..n).collect());
        }
    }
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    lines
}

/// The messages' lines, the newest at the bottom (at most `MAX_LINES`).
fn lines(notices: &[Notice]) -> Vec<Line> {
    let n = line_chars();
    let mut out = Vec::new();
    for m in notices {
        let (text, name, name_ink, ink) = match &m.from {
            Some(from) => {
                let ink = if m.own { OWN } else { NAME };
                (format!("{from}: {}", m.text), from.chars().count() + 1, ink, TEXT)
            }
            None => (m.text.clone(), 0, NEWS, NEWS),
        };
        let mut name = name;
        for text in wrap(&text, n) {
            let len = text.chars().count();
            out.push(Line { text, name: name.min(len), name_ink, ink });
            name = name.saturating_sub(len + 1);
        }
    }
    let extra = out.len().saturating_sub(MAX_LINES);
    out.drain(..extra);
    out
}

/// The cockpit panel's text and border (transparent elsewhere), and its background; what it
/// shows (the lines and the chat field).
#[derive(Component)]
struct Panel(Option<(Vec<Line>, Option<String>)>, Handle<Image>);

#[derive(Component)]
struct Back;

/// The panel next to the HUD glass's lower right corner, upright and facing back (level on
/// the screen), its bottom level with the glass's.
#[allow(clippy::too_many_arguments)]
fn cockpit_panel(
    mut commands: Commands,
    notices: Res<PlayerNotices>,
    time: Res<Time<Real>>,
    roots: Query<(Entity, &GlobalTransform), With<CockpitRoot>>,
    glass: Query<(Entity, &GlobalTransform, &Mesh3d), With<HudGlass>>,
    mut panels: Query<(
        &mut Panel,
        &mut Transform,
        &mut Visibility,
        &MeshMaterial3d<StandardMaterial>,
    )>,
    mut backs: Query<
        (&mut Transform, &mut Visibility, &MeshMaterial3d<StandardMaterial>),
        (With<Back>, Without<Panel>),
    >,
    mut corner: Local<Option<(Entity, Vec3)>>,
    mut up: Local<f32>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let Ok((root, root_tf)) = roots.single() else { return };
    if panels.is_empty() {
        let material = |m: &mut Assets<StandardMaterial>, image| {
            m.add(StandardMaterial {
                base_color_texture: image,
                unlit: true,
                fog_enabled: false,
                alpha_mode: AlphaMode::Blend,
                ..default()
            })
        };
        let quad = meshes.add(Rectangle::new(1.0, 1.0));
        let image = images.add(image(&Canvas::new(W, PITCH)));
        let front = material(&mut materials, Some(image.clone()));
        let back = material(&mut materials, None);
        let parts = [
            commands
                .spawn((
                    Panel(None, image),
                    Mesh3d(quad.clone()),
                    MeshMaterial3d(front),
                    Transform::default(),
                    Visibility::Hidden,
                    NotShadowCaster,
                ))
                .id(),
            commands
                .spawn((
                    Back,
                    Mesh3d(quad),
                    MeshMaterial3d(back),
                    Transform::default(),
                    Visibility::Hidden,
                    NotShadowCaster,
                ))
                .id(),
        ];
        commands.entity(root).add_children(&parts);
        return;
    }
    let shown = notices.shown();
    let typing = notices.typing.is_some();
    if shown.is_empty() && !typing {
        *up = 0.0;
        for (_, _, mut v, _) in &mut panels {
            v.set_if_neq(Visibility::Hidden);
        }
        for (_, mut v, _) in &mut backs {
            v.set_if_neq(Visibility::Hidden);
        }
        return;
    }
    *up += time.delta_secs();
    // The glass's lower right corner in the cockpit frame (the eye at the origin).
    let Ok((g, glass_tf, mesh)) = glass.single() else { return };
    if corner.is_none_or(|(e, _)| e != g) {
        let Some(aabb) = meshes.get(&mesh.0).and_then(|m| m.compute_aabb()) else { return };
        let to_root = root_tf.affine().inverse() * glass_tf.affine();
        let (lo, hi) = (Vec3::from(aabb.min()), Vec3::from(aabb.max()));
        let pts = (0..8).map(|k| {
            to_root.transform_point3(Vec3::new(
                if k & 1 == 0 { lo.x } else { hi.x },
                if k & 2 == 0 { lo.y } else { hi.y },
                if k & 4 == 0 { lo.z } else { hi.z },
            ))
        });
        let (min, max) = pts.fold((Vec3::MAX, Vec3::MIN), |(a, b), p| (a.min(p), b.max(p)));
        *corner = Some((g, Vec3::new(max.x, min.y, (min.z + max.z) / 2.0)));
    }
    let Some((_, c)) = *corner else { return };
    // The field's text with a blinking cursor.
    let field = notices.typing.as_ref().map(|t| {
        let blink = (time.elapsed_secs() * 2.0) as u32 % 2 == 0;
        format!("{t}{}", if blink { "_" } else { " " })
    });
    let want = (lines(shown), field);
    let mut height = PITCH;
    for (mut panel, _, _, _) in &mut panels {
        if panel.0.as_ref() != Some(&want) {
            let canvas = draw(&want.0, want.1.as_deref());
            if let Some(mut image) = images.get_mut(&panel.1) {
                *image = self::image(&canvas);
            }
            panel.0 = Some(want.clone());
        }
        height = canvas_height(want.0.len(), want.1.is_some());
    }
    // Right of the corner, its bottom level with the glass's, then towards the eye.
    let d = c.length();
    let size = Vec2::new(WIDTH, WIDTH * height as f32 / W as f32) * d;
    let centre = (c + Vec3::new(GAP * d + size.x / 2.0, size.y / 2.0, 0.0)) * PULL;
    let place = |forward: f32| {
        let at = centre * (1.0 - forward);
        Transform::from_translation(at).with_scale(size.extend(1.0) * PULL * (1.0 - forward))
    };
    let a = alpha(*up, shown, typing);
    for (_, mut tf, mut v, m) in &mut panels {
        *tf = place(0.005);
        v.set_if_neq(Visibility::Inherited);
        if let Some(mut m) = materials.get_mut(&m.0) {
            m.base_color = Color::WHITE.with_alpha(a);
        }
    }
    for (mut tf, mut v, m) in &mut backs {
        *tf = place(0.0);
        v.set_if_neq(Visibility::Inherited);
        if let Some(mut m) = materials.get_mut(&m.0) {
            m.base_color = Color::BLACK.with_alpha(a * BACK);
        }
    }
}

/// The panel's height (pixels) with this many lines, and the chat field under them.
fn canvas_height(lines: usize, field: bool) -> usize {
    let rows = lines + field as usize;
    2 * PAD + rows.max(1) * PITCH + if field && lines > 0 { PAD } else { 0 }
}

/// The canvas as the panel's texture.
fn image(c: &Canvas) -> Image {
    let size = Extent3d { width: c.pw() as u32, height: c.ph() as u32, depth_or_array_layers: 1 };
    let data: Vec<u8> = c.px.iter().flat_map(|&i| COLOURS[i as usize]).collect();
    let mut image =
        Image::new(size, TextureDimension::D2, data, TextureFormat::Rgba8UnormSrgb, default());
    image.sampler = ImageSampler::linear();
    image
}

/// The lines, and under them the chat field (`field`: its text), in a border.
fn draw(lines: &[Line], field: Option<&str>) -> Canvas {
    let h = canvas_height(lines.len(), field.is_some());
    let mut c = Canvas::new(W, h);
    let pen = GLYPH / 8.0;
    let row = |i: usize| (PAD + i * PITCH + (PITCH - GLYPH as usize) / 2) as f32;
    let x0 = PAD as f32;
    for (i, l) in lines.iter().enumerate() {
        let (name, rest): (String, String) = {
            let b = l.text.char_indices().nth(l.name).map_or(l.text.len(), |(b, _)| b);
            (l.text[..b].to_string(), l.text[b..].to_string())
        };
        let x = stroke_text(&mut c, x0, row(i), &name, GLYPH, pen, l.name_ink);
        stroke_text(&mut c, x, row(i), &rest, GLYPH, pen, l.ink);
    }
    if let Some(text) = field {
        let y = if lines.is_empty() {
            row(0)
        } else {
            let sep = (PAD + lines.len() * PITCH + PAD / 2) as f32;
            c.stroke(Vec2::new(x0, sep), Vec2::new(W as f32 - x0, sep), 2.0, BORDER);
            row(lines.len()) + PAD as f32
        };
        // The end of the text (with the cursor) when it is longer than the field.
        let room = line_chars() - 2;
        let n = text.chars().count();
        let tail: String = text.chars().skip(n.saturating_sub(room)).collect();
        let x = stroke_text(&mut c, x0, y, "> ", GLYPH, pen, OWN);
        stroke_text(&mut c, x, y, &tail, GLYPH, pen, TEXT);
    }
    c.frame(Vec2::splat(3.0), Vec2::new(W as f32 - 3.0, h as f32 - 3.0), 4.0, BORDER);
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player(id: u8, name: &str, status: PlayerStatus) -> PlayerInfo {
        PlayerInfo { id: PlayerId(id), name: name.into(), status, ping: 0, host: false, rate: 0 }
    }

    #[test]
    fn roster_changes_become_notices() {
        let me = player(0, "ME", PlayerStatus::Ops);
        let ann = player(1, "ANN", PlayerStatus::Ops);
        let bob = player(2, "BOB", PlayerStatus::Ops);
        let gone = PlayerStatus::Reconnecting { seconds: 30 };
        let mut n = PlayerNotices::default();
        // The first roster is who is already there.
        n.roster_changed(&[], &[me.clone(), ann.clone()], me.id);
        assert!(n.log().is_empty());
        n.roster_changed(
            &[me.clone(), ann.clone()],
            &[me.clone(), ann.clone(), bob.clone()],
            me.id,
        );
        n.roster_changed(
            &[me.clone(), ann.clone(), bob.clone()],
            &[me.clone(), bob.clone()],
            me.id,
        );
        let dropped = player(2, "BOB", gone);
        n.roster_changed(&[me.clone(), bob.clone()], &[me.clone(), dropped.clone()], me.id);
        n.roster_changed(&[me.clone(), dropped], &[me.clone(), bob], me.id);
        let texts: Vec<&str> = n.log().iter().map(|m| m.text.as_str()).collect();
        assert_eq!(texts, ["BOB joined", "ANN left", "BOB lost connection", "BOB joined"]);
    }

    #[test]
    fn fades_in_holds_and_fades_out() {
        let at = |age: f32| [Notice { from: None, text: String::new(), own: false, age }];
        let life = at(0.0)[0].life();
        assert_eq!(alpha(0.0, &at(0.0), false), 0.0);
        assert_eq!(alpha(FADE_IN, &at(FADE_IN), false), 1.0);
        assert_eq!(alpha(9.0, &at(life - FADE_OUT - 0.01), false), 1.0);
        assert!(alpha(9.0, &at(life - FADE_OUT / 2.0), false) < 0.6);
        assert_eq!(alpha(9.0, &at(life), false), 0.0);
        // Typing: up however old the messages are.
        assert_eq!(alpha(9.0, &at(life * 2.0), true), 1.0);
    }

    #[test]
    fn the_panel_shows_the_last_messages_still_up() {
        let mut n = PlayerNotices::default();
        for i in 0..8 {
            n.chat("ANN", &format!("message {i}"), false);
        }
        assert_eq!(n.log().len(), 8);
        assert_eq!(n.shown().len(), MAX_NOTICES);
        assert_eq!(n.shown().last().unwrap().text, "message 7");
        for m in &mut n.log {
            m.age = 100.0;
        }
        assert!(n.shown().is_empty());
        n.typing = Some(String::new());
        assert_eq!(n.shown().len(), MAX_NOTICES);
    }

    #[test]
    fn long_messages_wrap_between_words() {
        assert_eq!(wrap("one two three", 7), ["one two", "three"]);
        assert_eq!(wrap("abcdefghij", 4), ["abcd", "efgh", "ij"]);
        assert_eq!(wrap("ab abcdefgh", 4), ["ab", "abcd", "efgh"]);
        assert_eq!(wrap("", 4), [""]);
        let m = Notice { from: Some("BOB".into()), text: "x ".repeat(40), own: false, age: 0.0 };
        let l = lines(&[m]);
        assert!(l.len() > 1 && l.iter().all(|l| l.text.chars().count() <= line_chars()));
        assert_eq!((l[0].name, l[1].name), (4, 0));
        assert!(l[0].text.starts_with("BOB: x"));
    }
}
