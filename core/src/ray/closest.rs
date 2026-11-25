//! Closest points between the ray's line and other geometry. These ignore
//! the pick tolerance; dragging uses them to map a pointer to a value.

use super::Ray;
use crate::math::Point3;

/// Below this `1 − (d·u)²`, two lines count as parallel.
const PARALLEL: f64 = 1e-12;

impl Ray {
    /// Parameters `(t, s)` of the closest points between the ray's line
    /// (`start + t·direction`) and the line `origin + s·dir` (`dir` unit).
    /// `None` when the lines are parallel.
    pub fn closest_to_line(&self, origin: Point3, dir: Point3) -> Option<(f64, f64)> {
        let w = self.start - origin;
        let b = self.direction.dot(dir);
        let den = 1.0 - b * b;
        if den < PARALLEL {
            return None;
        }
        let dw = self.direction.dot(w);
        let uw = dir.dot(w);
        Some(((b * uw - dw) / den, (uw - b * dw) / den))
    }

    /// The parameter `s` on the line `origin + s·dir` (`dir` unit) closest
    /// to the ray's line. `None` when the lines are parallel.
    pub fn closest_on_line(&self, origin: Point3, dir: Point3) -> Option<f64> {
        self.closest_to_line(origin, dir).map(|(_, s)| s)
    }

    /// Distance `t` along the ray where it crosses the plane through
    /// `origin` with `normal`. `None` when parallel or behind the start.
    pub fn intersect_plane(&self, origin: Point3, normal: Point3) -> Option<f64> {
        let den = self.direction.dot(normal);
        if den.abs() < PARALLEL {
            return None;
        }
        let t = normal.dot(origin - self.start) / den;
        (t >= 0.0).then_some(t)
    }
}

#[cfg(test)]
mod tests {
    use super::super::Projection;
    use super::*;

    fn ray(start: Point3, dir: Point3) -> Ray {
        Ray::new(start, dir, Projection::Orthographic { radius: 0.1 }).unwrap()
    }

    #[test]
    fn skew_lines_meet_at_their_common_perpendicular() {
        let r = ray(Point3::new(5.0, 0.0, 2.0), -Point3::X);
        let (t, s) = r.closest_to_line(Point3::ZERO, Point3::Z).unwrap();
        assert!((t - 5.0).abs() < 1e-12 && (s - 2.0).abs() < 1e-12);
        assert_eq!(r.closest_to_line(Point3::ZERO, Point3::X), None);
    }

    #[test]
    fn planes_are_hit_in_front_only() {
        let r = ray(Point3::new(0.0, 0.0, 3.0), -Point3::Z);
        assert_eq!(r.intersect_plane(Point3::ZERO, Point3::Z), Some(3.0));
        assert_eq!(
            r.intersect_plane(Point3::new(0.0, 0.0, 4.0), Point3::Z),
            None
        );
        assert_eq!(r.intersect_plane(Point3::ZERO, Point3::X), None);
    }
}
