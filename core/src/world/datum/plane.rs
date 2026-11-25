use super::{DatumError, DatumFrame, check_point, check_pose};
use crate::math::{Axis, Point3};
use crate::pose::Pose;

/// An infinite oriented plane with an in-plane x direction, so that it
/// also fixes a 2D coordinate system (what a profile is drawn in). Stored
/// as the pose whose XY plane it is: the pose's z is the normal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DatumPlane {
    pose: Pose,
}

impl DatumPlane {
    /// The plane through `origin` with `normal`. `x_dir` is projected into
    /// the plane and normalized; it must not be parallel to `normal`.
    pub fn new(origin: Point3, normal: Point3, x_dir: Point3) -> Result<Self, DatumError> {
        check_point(origin)?;
        let normal = normal
            .try_normalize()
            .ok_or(DatumError::DegenerateDirection)?;
        x_dir
            .try_normalize()
            .ok_or(DatumError::DegenerateDirection)?;
        let x = x_dir
            .reject_from_normalized(normal)
            .try_normalize()
            .ok_or(DatumError::ParallelDirections)?;
        Ok(Self::from_finite_pose(Pose::from_axes(
            origin,
            x,
            normal.cross(x),
        )))
    }

    /// The plane through `origin` with `normal`, its axes laid out by the
    /// normal alone, with "up" up: the rule every plane made of a face
    /// follows, so that a sketch on a face and a sketch on a plane made
    /// from it have the same u, v and normal. For a normal not along the
    /// world's z, v is world +z projected into the plane and normalized,
    /// and u = v × normal, so (u, v, normal) is right-handed and a sketch
    /// on a side face reads the right way round seen from outside. A
    /// normal along +z (a top face) gives u = +x, v = +y; along −z (a
    /// bottom face, seen from below) u = +x, v = −y.
    pub fn laid_out(origin: Point3, normal: Point3) -> Result<Self, DatumError> {
        let normal = normal
            .try_normalize()
            .ok_or(DatumError::DegenerateDirection)?;
        let up = Point3::Z.reject_from_normalized(normal);
        let (x_dir, y_dir) = if up.length() < 1e-9 {
            (Point3::X, Point3::Y * normal.z.signum())
        } else {
            let v = up.normalize();
            (v.cross(normal), v)
        };
        check_point(origin)?;
        Ok(Self::from_finite_pose(Pose::from_axes(
            origin, x_dir, y_dir,
        )))
    }

    /// The XY plane of the pose; the pose must be finite.
    pub fn from_pose(pose: Pose) -> Result<Self, DatumError> {
        check_pose(pose)?;
        Ok(Self { pose })
    }

    /// For callers that built the pose from finite parts.
    pub(super) fn from_finite_pose(pose: Pose) -> Self {
        debug_assert!(pose.is_finite());
        Self { pose }
    }

    /// The pose whose XY plane this is.
    pub fn pose(&self) -> Pose {
        self.pose
    }

    /// The XY plane of the global frame.
    pub fn xy() -> Self {
        DatumFrame::global().plane(Axis::Z)
    }

    /// The YZ plane of the global frame.
    pub fn yz() -> Self {
        DatumFrame::global().plane(Axis::X)
    }

    /// The ZX plane of the global frame.
    pub fn zx() -> Self {
        DatumFrame::global().plane(Axis::Y)
    }

    /// A point on the plane, the 2D origin.
    pub fn origin(&self) -> Point3 {
        self.pose.translation()
    }

    /// The unit normal.
    pub fn normal(&self) -> Point3 {
        self.pose.axis(Axis::Z)
    }

    /// The unit in-plane x direction.
    pub fn x_dir(&self) -> Point3 {
        self.pose.axis(Axis::X)
    }

    /// The unit in-plane y direction, `normal × x_dir`.
    pub fn y_dir(&self) -> Point3 {
        self.pose.axis(Axis::Y)
    }
}
