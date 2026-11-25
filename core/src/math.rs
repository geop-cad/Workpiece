//! Point types, backed by `glam`'s double-precision vectors, and the
//! coordinate [`Axis`] names.
//!
//! `Point3` is used for both positions and directions in model space;
//! `Point2` for positions in a surface's (u, v) parameter space.

/// A point (or direction) in 3D model space.
pub type Point3 = glam::DVec3;
/// A point in a surface's (u, v) parameter space.
pub type Point2 = glam::DVec2;

/// One of the three axes of a coordinate frame.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum Axis {
    /// The x axis.
    X,
    /// The y axis.
    Y,
    /// The z axis.
    Z,
}

impl Axis {
    /// All three, in order.
    pub const ALL: [Axis; 3] = [Axis::X, Axis::Y, Axis::Z];

    /// The unit vector along this axis of the global frame.
    pub fn unit(self) -> Point3 {
        match self {
            Axis::X => Point3::X,
            Axis::Y => Point3::Y,
            Axis::Z => Point3::Z,
        }
    }

    /// 0, 1 or 2: the axis's coordinate index.
    pub fn index(self) -> usize {
        match self {
            Axis::X => 0,
            Axis::Y => 1,
            Axis::Z => 2,
        }
    }

    /// The lower-case name: `"x"`, `"y"` or `"z"`.
    pub fn name(self) -> &'static str {
        ["x", "y", "z"][self.index()]
    }

    /// The next axis, cyclically (x → y → z → x). For the plane normal to
    /// `self`, `next()` and `next().next()` are its in-plane x and y.
    pub fn next(self) -> Axis {
        match self {
            Axis::X => Axis::Y,
            Axis::Y => Axis::Z,
            Axis::Z => Axis::X,
        }
    }
}
