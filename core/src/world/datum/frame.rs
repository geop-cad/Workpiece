use super::{DatumAxis, DatumError, DatumPlane, check_point, check_pose};
use crate::math::{Axis, Point3};
use crate::pose::Pose;

/// A right-handed orthonormal coordinate frame: where a pose places the
/// global frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DatumFrame {
    pose: Pose,
}

impl DatumFrame {
    /// The frame at `origin` whose x axis is `x_dir` (normalized) and whose
    /// y axis is `y_hint` made orthogonal to x (Gram–Schmidt); z = x × y.
    pub fn new(origin: Point3, x_dir: Point3, y_hint: Point3) -> Result<Self, DatumError> {
        check_point(origin)?;
        let x = x_dir
            .try_normalize()
            .ok_or(DatumError::DegenerateDirection)?;
        y_hint
            .try_normalize()
            .ok_or(DatumError::DegenerateDirection)?;
        let y = y_hint
            .reject_from_normalized(x)
            .try_normalize()
            .ok_or(DatumError::ParallelDirections)?;
        Ok(Self {
            pose: Pose::from_axes(origin, x, y),
        })
    }

    /// The global frame.
    pub fn global() -> Self {
        Self {
            pose: Pose::IDENTITY,
        }
    }

    /// The frame a pose places the global frame at; the pose must be
    /// finite.
    pub fn from_pose(pose: Pose) -> Result<Self, DatumError> {
        check_pose(pose)?;
        Ok(Self { pose })
    }

    /// The pose that places the global frame at this frame.
    pub fn pose(&self) -> Pose {
        self.pose
    }

    /// The origin.
    pub fn origin(&self) -> Point3 {
        self.pose.translation()
    }

    /// The unit direction of one of the frame's axes.
    pub fn direction(&self, axis: Axis) -> Point3 {
        self.pose.axis(axis)
    }

    /// The unit x axis direction.
    pub fn x(&self) -> Point3 {
        self.direction(Axis::X)
    }

    /// The unit y axis direction.
    pub fn y(&self) -> Point3 {
        self.direction(Axis::Y)
    }

    /// The unit z axis direction.
    pub fn z(&self) -> Point3 {
        self.direction(Axis::Z)
    }

    /// One of the frame's axes as a datum axis.
    pub fn axis(&self, axis: Axis) -> DatumAxis {
        // The pose is finite (checked on construction), so the origin is
        // finite and the direction unit length: `new` cannot fail.
        DatumAxis::new(self.origin(), self.direction(axis)).expect("frame poses are finite")
    }

    /// The frame's plane normal to `axis` (the YZ plane for x, ZX for y,
    /// XY for z), with the next axis cyclically as its x direction.
    pub fn plane(&self, axis: Axis) -> DatumPlane {
        let x = self.direction(axis.next());
        let y = self.direction(axis.next().next());
        DatumPlane::from_finite_pose(Pose::from_axes(self.origin(), x, y))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn planes_are_normal_to_their_axis() -> Result<(), DatumError> {
        let f = DatumFrame::new(Point3::new(1.0, 2.0, 3.0), Point3::Y, -Point3::X)?;
        assert!(f.z().abs_diff_eq(Point3::Z, 1e-12));
        for axis in Axis::ALL {
            let p = f.plane(axis);
            assert!(p.normal().abs_diff_eq(f.direction(axis), 1e-12));
            assert!(p.origin().abs_diff_eq(f.origin(), 1e-12));
        }
        assert!(f.axis(Axis::X).direction().abs_diff_eq(Point3::Y, 1e-12));
        Ok(())
    }

    #[test]
    fn non_finite_poses_are_rejected() {
        let nan = Pose::from_translation(Point3::new(f64::NAN, 0.0, 0.0));
        assert_eq!(DatumFrame::from_pose(nan), Err(DatumError::NonFinitePose));
        assert_eq!(DatumPlane::from_pose(nan), Err(DatumError::NonFinitePose));
    }
}
