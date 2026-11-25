//! Threads that are said of the model, not cut into it.

use crate::math::Point3;

/// A thread on a cylindrical face — of a rod, or of a hole — that is
/// not modelled: the face stays a plain cylinder, and the thread is
/// shown on it and drawn the way drawings show threads (ISO 6410).
///
/// It is held by where it is, like what else is said of the model: a
/// step that changes the face afterwards leaves it standing.
#[derive(Debug, Clone, PartialEq)]
pub struct Thread {
    /// The face it is on, by the name it had when the thread was made.
    pub face: String,
    /// The point of the axis where it starts.
    pub origin: Point3,
    /// The axis, from its start to its end (a unit vector).
    pub axis: Point3,
    /// How long it is.
    pub length: f64,
    /// Its major diameter: over the crests of a screw's thread, in the
    /// roots of a nut's.
    pub major: f64,
    /// Its minor diameter: in the roots of a screw's thread, over the
    /// crests of a nut's.
    pub minor: f64,
    /// How far it advances in a turn.
    pub pitch: f64,
    /// Whether it is left-handed.
    pub left: bool,
    /// Whether it is in a hole (a nut's thread), not on a rod.
    pub internal: bool,
    /// What it is called on a drawing: `M6`, `M8×1`.
    pub designation: String,
    /// Whether a drawing marks where it ends with a wide line across
    /// (ISO 6410-1): at its start, at its end. Not where it runs on out
    /// of the part — through a hole, at a hole's mouth.
    pub ends: [bool; 2],
}

impl Thread {
    /// The diameter its thin line is drawn at: where the roots are.
    pub fn roots(&self) -> f64 {
        if self.internal {
            self.major
        } else {
            self.minor
        }
    }

    /// The diameter of the face it is on: where the crests are.
    pub fn crests(&self) -> f64 {
        if self.internal {
            self.minor
        } else {
            self.major
        }
    }
}
