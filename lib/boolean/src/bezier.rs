//! Bézier pieces of NURBS curves and surfaces, and what clipping needs of
//! them: cutting a piece down to part of itself, how far it reaches along
//! a direction, and where along one of its parameters it can be inside a
//! slab (the clip of Bézier clipping, read off the convex hull of the
//! distances of its control points).
//!
//! Pieces are kept in homogeneous form, so rational curves and surfaces
//! go through the same arithmetic as polynomial ones and nothing is
//! divided before it has to be.

use shapecraft_core::math::{Point2, Point3};
use shapecraft_core::world::topo::nurbs::{ControlPoint, NurbsCurve, NurbsSurface};

/// A homogeneous point `(w·x, w·y, w·z, w)`.
pub(crate) type H = [f64; 4];

/// The point of `h` in space.
pub(crate) fn point(h: &H) -> Point3 {
    Point3::new(h[0], h[1], h[2]) / h[3]
}

fn lerp(a: &H, b: &H, s: f64) -> H {
    std::array::from_fn(|k| a[k] + (b[k] - a[k]) * s)
}

/// A control point with coordinates in space (a pcurve's lie in the
/// plane `z = 0`).
pub(crate) trait Coordinates: ControlPoint {
    /// The point in space.
    fn space(self) -> Point3;
}

impl Coordinates for Point3 {
    fn space(self) -> Point3 {
        self
    }
}

impl Coordinates for Point2 {
    fn space(self) -> Point3 {
        self.extend(0.0)
    }
}

/// A box along the axes.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Bounds {
    /// The least corner.
    pub lo: Point3,
    /// The greatest corner.
    pub hi: Point3,
}

impl Bounds {
    /// The box around nothing.
    pub(crate) const EMPTY: Bounds = Bounds {
        lo: Point3::INFINITY,
        hi: Point3::NEG_INFINITY,
    };

    /// The box around `points`.
    pub(crate) fn of(points: impl IntoIterator<Item = Point3>) -> Self {
        let mut b = Self::EMPTY;
        for p in points {
            b.add(p);
        }
        b
    }

    /// Grows it to hold `p`.
    pub(crate) fn add(&mut self, p: Point3) {
        self.lo = self.lo.min(p);
        self.hi = self.hi.max(p);
    }

    /// The box around both.
    pub(crate) fn union(self, other: Bounds) -> Bounds {
        Bounds {
            lo: self.lo.min(other.lo),
            hi: self.hi.max(other.hi),
        }
    }

    /// Whether the two come within `pad` of each other.
    pub(crate) fn meets(&self, other: &Bounds, pad: f64) -> bool {
        (self.lo - other.hi).max_element() <= pad && (other.lo - self.hi).max_element() <= pad
    }

    /// Whether `p` is within `pad` of it.
    pub(crate) fn holds(&self, p: Point3, pad: f64) -> bool {
        (self.lo - p).max_element() <= pad && (p - self.hi).max_element() <= pad
    }

    /// Whether the stretch from `from` to `to` comes within `pad` of
    /// it: `false` only where it surely does not.
    pub(crate) fn meets_stretch(&self, from: Point3, to: Point3, pad: f64) -> bool {
        let (mut t0, mut t1) = (0.0_f64, 1.0_f64);
        let d = to - from;
        for k in 0..3 {
            let (lo, hi) = (self.lo[k] - pad, self.hi[k] + pad);
            if d[k] == 0.0 {
                if from[k] < lo || from[k] > hi {
                    return false;
                }
                continue;
            }
            let (a, b) = ((lo - from[k]) / d[k], (hi - from[k]) / d[k]);
            t0 = t0.max(a.min(b));
            t1 = t1.min(a.max(b));
        }
        // (A little slack along it: rounding must not lose a meeting.)
        t0 <= t1 + 1e-9
    }

    /// Its diagonal's length; zero for an empty box.
    pub(crate) fn size(&self) -> f64 {
        if self.lo.x > self.hi.x {
            0.0
        } else {
            (self.hi - self.lo).length()
        }
    }
}

