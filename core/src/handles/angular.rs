use glam::DQuat;

use crate::math::Point3;
use crate::ray::Ray;
use crate::widgets::{MapFn, WidgetHandler, map_handler};

/// An angle about an axis: a ring around it with a knob at the angle,
/// measured from the `zero` direction. Dragging turns the knob and
/// reports the new angle, in degrees.
pub struct AngularHandle<M> {
    /// A point on the axis: the ring's center.
    pub center: Point3,
    /// The unit axis the angle turns about (right-handed).
    pub axis: Point3,
    /// The unit direction of angle 0, normal to the axis.
    pub zero: Point3,
    /// The current angle in degrees.
    pub value: f64,
    /// The message for a dragged angle.
    pub on_drag: WidgetHandler<f64, M>,
}

impl<M: 'static> AngularHandle<M> {
    /// A handle about `axis` (normalized) through `center`, angle 0 along
    /// `zero` (made normal to the axis); `None` if either is degenerate.
    pub fn new(
        center: Point3,
        axis: Point3,
        zero: Point3,
        value: f64,
        on_drag: impl Fn(f64) -> M + 'static,
    ) -> Option<Self> {
        let axis = axis.try_normalize()?;
        let zero = zero.reject_from_normalized(axis).try_normalize()?;
        Some(Self {
            center,
            axis,
            zero,
            value,
            on_drag: Box::new(on_drag),
        })
    }

    /// The unit direction of the current angle.
    pub fn direction(&self) -> Point3 {
        DQuat::from_axis_angle(self.axis, self.value.to_radians()) * self.zero
    }

    /// Turns the angle by the pointer's motion about the axis, in the
    /// ring's plane. No message when the view is edge-on to it, or when the
    /// result would not be finite.
    pub fn drag(&self, from: &Ray, to: &Ray) -> Option<M> {
        let arm = |ray: &Ray| {
            ray.intersect_plane(self.center, self.axis)
                .map(|t| ray.at(t) - self.center)
        };
        let (v0, v1) = (arm(from)?, arm(to)?);
        let turn = self.axis.dot(v0.cross(v1)).atan2(v0.dot(v1));
        let value = self.value + turn.to_degrees();
        value.is_finite().then(|| (self.on_drag)(value))
    }

    pub(crate) fn map_rc<N: 'static>(self, f: &MapFn<M, N>) -> AngularHandle<N> {
        AngularHandle {
            center: self.center,
            axis: self.axis,
            zero: self.zero,
            value: self.value,
            on_drag: map_handler(self.on_drag, f),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ray::Projection;

    fn down(x: f64, y: f64) -> Ray {
        let ortho = Projection::Orthographic { radius: 0.05 };
        Ray::new(Point3::new(x, y, 10.0), -Point3::Z, ortho).unwrap()
    }

    #[test]
    fn dragging_turns_the_angle() {
        let h = AngularHandle::new(Point3::ZERO, Point3::Z, Point3::X, 10.0, |a| a).unwrap();
        // A quarter turn counter-clockwise about z.
        let angle = h.drag(&down(1.0, 0.0), &down(0.0, 1.0)).unwrap();
        assert!((angle - 100.0).abs() < 1e-9);
        assert!(h.direction().abs_diff_eq(
            Point3::new(10f64.to_radians().cos(), 10f64.to_radians().sin(), 0.0),
            1e-12
        ));
    }
}
