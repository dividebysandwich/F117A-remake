//! What the mouse points at in the cockpit (desktop): the nearest cockpit part along the ray
//! through the cursor, so a control hidden behind the stick, the throttle or the glare shield
//! is neither clicked nor shown a tooltip.
//!
//! A control is hit on its face rectangle (`Control::face_hit`) or on any of its meshes (the
//! gear lever's handle, a cap's sides); every other `Solid` mesh (panel, frame, seat, levers,
//! displays, the kneeboard) hides what is behind it. The HUD combiner glass is not `Solid`: it
//! is seen through.

use std::collections::HashMap;

use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;

use super::CockpitRoot;
use super::controls::Control;
use crate::camera::EyeCamera;

/// A cockpit mesh the mouse can point at; it hides what is behind it.
#[derive(Component)]
pub struct Solid;

/// A control's face is still chosen over a mesh this much closer (metres): the cap's top and
/// a flush bezel around it.
const FACE_SLACK: f32 = 0.001;

/// What the cursor points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Pointed {
    /// A pushable control.
    Control(Entity),
    /// A part with a tooltip (`Tip`): the entity carrying it.
    Tip(Entity),
}

/// Triangles of a mesh in its own frame, with their bounds.
struct Shape {
    min: Vec3,
    max: Vec3,
    tris: Vec<[Vec3; 3]>,
}

impl Shape {
    fn of(mesh: &Mesh) -> Option<Self> {
        if mesh.primitive_topology() != PrimitiveTopology::TriangleList {
            return None;
        }
        let Some(VertexAttributeValues::Float32x3(p)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            return None;
        };
        let v = |i: usize| p.get(i).map(|&a| Vec3::from(a));
        let index: Vec<usize> = match mesh.indices() {
            Some(Indices::U16(i)) => i.iter().map(|&i| i as usize).collect(),
            Some(Indices::U32(i)) => i.iter().map(|&i| i as usize).collect(),
            None => (0..p.len()).collect(),
        };
        let tris: Vec<[Vec3; 3]> = index
            .as_chunks::<3>()
            .0
            .iter()
            .filter_map(|&[a, b, c]| Some([v(a)?, v(b)?, v(c)?]))
            .collect();
        let (min, max) =
            tris.iter().flatten().fold(
                (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
                |(lo, hi), &p| (lo.min(p), hi.max(p)),
            );
        (!tris.is_empty()).then_some(Self { min, max, tris })
    }

    /// Nearest hit along o + t d (t > 0), if nearer than `best`.
    fn hit(&self, o: Vec3, d: Vec3, best: f32) -> Option<f32> {
        // Bounds first (slabs), a little larger for flat parts.
        let (mut t0, mut t1) = (0.0f32, best);
        for k in 0..3 {
            let (lo, hi) = (self.min[k] - 1e-4, self.max[k] + 1e-4);
            if d[k].abs() < 1e-12 {
                if o[k] < lo || o[k] > hi {
                    return None;
                }
                continue;
            }
            let (a, b) = ((lo - o[k]) / d[k], (hi - o[k]) / d[k]);
            t0 = t0.max(a.min(b));
            t1 = t1.min(a.max(b));
            if t0 > t1 {
                return None;
            }
        }
        self.tris
            .iter()
            .filter_map(|t| ray_triangle(o, d, t))
            .filter(|&t| t < best)
            .min_by(f32::total_cmp)
    }
}

/// Möller-Trumbore, both sides.
fn ray_triangle(o: Vec3, d: Vec3, [a, b, c]: &[Vec3; 3]) -> Option<f32> {
    let (e1, e2) = (*b - *a, *c - *a);
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-18 {
        return None;
    }
    let inv = 1.0 / det;
    let s = o - *a;
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = d.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(q) * inv;
    (t > 0.0).then_some(t)
}

/// The cockpit meshes' triangles, read once per mesh.
#[derive(Resource, Default)]
pub(super) struct Shapes(HashMap<AssetId<Mesh>, Option<Shape>>);

/// Forgets the old cockpit's meshes when it is rebuilt.
pub(super) fn forget_shapes(mut shapes: ResMut<Shapes>, added: Query<(), Added<CockpitRoot>>) {
    if !added.is_empty() {
        shapes.0.clear();
    }
}

/// A cockpit mesh with its frame and visibility.
type SolidMesh = (Entity, &'static Mesh3d, &'static GlobalTransform, &'static InheritedVisibility);

#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct Picker<'w, 's> {
    shapes: ResMut<'w, Shapes>,
    meshes: Res<'w, Assets<Mesh>>,
    solids: Query<'w, 's, SolidMesh, With<Solid>>,
    parents: Query<'w, 's, &'static ChildOf>,
    tips: Query<'w, 's, (), With<super::tooltips::Tip>>,
    windows: Query<'w, 's, &'static Window>,
    cams: Query<'w, 's, (&'static Camera, &'static GlobalTransform), With<EyeCamera>>,
    root: Query<'w, 's, &'static GlobalTransform, With<CockpitRoot>>,
}

impl Picker<'_, '_> {
    /// The mouse cursor in the window (logical pixels), if it is over it.
    pub fn cursor(&self) -> Option<Vec2> {
        self.windows.single().ok()?.cursor_position()
    }

