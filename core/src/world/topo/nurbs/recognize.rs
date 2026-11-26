//! Recognizing plane curves: every curve is stored as a NURBS, but a
//! sketch wants to know when one is a line, an arc or a circle, so it can
//! be constrained and dimensioned like one.
//!
//! A curve *is* a simpler shape when it stays within a tolerance of it:
//! with an edge's tolerance, that is when the simpler shape is as good a
//! description of the edge as its own curve.

use std::f64::consts::TAU;

use super::curve::Curve2;
use crate::math::Point2;

/// Samples per knot span when a curve is compared with a circle.
const SAMPLES_PER_SPAN: usize = 16;

/// What a plane curve is.
#[derive(Debug, Clone, PartialEq)]
pub enum Recognized {
    /// A single point: the curve never leaves it.
    Point(Point2),
    /// A straight segment, from start to end.
    Line(Point2, Point2),
    /// A circular arc, counter-clockwise from `start` to `end` (which may
    /// be the reverse of the curve's direction).
    Arc {
        /// The center.
        center: Point2,
        /// Where the arc starts.
        start: Point2,
        /// Where it ends.
        end: Point2,
    },
    /// A full circle.
    Circle {
        /// The center.
        center: Point2,
        /// The radius.
        radius: f64,
    },
    /// None of those: the piece of the curve itself.
    Spline(Curve2),
}

/// What the piece `[t0, t1]` of `curve` is within `tolerance`, and how far
/// the curve strays from it (0 for a spline, which is the curve). `None`
/// for an empty range.
pub fn recognize(curve: &Curve2, t0: f64, t1: f64, tolerance: f64) -> Option<(Recognized, f64)> {
    let piece = curve.trimmed(t0, t1).ok()?;
    let points = piece.control_points();
    let (start, end) = (points[0], points[points.len() - 1]);

    // A point or a line: the control points bound the curve.
    let spread = points.iter().map(|p| p.distance(start)).fold(0.0, f64::max);
    if spread <= tolerance {
        return Some((Recognized::Point(start), spread));
    }
    if let Some(dir) = (end - start).try_normalize() {
        let length = start.distance(end);
        let off = |p: &Point2| {
            let along = (*p - start).dot(dir);
            let across = (*p - start).perp_dot(dir).abs();
            // Beyond the ends counts as well: the curve must not overshoot.
            across.max(-along).max(along - length)
        };
        let deviation = points.iter().map(off).fold(0.0, f64::max);
        if deviation <= tolerance {
            return Some((Recognized::Line(start, end), deviation));
        }
    }

    // A circle or an arc: sampled, since control points of a circle lie
    // off it.
    let spans = piece.knots().windows(2).filter(|k| k[1] > k[0]).count();
    let n = (spans * SAMPLES_PER_SPAN).max(SAMPLES_PER_SPAN);
    let (a, b) = piece.domain();
    let samples: Vec<Point2> = (0..=n)
        .map(|k| piece.point_at(a + (b - a) * k as f64 / n as f64))
        .collect();
    let closed = start.distance(end) <= tolerance;
    let fit = if closed {
        circumcenter(samples[0], samples[n / 3], samples[2 * n / 3])
    } else {
        circumcenter(samples[0], samples[n / 2], samples[n])
    };
    if let Some(center) = fit {
        let radius = samples.iter().map(|p| p.distance(center)).sum::<f64>() / (n + 1) as f64;
        let deviation = samples
            .iter()
            .map(|p| (p.distance(center) - radius).abs())
            .fold(0.0, f64::max);
        // The angle swept, sample to sample: it must run one way round.
        let mut sweep = 0.0;
        let mut one_way = true;
        for pair in samples.windows(2) {
            let (u, v) = (pair[0] - center, pair[1] - center);
            let step = libm::atan2(u.perp_dot(v), u.dot(v));
            one_way &= sweep * step >= 0.0;
            sweep += step;
        }
        if deviation <= tolerance && one_way && radius.is_finite() {
            if closed && (sweep.abs() - TAU).abs() < 1e-6 {
                return Some((Recognized::Circle { center, radius }, deviation));
            }
            if !closed && sweep.abs() < TAU {
                let (start, end) = if sweep > 0.0 {
                    (start, end)
                } else {
                    (end, start)
                };
                return Some((Recognized::Arc { center, start, end }, deviation));
            }
        }
    }
    Some((Recognized::Spline(piece), 0.0))
}

