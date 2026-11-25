//! Pick rays: a pointer position turned into a half-line with a pick
//! tolerance, used to hit-test and drag handles.
//!
//! The tolerance depends on the camera: an orthographic view picks inside a
//! cylinder of constant radius, a perspective view inside a cone that widens
//! with distance from the eye. The hit tests live in private submodules.

mod closest;
mod hit;

use crate::math::Point3;

/// How the pick tolerance grows along a ray.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Projection {
    /// Constant pick radius (a cylinder).
    Orthographic {
        /// Pick radius, in model units.
        radius: f64,
    },
    /// Pick radius `slope · t` at distance `t` from the eye (a cone).
    Perspective {
        /// Pick radius per unit distance.
        slope: f64,
    },
}

/// A half-line from `start` along a unit `direction`, with a pick
/// tolerance given by its [`Projection`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ray {
    start: Point3,
    direction: Point3,
    projection: Projection,
}

impl Ray {
    /// The ray from `start` along `direction` (normalized). `None` when the
    /// direction is zero or not finite.
    pub fn new(start: Point3, direction: Point3, projection: Projection) -> Option<Self> {
        Some(Self {
            start,
            direction: direction.try_normalize()?,
            projection,
        })
    }

    /// Where the ray starts (the eye, or a point on the near plane).
    pub fn start(&self) -> Point3 {
        self.start
    }

    /// The unit direction.
    pub fn direction(&self) -> Point3 {
        self.direction
    }

    /// The pick tolerance model.
    pub fn projection(&self) -> Projection {
        self.projection
    }

    /// The point at distance `t` along the ray.
    pub fn at(&self, t: f64) -> Point3 {
        self.start + self.direction * t
    }

    /// The pick radius at distance `t` along the ray.
    pub fn radius_at(&self, t: f64) -> f64 {
        match self.projection {
            Projection::Orthographic { radius } => radius,
            Projection::Perspective { slope } => slope * t.max(0.0),
        }
    }
}
