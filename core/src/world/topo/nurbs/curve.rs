//! NURBS curves in model space (edge curves) and parameter space (pcurves).

use super::basis::{HELD, basis, basis_derivatives, basis_held, derivatives_held, find_span};
use super::error::NurbsError;
use super::point::ControlPoint;
use super::validate::{check_basis, check_weights};
use crate::math::{Point2, Point3};

/// A NURBS curve over control points `P`: [`Point3`] for model-space edge
/// curves, [`Point2`] for pcurves in a surface's (u, v) space.
#[derive(Debug, Clone, PartialEq)]
pub struct NurbsCurve<P> {
    degree: usize,
    knots: Vec<f64>,
    control_points: Vec<P>,
    weights: Vec<f64>,
}

/// A model-space NURBS curve (the geometry of an edge).
pub type Curve3 = NurbsCurve<Point3>;
/// A parameter-space NURBS curve (a coedge's pcurve on its face's surface).
pub type Curve2 = NurbsCurve<Point2>;

/// Steps per knot span of a sampled curve: enough for a quarter circle
/// to look round.
pub const SEGMENTS_PER_SPAN: usize = 12;

impl<P: ControlPoint> NurbsCurve<P> {
    /// A curve from its full definition. `weights: None` means non-rational
    /// (all weights 1).
    pub fn new(
        degree: usize,
        knots: Vec<f64>,
        control_points: Vec<P>,
        weights: Option<Vec<f64>>,
    ) -> Result<Self, NurbsError> {
        let n = control_points.len();
        check_basis(degree, n, &knots)?;
        if control_points.iter().any(|c| !c.is_finite()) {
            return Err(NurbsError::NonFinitePoint);
        }
        let weights = weights.unwrap_or_else(|| vec![1.0; n]);
        check_weights(&weights, n)?;
        Ok(Self {
            degree,
            knots,
            control_points,
            weights,
        })
    }

    /// The degree-1 segment from `a` to `b` on `[0, 1]`.
    pub fn line(a: P, b: P) -> Result<Self, NurbsError> {
        Self::new(1, vec![0.0, 0.0, 1.0, 1.0], vec![a, b], None)
    }

    /// Polynomial degree.
    pub fn degree(&self) -> usize {
        self.degree
    }

    /// Knot vector.
    pub fn knots(&self) -> &[f64] {
        &self.knots
    }

    /// Control points.
    pub fn control_points(&self) -> &[P] {
        &self.control_points
    }

    /// Weights, one per control point.
    pub fn weights(&self) -> &[f64] {
        &self.weights
    }

    /// `true` when any weight differs from the first one.
    pub fn is_rational(&self) -> bool {
        self.weights.iter().any(|w| *w != self.weights[0])
    }

    /// The parameter domain `[knots[p], knots[n]]`.
    pub fn domain(&self) -> (f64, f64) {
        (
            self.knots[self.degree],
            self.knots[self.control_points.len()],
        )
    }