/// The center of the circle through three points; `None` when they are
/// (nearly) on a line.
pub fn circumcenter(a: Point2, b: Point2, c: Point2) -> Option<Point2> {
    let (ab, ac) = (b - a, c - a);
    let d = 2.0 * ab.perp_dot(ac);
    if d.abs() <= 1e-12 * ab.length() * ac.length() || d == 0.0 {
        return None;
    }
    let (b2, c2) = (ab.length_squared(), ac.length_squared());
    let center = a + Point2::new(ac.y * b2 - ab.y * c2, ab.x * c2 - ac.x * b2) / d;
    center.is_finite().then_some(center)
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: f64 = std::f64::consts::FRAC_1_SQRT_2;

    fn circle() -> Curve2 {
        let p = |x: f64, y: f64| Point2::new(2.0 + 3.0 * x, 1.0 + 3.0 * y);
        Curve2::new(
            2,
            vec![
                0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0, 1.0,
            ],
            vec![
                p(1.0, 0.0),
                p(1.0, 1.0),
                p(0.0, 1.0),
                p(-1.0, 1.0),
                p(-1.0, 0.0),
                p(-1.0, -1.0),
                p(0.0, -1.0),
                p(1.0, -1.0),
                p(1.0, 0.0),
            ],
            Some(vec![1.0, W, 1.0, W, 1.0, W, 1.0, W, 1.0]),
        )
        .unwrap()
    }

    #[test]
    fn circles_and_arcs_are_recognized() {
        let c = circle();
        let (whole, deviation) = recognize(&c, 0.0, 1.0, 1e-7).unwrap();
        let Recognized::Circle { center, radius } = whole else {
            panic!("expected a circle, got {whole:?}");
        };
        assert!(center.abs_diff_eq(Point2::new(2.0, 1.0), 1e-9) && (radius - 3.0).abs() < 1e-9);
        assert!(deviation < 1e-9);
        // A piece of it is an arc, counter-clockwise.
        let (arc, _) = recognize(&c, 0.1, 0.4, 1e-7).unwrap();
        let Recognized::Arc { center, start, end } = arc else {
            panic!("expected an arc, got {arc:?}");
        };
        assert!(center.abs_diff_eq(Point2::new(2.0, 1.0), 1e-9));
        assert!(start.abs_diff_eq(c.point_at(0.1), 1e-9) && end.abs_diff_eq(c.point_at(0.4), 1e-9));
        // Reversed, it is the same arc: still counter-clockwise.
        let (arc, _) = recognize(&c.reversed(), 0.6, 0.9, 1e-7).unwrap();
        let Recognized::Arc { start, .. } = arc else {
            panic!("expected an arc");
        };
        assert!(start.abs_diff_eq(c.point_at(0.1), 1e-9));
    }

    #[test]
    fn lines_are_lines_within_the_tolerance_only() {
        let bent = |sag: f64| {
            Curve2::new(
                2,
                vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                vec![Point2::ZERO, Point2::new(1.0, sag), Point2::new(2.0, 0.0)],
                None,
            )
            .unwrap()
        };
        let is_line = |sag: f64, tolerance: f64| {
            matches!(
                recognize(&bent(sag), 0.0, 1.0, tolerance),
                Some((Recognized::Line(..), _))
            )
        };
        assert!(is_line(0.0, 1e-7));
        assert!(is_line(5e-8, 1e-7));
        assert!(!is_line(5e-7, 1e-7));
        assert!(is_line(5e-7, 1e-6));
        // What is neither stays a spline, with its own control points.
        let (spline, deviation) = recognize(&bent(0.5), 0.0, 1.0, 1e-7).unwrap();
        assert!(matches!(spline, Recognized::Spline(_)) && deviation == 0.0);
        // A curve that never moves is a point.
        let still = Curve2::line(Point2::ONE, Point2::ONE + Point2::splat(1e-9)).unwrap();
        assert!(matches!(
            recognize(&still, 0.0, 1.0, 1e-7),
            Some((Recognized::Point(_), _))
        ));
    }
}