/// How many numbers along a parameter are held in place, without
/// asking for memory: enough for a degree of fifteen.
const HELD: usize = 16;

/// A Bézier piece: a point, a curve or a surface patch over a box of the
/// parameters of what it was cut from.
#[derive(Debug, Clone)]
pub(crate) struct Piece {
    /// How many control points along each parameter (1 where there is
    /// none).
    pub n: [usize; 2],
    /// The control points, the first parameter slow.
    pub cp: Vec<H>,
    /// The parameters it covers.
    pub range: [(f64, f64); 2],
}

/// The Bézier points over `[lo, hi]` of the B-spline of `degree` on
/// `knots` whose control points around `span` are `points`: the blossom
/// at `lo` and `hi` in every mix.
fn extract(degree: usize, knots: &[f64], span: usize, points: &[H], lo: f64, hi: f64) -> Vec<H> {
    let mut out = Vec::with_capacity(degree + 1);
    // (Room to work in, held in place for the degrees there are in
    // practice: this is done for every span of every curve.)
    let mut held = [[0.0; 4]; 16];
    let mut more = Vec::new();
    let d: &mut [H] = if degree < held.len() {
        &mut held[..=degree]
    } else {
        more.resize(degree + 1, [0.0; 4]);
        &mut more
    };
    for k in 0..=degree {
        d.copy_from_slice(points);
        for r in 1..=degree {
            // The blossom's arguments: `lo` first, then `hi` k times.
            let x = if r > degree - k { hi } else { lo };
            for j in (r..=degree).rev() {
                let i = span - degree + j;
                let a = (x - knots[i]) / (knots[i + degree - r + 1] - knots[i]);
                d[j] = lerp(&d[j - 1], &d[j], a);
            }
        }
        out.push(d[degree]);
    }
    out
}

/// The knot spans of a spline of `degree` with `count` control points
/// that reach into `[lo, hi]`: the span's index and the part of it in
/// the range.
fn spans(degree: usize, knots: &[f64], count: usize, lo: f64, hi: f64) -> Vec<(usize, f64, f64)> {
    let tiny = (hi - lo).abs() * 1e-12;
    (degree..count)
        .filter(|&i| knots[i + 1] > knots[i])
        .filter_map(|i| {
            let (a, b) = (knots[i].max(lo), knots[i + 1].min(hi));
            (b - a > tiny).then_some((i, a, b))
        })
        .collect()
}

/// The Bézier pieces of `curve` between `t0` and `t1`.
pub(crate) fn curve_pieces<P: Coordinates>(curve: &NurbsCurve<P>, t0: f64, t1: f64) -> Vec<Piece> {
    let (p, knots) = (curve.degree(), curve.knots());
    let h: Vec<H> = curve
        .control_points()
        .iter()
        .zip(curve.weights())
        .map(|(c, &w)| {
            let c = c.space();
            [c.x * w, c.y * w, c.z * w, w]
        })
        .collect();
    spans(p, knots, h.len(), t0, t1)
        .into_iter()
        .map(|(i, a, b)| Piece {
            n: [p + 1, 1],
            cp: extract(p, knots, i, &h[i - p..=i], a, b),
            range: [(a, b), (0.0, 0.0)],
        })
        .collect()
}

