use crate::math::Point3;
use crate::ray::Ray;
use crate::widgets::{MapFn, WidgetHandler, map_handler};

/// An arrow from `origin` along `direction`, its tip at `value`; dragging
/// it moves the tip along the line and reports the new value.
pub struct LinearHandle<M> {
    /// Where the value is zero.
    pub origin: Point3,
    /// The unit direction values are measured along.
    pub direction: Point3,
    /// The current value: the tip sits at `origin + direction · value`.
    pub value: f64,
    /// The message for a dragged value.
    pub on_drag: WidgetHandler<f64, M>,
}

impl<M: 'static> LinearHandle<M> {
    /// A handle along `direction` (normalized; `None` when zero); dragging
    /// the tip to `x` produces `on_drag(x)`.
    pub fn new(
        origin: Point3,
        direction: Point3,
        value: f64,
        on_drag: impl Fn(f64) -> M + 'static,
    ) -> Option<Self> {
        Some(Self {
            origin,
            direction: direction.try_normalize()?,
            value,
            on_drag: Box::new(on_drag),
        })
    }

    /// The tip of the arrow.
    pub fn tip(&self) -> Point3 {
        self.origin + self.direction * self.value
    }

    /// Moves the tip by the pointer's motion along the line. No message
    /// while the view looks straight down the line, or when the result
    /// would not be finite.
    pub fn drag(&self, from: &Ray, to: &Ray) -> Option<M> {
        let along = |ray: &Ray| ray.closest_on_line(self.origin, self.direction);
        let value = self.value + along(to)? - along(from)?;
        value.is_finite().then(|| (self.on_drag)(value))
    }

    pub(crate) fn map_rc<N: 'static>(self, f: &MapFn<M, N>) -> LinearHandle<N> {
        LinearHandle {
            origin: self.origin,
            direction: self.direction,
            value: self.value,
            on_drag: map_handler(self.on_drag, f),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ray::Projection;

    fn ray_at(z: f64) -> Ray {
        let ortho = Projection::Orthographic { radius: 0.05 };
        Ray::new(Point3::new(10.0, 0.0, z), -Point3::X, ortho).unwrap()
    }

    #[test]
    fn dragging_moves_the_value_along_the_line() {
        let h = LinearHandle::new(Point3::ZERO, Point3::Z, 1.0, |x| x).unwrap();
        let value = h.drag(&ray_at(1.0), &ray_at(3.0)).unwrap();
        assert!((value - 3.0).abs() < 1e-12);
    }
}