    /// Points along the curve from `t0` to `t1`, close enough together
    /// to stand in for it when it is drawn, picked or laid out flat:
    /// every knot between the two, and between knots as many steps as
    /// the curve turns there — one where it runs straight, up to
    /// [`SEGMENTS_PER_SPAN`] (a quarter circle's worth) where it bends.
    pub fn polyline(&self, t0: f64, t1: f64) -> Vec<P> {
        let (lo, hi) = (t0.min(t1), t0.max(t1));
        let mut stops = vec![lo];
        for &k in &self.knots {
            if k > lo && k < hi && stops.last().is_none_or(|&l| k > l) {
                stops.push(k);
            }
        }
        stops.push(hi);
        let straight = self.degree <= 1 && !self.is_rational();
        // The angle between two directions.
        let turn = |a: P, b: P| {
            let (aa, bb, ab) = (a.dot(a), b.dot(b), a.dot(b));
            if aa <= 0.0 || bb <= 0.0 {
                0.0
            } else {
                libm::acos((ab / (aa * bb).sqrt()).clamp(-1.0, 1.0))
            }
        };
        // (Half as far apart as a surface's own steps, and never fewer
        // than a handful to a bent span: a curve drawn or pointed at
        // shows its corners sooner than a face does.)
        let finest = core::f64::consts::FRAC_PI_4 / SEGMENTS_PER_SPAN as f64;
        let mut points = vec![self.point_at(lo)];
        for pair in stops.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let steps = if straight {
                1
            } else {
                // Inside the span, where the directions are its own.
                let at = |f: f64| self.derivative_at(a + (b - a) * f).1;
                let (d0, d1, d2) = (at(1e-3), at(0.5), at(1.0 - 1e-3));
                let turned = turn(d0, d1) + turn(d1, d2);
                ((turned / finest).ceil() as usize).clamp(4, 4 * SEGMENTS_PER_SPAN)
            };
            for k in 1..=steps {
                points.push(self.point_at(a + (b - a) * k as f64 / steps as f64));
            }
        }
        if t0 > t1 {
            points.reverse();
        }
        points
    }

    /// The point at parameter `t` (clamped to the domain).
    pub fn point_at(&self, t: f64) -> P {
        let n = self.control_points.len();
        let span = find_span(self.degree, &self.knots, n, t);
        let (lo, hi) = self.domain();
        let t = t.clamp(lo, hi);
        let mut held = [0.0; HELD];
        let row;
        let b: &[f64] = if self.degree < HELD {
            basis_held(self.degree, &self.knots, span, t, &mut held);
            &held[..=self.degree]
        } else {
            row = basis(self.degree, &self.knots, span, t);
            &row
        };
        let mut acc = P::ZERO;
        let mut w = 0.0;
        for (k, bk) in b.iter().enumerate() {
            let i = span - self.degree + k;
            let wi = bk * self.weights[i];
            acc = acc + self.control_points[i] * wi;
            w += wi;
        }
        acc / w
    }

    /// The point at parameter `t` (clamped to the domain) and the
    /// derivative there.
    pub fn derivative_at(&self, t: f64) -> (P, P) {
        let n = self.control_points.len();
        let span = find_span(self.degree, &self.knots, n, t);
        let (lo, hi) = self.domain();
        let t = t.clamp(lo, hi);
        let (mut hb, mut hd) = ([0.0; HELD], [0.0; HELD]);
        let rows;
        let (b, d): (&[f64], &[f64]) = if self.degree < HELD {
            derivatives_held(self.degree, &self.knots, span, t, &mut hb, &mut hd);
            (&hb, &hd)
        } else {
            rows = basis_derivatives(self.degree, &self.knots, span, t);
            (&rows.0, &rows.1)
        };
        let (mut a, mut da) = (P::ZERO, P::ZERO);
        let (mut w, mut dw) = (0.0, 0.0);
        for k in 0..=self.degree {
            let i = span - self.degree + k;
            let (wi, c) = (self.weights[i], self.control_points[i]);
            a = a + c * (b[k] * wi);
            da = da + c * (d[k] * wi);
            w += b[k] * wi;
            dw += d[k] * wi;
        }
        let point = a / w;
        (point, (da + point * -dw) / w)
    }

    /// How often `u` occurs in the knot vector.
    fn multiplicity(&self, u: f64) -> usize {
        self.knots.iter().filter(|&&k| k == u).count()
    }

    /// The same curve with the knot `u` (inside the domain) inserted once
    /// (Boehm's algorithm, in homogeneous coordinates).
    fn with_knot(&self, u: f64) -> Self {
        let (p, n) = (self.degree, self.control_points.len());
        let k = find_span(p, &self.knots, n, u);
        let at = |i: usize| (self.control_points[i] * self.weights[i], self.weights[i]);
        let mut points = Vec::with_capacity(n + 1);
        let mut weights = Vec::with_capacity(n + 1);
        for i in 0..=n {
            let (q, w) = if i + p <= k {
                at(i)
            } else if i > k {
                at(i - 1)
            } else {
                let a = (u - self.knots[i]) / (self.knots[i + p] - self.knots[i]);
                let ((p1, w1), (p0, w0)) = (at(i), at(i - 1));
                (p1 * a + p0 * (1.0 - a), w1 * a + w0 * (1.0 - a))
            };
            points.push(q / w);
            weights.push(w);
        }
        let mut knots = self.knots.clone();
        knots.insert(k + 1, u);
        Self {
            degree: p,
            knots,
            control_points: points,
            weights,
        }
    }

    /// The piece of the curve over `[t0, t1]` (clamped to the domain), as
    /// a curve of its own on that range: the same points at the same
    /// parameters, with control points of the piece alone.
    pub fn trimmed(&self, t0: f64, t1: f64) -> Result<Self, NurbsError> {
        let (lo, hi) = self.domain();
        let (t0, t1) = (t0.clamp(lo, hi), t1.clamp(lo, hi));
        if t0 >= t1 {
            return Err(NurbsError::EmptyDomain);
        }
        let p = self.degree;
        let mut c = self.clone();
        for t in [t0, t1] {
            while c.multiplicity(t) < p {
                c = c.with_knot(t);
            }
        }
        // The last knot at t0 and the first at t1 bound the piece.
        let ks = c.knots.iter().rposition(|&k| k <= t0).unwrap_or(p).max(p);
        let n = c.control_points.len();
        let ke = c.knots.iter().position(|&k| k >= t1).unwrap_or(n).min(n);
        let mut knots = vec![t0; p + 1];
        knots.extend_from_slice(&c.knots[ks + 1..ke]);
        knots.extend(std::iter::repeat_n(t1, p + 1));
        Self::new(
            p,
            knots,
            c.control_points[ks - p..ke].to_vec(),
            Some(c.weights[ks - p..ke].to_vec()),
        )
    }

    /// The same curve at the same parameters, of the higher degree
    /// `degree` (itself, if it is of that degree or a higher one).
    pub fn elevated(&self, degree: usize) -> Result<Self, NurbsError> {
        let p = self.degree;
        if degree <= p {
            return Ok(self.clone());
        }
        let (lo, hi) = self.domain();
        let mut breaks: Vec<f64> = self
            .knots
            .iter()
            .copied()
            .filter(|k| *k >= lo && *k <= hi)
            .collect();
        breaks.dedup();
        let mut whole: Option<Self> = None;
        for pair in breaks.windows(2) {
            // One span: a Bézier piece, in weighted coordinates.
            let piece = self.trimmed(pair[0], pair[1])?;
            let mut points: Vec<(P, f64)> = piece
                .control_points
                .iter()
                .zip(&piece.weights)
                .map(|(&point, &w)| (point * w, w))
                .collect();
            for q in p..degree {
                let n = (q + 1) as f64;
                let mut raised = vec![points[0]];
                for i in 1..=q {
                    let f = i as f64 / n;
                    let (a, b) = (points[i - 1], points[i]);
                    raised.push((a.0 * f + b.0 * (1.0 - f), a.1 * f + b.1 * (1.0 - f)));
                }
                raised.push(points[q]);
                points = raised;
            }
            let mut knots = vec![pair[0]; degree + 1];
            knots.extend(std::iter::repeat_n(pair[1], degree + 1));
            let raised = Self::new(
                degree,
                knots,
                points.iter().map(|(point, w)| *point / *w).collect(),
                Some(points.iter().map(|(_, w)| *w).collect()),
            )?;
            whole = Some(match whole {
                None => raised,
                Some(sofar) => sofar.joined(&raised, 1.0)?,
            });
        }
        whole.ok_or(NurbsError::EmptyDomain)
    }

    /// This curve and then `next`, which starts where this one ends, as
    /// one curve of their degree (they have the same): this one at its
    /// own parameters, and `next` after it, its parameters stretched
    /// `stretch` times.
    pub fn joined(&self, next: &Self, stretch: f64) -> Result<Self, NurbsError> {
        let p = self.degree;
        if next.degree != p || stretch <= 0.0 || !stretch.is_finite() {
            return Err(NurbsError::EmptyDomain);
        }
        // Both ending and starting in a control point of their own.
        let (a0, a1) = self.domain();
        let (b0, b1) = next.domain();
        let (a, b) = (self.trimmed(a0, a1)?, next.trimmed(b0, b1)?);
        let n = a.control_points.len();
        let mut knots = a.knots[..n + p].to_vec();
        knots.extend(b.knots[p + 1..].iter().map(|k| a1 + (k - b0) * stretch));
        let mut points = a.control_points.clone();
        points.extend_from_slice(&b.control_points[1..]);
        // (As heavy where they meet.)
        let scale = a.weights[n - 1] / b.weights[0];
        let mut weights = a.weights.clone();
        weights.extend(b.weights[1..].iter().map(|w| w * scale));
        Self::new(p, knots, points, Some(weights))
    }

    /// The same curve traversed the other way, on the mirrored domain.
    pub fn reversed(&self) -> Self {
        let (lo, hi) = (self.knots[0], self.knots[self.knots.len() - 1]);
        Self {
            degree: self.degree,
            knots: self.knots.iter().rev().map(|k| lo + hi - k).collect(),
            control_points: self.control_points.iter().rev().copied().collect(),
            weights: self.weights.iter().rev().copied().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curves_end_to_end_are_joined_into_one() -> anyhow::Result<()> {
        let h = core::f64::consts::FRAC_1_SQRT_2;
        let quarter = |from: Point3, corner: Point3, to: Point3| {
            Curve3::new(
                2,
                vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                vec![from, corner, to],
                Some(vec![1.0, h, 1.0]),
            )
        };
        let a = quarter(Point3::X, Point3::new(1.0, 1.0, 0.0), Point3::Y)?;
        let b = quarter(Point3::Y, Point3::new(-1.0, 1.0, 0.0), -Point3::X)?;
        let both = a.joined(&b, 2.0)?;
        assert_eq!(both.domain(), (0.0, 3.0));
        for i in 0..=10 {
            let t = f64::from(i) / 10.0;
            assert!(both.point_at(t).distance(a.point_at(t)) < 1e-12);
            assert!(both.point_at(1.0 + 2.0 * t).distance(b.point_at(t)) < 1e-12);
        }
        assert!(a.joined(&Curve3::line(Point3::Y, Point3::Z)?, 1.0).is_err());
        // A curve of a higher degree is the same curve.
        let higher = both.elevated(4)?;
        assert_eq!((higher.degree(), higher.domain()), (4, (0.0, 3.0)));
        for i in 0..=30 {
            let t = f64::from(i) / 10.0;
            assert!(higher.point_at(t).distance(both.point_at(t)) < 1e-12);
        }
        Ok(())
    }

    #[test]
    fn rational_quarter_circle_is_on_the_circle() -> anyhow::Result<()> {
        let h = core::f64::consts::FRAC_1_SQRT_2;
        let c = Curve3::new(
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![Point3::X, Point3::new(1.0, 1.0, 0.0), Point3::Y],
            Some(vec![1.0, h, 1.0]),
        )?;
        assert!(c.is_rational());
        for i in 0..=10 {
            let p = c.point_at(i as f64 / 10.0);
            assert!((p.length() - 1.0).abs() < 1e-12);
        }
        assert!(c.point_at(1.0).abs_diff_eq(Point3::Y, 1e-12));
        Ok(())
    }

    #[test]
    fn reversed_runs_backwards() -> anyhow::Result<()> {
        let c = Curve2::new(
            2,
            vec![0.0, 0.0, 0.0, 2.0, 2.0, 2.0],
            vec![Point2::ZERO, Point2::new(1.0, 2.0), Point2::new(3.0, 0.0)],
            None,
        )?;
        let r = c.reversed();
        for i in 0..=8 {
            let t = i as f64 / 4.0;
            assert!(c.point_at(t).abs_diff_eq(r.point_at(2.0 - t), 1e-12));
        }
        Ok(())
    }

    #[test]
    fn trimmed_pieces_keep_the_curve() -> anyhow::Result<()> {
        let h = core::f64::consts::FRAC_1_SQRT_2;
        // A half circle of two rational quadratic spans.
        let c = Curve2::new(
            2,
            vec![0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0],
            vec![
                Point2::new(1.0, 0.0),
                Point2::new(1.0, 1.0),
                Point2::new(0.0, 1.0),
                Point2::new(-1.0, 1.0),
                Point2::new(-1.0, 0.0),
            ],
            Some(vec![1.0, h, 1.0, h, 1.0]),
        )?;
        for (a, b) in [(0.2, 0.9), (0.0, 0.5), (0.5, 0.7), (0.0, 1.0)] {
            let piece = c.trimmed(a, b)?;
            assert_eq!(piece.domain(), (a, b));
            for i in 0..=10 {
                let t = a + (b - a) * f64::from(i) / 10.0;
                assert!(
                    piece.point_at(t).abs_diff_eq(c.point_at(t), 1e-12),
                    "{a} {b} {t}"
                );
            }
            // The piece's control points end on the curve.
            assert!(piece.control_points()[0].abs_diff_eq(c.point_at(a), 1e-12));
        }
        assert!(c.trimmed(0.5, 0.5).is_err());
        Ok(())
    }

    #[test]
    fn rejects_bad_input() {
        let (a, b) = (Point3::ZERO, Point3::ONE);
        assert_eq!(
            Curve3::new(0, vec![0.0, 1.0], vec![a], None),
            Err(NurbsError::ZeroDegree)
        );
        assert_eq!(
            Curve3::new(1, vec![0.0, 0.0, 1.0], vec![a, b], None),
            Err(NurbsError::KnotCountMismatch)
        );
        assert_eq!(
            Curve3::new(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![a, b],
                Some(vec![1.0, 0.0])
            ),
            Err(NurbsError::BadWeight)
        );
        assert_eq!(
            Curve3::new(1, vec![0.0, 0.0, 1.0, 1.0], vec![a, Point3::NAN], None),
            Err(NurbsError::NonFinitePoint)
        );
    }
}