/// The Bézier patches of `surface` over the box `u` × `v`.
pub(crate) fn surface_pieces(surface: &NurbsSurface, u: (f64, f64), v: (f64, f64)) -> Vec<Piece> {
    let ((pu, pv), (ku, kv)) = (surface.degrees(), surface.knots());
    let (nu, nv) = surface.counts();
    let at = |i: usize, j: usize| {
        let (c, w) = (surface.control_point(i, j), surface.weights()[i * nv + j]);
        [c.x * w, c.y * w, c.z * w, w]
    };
    let mut out = Vec::new();
    for (i, ua, ub) in spans(pu, ku, nu, u.0, u.1) {
        for (j, va, vb) in spans(pv, kv, nv, v.0, v.1) {
            // Along u for each column of the span, then along v.
            let columns: Vec<Vec<H>> = (j - pv..=j)
                .map(|col| {
                    let line: Vec<H> = (i - pu..=i).map(|row| at(row, col)).collect();
                    extract(pu, ku, i, &line, ua, ub)
                })
                .collect();
            let mut cp = Vec::with_capacity((pu + 1) * (pv + 1));
            for a in 0..=pu {
                let line: Vec<H> = columns.iter().map(|c| c[a]).collect();
                cp.extend(extract(pv, kv, j, &line, va, vb));
            }
            out.push(Piece {
                n: [pu + 1, pv + 1],
                cp,
                range: [(ua, ub), (va, vb)],
            });
        }
    }
    out
}

/// Where the upper hull of the points `(i / (m − 1), y[i])` is at or
/// above zero.
fn nonnegative(y: &[f64]) -> Option<(f64, f64)> {
    let m = y.len();
    let x = |i: usize| {
        if m == 1 {
            0.0
        } else {
            i as f64 / (m - 1) as f64
        }
    };
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for i in 0..m {
        if y[i] >= 0.0 {
            lo = lo.min(x(i));
            hi = hi.max(x(i));
        }
        for j in i + 1..m {
            if (y[i] < 0.0) != (y[j] < 0.0) {
                let at = x(i) + (x(j) - x(i)) * (-y[i] / (y[j] - y[i]));
                lo = lo.min(at);
                hi = hi.max(at);
            }
        }
    }
    (lo <= hi).then_some((lo, if m == 1 { 1.0 } else { hi }))
}

impl Piece {
    /// A point.
    pub(crate) fn point(p: Point3) -> Self {
        Self {
            n: [1, 1],
            cp: vec![[p.x, p.y, p.z, 1.0]],
            range: [(0.0, 0.0); 2],
        }
    }

    /// The straight piece from `a` to `b`, on `[0, 1]`.
    pub(crate) fn segment(a: Point3, b: Point3) -> Self {
        Self {
            n: [2, 1],
            cp: vec![[a.x, a.y, a.z, 1.0], [b.x, b.y, b.z, 1.0]],
            range: [(0.0, 1.0), (0.0, 0.0)],
        }
    }

    /// How many parameters it has.
    pub(crate) fn dims(&self) -> usize {
        usize::from(self.n[0] > 1) + usize::from(self.n[1] > 1)
    }

    /// Control point `(i, j)` in space.
    pub(crate) fn at(&self, i: usize, j: usize) -> Point3 {
        point(&self.cp[i * self.n[1] + j])
    }

    /// The box around it.
    pub(crate) fn bounds(&self) -> Bounds {
        Bounds::of(self.cp.iter().map(point))
    }

    /// The middle of the parameters it covers.
    pub(crate) fn middle(&self) -> [f64; 2] {
        self.range.map(|(a, b)| (a + b) / 2.0)
    }

    /// From its start to its end along parameter `axis`, averaged over
    /// the other.
    pub(crate) fn extent(&self, axis: usize) -> Point3 {
        let last = self.n[axis] - 1;
        let other = self.n[1 - axis];
        let mut sum = Point3::ZERO;
        for k in 0..other {
            let (a, b) = if axis == 0 {
                (self.at(0, k), self.at(last, k))
            } else {
                (self.at(k, 0), self.at(k, last))
            };
            sum += b - a;
        }
        sum / other as f64
    }

    /// How long it is along parameter `axis`: its control points' way
    /// from start to end, averaged over the other. (Not how far apart
    /// its ends are: a piece that closes around ends where it starts.)
    pub(crate) fn span(&self, axis: usize) -> f64 {
        let (count, other) = (self.n[axis], self.n[1 - axis]);
        let mut sum = 0.0;
        for k in 0..other {
            for i in 1..count {
                let (a, b) = if axis == 0 {
                    (self.at(i - 1, k), self.at(i, k))
                } else {
                    (self.at(k, i - 1), self.at(k, i))
                };
                sum += a.distance(b);
            }
        }
        sum / other as f64
    }

