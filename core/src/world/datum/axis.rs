use super::{DatumError, check_point};
use crate::math::Point3;

/// An infinite oriented line: a point on it and a unit direction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DatumAxis {
    origin: Point3,
    direction: Point3,
}

impl DatumAxis {
    /// The axis through `origin` along `direction` (normalized).
    pub fn new(origin: Point3, direction: Point3) -> Result<Self, DatumError> {
        check_point(origin)?;
        let direction = direction
            .try_normalize()
            .ok_or(DatumError::DegenerateDirection)?;
        Ok(Self { origin, direction })
    }

    /// A point on the axis.
    pub fn origin(&self) -> Point3 {
        self.origin
    }

    /// The unit direction.
    pub fn direction(&self) -> Point3 {
        self.direction
    }
}
