//! Hit tests: does geometry lie within the ray's pick tolerance? Each
//! returns the distance `t` along the ray of the hit, so the nearest of
//! several hits can be chosen.

use super::Ray;
use crate::math::Point3;

impl Ray {
    /// Hits `p` if it is in front of the start and within the pick radius.
    pub fn hit_point(&self, p: Point3) -> Option<f64> {
        let t = self.direction.dot(p - self.start);
        (t >= 0.0 && p.distance(self.at(t)) <= self.radius_at(t)).then_some(t)
    }

    /// Hits the segment `a`–`b` at its point closest to the ray.
    pub fn hit_segment(&self, a: Point3, b: Point3) -> Option<f64> {
        let Some(dir) = (b - a).try_normalize() else {
            return self.hit_point(a);
        };
        match self.closest_on_line(a, dir) {
            Some(s) => self.hit_point(a + dir * s.clamp(0.0, a.distance(b))),
            // Parallel to the ray: its nearer end, if either is hit.
            None => [self.hit_point(a), self.hit_point(b)]
                .into_iter()
                .flatten()
                .min_by(f64::total_cmp),
        }
    }

    /// Hits the circle with `center`, unit `normal` and `radius` where the
    /// ray crosses its plane. A circle seen edge-on is not hit.
    pub fn hit_circle(&self, center: Point3, normal: Point3, radius: f64) -> Option<f64> {
        let t = self.intersect_plane(center, normal)?;
        let off_ring = (self.at(t).distance(center) - radius).abs();
        (off_ring <= self.radius_at(t)).then_some(t)
    }

    /// Hits the triangle `a`, `b`, `c` where the ray's center line crosses
    /// it, from either side (Möller–Trumbore). Faces are picked exactly:
    /// the pick radius is for points and lines, which have no area.
    pub fn hit_triangle(&self, a: Point3, b: Point3, c: Point3) -> Option<f64> {
        let (e1, e2) = (b - a, c - a);
        let p = self.direction.cross(e2);
        let det = e1.dot(p);
        if det.abs() <= f64::EPSILON * e1.length() * e2.length() {
            return None;
        }
        let s = self.start - a;
        let u = s.dot(p) / det;
        let q = s.cross(e1);
        let v = self.direction.dot(q) / det;
        let t = e2.dot(q) / det;
        (u >= 0.0 && v >= 0.0 && u + v <= 1.0 && t >= 0.0).then_some(t)
    }
}

#[cfg(test)]
mod tests {
    use super::super::Projection;
    use super::*;

    #[test]
    fn orthographic_picks_in_a_cylinder() {
        let r = Ray::new(
            Point3::ZERO,
            Point3::X,
            Projection::Orthographic { radius: 0.1 },
        )
        .unwrap();
        assert_eq!(r.hit_point(Point3::new(100.0, 0.05, 0.0)), Some(100.0));
        assert_eq!(r.hit_point(Point3::new(1.0, 0.2, 0.0)), None);
        assert_eq!(r.hit_point(Point3::new(-1.0, 0.0, 0.0)), None);
    }

    #[test]
    fn perspective_picks_in_a_cone() {
        let r = Ray::new(
            Point3::ZERO,
            Point3::X,
            Projection::Perspective { slope: 0.1 },
        )
        .unwrap();
        assert!(r.hit_point(Point3::new(10.0, 0.5, 0.0)).is_some());
        assert!(r.hit_point(Point3::new(1.0, 0.5, 0.0)).is_none());
    }

    #[test]
    fn segments_and_circles() {
        let r = Ray::new(
            Point3::new(0.0, 0.0, 5.0),
            -Point3::Z,
            Projection::Orthographic { radius: 0.1 },
        )
        .unwrap();
        // The segment passes under the ray; one beside it does not.
        assert!(
            r.hit_segment(Point3::new(-1.0, 0.0, 0.0), Point3::X)
                .is_some()
        );
        assert!(
            r.hit_segment(Point3::X, Point3::new(2.0, 0.0, 0.0))
                .is_none()
        );
        // A unit circle around (1, 0, 0) passes through the origin.
        assert_eq!(r.hit_circle(Point3::X, Point3::Z, 1.0), Some(5.0));
        assert_eq!(r.hit_circle(Point3::X, Point3::Z, 0.5), None);
    }

    #[test]
    fn triangles_are_hit_inside_only() {
        let ortho = Projection::Orthographic { radius: 0.1 };
        let down = |x: f64, y: f64| Ray::new(Point3::new(x, y, 5.0), -Point3::Z, ortho).unwrap();
        let (a, b, c) = (Point3::ZERO, Point3::X, Point3::Y);
        assert_eq!(down(0.2, 0.2).hit_triangle(a, b, c), Some(5.0));
        // Either winding.
        assert_eq!(down(0.2, 0.2).hit_triangle(a, c, b), Some(5.0));
        assert_eq!(down(0.8, 0.8).hit_triangle(a, b, c), None);
        // Behind the start.
        let up = Ray::new(Point3::new(0.2, 0.2, 5.0), Point3::Z, ortho).unwrap();
        assert_eq!(up.hit_triangle(a, b, c), None);
    }
}