    /// The window's size (logical pixels).
    pub fn window_size(&self) -> Option<Vec2> {
        self.windows.single().ok().map(|w| w.size())
    }

    /// The ray through the mouse cursor (world), if it is over the window.
    pub fn cursor_ray(&self) -> Option<Ray3d> {
        let Ok((camera, cam_tf)) = self.cams.single() else { return None };
        camera.viewport_to_world(cam_tf, self.cursor()?).ok()
    }

    /// The segment between two points of the cockpit frame as a world ray, and its length.
    pub fn segment(&self, from: Vec3, to: Vec3) -> Option<(Ray3d, f32)> {
        let root_tf = self.root.single().ok()?;
        let (a, b) = (root_tf.transform_point(from), root_tf.transform_point(to));
        let (dir, len) = Dir3::new_and_length(b - a).ok()?;
        Some((Ray3d::new(a, dir), len))
    }

    /// The nearest cockpit part along the ray: a control, a part with a tooltip, or None
    /// (nothing, or a bare part of the cockpit in front). Only the `usable` controls are hit on
    /// their face rectangles; the others' meshes still hide what is behind them.
    pub fn pick(
        &mut self,
        ray: Ray3d,
        controls: &[(Entity, &Control, &Transform)],
        usable: impl Fn(&Control) -> bool,
    ) -> Option<Pointed> {
        self.pick_within(ray, f32::INFINITY, controls, usable)
    }

    /// As `pick`, only the parts within `reach` (metres along the ray).
    pub fn pick_within(
        &mut self,
        ray: Ray3d,
        reach: f32,
        controls: &[(Entity, &Control, &Transform)],
        usable: impl Fn(&Control) -> bool,
    ) -> Option<Pointed> {
        let Ok(root_tf) = self.root.single() else { return None };
        // Distances are along the world ray in every frame (directions are not normalized).
        let (o, d) = (ray.origin, *ray.direction);
        let mut best = reach;
        let mut face: Option<Entity> = None;
        let inv = root_tf.affine().inverse();
        let (ro, rd) = (inv.transform_point3(o), inv.transform_vector3(d));
        for &(e, c, tf) in controls {
            if !usable(c) {
                continue;
            }
            let Some(t) = c.face_hit(tf, ro, rd) else { continue };
            if t < best {
                best = t;
                face = Some(e);
            }
        }
        let mut nearest: Option<(f32, Entity)> = None;
        for (e, mesh, tf, vis) in &self.solids {
            if !vis.get() {
                continue;
            }
            let id = mesh.id();
            if !self.shapes.0.contains_key(&id) {
                // Not cached before the mesh is loaded.
                let Some(m) = self.meshes.get(id) else { continue };
                self.shapes.0.insert(id, Shape::of(m));
            }
            let Some(shape) = &self.shapes.0[&id] else { continue };
            let inv = tf.affine().inverse();
            let limit = nearest.map_or(best + FACE_SLACK, |(t, _)| t);
            if let Some(t) = shape.hit(inv.transform_point3(o), inv.transform_vector3(d), limit) {
                nearest = Some((t, e));
            }
        }
        let is_control = |e: Entity| controls.iter().any(|&(c, _, _)| c == e);
        match nearest {
            // Something in front of the nearest face: what it belongs to.
            Some((t, e)) if face.is_none() || t + FACE_SLACK < best => self.owner(e, is_control),
            _ => face.map(Pointed::Control),
        }
    }

    /// The control a mesh belongs to, else the nearest part with a tooltip it belongs to.
    fn owner(&self, mut e: Entity, is_control: impl Fn(Entity) -> bool) -> Option<Pointed> {
        let mut tip = None;
        loop {
            if is_control(e) {
                return Some(Pointed::Control(e));
            }
            if tip.is_none() && self.tips.contains(e) {
                tip = Some(Pointed::Tip(e));
            }
            match self.parents.get(e) {
                Ok(p) if !self.root.contains(e) => e = p.parent(),
                _ => return tip,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ray_hits_the_nearest_face_only() {
        // A 1 m square in the XY plane, the ray coming down -Z from 2 m (d twice unit length).
        let shape = Shape::of(&Mesh::from(Rectangle::new(1.0, 1.0))).unwrap();
        let d = Vec3::new(0.0, 0.0, -2.0);
        let t = shape.hit(Vec3::new(0.2, 0.1, 2.0), d, f32::INFINITY).unwrap();
        assert!((t - 1.0).abs() < 1e-6, "{t}");
        // Something nearer was hit already; beside the square; looking away from it.
        assert_eq!(shape.hit(Vec3::new(0.2, 0.1, 2.0), d, 0.5), None);
        assert_eq!(shape.hit(Vec3::new(0.8, 0.0, 2.0), d, f32::INFINITY), None);
        assert_eq!(shape.hit(Vec3::new(0.2, 0.1, 2.0), -d, f32::INFINITY), None);
    }
}
