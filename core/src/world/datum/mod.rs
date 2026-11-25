//! Datums: reference geometry (frames, axes, planes) that features and
//! profiles are placed on. Each kind lives in its own file.

mod axis;
mod frame;
mod plane;

pub use axis::DatumAxis;
pub use frame::DatumFrame;
pub use plane::DatumPlane;

/// Any datum, as stored in [`crate::world::World::datums`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Datum {
    /// A coordinate frame.
    Frame(DatumFrame),
    /// An infinite oriented line.
    Axis(DatumAxis),
    /// An infinite oriented plane.
    Plane(DatumPlane),
}

impl From<DatumFrame> for Datum {
    fn from(d: DatumFrame) -> Self {
        Datum::Frame(d)
    }
}

impl From<DatumAxis> for Datum {
    fn from(d: DatumAxis) -> Self {
        Datum::Axis(d)
    }
}

impl From<DatumPlane> for Datum {
    fn from(d: DatumPlane) -> Self {
        Datum::Plane(d)
    }
}

/// Why a datum could not be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DatumError {
    /// A direction was zero-length or not finite.
    #[error("direction is zero-length or not finite")]
    DegenerateDirection,
    /// Two directions that must span a plane are parallel.
    #[error("directions are parallel")]
    ParallelDirections,
    /// A point coordinate was not finite.
    #[error("point is not finite")]
    NonFinitePoint,
    /// A pose had a component that was not finite.
    #[error("pose is not finite")]
    NonFinitePose,
}

pub(crate) fn check_pose(pose: crate::pose::Pose) -> Result<(), DatumError> {
    if pose.is_finite() {
        Ok(())
    } else {
        Err(DatumError::NonFinitePose)
    }
}

pub(crate) fn check_point(p: crate::math::Point3) -> Result<(), DatumError> {
    if p.is_finite() {
        Ok(())
    } else {
        Err(DatumError::NonFinitePoint)
    }
}
