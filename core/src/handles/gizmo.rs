use crate::math::rotation;

use crate::math::{Axis, Point3};
use crate::pose::Pose;
use crate::ray::Ray;
use crate::widgets::{MapFn, WidgetHandler, map_handler};

/// The part of a gizmo the pointer grabbed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GizmoPart {
    /// The center knob, which moves freely in the view's plane.
    Free,
    /// The arrow that translates along an axis.
    Translate(Axis),
    /// The ring that rotates about an axis.
    Rotate(Axis),
    /// The outer ring, facing the eye, that scales all dimensions at
    /// once: by how much farther from the centre the pointer is than
    /// where it took hold.
    Scale,
}

/// A translate/rotate gizmo at a pose: a center knob, an arrow `size`
/// pixels long along each local axis and a ring around each (see
/// [`shape`](super::shape)), the same size on screen however far away;
/// with a scale, an outer ring facing the eye that scales all at once.
pub struct Gizmo<M> {
    /// Where the gizmo sits and how it is turned.
    pub pose: Pose,
    /// Arrow length, in pixels.
    pub size: f64,
    /// The message for a dragged pose.
    pub on_drag: WidgetHandler<Pose, M>,
    /// The message for a drag of the outer ring: the factor it scales
    /// by since the drag before. None: the gizmo does not scale.
    pub on_scale: Option<WidgetHandler<f64, M>>,
}

impl<M: 'static> Gizmo<M> {
    /// A gizmo at `pose`; dragging it to `p` produces `on_drag(p)`.
    pub fn new(pose: Pose, size: f64, on_drag: impl Fn(Pose) -> M + 'static) -> Self {
        Self {
            pose,
            size,
            on_drag: Box::new(on_drag),
            on_scale: None,
        }
    }

    /// The same gizmo, scaling all at once by its outer ring: a drag of
    /// it by a factor produces `on_scale(factor)`.
    #[must_use]
    pub fn with_scale(mut self, on_scale: impl Fn(f64) -> M + 'static) -> Self {
        self.on_scale = Some(Box::new(on_scale));
        self
    }

    /// The gizmo's centre.
    pub fn origin(&self) -> Point3 {
        self.pose.translation()
    }

    /// The unit direction of a local axis in model space.
    pub fn axis(&self, axis: Axis) -> Point3 {
        self.pose.axis(axis)
    }

    /// Moves or turns the pose by the pointer's motion on the grabbed part:
    /// in the plane facing the pointer for the center knob, along the axis
    /// line for an arrow, about the axis in the ring's plane for a ring;
    /// scales by the outer ring. No message when the view is edge-on to
    /// the motion, or when the result would not be finite.
    pub fn drag(&self, part: GizmoPart, from: &Ray, to: &Ray) -> Option<M> {
        if part == GizmoPart::Scale {
            let by = Self::factor(self.origin(), from, to)?;
            return self.on_scale.as_ref().map(|on| on(by));
        }
        let pose = Self::moved(self.pose, part, from, to)?;
        Some((self.on_drag)(pose))
    }

    /// `pose` moved or turned by the pointer's motion from `from` to `to`
    /// on `part` of a gizmo there (none for the outer ring, which
    /// scales).
    pub fn moved(pose: Pose, part: GizmoPart, from: &Ray, to: &Ray) -> Option<Pose> {
        let o = pose.translation();
        let moved = match part {
            GizmoPart::Free => {
                // In the plane through the origin facing the pointer.
                let n = from.direction();
                let on_plane = |ray: &Ray| ray.intersect_plane(o, n).map(|t| ray.at(t));
                Pose::from_translation(on_plane(to)? - on_plane(from)?) * pose
            }
            GizmoPart::Translate(axis) => {
                let a = pose.axis(axis);
                let along = |ray: &Ray| ray.closest_on_line(o, a);
                Pose::from_translation(a * (along(to)? - along(from)?)) * pose
            }
            GizmoPart::Rotate(axis) => {
                let a = pose.axis(axis);
                let arm = |ray: &Ray| ray.intersect_plane(o, a).map(|t| ray.at(t) - o);
                let (v0, v1) = (arm(from)?, arm(to)?);
                let angle = libm::atan2(a.dot(v0.cross(v1)), v0.dot(v1));
                let turn = rotation(a, angle);
                Pose::from_rotation_translation(turn * pose.rotation(), o)
            }
            GizmoPart::Scale => return None,
        };
        moved.is_finite().then_some(moved)
    }

    /// The factor a drag of the outer ring of a gizmo at `origin` from
    /// `from` to `to` scales by: how much farther from the centre the
    /// pointer is, in the plane through it facing the pointer.
    pub fn factor(origin: Point3, from: &Ray, to: &Ray) -> Option<f64> {
        let n = from.direction();
        let reach = |ray: &Ray| {
            ray.intersect_plane(origin, n)
                .map(|t| ray.at(t).distance(origin))
        };
        let (was, now) = (reach(from)?, reach(to)?);
        let by = now / was;
        (was > 0.0 && by.is_finite() && by > 0.0).then_some(by)
    }

    pub(crate) fn map_rc<N: 'static>(self, f: &MapFn<M, N>) -> Gizmo<N> {
        Gizmo {
            pose: self.pose,
            size: self.size,
            on_drag: map_handler(self.on_drag, f),
            on_scale: self.on_scale.map(|on| map_handler(on, f)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::rotation_z;
    use crate::ray::Projection;
    use std::f64::consts::FRAC_PI_2;

    const ORTHO: Projection = Projection::Orthographic { radius: 0.05 };

    fn gizmo() -> Gizmo<Pose> {
        Gizmo::new(Pose::from_translation(Point3::Z), 1.0, |p| p)
    }

    /// A ray looking down −z through (x, y).
    fn down(x: f64, y: f64) -> Ray {
        Ray::new(Point3::new(x, y, 10.0), -Point3::Z, ORTHO).unwrap()
    }

    #[test]
    fn arrows_translate_and_rings_rotate() -> anyhow::Result<()> {
        let g = gizmo();
        let x = GizmoPart::Translate(Axis::X);
        let pose = g.drag(x, &down(0.5, 0.0), &down(2.5, 0.0));
        let pose = pose.ok_or_else(|| anyhow::anyhow!("no translation"))?;
        assert!(
            pose.translation()
                .abs_diff_eq(Point3::new(2.0, 0.0, 1.0), 1e-12)
        );

        let z = GizmoPart::Rotate(Axis::Z);
        let pose = g.drag(z, &down(1.0, 0.0), &down(0.0, 1.0));
        let pose = pose.ok_or_else(|| anyhow::anyhow!("no rotation"))?;
        assert!(pose.translation().abs_diff_eq(Point3::Z, 1e-12));
        assert!(pose.rotation().abs_diff_eq(rotation_z(FRAC_PI_2), 1e-12));

        // The center knob follows the pointer in the plane facing it.
        let free = g.drag(GizmoPart::Free, &down(0.0, 0.0), &down(1.0, 2.0));
        let free = free.ok_or_else(|| anyhow::anyhow!("no free move"))?;
        assert!(
            free.translation()
                .abs_diff_eq(Point3::new(1.0, 2.0, 1.0), 1e-12)
        );

        // The outer ring scales by how much farther out the pointer is.
        let by = Gizmo::<Pose>::factor(Point3::Z, &down(1.0, 0.0), &down(1.5, 0.0));
        assert!(by.is_some_and(|by| (by - 1.5).abs() < 1e-12));
        Ok(())
    }
}
