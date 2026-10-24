//! Helmet-mounted displays (the AH-1Z's, and the F-16's HMCS): symbology on the pilot's visor,
//! collimated to infinity like the HUD (assets/shaders/hud_glass.wgsl) and carried by the head
//! (the eye camera with the mouse head look, a headset camera in VR), shown in the cockpit
//! views. The AH-1Z's shows:
//!
//! - the line of sight reticle (where a helmet-slaved gun points): the smoothed helmet sight,
//!   so head tremor does not shake it, else the centre;
//! - the horizon line;
//! - the locked target: a box on a ground target, a diamond on an aircraft; off the visor, an
//!   arrow from the reticle towards it;
//! - the turret gun's pipper (where its rounds go, selected or not) while it is slaved to the
//!   helmet;
//! - with Sidewinders selected, their seeker's circle (the seeker follows the helmet sight as
//!   far as its gimbals turn and locks the aircraft nearest its middle);
//! - the targeting pod's line of sight (a small diamond) when it is not slaved;
//! - the gun line (rounds and what it is slaved to) under the reticle while the gun is
//!   selected.
//!
//! The HMCS shows only the horizon, the route's waypoints (the current steerpoint larger, with
//! its number), the lock and the Sidewinder seeker's circle.
//!
//! The horizon, waypoints, lock, pipper and pod sight are drawn where they are in the world.
//!
//! Canvas pixels are angular as on the modern HUD (`hud::modern`): (x, y) is the direction
//! (x - CX, CY - y) / SCALE in the tangent plane one unit in front of the head.

use bevy::camera::visibility::{NoFrustumCulling, RenderLayers};
use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use crate::aircraft::Hmd;
use crate::camera::{EyeCamera, View, ViewState};
use crate::canvas::{Canvas, CanvasTexture, CanvasUpload};
use crate::cockpit3d::hud_glass::HmdMaterial;
use crate::flight::{Aircraft, Player};
use crate::font::{Align, stroke_text_at};
use crate::hud::{HudCues, Stores, Waypoints};
use crate::settings::{AppState, Settings};
use crate::world::WorldPos;

pub struct HmdPlugin;

impl Plugin for HmdPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, attach.run_if(in_state(AppState::Flight)))
            .add_systems(OnExit(AppState::Flight), despawn)
            .add_systems(
                PostUpdate,
                draw.after(bevy::transform::TransformSystems::Propagate)
                    .before(CanvasUpload)
                    .run_if(in_state(AppState::Flight)),
            );
    }
}

/// Canvas size, centre (the line of sight) and pixels per unit of the tangent plane: the visor
/// shows 40 x 30 degrees.
const WIDTH: usize = 880;
const HEIGHT: usize = 640;
const CX: f32 = WIDTH as f32 / 2.0;
const CY: f32 = HEIGHT as f32 / 2.0;
const SCALE: f32 = 1200.0;
/// The visor quad's distance in front of the eye (m); its size covers the canvas.
const DISTANCE: f32 = 0.6;
/// Symbols stay this many pixels inside the canvas edge.
const MARGIN: f32 = 30.0;
/// Ink (any colour but the transparent 0) and pen widths.
const INK: u8 = 15;
const PEN: f32 = 2.4;
const THIN: f32 = 1.8;
const TEXT: f32 = 15.0;

#[derive(Component)]
struct Visor;