    /// How far it reaches along `n`: the least and greatest of its
    /// control points there.
    pub(crate) fn reach(&self, n: Point3) -> (f64, f64) {
        let mut r = (f64::INFINITY, f64::NEG_INFINITY);
        for h in &self.cp {
            let d = point(h).dot(n);
            r = (r.0.min(d), r.1.max(d));
        }
        r
    }

    /// How far it is from straight (a curve: from the line through its
    /// ends) or flat (a patch: from the plane of its corners).
    pub(crate) fn bend(&self) -> f64 {
        match self.dims() {
            0 => 0.0,
            1 => {
                let (a, b) = (point(&self.cp[0]), point(&self.cp[self.cp.len() - 1]));
                let Some(d) = (b - a).try_normalize() else {
                    return self.bounds().size();
                };
                let off = |h: &H| (point(h) - a).reject_from_normalized(d).length();
                self.cp.iter().map(off).fold(0.0, f64::max)
            }
            _ => {
                let Some(n) = self.extent(0).cross(self.extent(1)).try_normalize() else {
                    return self.bounds().size();
                };
                let (lo, hi) = self.reach(n);
                hi - lo
            }
        }
    }

    /// The part of it over `[x0, x1]` of `[0, 1]` along parameter `axis`.
    pub(crate) fn restrict(&mut self, axis: usize, x0: f64, x1: f64) {
        let (count, other) = (self.n[axis], self.n[1 - axis]);
        let stride = if axis == 0 { self.n[1] } else { 1 };
        let step = if axis == 0 { 1 } else { self.n[1] };
        // (Held in place for the degrees there are in practice: this is
        // called too often to ask for memory each time.)
        let mut held = [[0.0; 4]; HELD];
        let mut more = Vec::new();
        let line: &mut [H] = if count <= HELD {
            &mut held[..count]
        } else {
            more.resize(count, [0.0; 4]);
            &mut more
        };
        for k in 0..other {
            for (i, l) in line.iter_mut().enumerate() {
                *l = self.cp[k * step + i * stride];
            }
            // Keep [0, x1], then of that what is right of x0.
            if x1 < 1.0 {
                for r in 1..count {
                    for i in (r..count).rev() {
                        line[i] = lerp(&line[i - 1], &line[i], x1);
                    }
                }
            }
            if x0 > 0.0 {
                let s = x0 / x1;
                for r in 1..count {
                    for i in 0..count - r {
                        line[i] = lerp(&line[i], &line[i + 1], s);
                    }
                }
            }
            for (i, l) in line.iter().enumerate() {
                self.cp[k * step + i * stride] = *l;
            }
        }
        let (a, b) = self.range[axis];
        self.range[axis] = (a + (b - a) * x0, a + (b - a) * x1);
    }

    /// The point at `x`, each parameter as a part of `[0, 1]`.
    pub(crate) fn eval(&self, x: [f64; 2]) -> Point3 {
        let (nu, nv) = (self.n[0], self.n[1]);
        if nu > HELD || nv > HELD {
            return self.eval_long(x);
        }
        let mut row = [[0.0; 4]; HELD];
        let mut line = [[0.0; 4]; HELD];
        for (i, at) in row[..nu].iter_mut().enumerate() {
            line[..nv].copy_from_slice(&self.cp[i * nv..(i + 1) * nv]);
            for r in 1..nv {
                for k in 0..nv - r {
                    line[k] = lerp(&line[k], &line[k + 1], x[1]);
                }
            }
            *at = line[0];
        }
        for r in 1..nu {
            for k in 0..nu - r {
                row[k] = lerp(&row[k], &row[k + 1], x[0]);
            }
        }
        point(&row[0])
    }

