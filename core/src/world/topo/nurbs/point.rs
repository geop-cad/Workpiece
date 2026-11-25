//! What a NURBS control point must support.

use core::ops::{Add, Div, Mul};

use crate::math::{Point2, Point3};

/// A control point type: a vector space over `f64` (the glam vectors).
pub trait ControlPoint:
    Copy
    + core::fmt::Debug
    + PartialEq
    + Add<Output = Self>
    + Mul<f64, Output = Self>
    + Div<f64, Output = Self>
{
    /// The origin.
    const ZERO: Self;

    /// `true` when every coordinate is finite.
    fn is_finite(self) -> bool;

    /// The dot product with `other`.
    fn dot(self, other: Self) -> f64;
}

impl ControlPoint for Point3 {
    const ZERO: Self = Point3::ZERO;

    fn is_finite(self) -> bool {
        Point3::is_finite(self)
    }

    fn dot(self, other: Self) -> f64 {
        Point3::dot(self, other)
    }
}

impl ControlPoint for Point2 {
    const ZERO: Self = Point2::ZERO;

    fn is_finite(self) -> bool {
        Point2::is_finite(self)
    }

    fn dot(self, other: Self) -> f64 {
        Point2::dot(self, other)
    }
}