/// The eye camera: whether it is a camera itself (else the VR tracking root) and its children.
type Eyes<'w, 's> =
    Query<'w, 's, (Entity, Has<Camera3d>, Option<&'static Children>), With<EyeCamera>>;

/// The camera turning with the head: the eye camera, or in VR (where the eye camera is the
/// tracking root) its first headset camera.
fn head_camera(eyes: &Eyes, cameras: &Query<(), With<Camera3d>>) -> Option<Entity> {
    let (e, camera, children) = eyes.iter().next()?;
    if camera {
        return Some(e);
    }
    children?.iter().find(|&c| cameras.contains(c))
}

/// Spawns the visor on the head camera of an aircraft with a helmet-mounted display (and moves
/// it to the camera when the rig is rebuilt), shown in the cockpit views.
#[allow(clippy::too_many_arguments)]
fn attach(
    mut commands: Commands,
    settings: Res<Settings>,
    view: Res<ViewState>,
    eyes: Eyes,
    cameras: Query<(), With<Camera3d>>,
    mut visors: Query<(Entity, Option<&ChildOf>, &mut Visibility), With<Visor>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<HmdMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let head = head_camera(&eyes, &cameras).filter(|_| settings.aircraft.spec().hmd != Hmd::None);
    let Some(head) = head else {
        for (e, ..) in &visors {
            commands.entity(e).try_despawn();
        }
        return;
    };
    let shown =
        matches!(view.view, View::Cockpit | View::LookLeft | View::LookRight | View::LookRear);
    if let Some((e, parent, mut vis)) = visors.iter_mut().next() {
        if parent.map(|p| p.parent()) != Some(head) {
            commands.entity(head).add_child(e);
        }
        vis.set_if_neq(if shown { Visibility::Inherited } else { Visibility::Hidden });
        return;
    }
    let tex = CanvasTexture::new(Canvas::new(WIDTH, HEIGHT), &mut images, true);
    if let Some(mut img) = images.get_mut(&tex.image) {
        img.sampler = bevy::image::ImageSampler::linear();
    }
    let material = materials.add(HmdMaterial {
        tint: Vec4::ZERO,
        image: tex.image.clone(),
        proj: Vec4::new(CX, CY, SCALE, SCALE),
        style: Vec4::new(1.0, 1.0, 0.0, 0.0),
    });
    // A little larger than the canvas (the shader leaves the rest clear).
    let size = Vec2::new(WIDTH as f32, HEIGHT as f32) / SCALE * DISTANCE * 1.1;
    let visor = commands
        .spawn((
            Visor,
            tex,
            Mesh3d(meshes.add(Rectangle::from_size(size))),
            MeshMaterial3d(material),
            Transform::from_xyz(0.0, 0.0, -DISTANCE),
            if shown { Visibility::Inherited } else { Visibility::Hidden },
            // Only the eye cameras (not the targeting pod's or the recon cameras).
            RenderLayers::layer(crate::camera::MAIN_LOD_LAYER),
            NoFrustumCulling,
            NotShadowCaster,
        ))
        .id();
    commands.entity(head).add_child(visor);
}

fn despawn(mut commands: Commands, visors: Query<Entity, With<Visor>>) {
    for e in &visors {
        commands.entity(e).try_despawn();
    }
}

/// Pixel of a direction in the head frame (x right, y up, -z forward); None behind.
fn project(d: Vec3) -> Option<Vec2> {
    let z = -d.z;
    (z > 1e-3).then(|| Vec2::new(CX + SCALE * d.x / z, CY - SCALE * d.y / z))
}

fn on_visor(p: Vec2) -> bool {
    p.cmpge(Vec2::splat(MARGIN)).all()
        && p.cmple(Vec2::new(WIDTH as f32, HEIGHT as f32) - MARGIN).all()
}

/// Draws the visor's canvas for the head's pose this frame (after the transforms are
/// propagated, so the symbols keep up with the head).
#[allow(clippy::too_many_arguments)]
fn draw(
    mut visors: Query<(&ChildOf, &mut CanvasTexture, &Visibility), With<Visor>>,
    heads: Query<&GlobalTransform>,
    aircraft: Query<(&Aircraft, &WorldPos), With<Player>>,
    cues: Res<HudCues>,
    stores: Res<Stores>,
    mfd: Res<crate::mfd::MfdState>,
    combat: Option<Res<crate::combat::Combat>>,
    waypoints: Res<Waypoints>,
    settings: Res<Settings>,
    time: Res<Time>,
) {
    let Ok((parent, mut tex, vis)) = visors.single_mut() else { return };
    if *vis == Visibility::Hidden {
        return;
    }
    let (Ok(head), Ok((a, pos))) = (heads.get(parent.parent()), aircraft.single()) else {
        return;
    };
    let head = head.compute_transform().rotation;
    let tex = &mut *tex;
    tex.canvas.px.fill(0);
    let full = settings.aircraft.spec().hmd == Hmd::Full;
    let waypoints = if full {
        Vec::new()
    } else {
        waypoints
            .points
            .iter()
            .enumerate()
            .filter_map(|(i, w)| {
                let p = crate::world::wu_to_bevy(w.east, w.north, a.ground_altitude as f64);
                Some(((p - pos.0).as_vec3().try_normalize()?, i, i == waypoints.current))
            })
            .collect()
    };
    let frame = Frame {
        to_head: head.inverse(),
        aircraft: a.orientation,
        reticle: full,
        sight: mfd.av.helmet,
        lock: cues.helmet_lock,
        gun: cues.turret_gun.filter(|_| full && stores.gun_slave == Some("HMS")).map(|g| g.dir),
        // East, north, up to the world frame (x east, y up, -z north).
        seeker: combat
            .and_then(|c| c.sim.helmet_seeker())
            .map(|[e, n, u]| Vec3::new(e as f32, u as f32, -n as f32)),
        pod: (full && mfd.av.tgp.mode != crate::avionics::TgpMode::Slaved)
            .then_some(mfd.av.tgp.los)
            .flatten(),
        waypoints,
        gun_line: stores
            .gun_slave
            .filter(|_| full && stores.gun_selected)
            .map(|s| format!("GUN {} {s}", stores.gun_rounds.unwrap_or(0))),
        blink: crate::hud::blink(&time),
    };
    draw_visor(&mut tex.canvas, &frame);
    tex.dirty = true;
}

/// What the visor shows this frame.
struct Frame {
    /// World to head frame.
    to_head: Quat,
    /// The aircraft's attitude (the cues are in its frame).
    aircraft: Quat,
    /// The line of sight reticle (not on the HMCS).
    reticle: bool,
    /// The smoothed helmet sight (world).
    sight: Option<Vec3>,
    /// The locked target (aircraft frame) and whether it is an aircraft.
    lock: Option<(Vec3, bool)>,
    /// The turret gun's pipper (aircraft frame) while it is slaved to the helmet.
    gun: Option<Vec3>,
    /// Where the Sidewinder seeker looks (world).
    seeker: Option<Vec3>,
    /// The targeting pod's line of sight (world) when it is not slaved.
    pod: Option<Vec3>,
    /// The route's waypoints (world direction, index, the current steerpoint).
    waypoints: Vec<(Vec3, usize, bool)>,
    gun_line: Option<String>,
    blink: bool,
}

fn draw_visor(c: &mut Canvas, f: &Frame) {
    c.reset_clip();
    let head = |d_aircraft: Vec3| f.to_head * (f.aircraft * d_aircraft);
    let lo = Vec2::splat(MARGIN);
    let hi = Vec2::new(WIDTH as f32, HEIGHT as f32) - MARGIN;
    let centre =
        f.sight.and_then(|d| project(f.to_head * d)).map_or(Vec2::new(CX, CY), |p| p.clamp(lo, hi));
    // Line of sight reticle.
    if f.reticle {
        for d in [Vec2::X, Vec2::Y, Vec2::NEG_X, Vec2::NEG_Y] {
            c.stroke(centre + d * 6.0, centre + d * 18.0, PEN, INK);
        }
    }
    if let Some(p) = f.seeker.and_then(|d| project(f.to_head * d)) {
        c.ring(p, stealth_sim::HELMET_SEEKER.tan() as f32 * SCALE, THIN, INK);
    }
    horizon(c, f.to_head);
    for &(d, i, current) in &f.waypoints {
        let Some(p) = project(f.to_head * d).filter(|&p| on_visor(p)) else { continue };
        if current {
            c.ring(p, 9.0, PEN, INK);
            let n = (i + 1).to_string();
            stroke_text_at(c, p.x, p.y + 28.0, &n, TEXT, PEN * 0.85, INK, Align::Centre);
        } else {
            c.ring(p, 5.0, THIN, INK);
        }
    }
    if let Some(p) = f.pod.and_then(|d| project(f.to_head * d)).filter(|&p| on_visor(p)) {
        diamond(c, p, 7.0, THIN);
    }
    if let Some((d, air)) = f.lock {
        let d = head(d);
        match project(d).filter(|&p| on_visor(p)) {
            Some(p) if air => diamond(c, p, 16.0, PEN),
            Some(p) => c.frame(p - Vec2::splat(14.0), p + Vec2::splat(14.0), PEN, INK),
            // Off the visor: an arrow from the reticle towards it.
            None => {
                let dir = Vec2::new(d.x, -d.y).try_normalize().unwrap_or(Vec2::Y);
                let (a, b) = (centre + dir * 40.0, centre + dir * 110.0);
                let side = Vec2::new(-dir.y, dir.x);
                c.stroke(a, b, PEN, INK);
                c.polyline(
                    &[b - dir * 14.0 + side * 9.0, b, b - dir * 14.0 - side * 9.0],
                    PEN,
                    INK,
                );
            }
        }
    }
    if let Some(g) = f.gun.and_then(|d| project(head(d))) {
        // Kept on the visor, crossed out when it is not where the rounds go.
        let q = g.clamp(lo, hi);
        let r = 20.0;
        c.ring(q, r, PEN, INK);
        c.ring(q, 1.5, PEN * 1.4, INK);
        if q != g && f.blink {
            c.stroke(q - Vec2::splat(r * 0.7), q + Vec2::splat(r * 0.7), THIN, INK);
            c.stroke(q + Vec2::new(-r * 0.7, r * 0.7), q + Vec2::new(r * 0.7, -r * 0.7), THIN, INK);
        }
    }
    if let Some(s) = &f.gun_line {
        stroke_text_at(c, centre.x, centre.y + 150.0, s, TEXT, PEN * 0.85, INK, Align::Centre);
    }
}

/// The horizon: the level great circle round the head's heading, as far as the visor reaches.
fn horizon(c: &mut Canvas, to_head: Quat) {
    let fwd = to_head.inverse() * Vec3::NEG_Z;
    // Looking straight down (up), the horizon lies the way the top (bottom) of the head points.
    let up = to_head.inverse() * Vec3::Y * -fwd.y.signum();
    let level = Vec3::new(fwd.x, 0.0, fwd.z)
        .try_normalize()
        .unwrap_or_else(|| up.with_y(0.0).normalize_or(Vec3::NEG_Z));
    let right = Vec3::new(-level.z, 0.0, level.x);
    let pts: Vec<Option<Vec2>> = (-30..=30)
        .map(|k| {
            let t = (k as f32 * 2.0).to_radians();
            project(to_head * (level * t.cos() + right * t.sin()))
        })
        .collect();
    let mut run = Vec::new();
    for p in pts.into_iter().chain([None]) {
        match p {
            Some(p) => run.push(p),
            None => {
                if run.len() > 1 {
                    c.polyline(&run, PEN, INK);
                }
                run.clear();
            }
        }
    }
}

fn diamond(c: &mut Canvas, p: Vec2, r: f32, w: f32) {
    c.polyline(
        &[
            p + Vec2::new(0.0, -r),
            p + Vec2::new(r, 0.0),
            p + Vec2::new(0.0, r),
            p + Vec2::new(-r, 0.0),
            p + Vec2::new(0.0, -r),
        ],
        w,
        INK,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(head: Quat) -> Frame {
        Frame {
            to_head: head.inverse(),
            aircraft: Quat::IDENTITY,
            reticle: true,
            sight: None,
            lock: None,
            gun: None,
            seeker: None,
            pod: None,
            waypoints: Vec::new(),
            gun_line: None,
            blink: true,
        }
    }

    fn ink(c: &Canvas, x0: usize, x1: usize, y0: usize, y1: usize) -> usize {
        (y0..y1)
            .flat_map(|y| (x0..x1).map(move |x| (x, y)))
            .filter(|&(x, y)| c.px[y * c.w + x] != 0)
            .count()
    }

    /// Level: the horizon runs through the reticle; looking 10 degrees down it is above it.
    #[test]
    fn horizon_follows_the_head() {
        let mut c = Canvas::new(WIDTH, HEIGHT);
        draw_visor(&mut c, &frame(Quat::IDENTITY));
        assert!(ink(&c, 0, 200, CY as usize - 3, CY as usize + 3) > 300);
        let down = Quat::from_rotation_x(-10f32.to_radians());
        let mut c = Canvas::new(WIDTH, HEIGHT);
        draw_visor(&mut c, &frame(down));
        let y = CY - SCALE * 10f32.to_radians().tan();
        assert!(ink(&c, 0, 200, y as usize - 3, y as usize + 3) > 300);
        assert_eq!(ink(&c, 0, 200, CY as usize - 3, CY as usize + 3), 0);
    }

    /// A lock and the gun pipper sit on their directions; a lock behind gets an arrow.
    #[test]
    fn lock_and_pipper() {
        let right = Vec3::new(0.1, 0.0, -1.0);
        let mut f = frame(Quat::from_rotation_x(0.05));
        f.lock = Some((right, false));
        f.gun = Some(right);
        let p = project(f.to_head * right).unwrap();
        let mut c = Canvas::new(WIDTH, HEIGHT);
        draw_visor(&mut c, &f);
        let (x, y) = (p.x as usize, p.y as usize);
        assert!(ink(&c, x + 12, x + 17, y - 3, y + 3) > 0, "box edge");
        assert!(ink(&c, x + 18, x + 23, y - 3, y + 3) > 0, "pipper ring");
        // Behind: the arrow points right from the reticle.
        f.lock = Some((Vec3::new(1.0, 0.0, 1.0), false));
        f.gun = None;
        let mut c = Canvas::new(WIDTH, HEIGHT);
        draw_visor(&mut c, &f);
        let (cx, cy) = (CX as usize, CY as usize);
        assert!(ink(&c, cx + 60, cx + 100, cy - 4, cy + 4) > 0);
        assert_eq!(ink(&c, cx - 100, cx - 60, cy - 4, cy + 4), 0);
    }

    /// The reticle sits on the smoothed sight, not the centre of the head.
    #[test]
    fn reticle_on_the_sight() {
        let mut f = frame(Quat::IDENTITY);
        f.sight = Some(Vec3::new(0.05, 0.0, -1.0).normalize());
        let mut c = Canvas::new(WIDTH, HEIGHT);
        draw_visor(&mut c, &f);
        let (x, cy) = ((CX + SCALE * 0.05) as usize, CY as usize);
        assert!(ink(&c, x + 6, x + 18, cy - 2, cy + 2) > 0);
        assert_eq!(ink(&c, CX as usize - 4, CX as usize + 4, cy - 30, cy - 6), 0);
    }

    /// The HMCS: no reticle; the waypoints where they are, the steerpoint larger.
    #[test]
    fn hmcs_waypoints_no_reticle() {
        let mut f = frame(Quat::IDENTITY);
        f.reticle = false;
        let (a, b) = (Vec3::new(0.1, -0.05, -1.0), Vec3::new(-0.1, -0.05, -1.0));
        f.waypoints = vec![(a.normalize(), 0, true), (b.normalize(), 1, false)];
        let mut c = Canvas::new(WIDTH, HEIGHT);
        draw_visor(&mut c, &f);
        let (cx, cy) = (CX as usize, CY as usize);
        assert_eq!(ink(&c, cx - 4, cx + 4, cy - 20, cy - 5), 0, "no reticle");
        let p = project(a).unwrap();
        let (x, y) = (p.x as usize, p.y as usize);
        assert!(ink(&c, x + 7, x + 11, y - 2, y + 2) > 0, "steerpoint ring");
        assert!(ink(&c, x - 8, x + 8, y + 18, y + 38) > 0, "its number");
        let q = project(b).unwrap();
        let (x, y) = (q.x as usize, q.y as usize);
        assert!(ink(&c, x + 3, x + 7, y - 2, y + 2) > 0, "waypoint ring");
        assert_eq!(ink(&c, x + 8, x + 11, y - 2, y + 2), 0, "smaller");
    }
}