    /// [`eval`](Self::eval) for a piece of a degree too high to be
    /// held in place.
    fn eval_long(&self, x: [f64; 2]) -> Point3 {
        let mut row: Vec<H> = (0..self.n[0])
            .map(|i| {
                let mut line = self.cp[i * self.n[1]..(i + 1) * self.n[1]].to_vec();
                for r in 1..line.len() {
                    for k in 0..line.len() - r {
                        line[k] = lerp(&line[k], &line[k + 1], x[1]);
                    }
                }
                line[0]
            })
            .collect();
        for r in 1..row.len() {
            for k in 0..row.len() - r {
                row[k] = lerp(&row[k], &row[k + 1], x[0]);
            }
        }
        point(&row[0])
    }

    /// Its two halves along parameter `axis`.
    pub(crate) fn halves(&self, axis: usize) -> [Piece; 2] {
        let (mut a, mut b) = (self.clone(), self.clone());
        a.restrict(axis, 0.0, 0.5);
        b.restrict(axis, 0.5, 1.0);
        [a, b]
    }

    /// Where along parameter `axis` (as part of `[0, 1]`) it can be
    /// between `lo` and `hi` along `n`; `None` if nowhere. A point of the
    /// piece is a mix of its control points, so its distance along `n`,
    /// seen over `axis`, lies in the convex hull of theirs.
    pub(crate) fn clip(&self, axis: usize, n: Point3, lo: f64, hi: f64) -> Option<(f64, f64)> {
        let (count, other) = (self.n[axis], self.n[1 - axis]);
        // Above `lo`: n·P − lo ≥ 0, times the weight; below `hi` alike.
        let mut held = [[f64::NEG_INFINITY; HELD]; 2];
        let mut more = [Vec::new(), Vec::new()];
        let [above, below] = if count <= HELD {
            let [a, b] = &mut held;
            [&mut a[..count], &mut b[..count]]
        } else {
            for m in &mut more {
                m.resize(count, f64::NEG_INFINITY);
            }
            let [a, b] = &mut more;
            [&mut a[..], &mut b[..]]
        };
        for i in 0..count {
            for k in 0..other {
                let h = if axis == 0 {
                    &self.cp[i * self.n[1] + k]
                } else {
                    &self.cp[k * self.n[1] + i]
                };
                let d = h[0] * n.x + h[1] * n.y + h[2] * n.z;
                above[i] = above[i].max(d - lo * h[3]);
                below[i] = below[i].max(hi * h[3] - d);
            }
        }
        let (a0, a1) = nonnegative(above)?;
        let (b0, b1) = nonnegative(below)?;
        let (x0, x1) = (a0.max(b0), a1.min(b1));
        (x0 <= x1).then_some((x0, x1))
    }
}

/// The polynomial with Bernstein coefficients `c` at `t`.
pub(crate) fn bernstein(c: &[f64], t: f64) -> f64 {
    let mut c = c.to_vec();
    for r in 1..c.len() {
        for k in 0..c.len() - r {
            c[k] += (c[k + 1] - c[k]) * t;
        }
    }
    c[0]
}

