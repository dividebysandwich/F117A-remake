//! Coop players joining and leaving: each one fades in, stays up a few seconds and fades out
//! on a small panel in the cockpit, next to the HUD's lower right corner (on the screen and in
//! the headset alike).

use bevy::camera::primitives::MeshAabb;
use bevy::image::ImageSampler;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use stealth_campaign::PlayerId;
use stealth_net::protocol::{PlayerInfo, PlayerStatus};

use crate::canvas::Canvas;
use crate::cockpit3d::{CockpitRoot, HudGlass};
use crate::font::{Align, stroke_text_at, stroke_width};

/// Seconds a notice fades in, stays and fades out.
const FADE_IN: f32 = 0.3;
const HOLD: f32 = 3.0;
const FADE_OUT: f32 = 0.6;
/// Notices up at most (the oldest go).
const MAX_NOTICES: usize = 4;

/// Cockpit panel canvas (pixels), its angular width (radians) and its gap to the glass.
const W: usize = 512;
const H: usize = 80;
const WIDTH: f32 = 0.2;
const GAP: f32 = 0.015;
/// The panel is pulled this far towards the eye from the glass (in front of the glareshield).
const PULL: f32 = 0.8;
/// Canvas inks and their colours (sRGB; not the palette's, which dims at night).
const TEXT: u8 = 1;
const BORDER: u8 = 2;
const COLOURS: [[u8; 4]; 3] = [[0, 0, 0, 0], [255, 255, 255, 255], [215, 215, 215, 255]];
/// Background opacity.
const BACK: f32 = 0.6;

pub struct PlayerNoticesPlugin;

impl Plugin for PlayerNoticesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PlayerNotices>()
            .add_systems(Update, (age, cockpit_panel).chain());
    }
}

/// The notices up, each with its age (seconds), oldest first.
#[derive(Resource, Default)]
pub struct PlayerNotices(pub Vec<(String, f32)>);

impl PlayerNotices {
    pub fn push(&mut self, text: String) {
        self.0.push((text, 0.0));
        let extra = self.0.len().saturating_sub(MAX_NOTICES);
        self.0.drain(..extra);
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

/// Opacity at an age.
fn alpha(age: f32) -> f32 {
    (age / FADE_IN).min(1.0).min((FADE_IN + HOLD + FADE_OUT - age) / FADE_OUT).clamp(0.0, 1.0)
}

fn age(time: Res<Time<Real>>, mut notices: ResMut<PlayerNotices>) {
    if notices.0.is_empty() {
        return;
    }
    let dt = time.delta_secs();
    notices.0.retain_mut(|(_, t)| {
        *t += dt;
        *t < FADE_IN + HOLD + FADE_OUT
    });
}

/// The cockpit panel's text and border (transparent elsewhere), and its background.
#[derive(Component)]
struct Panel(String, Handle<Image>);

#[derive(Component)]
struct Back;

/// The newest notice on a panel next to the HUD glass's lower right corner, upright and
/// facing back (level on the screen).
#[allow(clippy::too_many_arguments)]
fn cockpit_panel(
    mut commands: Commands,
    notices: Res<PlayerNotices>,
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
        let quad = meshes.add(Rectangle::new(1.0, H as f32 / W as f32));
        let size = Extent3d { width: W as u32, height: H as u32, depth_or_array_layers: 1 };
        let mut image = Image::new_fill(
            size,
            TextureDimension::D2,
            &[0, 0, 0, 0],
            TextureFormat::Rgba8UnormSrgb,
            default(),
        );
        image.sampler = ImageSampler::linear();
        let image = images.add(image);
        let front = material(&mut materials, Some(image.clone()));
        let back = material(&mut materials, None);
        let parts = [
            commands
                .spawn((
                    Panel(String::new(), image),
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
    let Some((text, t)) = notices.0.last() else {
        for (_, _, mut v, _) in &mut panels {
            v.set_if_neq(Visibility::Hidden);
        }
        for (_, mut v, _) in &mut backs {
            v.set_if_neq(Visibility::Hidden);
        }
        return;
    };
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
    // Right of the corner, its bottom level with the glass's, then towards the eye.
    let d = c.length();
    let size = Vec2::new(WIDTH, WIDTH * H as f32 / W as f32) * d;
    let centre = (c + Vec3::new(GAP * d + size.x / 2.0, size.y / 2.0, 0.0)) * PULL;
    let place = |forward: f32| {
        let at = centre * (1.0 - forward);
        Transform::from_translation(at).with_scale(Vec3::splat(size.x * PULL * (1.0 - forward)))
    };
    let a = alpha(*t);
    for (mut panel, mut tf, mut v, m) in &mut panels {
        if panel.0 != *text {
            panel.0 = text.clone();
            let c = draw(&text.to_uppercase());
            if let Some(mut image) = images.get_mut(&panel.1) {
                image.data = Some(c.px.iter().flat_map(|&i| COLOURS[i as usize]).collect());
            }
        }
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

/// The text centred in a border, as large as fits.
fn draw(text: &str) -> Canvas {
    let mut c = Canvas::new(W, H);
    let h = (H as f32 * 0.42).min((W as f32 - 48.0) / stroke_width(text, 1.0).max(1.0));
    stroke_text_at(&mut c, W as f32 / 2.0, H as f32 / 2.0, text, h, h / 8.0, TEXT, Align::Centre);
    c.frame(Vec2::splat(3.0), Vec2::new(W as f32 - 3.0, H as f32 - 3.0), 4.0, BORDER);
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
        assert!(n.0.is_empty());
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
        let texts: Vec<&str> = n.0.iter().map(|(s, _)| s.as_str()).collect();
        assert_eq!(texts, ["BOB joined", "ANN left", "BOB lost connection", "BOB joined"]);
    }

    #[test]
    fn fades_in_holds_and_fades_out() {
        assert_eq!(alpha(0.0), 0.0);
        assert_eq!(alpha(FADE_IN), 1.0);
        assert_eq!(alpha(FADE_IN + HOLD), 1.0);
        assert!(alpha(FADE_IN + HOLD + FADE_OUT / 2.0) < 0.6);
        assert_eq!(alpha(FADE_IN + HOLD + FADE_OUT), 0.0);
    }
}
