//! Rigid placements as unit dual quaternions.

use std::ops::Mul;

use glam::{DMat3, DQuat, EulerRot};

use crate::math::{Axis, Point3};

/// A rigid motion (rotation then translation) stored as a unit dual
/// quaternion `real + ε·dual`, with `real` the rotation and
/// `dual = ½·t·real` for the translation `t`.
///
/// Composition is the dual-quaternion product: `(a * b)` applies `b`
/// first, then `a`.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(from = "Placement", into = "Placement")]
pub struct Pose {
    real: DQuat,
    dual: DQuat,
}

/// How a pose is stored: its rotation and translation, which read better
/// than the dual quaternion.
#[derive(serde::Serialize, serde::Deserialize)]
struct Placement {
    rotation: DQuat,
    translation: Point3,
}

impl From<Placement> for Pose {
    fn from(p: Placement) -> Self {
        Pose::from_rotation_translation(p.rotation, p.translation)
    }
}

impl From<Pose> for Placement {
    fn from(p: Pose) -> Self {
        Placement {
            rotation: p.rotation(),
            translation: p.translation(),
        }
    }
}

impl Default for Pose {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Pose {
    /// No motion.
    pub const IDENTITY: Self = Self {
        real: DQuat::IDENTITY,
        dual: DQuat::from_xyzw(0.0, 0.0, 0.0, 0.0),
    };

    /// Rotates by `rotation` (normalized here), then translates by
    /// `translation`.
    pub fn from_rotation_translation(rotation: DQuat, translation: Point3) -> Self {
        let real = rotation.normalize();
        let t = DQuat::from_xyzw(translation.x, translation.y, translation.z, 0.0);
        Self {
            real,
            dual: t * real * 0.5,
        }
    }

    /// A pure translation.
    pub fn from_translation(translation: Point3) -> Self {
        Self::from_rotation_translation(DQuat::IDENTITY, translation)
    }

    /// A pure rotation about the origin.
    pub fn from_rotation(rotation: DQuat) -> Self {
        Self::from_rotation_translation(rotation, Point3::ZERO)
    }

    /// The pose whose x and y axes are `x` and `y` (orthonormal; z is
    /// `x × y`) at `origin`.
    pub fn from_axes(origin: Point3, x: Point3, y: Point3) -> Self {
        let rotation = DQuat::from_mat3(&DMat3::from_cols(x, y, x.cross(y)));
        Self::from_rotation_translation(rotation, origin)
    }

    /// The direction a global axis is turned to.
    pub fn axis(&self, axis: Axis) -> Point3 {
        self.real * axis.unit()
    }

    /// The rotation part.
    pub fn rotation(&self) -> DQuat {
        self.real
    }

    /// The translation part, `2·dual·real*`.
    pub fn translation(&self) -> Point3 {
        let t = self.dual * self.real.conjugate() * 2.0;
        Point3::new(t.x, t.y, t.z)
    }

    /// The rotation as Euler angles in degrees, `[x, y, z]`: turning about
    /// the fixed x axis, then the fixed y axis, then the fixed z axis.
    pub fn euler_degrees(&self) -> [f64; 3] {
        // Fixed x, y, z is moving z, y, x.
        let (z, y, x) = self.real.to_euler(EulerRot::ZYX);
        [x, y, z].map(f64::to_degrees)
    }

    /// The pose with the same translation and the rotation of Euler angles
    /// `[x, y, z]` in degrees (see [`euler_degrees`](Self::euler_degrees)).
    pub fn with_euler_degrees(&self, [x, y, z]: [f64; 3]) -> Self {
        let [x, y, z] = [x, y, z].map(f64::to_radians);
        Self::from_rotation_translation(
            DQuat::from_euler(EulerRot::ZYX, z, y, x),
            self.translation(),
        )
    }

    /// The pose with the same rotation and translation `t`.
    pub fn with_translation(&self, t: Point3) -> Self {
        Self::from_rotation_translation(self.real, t)
    }

    /// The real (rotation) and dual quaternion parts.
    pub fn parts(&self) -> (DQuat, DQuat) {
        (self.real, self.dual)
    }

    /// Moves a point.
    pub fn transform_point(&self, p: Point3) -> Point3 {
        self.real * p + self.translation()
    }

    /// Turns a direction (ignores the translation).
    pub fn transform_vector(&self, v: Point3) -> Point3 {
        self.real * v
    }

    /// Whether every component is finite (no NaN or infinity).
    pub fn is_finite(&self) -> bool {
        self.real.is_finite() && self.dual.is_finite()
    }

    /// The opposite motion. For a unit dual quaternion this is the
    /// quaternion conjugate of both parts.
    pub fn inverse(&self) -> Self {
        Self {
            real: self.real.conjugate(),
            dual: self.dual.conjugate(),
        }
    }
}

impl Mul for Pose {
    type Output = Pose;

    fn mul(self, rhs: Pose) -> Pose {
        Pose {
            real: self.real * rhs.real,
            dual: self.real * rhs.dual + self.dual * rhs.real,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::FRAC_PI_2;

    #[test]
    fn parts_round_trip_and_points_move() {
        let r = DQuat::from_rotation_z(FRAC_PI_2);
        let t = Point3::new(1.0, 2.0, 3.0);
        let pose = Pose::from_rotation_translation(r, t);
        assert!(pose.translation().abs_diff_eq(t, 1e-12));
        assert!(pose.rotation().abs_diff_eq(r, 1e-12));
        assert!(
            pose.transform_point(Point3::X)
                .abs_diff_eq(Point3::new(1.0, 3.0, 3.0), 1e-12)
        );
    }

    #[test]
    fn composition_applies_right_first_and_inverse_undoes() {
        let a = Pose::from_translation(Point3::X);
        let b = Pose::from_rotation(DQuat::from_rotation_z(FRAC_PI_2));
        let p = Point3::new(1.0, 0.0, 5.0);
        let ab = a * b;
        assert!(
            ab.transform_point(p)
                .abs_diff_eq(a.transform_point(b.transform_point(p)), 1e-12)
        );
        let back = ab.inverse() * ab;
        assert!(back.transform_point(p).abs_diff_eq(p, 1e-12));
    }

    #[test]
    fn euler_angles_round_trip() {
        let pose = Pose::from_translation(Point3::new(1.0, 2.0, 3.0))
            .with_euler_degrees([10.0, -20.0, 30.0]);
        let [x, y, z] = pose.euler_degrees();
        assert!((x - 10.0).abs() < 1e-9 && (y + 20.0).abs() < 1e-9 && (z - 30.0).abs() < 1e-9);
        assert!(
            pose.translation()
                .abs_diff_eq(Point3::new(1.0, 2.0, 3.0), 1e-12)
        );
        // Fixed axes: a quarter turn about x, then one about z.
        let turned = Pose::IDENTITY.with_euler_degrees([90.0, 0.0, 90.0]);
        // y → z (about x), then z stays z (about z).
        assert!(
            turned
                .transform_vector(Point3::Y)
                .abs_diff_eq(Point3::Z, 1e-12)
        );
        // x stays x (about x), then x → y (about z).
        assert!(
            turned
                .transform_vector(Point3::X)
                .abs_diff_eq(Point3::Y, 1e-12)
        );
    }
}
