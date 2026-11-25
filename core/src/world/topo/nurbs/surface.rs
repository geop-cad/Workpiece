//! Tensor-product NURBS surfaces.

use super::basis::{HELD, basis, basis_derivatives, basis_held, derivatives_held, find_span};
use super::error::NurbsError;
use super::validate::{check_basis, check_weights};
use crate::math::Point3;

/// A tensor-product NURBS surface in model space.
///
/// Control points are stored row-major with `u` as the slow index:
/// point `(i, j)` is at `i * count_v + j`.
#[derive(Debug, Clone, PartialEq)]
pub struct NurbsSurface {
    degree_u: usize,
    degree_v: usize,
    knots_u: Vec<f64>,
    knots_v: Vec<f64>,
    count_u: usize,
    count_v: usize,
    control_points: Vec<Point3>,
    weights: Vec<f64>,
}

impl NurbsSurface {
    /// A surface from its full definition; `control_points.len()` must be
    /// `count_u * count_v`. `weights: None` means non-rational.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        degree_u: usize,
        degree_v: usize,
        knots_u: Vec<f64>,
        knots_v: Vec<f64>,
        count_u: usize,
        count_v: usize,
        control_points: Vec<Point3>,
        weights: Option<Vec<f64>>,
    ) -> Result<Self, NurbsError> {
        check_basis(degree_u, count_u, &knots_u)?;
        check_basis(degree_v, count_v, &knots_v)?;
        let n = count_u * count_v;
        if control_points.len() != n {
            return Err(NurbsError::ControlGridMismatch);
        }
        if control_points.iter().any(|c| !c.is_finite()) {
            return Err(NurbsError::NonFinitePoint);
        }
        let weights = weights.unwrap_or_else(|| vec![1.0; n]);
        check_weights(&weights, n)?;
        Ok(Self {
            degree_u,
            degree_v,
            knots_u,
            knots_v,
            count_u,
            count_v,
            control_points,
            weights,
        })
    }

    /// The bilinear patch through four corners on `[0, 1]²`:
    /// `p00` at (0, 0), `p10` at (1, 0), `p01` at (0, 1), `p11` at (1, 1).
    pub fn bilinear(
        p00: Point3,
        p10: Point3,
        p01: Point3,
        p11: Point3,
    ) -> Result<Self, NurbsError> {
        let k = vec![0.0, 0.0, 1.0, 1.0];
        Self::new(1, 1, k.clone(), k, 2, 2, vec![p00, p01, p10, p11], None)
    }

    /// Degrees `(u, v)`.
    pub fn degrees(&self) -> (usize, usize) {
        (self.degree_u, self.degree_v)
    }

    /// Knot vectors `(u, v)`.
    pub fn knots(&self) -> (&[f64], &[f64]) {
        (&self.knots_u, &self.knots_v)
    }

    /// Control point counts `(u, v)`.
    pub fn counts(&self) -> (usize, usize) {
        (self.count_u, self.count_v)
    }

    /// Control points, row-major with `u` slow.
    pub fn control_points(&self) -> &[Point3] {
        &self.control_points
    }

    /// Control point `(i, j)`.
    pub fn control_point(&self, i: usize, j: usize) -> Point3 {
        self.control_points[i * self.count_v + j]
    }

    /// Weights, laid out like the control points.
    pub fn weights(&self) -> &[f64] {
        &self.weights
    }

    /// The parameter domain `((u0, u1), (v0, v1))`.
    pub fn domain(&self) -> ((f64, f64), (f64, f64)) {
        (
            (self.knots_u[self.degree_u], self.knots_u[self.count_u]),
            (self.knots_v[self.degree_v], self.knots_v[self.count_v]),
        )
    }

    /// The point at `(u, v)` (clamped to the domain).
    pub fn point_at(&self, u: f64, v: f64) -> Point3 {
        let ((u0, u1), (v0, v1)) = self.domain();
        let su = find_span(self.degree_u, &self.knots_u, self.count_u, u);
        let sv = find_span(self.degree_v, &self.knots_v, self.count_v, v);
        let (mut hu, mut hv) = ([0.0; HELD], [0.0; HELD]);
        let (ru, rv);
        let (bu, bv): (&[f64], &[f64]) = if self.degree_u < HELD && self.degree_v < HELD {
            basis_held(self.degree_u, &self.knots_u, su, u.clamp(u0, u1), &mut hu);
            basis_held(self.degree_v, &self.knots_v, sv, v.clamp(v0, v1), &mut hv);
            (&hu[..=self.degree_u], &hv[..=self.degree_v])
        } else {
            ru = basis(self.degree_u, &self.knots_u, su, u.clamp(u0, u1));
            rv = basis(self.degree_v, &self.knots_v, sv, v.clamp(v0, v1));
            (&ru, &rv)
        };
        let mut acc = Point3::ZERO;
        let mut w = 0.0;
        for (a, nu) in bu.iter().enumerate() {
            let i = su - self.degree_u + a;
            for (b, nv) in bv.iter().enumerate() {
                let j = sv - self.degree_v + b;
                let k = i * self.count_v + j;
                let wk = nu * nv * self.weights[k];
                acc += self.control_points[k] * wk;
                w += wk;
            }
        }
        acc / w
    }

    /// The surface along `u` between `from` and `to` at `v`: where its
    /// point and derivatives are asked for at many `u` of one `v`,
    /// what is the same for all of them is worked out once.
    pub fn along_u(&self, v: f64, from: f64, to: f64) -> AlongU<'_> {
        let (_, (v0, v1)) = self.domain();
        let mut out = AlongU {
            surface: self,
            at: v,
            first: 0,
            rows: Vec::new(),
        };
        if self.degree_u >= HELD || self.degree_v >= HELD {
            return out;
        }
        let sv = find_span(self.degree_v, &self.knots_v, self.count_v, v);
        let (mut bv, mut dv) = ([0.0; HELD], [0.0; HELD]);
        derivatives_held(
            self.degree_v,
            &self.knots_v,
            sv,
            v.clamp(v0, v1),
            &mut bv,
            &mut dv,
        );
        let low = find_span(self.degree_u, &self.knots_u, self.count_u, from.min(to));
        let high = find_span(self.degree_u, &self.knots_u, self.count_u, from.max(to));
        out.first = low - self.degree_u;
        out.rows = (out.first..=high)
            .map(|i| {
                let row = i * self.count_v + sv - self.degree_v;
                let (mut c, mut cw, mut cv, mut cvw) = (Point3::ZERO, 0.0, Point3::ZERO, 0.0);
                for j in 0..=self.degree_v {
                    let (wk, p) = (self.weights[row + j], self.control_points[row + j]);
                    c += p * (bv[j] * wk);
                    cw += bv[j] * wk;
                    cv += p * (dv[j] * wk);
                    cvw += dv[j] * wk;
                }
                (c, cw, cv, cvw)
            })
            .collect();
        out
    }

    /// The point at `(u, v)` (clamped to the domain) and the partial
    /// derivatives `∂u` and `∂v` there.
    pub fn derivatives_at(&self, u: f64, v: f64) -> (Point3, Point3, Point3) {
        let ((u0, u1), (v0, v1)) = self.domain();
        let su = find_span(self.degree_u, &self.knots_u, self.count_u, u);
        let sv = find_span(self.degree_v, &self.knots_v, self.count_v, v);
        let (mut hu, mut hdu, mut hv, mut hdv) =
            ([0.0; HELD], [0.0; HELD], [0.0; HELD], [0.0; HELD]);
        let (ru, rv);
        let (bu, du, bv, dv): (&[f64], &[f64], &[f64], &[f64]) =
            if self.degree_u < HELD && self.degree_v < HELD {
                let (u, v) = (u.clamp(u0, u1), v.clamp(v0, v1));
                derivatives_held(self.degree_u, &self.knots_u, su, u, &mut hu, &mut hdu);
                derivatives_held(self.degree_v, &self.knots_v, sv, v, &mut hv, &mut hdv);
                (&hu, &hdu, &hv, &hdv)
            } else {
                ru = basis_derivatives(self.degree_u, &self.knots_u, su, u.clamp(u0, u1));
                rv = basis_derivatives(self.degree_v, &self.knots_v, sv, v.clamp(v0, v1));
                (&ru.0, &ru.1, &rv.0, &rv.1)
            };
        let (mut a, mut au, mut av) = (Point3::ZERO, Point3::ZERO, Point3::ZERO);
        let (mut w, mut wu, mut wv) = (0.0, 0.0, 0.0);
        let across = self.degree_v + 1;
        for i in 0..=self.degree_u {
            let row = (su - self.degree_u + i) * self.count_v + sv - self.degree_v;
            let weights = &self.weights[row..row + across];
            let points = &self.control_points[row..row + across];
            let (bui, dui) = (bu[i], du[i]);
            for j in 0..across {
                let (wk, c) = (weights[j], points[j]);
                let (s, su, sv) = (bui * bv[j] * wk, dui * bv[j] * wk, bui * dv[j] * wk);
                a += c * s;
                au += c * su;
                av += c * sv;
                w += s;
                wu += su;
                wv += sv;
            }
        }
        let point = a / w;
        (point, (au - point * wu) / w, (av - point * wv) / w)
    }
}

