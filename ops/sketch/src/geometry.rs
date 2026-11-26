//! The curves of a sketch's entities: as polylines for drawing and
//! pointing at, and as exact NURBS for the profile.

use std::f64::consts::{FRAC_PI_2, TAU};

use workpiece_core::math::Point2;
use workpiece_core::world::topo::Curve2;

use crate::model::{Id, Shape, SketchArgs};

/// Segments a full circle is drawn with.
const CIRCLE_SEGMENTS: usize = 96;

/// The angle of `v`, in `[0, 2π)`.
pub(crate) fn angle(v: Point2) -> f64 {
    libm::atan2(v.y, v.x).rem_euclid(TAU)
}

/// The angle swept counter-clockwise from direction `from` to `to`, in
/// `(0, 2π]`.
pub(crate) fn sweep(from: Point2, to: Point2) -> f64 {
    let s = (angle(to) - angle(from)).rem_euclid(TAU);
    if s <= 1e-12 { TAU } else { s }
}

/// The unit vector at `angle`.
pub(crate) fn unit(angle: f64) -> Point2 {
    Point2::new(libm::cos(angle), libm::sin(angle))
}

/// The point of segment `a`–`b` nearest to `p`.
pub(crate) fn nearest_on_segment(p: Point2, a: Point2, b: Point2) -> Point2 {
    let d = b - a;
    let l2 = d.length_squared();
    if l2 == 0.0 {
        return a;
    }
    a + d * ((p - a).dot(d) / l2).clamp(0.0, 1.0)
}

/// The knots of a clamped uniform B-spline of `degree` with `n` control
/// points, on `[0, 1]`.
pub(crate) fn uniform_knots(degree: usize, n: usize) -> Vec<f64> {
    let spans = n - degree;
    let mut knots = vec![0.0; degree + 1];
    knots.extend((1..spans).map(|i| i as f64 / spans as f64));
    knots.extend(std::iter::repeat_n(1.0, degree + 1));
    knots
}

/// The control points of the clamped uniform B-spline of `degree` (as
/// far as the points allow one) that goes through `through`, one after
/// the other. `None` for fewer than two points.
pub(crate) fn interpolated(through: &[Point2], degree: usize) -> Option<Vec<Point2>> {
    let n = through.len();
    if n < 2 {
        return None;
    }
    let degree = degree.clamp(1, n - 1);
    let knots = uniform_knots(degree, n);
    // Through each point where its own control point weighs most.
    let sites: Vec<f64> = (0..n)
        .map(|j| knots[j + 1..=j + degree].iter().sum::<f64>() / degree as f64)
        .collect();
    // What each control point weighs at each site.
    let mut rows = vec![vec![0.0; n]; n];
    for j in 0..n {
        let mut unit = vec![Point2::ZERO; n];
        unit[j] = Point2::X;
        let basis = Curve2::new(degree, knots.clone(), unit, None).ok()?;
        for (i, row) in rows.iter_mut().enumerate() {
            row[j] = basis.point_at(sites[i]).x;
        }
    }
    let mut wanted = through.to_vec();
    for c in 0..n {
        let pivot = (c..n).max_by(|a, b| rows[*a][c].abs().total_cmp(&rows[*b][c].abs()))?;
        if rows[pivot][c].abs() < 1e-12 {
            return None;
        }
        rows.swap(c, pivot);
        wanted.swap(c, pivot);
        let (top, at) = (rows[c].clone(), wanted[c]);
        for r in 0..n {
            if r == c {
                continue;
            }
            let m = rows[r][c] / top[c];
            if m != 0.0 {
                for (x, y) in rows[r].iter_mut().zip(&top) {
                    *x -= m * y;
                }
                wanted[r] -= at * m;
            }
        }
    }
    Some((0..n).map(|c| wanted[c] / rows[c][c]).collect())
}

/// A circular arc of `sweep` radians counter-clockwise from `start`
/// around `center`, as rational quadratic pieces of at most a quarter
/// turn, on `[0, 1]`.
pub(crate) fn arc_curve(center: Point2, start: Point2, sweep: f64) -> Option<Curve2> {
    let pieces = (sweep / FRAC_PI_2 - 1e-9).ceil().max(1.0) as usize;
    let step = sweep / pieces as f64;
    let (radius, from) = (center.distance(start), angle(start - center));
    let corner_weight = libm::cos(step / 2.0);
    let mut points = vec![start];
    let mut weights = vec![1.0];
    let mut knots = vec![0.0; 3];
    for k in 0..pieces {
        let a = from + step * k as f64;
        points.push(center + unit(a + step / 2.0) * (radius / corner_weight));
        points.push(center + unit(a + step) * radius);
        weights.extend([corner_weight, 1.0]);
        let t = (k + 1) as f64 / pieces as f64;
        knots.extend([t, t]);
    }
    knots.push(1.0);
    Curve2::new(2, knots, points, Some(weights)).ok()
}

