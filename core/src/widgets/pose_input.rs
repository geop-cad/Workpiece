use super::{MapFn, WidgetHandler, map_handler};
use crate::math::Axis;
use crate::pose::Pose;

/// Fields for a pose: its position, and its rotation as Euler angles in
/// degrees (turning about the fixed x, then y, then z axis; see
/// [`Pose::euler_degrees`]).
pub struct PoseInput<M> {
    /// The caption.
    pub label: String,
    /// The current pose.
    pub value: Pose,
    /// The message for an edited pose.
    pub on_change: WidgetHandler<Pose, M>,
}

impl<M: 'static> PoseInput<M> {
    /// Fields showing `value`; editing them to `p` produces
    /// `on_change(p)`.
    pub fn new(
        label: impl Into<String>,
        value: Pose,
        on_change: impl Fn(Pose) -> M + 'static,
    ) -> Self {
        Self {
            label: label.into(),
            value,
            on_change: Box::new(on_change),
        }
    }

    /// The position's coordinates.
    pub fn position(&self) -> [f64; 3] {
        self.value.translation().to_array()
    }

    /// The Euler angles in degrees (never −0).
    pub fn angles(&self) -> [f64; 3] {
        self.value.euler_degrees().map(|a| a + 0.0)
    }

    /// Enters `value` as the position's `axis` coordinate. Non-finite
    /// values are ignored: no message.
    pub fn set_position(&self, axis: Axis, value: f64) -> Option<M> {
        let mut t = self.value.translation();
        t[axis.index()] = value;
        self.set(self.value.with_translation(t))
    }

    /// Enters `degrees` as the Euler angle about `axis`. Non-finite values
    /// are ignored: no message.
    pub fn set_angle(&self, axis: Axis, degrees: f64) -> Option<M> {
        let mut angles = self.angles();
        angles[axis.index()] = degrees;
        self.set(self.value.with_euler_degrees(angles))
    }

    fn set(&self, pose: Pose) -> Option<M> {
        pose.is_finite().then(|| (self.on_change)(pose))
    }

    pub(crate) fn map_rc<N: 'static>(self, f: &MapFn<M, N>) -> PoseInput<N> {
        PoseInput {
            label: self.label,
            value: self.value,
            on_change: map_handler(self.on_change, f),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Point3;

    #[test]
    fn fields_edit_one_coordinate_or_angle() {
        let start =
            Pose::from_translation(Point3::new(1.0, 2.0, 3.0)).with_euler_degrees([0.0, 0.0, 30.0]);
        let input = PoseInput::new("Pose", start, |p| p);
        let moved = input.set_position(Axis::Y, 5.0).expect("finite");
        assert!(
            moved
                .translation()
                .abs_diff_eq(Point3::new(1.0, 5.0, 3.0), 1e-12)
        );
        assert!(
            (moved.euler_degrees()[2] - 30.0).abs() < 1e-9,
            "keeps the rotation"
        );
        let turned = input.set_angle(Axis::X, 45.0).expect("finite");
        let [x, _, z] = turned.euler_degrees();
        assert!((x - 45.0).abs() < 1e-9 && (z - 30.0).abs() < 1e-9);
        assert!(
            turned.translation().abs_diff_eq(start.translation(), 1e-12),
            "keeps the position"
        );
        assert!(input.set_angle(Axis::Z, f64::NAN).is_none());
    }
}