/// The parameters in `[0, 1]` at which the polynomial with Bernstein
/// coefficients `c` is zero: clipped to where the hull of its graph
/// crosses the axis, halved where that does not narrow it. What is
/// within `zero` of zero counts as zero; `None` if the polynomial is
/// that throughout, or along more stretches than can be told apart.
pub(crate) fn roots(c: &[f64], zero: f64) -> Option<Vec<f64>> {
    let scale = c.iter().fold(0.0_f64, |m, x| m.max(x.abs()));
    if scale <= zero {
        return None;
    }
    // Rounding must not hide a root; a band this thin does not make two
    // of one.
    let zero = scale * 1e-15;
    let mut budget = 4096;
    let mut out = Vec::new();
    let mut stack = vec![(c.to_vec(), 0.0, 1.0)];
    while let Some((mut c, mut a, mut b)) = stack.pop() {
        loop {
            budget -= 1;
            if budget == 0 {
                return None;
            }
            let up: Vec<f64> = c.iter().map(|x| x + zero).collect();
            let down: Vec<f64> = c.iter().map(|x| zero - x).collect();
            let (Some(p), Some(q)) = (nonnegative(&up), nonnegative(&down)) else {
                break;
            };
            let (x0, x1) = (p.0.max(q.0), p.1.min(q.1));
            if x0 > x1 {
                break;
            }
            if b - a < 1e-13 || c.iter().all(|x| x.abs() <= zero) {
                out.push((a + b) / 2.0);
                break;
            }
            if x1 - x0 > 0.8 {
                let mut left = Piece {
                    n: [c.len(), 1],
                    cp: c.iter().map(|&x| [x, 0.0, 0.0, 1.0]).collect(),
                    range: [(a, b), (0.0, 0.0)],
                };
                let [l, r] = left.halves(0);
                left = l;
                stack.push((
                    r.cp.iter().map(|h| h[0]).collect(),
                    r.range[0].0,
                    r.range[0].1,
                ));
                c = left.cp.iter().map(|h| h[0]).collect();
                (a, b) = left.range[0];
                continue;
            }
            let mut piece = Piece {
                n: [c.len(), 1],
                cp: c.iter().map(|&x| [x, 0.0, 0.0, 1.0]).collect(),
                range: [(a, b), (0.0, 0.0)],
            };
            let (x0, x1) = ((x0 - 1e-12).max(0.0), (x1 + 1e-12).min(1.0));
            piece.restrict(0, x0, x1.max(x0 + 1e-15).min(1.0));
            c = piece.cp.iter().map(|h| h[0]).collect();
            (a, b) = piece.range[0];
        }
    }
    out.sort_by(f64::total_cmp);
    out.dedup_by(|x, y| (*x - *y).abs() < 1e-10);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use shapecraft_core::world::topo::Curve3;

    #[test]
    fn pieces_are_the_curve_and_clips_keep_what_is_in_the_slab() -> anyhow::Result<()> {
        let h = std::f64::consts::FRAC_1_SQRT_2;
        // A half circle of two rational arcs.
        let c = Curve3::new(
            2,
            vec![0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0],
            vec![
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(1.0, 1.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
                Point3::new(-1.0, 1.0, 0.0),
                Point3::new(-1.0, 0.0, 0.0),
            ],
            Some(vec![1.0, h, 1.0, h, 1.0]),
        )?;
        let pieces = curve_pieces(&c, 0.1, 0.9);
        assert_eq!(pieces.len(), 2);
        assert!(pieces[0].at(0, 0).abs_diff_eq(c.point_at(0.1), 1e-14));
        assert!(pieces[1].at(2, 0).abs_diff_eq(c.point_at(0.9), 1e-14));
        // Where the first arc is between x = 0.5 and x = 0.6: around 60°.
        // (A wide slab is closed in on step by step; a thin one fast.)
        let mut arc = curve_pieces(&c, 0.0, 0.5).remove(0);
        for _ in 0..12 {
            let (x0, x1) = arc.clip(0, Point3::X, 0.5, 0.6).expect("it is");
            arc.restrict(0, x0, x1);
        }
        let (a, b) = arc.range[0];
        assert!((c.point_at(a).x - 0.6).abs() < 1e-9 && (c.point_at(b).x - 0.5).abs() < 1e-9);
        assert!(arc.clip(0, Point3::X, 2.0, 3.0).is_none());
        Ok(())
    }

    #[test]
    fn roots_of_polynomials_are_found() {
        // (t − 0.25)(t − 0.75) = t² − t + 3/16, in Bernstein form.
        let r = roots(&[3.0 / 16.0, -5.0 / 16.0, 3.0 / 16.0], 0.0).expect("not zero");
        assert_eq!(r.len(), 2);
        assert!((r[0] - 0.25).abs() < 1e-10 && (r[1] - 0.75).abs() < 1e-10);
        assert_eq!(roots(&[1.0, 2.0, 0.5], 0.0).expect("not zero").len(), 0);
        assert!(roots(&[0.0, 0.0], 0.0).is_none());
    }
}