/// A surface along `u` at one `v`: what [`NurbsSurface::along_u`]
/// gives.
#[derive(Debug, Clone)]
pub struct AlongU<'a> {
    surface: &'a NurbsSurface,
    /// The `v` it is at.
    at: f64,
    /// The first row of control points held.
    first: usize,
    /// For each row held: its points summed along v by the basis there
    /// (with their weights), the weights summed so, and the same by
    /// the basis's derivative.
    rows: Vec<(Point3, f64, Point3, f64)>,
}

impl AlongU<'_> {
    /// What [`NurbsSurface::derivatives_at`] gives at `(u, v)`, for a
    /// `u` between the two the curve was asked for.
    pub fn derivatives_at(&self, u: f64) -> (Point3, Point3, Point3) {
        let s = self.surface;
        let ((u0, u1), _) = s.domain();
        let su = find_span(s.degree_u, &s.knots_u, s.count_u, u);
        let low = su - s.degree_u;
        if s.degree_u >= HELD || low < self.first || su >= self.first + self.rows.len() {
            return s.derivatives_at(u, self.v());
        }
        let (mut bu, mut du) = ([0.0; HELD], [0.0; HELD]);
        derivatives_held(
            s.degree_u,
            &s.knots_u,
            su,
            u.clamp(u0, u1),
            &mut bu,
            &mut du,
        );
        let (mut a, mut au, mut av) = (Point3::ZERO, Point3::ZERO, Point3::ZERO);
        let (mut w, mut wu, mut wv) = (0.0, 0.0, 0.0);
        let rows = &self.rows[low - self.first..=su - self.first];
        for (i, (c, cw, cv, cvw)) in rows.iter().enumerate() {
            a += *c * bu[i];
            au += *c * du[i];
            av += *cv * bu[i];
            w += cw * bu[i];
            wu += cw * du[i];
            wv += cvw * bu[i];
        }
        let point = a / w;
        (point, (au - point * wu) / w, (av - point * wv) / w)
    }

    fn v(&self) -> f64 {
        self.at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derivatives_match_differences() -> anyhow::Result<()> {
        let h = core::f64::consts::FRAC_1_SQRT_2;
        // A quarter cylinder: a rational arc in u, a line in v.
        let s = NurbsSurface::new(
            2,
            1,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![0.0, 0.0, 2.0, 2.0],
            3,
            2,
            vec![
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 3.0),
                Point3::new(1.0, 1.0, 0.0),
                Point3::new(1.0, 1.0, 3.0),
                Point3::new(0.0, 1.0, 0.0),
                Point3::new(0.0, 1.0, 3.0),
            ],
            Some(vec![1.0, 1.0, h, h, 1.0, 1.0]),
        )?;
        let e = 1e-6;
        for (u, v) in [(0.2, 0.3), (0.7, 1.9), (0.0, 0.0), (1.0, 2.0)] {
            let (p, du, dv) = s.derivatives_at(u, v);
            assert!(p.abs_diff_eq(s.point_at(u, v), 1e-14));
            let (ua, ub) = ((u - e).max(0.0), (u + e).min(1.0));
            let (va, vb) = ((v - e).max(0.0), (v + e).min(2.0));
            let fu = (s.point_at(ub, v) - s.point_at(ua, v)) / (ub - ua);
            let fv = (s.point_at(u, vb) - s.point_at(u, va)) / (vb - va);
            assert!(du.abs_diff_eq(fu, 1e-5), "{du} {fu}");
            assert!(dv.abs_diff_eq(fv, 1e-5), "{dv} {fv}");
        }
        Ok(())
    }

    #[test]
    fn a_surface_along_u_is_the_surface() -> anyhow::Result<()> {
        let h = core::f64::consts::FRAC_1_SQRT_2;
        let s = NurbsSurface::new(
            2,
            1,
            vec![0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0],
            vec![0.0, 0.0, 2.0, 2.0],
            4,
            2,
            vec![
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 3.0),
                Point3::new(1.0, 1.0, 0.0),
                Point3::new(1.2, 1.0, 3.0),
                Point3::new(0.0, 1.0, 0.0),
                Point3::new(0.0, 1.5, 3.0),
                Point3::new(-1.0, 1.0, 0.5),
                Point3::new(-1.0, 1.0, 3.0),
            ],
            Some(vec![1.0, 1.0, h, h, 1.0, 1.0, 0.8, 0.9]),
        )?;
        for v in [0.0, 0.7, 2.0] {
            let along = s.along_u(v, 0.2, 0.9);
            for u in [0.2, 0.45, 0.5, 0.75, 0.9, 0.05] {
                let (p, du, dv) = along.derivatives_at(u);
                let (q, eu, ev) = s.derivatives_at(u, v);
                assert!(p.abs_diff_eq(q, 1e-14) && du.abs_diff_eq(eu, 1e-13));
                assert!(dv.abs_diff_eq(ev, 1e-13));
            }
        }
        Ok(())
    }

    #[test]
    fn bilinear_corners() -> anyhow::Result<()> {
        let s = NurbsSurface::bilinear(
            Point3::ZERO,
            Point3::X,
            Point3::Y,
            Point3::new(1.0, 1.0, 1.0),
        )?;
        assert!(s.point_at(1.0, 0.0).abs_diff_eq(Point3::X, 1e-12));
        assert!(s.point_at(0.0, 1.0).abs_diff_eq(Point3::Y, 1e-12));
        assert!(
            s.point_at(0.5, 0.5)
                .abs_diff_eq(Point3::new(0.5, 0.5, 0.25), 1e-12)
        );
        Ok(())
    }
}