impl SketchArgs {
    /// The exact curve of entity `id`, for the profile and for drawing.
    /// `None` for a point entity, or one too degenerate to be a curve.
    pub fn curve(&self, id: Id) -> Option<Curve2> {
        match &self.entities.get(&id)?.shape {
            Shape::Point(_) | Shape::Axis { .. } | Shape::Text(_) => None,
            Shape::Line(a, b) => Curve2::line(self.at(*a), self.at(*b)).ok(),
            Shape::Circle { center, radius } => {
                let c = self.at(*center);
                arc_curve(c, c + Point2::X * *radius, TAU)
            }
            Shape::Arc { center, start, end } => {
                let (c, s, e) = (self.at(*center), self.at(*start), self.at(*end));
                arc_curve(c, s, sweep(s - c, e - c))
            }
            Shape::Spline {
                control,
                degree,
                knots,
                weights,
            } => {
                let points: Vec<Point2> = control.iter().map(|&p| self.at(p)).collect();
                let n = points.len();
                if n < 2 {
                    return None;
                }
                let degree = (*degree).clamp(1, n - 1);
                let knots = if knots.len() == n + degree + 1 {
                    knots.clone()
                } else {
                    uniform_knots(degree, n)
                };
                let weights = (weights.len() == n).then(|| weights.clone());
                Curve2::new(degree, knots, points, weights).ok()
            }
        }
    }

    /// Points along entity `id`, close enough to draw it and to point at
    /// it.
    pub fn polyline(&self, id: Id) -> Vec<Point2> {
        match self.entities.get(&id).map(|e| &e.shape) {
            Some(Shape::Line(a, b)) => vec![self.at(*a), self.at(*b)],
            Some(Shape::Circle { .. } | Shape::Arc { .. }) => {
                let Some((_, c, r)) = self.circle(id) else {
                    return Vec::new();
                };
                let (from, total) = match self.entities[&id].shape {
                    Shape::Arc { start, end, .. } => {
                        let (s, e) = (self.at(start) - c, self.at(end) - c);
                        (angle(s), sweep(s, e))
                    }
                    _ => (0.0, TAU),
                };
                let n = ((total / TAU * CIRCLE_SEGMENTS as f64).ceil() as usize).max(2);
                (0..=n)
                    .map(|k| c + unit(from + total * k as f64 / n as f64) * r)
                    .collect()
            }
            // A text by its box.
            Some(Shape::Text(_)) => self
                .text_frame(id)
                .map_or_else(Vec::new, |frame| frame.outline()),
            Some(Shape::Spline { .. }) => self.curve(id).map_or_else(Vec::new, |curve| {
                let (t0, t1) = curve.domain();
                curve.polyline(t0, t1)
            }),
            _ => Vec::new(),
        }
    }

    /// How far `p` is from entity `id` (from its point, for a point
    /// entity).
    pub fn distance_to(&self, id: Id, p: Point2) -> f64 {
        // An axis goes on without end.
        if let (true, Some((a, b))) = (self.is_endless(id), self.line(id)) {
            return (b - a).normalize_or_zero().perp_dot(p - a).abs();
        }
        match self.entities.get(&id).map(|e| &e.shape) {
            Some(Shape::Point(q)) => self.at(*q).distance(p),
            Some(Shape::Circle { center, radius }) => (self.at(*center).distance(p) - radius).abs(),
            // Anywhere in its box is on a text.
            Some(Shape::Text(_)) if self.text_frame(id).is_some_and(|f| f.contains(p)) => 0.0,
            Some(_) => self
                .polyline(id)
                .windows(2)
                .map(|s| nearest_on_segment(p, s[0], s[1]).distance(p))
                .fold(f64::INFINITY, f64::min),
            None => f64::INFINITY,
        }
    }

    /// A point in the middle of entity `id`: where its badges sit.
    pub fn middle(&self, id: Id) -> Point2 {
        match self.entities.get(&id).map(|e| &e.shape) {
            Some(Shape::Circle { center, radius }) => {
                self.at(*center) + unit(std::f64::consts::FRAC_PI_4) * *radius
            }
            Some(Shape::Point(p)) => self.at(*p),
            _ => {
                let points = self.polyline(id);
                match points.len() {
                    0 => Point2::ZERO,
                    n if n % 2 == 1 => points[n / 2],
                    n => (points[n / 2 - 1] + points[n / 2]) / 2.0,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arcs_are_exact_circles_in_pieces() {
        let (c, s) = (Point2::new(1.0, 2.0), Point2::new(4.0, 2.0));
        for total in [0.3, FRAC_PI_2, 2.0, 4.5, TAU] {
            let curve = arc_curve(c, s, total).expect("an arc");
            for k in 0..=20 {
                let p = curve.point_at(f64::from(k) / 20.0);
                assert!((p.distance(c) - 3.0).abs() < 1e-12);
            }
            let end = curve.point_at(1.0);
            assert!(end.abs_diff_eq(c + unit(total) * 3.0, 1e-12), "{total}");
        }
    }

    #[test]
    fn sweeps_run_counter_clockwise() {
        assert!((sweep(Point2::X, Point2::Y) - FRAC_PI_2).abs() < 1e-12);
        assert!((sweep(Point2::Y, Point2::X) - 3.0 * FRAC_PI_2).abs() < 1e-12);
        assert_eq!(sweep(Point2::X, Point2::X), TAU);
    }
}
