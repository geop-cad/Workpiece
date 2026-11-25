use crate::math::Point3;
use crate::ray::Ray;
use crate::widgets::{MapFn, WidgetHandler, map_handler};

/// A knob at a point that moves over a plane through it: as a gizmo's
/// centre does, but kept in that plane (a hole moved over its face).
/// Dragging reports where the knob is now.
pub struct PlanarHandle<M> {
    /// Where the knob is.
    pub at: Point3,
    /// The unit normal of the plane it moves in.
    pub normal: Point3,
    /// The message for the knob dragged to a point.
    pub on_drag: WidgetHandler<Point3, M>,
}

impl<M: 'static> PlanarHandle<M> {
    /// A knob at `at` moving in the plane of `normal` (normalized; `None`
    /// when zero); dragging it to `p` produces `on_drag(p)`.
    pub fn new(
        at: Point3,
        normal: Point3,
        on_drag: impl Fn(Point3) -> M + 'static,
    ) -> Option<Self> {
        Some(Self {
            at,
            normal: normal.try_normalize()?,
            on_drag: Box::new(on_drag),
        })
    }

    /// Moves the knob by the pointer's motion over the plane. No message
    /// while the view is edge-on to it, or when the result would not be
    /// finite.
    pub fn drag(&self, from: &Ray, to: &Ray) -> Option<M> {
        let on = |ray: &Ray| ray.intersect_plane(self.at, self.normal).map(|t| ray.at(t));
        let moved = on(to)? - on(from)?;
        let at = self.at + moved.reject_from_normalized(self.normal);
        at.is_finite().then(|| (self.on_drag)(at))
    }

    pub(crate) fn map_rc<N: 'static>(self, f: &MapFn<M, N>) -> PlanarHandle<N> {
        PlanarHandle {
            at: self.at,
            normal: self.normal,
            on_drag: map_handler(self.on_drag, f),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ray::Projection;

    /// A ray from above and aside through `(x, y, 0)`.
    fn slanted(x: f64, y: f64) -> Ray {
        let ortho = Projection::Orthographic { radius: 0.05 };
        let towards = Point3::new(x, y, 0.0);
        let from = towards + Point3::new(1.0, 2.0, 5.0);
        Ray::new(from, towards - from, ortho).unwrap()
    }

    #[test]
    fn the_knob_follows_the_pointer_over_its_plane() {
        let h = PlanarHandle::new(Point3::new(1.0, 1.0, 0.0), Point3::Z, |p| p).unwrap();
        let at = h.drag(&slanted(1.0, 1.0), &slanted(3.0, -1.0)).unwrap();
        assert!(at.abs_diff_eq(Point3::new(3.0, -1.0, 0.0), 1e-12), "{at}");
    }
}
