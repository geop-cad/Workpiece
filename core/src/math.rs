//! Point types, backed by `glam`'s double-precision vectors, and the
//! coordinate [`Axis`] names.
//!
//! `Point3` is used for both positions and directions in model space;
//! `Point2` for positions in a surface's (u, v) parameter space.
//!
//! And the turns and angles of them, of the `libm` crate's sines and
//! arc tangents: a part builds the same on every target, in a browser
//! as on the desktop, and std's (which glam's [`glam::DQuat::from_axis_angle`],
//! [`glam::DVec2::from_angle`] and the like use) are the platform's C
//! library's, which differ in the last bit (see clippy.toml).

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

/// The unit vector at `angle` (radians) from the u axis.
pub fn from_angle(angle: f64) -> Point2 {
    let (sin, cos) = libm::sincos(angle);
    Point2::new(cos, sin)
}

/// The angle of `v` from the u axis, in radians, in (-π, π].
pub fn to_angle(v: Point2) -> f64 {
    libm::atan2(v.y, v.x)
}

/// The angle that turns `from` to `to`, in radians, in [-π, π]
/// (anticlockwise is positive).
pub fn angle_to(from: Point2, to: Point2) -> f64 {
    let cos = from.dot(to) / (from.length_squared() * to.length_squared()).sqrt();
    libm::acos(cos.clamp(-1.0, 1.0)) * from.perp_dot(to).signum()
}

/// The angle between `a` and `b` (neither zero), in radians, in [0, π].
pub fn angle_between(a: Point3, b: Point3) -> f64 {
    libm::acos((a.dot(b) / (a.length_squared() * b.length_squared()).sqrt()).clamp(-1.0, 1.0))
}

/// The turn by `angle` (radians) about `axis` (a unit vector),
/// right-handed.
pub fn rotation(axis: Point3, angle: f64) -> glam::DQuat {
    let (sin, cos) = libm::sincos(angle * 0.5);
    let v = axis * sin;
    glam::DQuat::from_xyzw(v.x, v.y, v.z, cos)
}

/// The turn by `angle` (radians) about the x axis.
pub fn rotation_x(angle: f64) -> glam::DQuat {
    rotation(Point3::X, angle)
}

/// The turn by `angle` (radians) about the y axis.
pub fn rotation_y(angle: f64) -> glam::DQuat {
    rotation(Point3::Y, angle)
}

/// The turn by `angle` (radians) about the z axis.
pub fn rotation_z(angle: f64) -> glam::DQuat {
    rotation(Point3::Z, angle)
}

/// The axis (a unit vector) and angle (radians, in [0, 2π]) of the
/// turn `q` (a unit quaternion); the x axis for no turn.
pub fn axis_angle(q: glam::DQuat) -> (Point3, f64) {
    let v = Point3::new(q.x, q.y, q.z);
    let length = v.length();
    if length >= 1e-8 {
        (v / length, 2.0 * libm::atan2(length, q.w))
    } else {
        (Point3::X, 0.0)
    }
}

/// The turn by `x` about the x axis, then `y` about the (fixed) y axis,
/// then `z` about the (fixed) z axis, in radians.
pub fn from_euler_xyz([x, y, z]: [f64; 3]) -> glam::DQuat {
    rotation_z(z) * rotation_y(y) * rotation_x(x)
}

/// The angles [`from_euler_xyz`] makes `q` (a unit quaternion) of:
/// `y` in [-π/2, π/2]; where that is ±π/2, `z` is 0.
pub fn to_euler_xyz(q: glam::DQuat) -> [f64; 3] {
    // (Of its matrix's entries, row and column: m.col(c)[r].)
    let m = glam::DMat3::from_quat(q);
    let cos_y = libm::hypot(m.x_axis.x, m.x_axis.y);
    let y = libm::atan2(-m.x_axis.z, cos_y);
    if cos_y > 16.0 * f64::EPSILON {
        let x = libm::atan2(m.y_axis.z, m.z_axis.z);
        let z = libm::atan2(m.x_axis.y, m.x_axis.x);
        [x, y, z]
    } else {
        [libm::atan2(-m.z_axis.y, m.y_axis.y), y, 0.0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // (glam's own, which they stand in for, to compare.)
    #[allow(clippy::disallowed_methods)]
    #[test]
    fn turns_are_glams_to_the_last_bits() {
        let axis = Point3::new(1.0, 2.0, 3.0).normalize();
        for angle in [0.0, 0.3, -1.2, 2.9] {
            let ours = rotation(axis, angle);
            assert!(ours.abs_diff_eq(glam::DQuat::from_axis_angle(axis, angle), 1e-15));
            let (a, t) = axis_angle(ours);
            assert!(rotation(a, t).abs_diff_eq(ours, 1e-15));
            assert!((from_angle(angle) - glam::DVec2::from_angle(angle)).length() < 1e-15);
        }
        for xyz in [
            [0.1, 0.2, 0.3],
            [-1.0, 0.7, 2.5],
            [0.4, std::f64::consts::FRAC_PI_2, 0.0],
        ] {
            let q = from_euler_xyz(xyz);
            assert!(q.abs_diff_eq(
                glam::DQuat::from_euler(glam::EulerRot::ZYX, xyz[2], xyz[1], xyz[0]),
                1e-15
            ));
            assert!(
                from_euler_xyz(to_euler_xyz(q)).abs_diff_eq(q, 1e-12)
                    || from_euler_xyz(to_euler_xyz(q)).abs_diff_eq(-q, 1e-12)
            );
        }
        let (a, b) = (Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0));
        assert!((angle_between(a, b) - a.angle_between(b)).abs() < 1e-15);
        let (u, v) = (Point2::new(1.0, 0.2), Point2::new(-0.3, 1.0));
        assert!((angle_to(u, v) - u.angle_to(v)).abs() < 1e-15);
    }
}
