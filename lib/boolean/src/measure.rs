//! The volume and area of a solid, from its faces' surfaces and loops
//! themselves: no mesh stands in for them.
//!
//! Both are integrals over each face's region of (u, v) — of the area
//! element, and of a third of the point's reach along the normal
//! (Gauss's theorem). Green's theorem turns an integral over a region
//! into one along its loops, so the loops' curves are all that describes
//! the region: `∬ f du dv = ∮ F dv`, with `F(u, v)` the integral of `f`
//! along u up to the loop.

use workpiece_core::math::Point2;
use workpiece_core::math::Point3;
use workpiece_core::world::topo::nurbs::{AlongU, NurbsSurface};

use crate::bezier::{curve_pieces, roots};
use crate::model::{Face, Solid};

/// What a solid measures.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Measures {
    /// The volume its faces enclose.
    pub volume: f64,
    /// The area of its faces.
    pub area: f64,
    /// Where its centre of gravity is, of one stuff throughout.
    pub centre: Point3,
}

/// What is summed over a face: its volume and area elements, and those
/// of the volume's first moments along x, y and z.
type Sums<const N: usize = 5> = [f64; N];

/// What is summed with the second moments as well: after the five of
/// [`Sums`], those of the volume about where it is seen from — xx, yy,
/// zz, xy, xz, yz.
const WITH_SECOND: usize = 11;

fn add<const N: usize>(sum: &mut Sums<N>, part: Sums<N>) {
    for (s, p) in sum.iter_mut().zip(part) {
        *s += p;
    }
}

/// Gauss–Legendre nodes and weights on `[-1, 1]`, 8 points.
const GAUSS: [(f64, f64); 8] = [
    (-0.960_289_856_497_536_2, 0.101_228_536_290_376_26),
    (-0.796_666_477_413_626_7, 0.222_381_034_453_374_48),
    (-0.525_532_409_916_329, 0.313_706_645_877_887_3),
    (-0.183_434_642_495_649_8, 0.362_683_783_378_362),
    (0.183_434_642_495_649_8, 0.362_683_783_378_362),
    (0.525_532_409_916_329, 0.313_706_645_877_887_3),
    (0.796_666_477_413_626_7, 0.222_381_034_453_374_48),
    (0.960_289_856_497_536_2, 0.101_228_536_290_376_26),
];

/// Gauss–Legendre nodes and weights on `[-1, 1]`, 4 points: for a
/// stretch that is one of many short ones.
const GAUSS_4: [(f64, f64); 4] = [
    (-0.861_136_311_594_052_6, 0.347_854_845_137_453_85),
    (-0.339_981_043_584_856_3, 0.652_145_154_862_546_1),
    (0.339_981_043_584_856_3, 0.652_145_154_862_546_1),
    (0.861_136_311_594_052_6, 0.347_854_845_137_453_85),
];

/// The integral of `f` from `a` to `b`, by four points.
fn gauss_4<const N: usize>(a: f64, b: f64, f: &mut dyn FnMut(f64) -> Sums<N>) -> Sums<N> {
    let (mid, half) = ((a + b) / 2.0, (b - a) / 2.0);
    let mut sum = [0.0; N];
    for (x, w) in GAUSS_4 {
        add(&mut sum, f(mid + half * x).map(|y| w * y));
    }
    sum.map(|s| s * half)
}

/// The integral of `f` from `a` to `b`.
fn gauss<const N: usize>(a: f64, b: f64, f: &mut dyn FnMut(f64) -> Sums<N>) -> Sums<N> {
    let (mid, half) = ((a + b) / 2.0, (b - a) / 2.0);
    let mut sum = [0.0; N];
    for (x, w) in GAUSS {
        add(&mut sum, f(mid + half * x).map(|y| w * y));
    }
    sum.map(|s| s * half)
}

/// The integral of `f` from `a` to `b`, to where its area part (which
/// is no polynomial on a surface that bends: a root is in it) is as
/// good as the others: by eight points, and where four give another
/// area — beyond what eight then still miss — by halves, each done the
/// same way. With each value `f` gives how large the area part is there
/// before anything cancels in it: what the two are told apart against.
fn gauss_close<const N: usize>(
    a: f64,
    b: f64,
    f: &mut dyn FnMut(f64) -> (Sums<N>, f64),
    depth: u32,
) -> Sums<N> {
    let (mid, half) = ((a + b) / 2.0, (b - a) / 2.0);
    let mut sum = [0.0; N];
    let mut size = 0.0;
    for (x, w) in GAUSS {
        let (y, weight) = f(mid + half * x);
        size += w * weight;
        add(&mut sum, y.map(|y| w * y));
    }
    if depth == 0 {
        return sum.map(|s| s * half);
    }
    let rough: f64 = GAUSS_4
        .iter()
        .map(|(x, w)| w * f(mid + half * x).0[1])
        .sum();
    // (What eight points miss is about the square of what four do.)
    if (rough - sum[1]).abs() <= 1e-4 * size {
        return sum.map(|s| s * half);
    }
    let mut both = gauss_close(a, mid, f, depth - 1);
    add(&mut both, gauss_close(mid, b, f, depth - 1));
    both
}

/// The integral of `f` from `a` to `b`, by two points: exact for a
/// cubic.
fn gauss_2<const N: usize>(a: f64, b: f64, f: &mut dyn FnMut(f64) -> Sums<N>) -> Sums<N> {
    let (mid, half) = ((a + b) / 2.0, (b - a) / 2.0);
    let x = half * 0.577_350_269_189_625_8;
    let mut sum = f(mid - x);
    add(&mut sum, f(mid + x));
    sum.map(|s| s * half)
}

/// Whether `surface` runs straight along u, every line of one v the
/// same way or all of them through one point: a flat face, the wall of
/// an extrusion or of a cylinder, a cone. What is summed over such a
/// surface is a cubic along u — its normal keeps its direction along
/// each line, and grows evenly — which two points sum exactly.
fn straight_along_u(surface: &NurbsSurface) -> bool {
    let (count_u, count_v) = surface.counts();
    if surface.degrees().0 != 1 || count_u != 2 {
        return false;
    }
    let (points, weights) = (surface.control_points(), surface.weights());
    let size = points
        .iter()
        .fold(0.0_f64, |m, p| m.max(p.distance(points[0])));
    let step = |j: usize| points[count_v + j] - points[j];
    // How much the lines draw together (none of it: they are parallel).
    let mut rate = 0.0;
    if let Some(k) = (1..count_v).max_by(|a, b| {
        let far = |j: &usize| points[*j].distance_squared(points[0]);
        far(a).total_cmp(&far(b))
    }) {
        let across = points[k] - points[0];
        if across.length_squared() > 0.0 {
            rate = (step(k) - step(0)).dot(across) / across.length_squared();
        }
    }
    // (Not through the point they meet in: past it the surface is
    // turned inside out.)
    if !rate.is_finite() || rate < -1.0 {
        return false;
    }
    (0..count_v).all(|j| {
        let same_weight = (weights[j] - weights[count_v + j]).abs() <= 1e-14 * weights[j].abs();
        let off = (step(j) - step(0)) - (points[j] - points[0]) * rate;
        same_weight && off.length() <= 1e-12 * size
    })
}

/// `surface` with its parameters changed over: its u is the other's v.
fn turned(surface: &NurbsSurface) -> Option<NurbsSurface> {
    let (count_u, count_v) = surface.counts();
    let (degree_u, degree_v) = surface.degrees();
    let (knots_u, knots_v) = surface.knots();
    let (points, weights) = (surface.control_points(), surface.weights());
    let mut turned_points = Vec::with_capacity(points.len());
    let mut turned_weights = Vec::with_capacity(points.len());
    for j in 0..count_v {
        for i in 0..count_u {
            turned_points.push(points[i * count_v + j]);
            turned_weights.push(weights[i * count_v + j]);
        }
    }
    NurbsSurface::new(
        degree_v,
        degree_u,
        knots_v.to_vec(),
        knots_u.to_vec(),
        count_v,
        count_u,
        turned_points,
        Some(turned_weights),
    )
    .ok()
}

/// The breaks of `knots` strictly between `a` and `b`, with the two.
fn stops(knots: &[f64], a: f64, b: f64) -> Vec<f64> {
    let (lo, hi) = (a.min(b), a.max(b));
    let mut out = vec![lo];
    for &k in knots {
        if k > lo && k < hi && out.last().is_none_or(|&l| k > l) {
            out.push(k);
        }
    }
    out.push(hi);
    if a > b {
        out.reverse();
    }
    out
}

/// The volume and area elements of a surface at `u` along one v: a third of the
/// point, seen from `from`, along `∂u × ∂v`, and that cross's length;
/// and the elements of the volume's first moments about `from`: half
/// the square of each coordinate, along that of the cross.
fn elements<const N: usize>(along: &AlongU<'_>, from: Point3, u: f64) -> Sums<N> {
    elements_of(along.derivatives_at(u), from)
}

/// The elements [`elements`] gives, of a point of a surface and its
/// slopes there.
fn elements_of<const N: usize>((p, du, dv): (Point3, Point3, Point3), from: Point3) -> Sums<N> {
    let n = du.cross(dv);
    let q = p - from;
    let moment = q * q * n / 2.0;
    let mut out = [0.0; N];
    for (o, x) in out
        .iter_mut()
        .zip([q.dot(n) / 3.0, n.length(), moment.x, moment.y, moment.z])
    {
        *o = x;
    }
    // (The second moments, where they are asked for: a fifth of the
    // products of two coordinates, along the normal — Gauss's theorem
    // again, of `x y q`, whose divergence is five times `x y`.)
    if N > 5 {
        let fifth = q.dot(n) / 5.0;
        let products = [
            q.x * q.x,
            q.y * q.y,
            q.z * q.z,
            q.x * q.y,
            q.x * q.z,
            q.y * q.z,
        ];
        for (o, x) in out[5..].iter_mut().zip(products) {
            *o = x * fifth;
        }
    }
    out
}

/// How many control points along a parameter a [`Strip`] holds: for a
/// degree of seven at most.
const STRIP: usize = 8;

/// The Bézier patches of a surface, by the spans of its knots: what a
/// surface that bends is summed over, patch by patch.
struct Patches {
    pieces: Vec<crate::bezier::Piece>,
    /// The spans along u, and along v: each from where to where.
    spans: [Vec<(f64, f64)>; 2],
}

impl Patches {
    /// Those of `surface`, if its degrees are no more than a [`Strip`]
    /// holds.
    fn of(surface: &NurbsSurface) -> Option<Patches> {
        let (pu, pv) = surface.degrees();
        if pu >= STRIP || pv >= STRIP {
            return None;
        }
        let (u, v) = surface.domain();
        let pieces = crate::bezier::surface_pieces(surface, u, v);
        let mut spans: [Vec<(f64, f64)>; 2] = [Vec::new(), Vec::new()];
        for piece in &pieces {
            for (axis, spans) in spans.iter_mut().enumerate() {
                if !spans.contains(&piece.range[axis]) {
                    spans.push(piece.range[axis]);
                }
            }
        }
        (pieces.len() == spans[0].len() * spans[1].len()).then_some(Patches { pieces, spans })
    }

    /// The surface along u at `v`, over the span of u that `u` is in.
    fn strip(&self, u: f64, v: f64) -> Option<Strip> {
        let within = |axis: usize, x: f64| {
            let spans = &self.spans[axis];
            let n = spans.len();
            (0..n).find(|&k| (k == 0 || x >= spans[k].0) && (k + 1 == n || x < spans[k].1))
        };
        let (i, j) = (within(0, u)?, within(1, v)?);
        let piece = &self.pieces[i * self.spans[1].len() + j];
        let [nu, nv] = piece.n;
        let ((ua, ub), (va, vb)) = (piece.range[0], piece.range[1]);
        let t = (v - va) / (vb - va);
        let mut strip = Strip {
            from: ua,
            rate: 1.0 / (ub - ua),
            n: nu,
            at: [[0.0; 4]; STRIP],
            across: [[0.0; 4]; STRIP],
        };
        // Each row along v brought to `v`: its point there, and how it
        // moves with v.
        for a in 0..nu {
            let mut row = [[0.0; 4]; STRIP];
            row[..nv].copy_from_slice(&piece.cp[a * nv..(a + 1) * nv]);
            let (point, slope) = casteljau(&mut row, nv, t);
            strip.at[a] = point;
            strip.across[a] = slope.map(|x| x / (vb - va));
        }
        Some(strip)
    }
}

/// The point of the Bézier curve of the `n` homogeneous points `row`
/// at `t`, and how it moves with `t` there. (`row` is worked in.)
fn casteljau(row: &mut [[f64; 4]; STRIP], n: usize, t: f64) -> ([f64; 4], [f64; 4]) {
    if n == 1 {
        return (row[0], [0.0; 4]);
    }
    // (Of the second degree, as an arc is: at once.)
    if n == 3 {
        let s = 1.0 - t;
        let mut point = [0.0; 4];
        let mut slope = [0.0; 4];
        for c in 0..4 {
            let (a, b) = (row[0][c] * s + row[1][c] * t, row[1][c] * s + row[2][c] * t);
            point[c] = a * s + b * t;
            slope[c] = (b - a) * 2.0;
        }
        return (point, slope);
    }
    for level in (2..n).rev() {
        for k in 0..level {
            let next = row[k + 1];
            for (x, y) in row[k].iter_mut().zip(next) {
                *x += (y - *x) * t;
            }
        }
    }
    let degree = (n - 1) as f64;
    let mut point = [0.0; 4];
    let mut slope = [0.0; 4];
    for c in 0..4 {
        point[c] = row[0][c] + (row[1][c] - row[0][c]) * t;
        slope[c] = (row[1][c] - row[0][c]) * degree;
    }
    (point, slope)
}

/// A surface along u at one v, over one span of its knots along u: a
/// Bézier curve of homogeneous points, and one of how they move with
/// v.
struct Strip {
    /// Where the span starts, and one over how long it is.
    from: f64,
    rate: f64,
    n: usize,
    at: [[f64; 4]; STRIP],
    across: [[f64; 4]; STRIP],
}

impl Strip {
    /// What [`NurbsSurface::derivatives_at`] gives at `u` and the
    /// strip's v.
    fn derivatives_at(&self, u: f64) -> (Point3, Point3, Point3) {
        let t = (u - self.from) * self.rate;
        let mut held = [[0.0; 4]; STRIP];
        held[..self.n].copy_from_slice(&self.at[..self.n]);
        let (a, au) = casteljau(&mut held, self.n, t);
        held[..self.n].copy_from_slice(&self.across[..self.n]);
        let (av, _) = casteljau(&mut held, self.n, t);
        let xyz = |h: [f64; 4]| Point3::new(h[0], h[1], h[2]);
        let point = xyz(a) / a[3];
        (
            point,
            (xyz(au) - point * au[3]) * (self.rate / a[3]),
            (xyz(av) - point * av[3]) / a[3],
        )
    }
}

fn face<const N: usize>(solid: &Solid, face: &Face, from: Point3) -> Sums<N> {
    // (With the second moments, what is summed is of a higher degree
    // than the rules that are exact for volume and area take: the next
    // rule up then.)
    let fine = N > 5;
    // Summed along u first, to the loops: along the way the surface
    // runs straight, if it does along one of its parameters — with the
    // parameters changed over for it, if that one is v.
    let held;
    let mut surface = &*face.surface;
    let mut straight = straight_along_u(surface);
    let mut over = false;
    if !straight
        && surface.degrees().1 == 1
        && surface.counts().1 == 2
        && let Some(other) = turned(surface).filter(straight_along_u)
    {
        held = other;
        (surface, straight, over) = (&held, true, true);
    }
    // A flat face laid out evenly: its point goes evenly with both
    // parameters.
    let affine = straight && {
        let p = surface.control_points();
        surface.degrees() == (1, 1)
            && p.len() == 4
            && surface.weights().iter().all(|w| *w == 1.0)
            && (p[3] - p[2] - p[1] + p[0]).length() <= 1e-12 * p[0].distance(p[3])
    };
    // A place of a loop, and how it moves along the loop, in the
    // parameters summed in.
    let placed = |p: Point2| if over { Point2::new(p.y, p.x) } else { p };
    let (iu, iv) = if over { (1, 0) } else { (0, 1) };
    let (knots_u, _) = surface.knots();
    // From the middle of the loops along u to the loop: as short a
    // way as there is, so that a small face on a large surface is not
    // the small difference of two long integrals.
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    // (The loops' Bézier pieces: cut once.)
    let pieces_of: Vec<Vec<crate::bezier::Piece>> = face
        .loops
        .iter()
        .flatten()
        .map(|step| {
            let (a, b) = solid.span(step);
            curve_pieces(&*step.pcurve, a.min(b), a.max(b))
        })
        .collect();
    for cut in &pieces_of {
        for piece in cut {
            for h in &piece.cp {
                lo = lo.min(h[iu] / h[3]);
                hi = hi.max(h[iu] / h[3]);
            }
        }
    }
    let ((d0, d1), _) = surface.domain();
    let u0 = if lo <= hi {
        ((lo + hi) / 2.0).clamp(d0, d1)
    } else {
        d0
    };
    let (degree_u, degree_v) = surface.degrees();
    let finer = degree_u.max(degree_v).div_ceil(3).max(1);
    // (A flat patch's area element is what the rule is exact for.)
    let bends = degree_u.max(degree_v) > 1 || surface.weights().iter().any(|w| *w != 1.0);
    let depth = if bends { 6 } else { 0 };
    // What an area element of the surface is about, a billionth of it:
    // below that one is rounding (along a side the surface comes to a
    // point at), and nothing to go by.
    let least = {
        let points = surface.control_points();
        let across = points
            .iter()
            .fold(0.0_f64, |m, p| m.max(p.distance(points[0])));
        let (_, (e0, e1)) = surface.domain();
        across * across / ((d1 - d0) * (e1 - e0)).max(1e-300) * 1e-9
    };
    // (A surface that bends, patch by patch: each span of it along u
    // as the curve it is at one v, which is asked about many times.)
    let patches = if straight { None } else { Patches::of(surface) };
    let upto = |u: f64, v: f64| {
        let mut sum = [0.0; N];
        let spans = stops(knots_u, u0, u);
        // (Across many spans each is short, and four points do for it
        // what eight do for a long one.)
        let many = spans.len() > 5;
        // (All of them at this one v: what that fixes is worked out
        // once.)
        let along = std::cell::OnceCell::new();
        for pair in spans.windows(2) {
            let strip = patches
                .as_ref()
                .and_then(|p| p.strip((pair[0] + pair[1]) / 2.0, v));
            // (A span of high degree in pieces: the rule is exact for
            // what a cubic span has, not for one of degree fifteen.)
            for k in 0..finer {
                let at = |i: usize| pair[0] + (pair[1] - pair[0]) * i as f64 / finer as f64;
                let mut element = |s: f64| match &strip {
                    Some(strip) => elements_of(strip.derivatives_at(s), from),
                    None => elements(along.get_or_init(|| surface.along_u(v, u0, u)), from, s),
                };
                // (Short as it is: the area of a span that bends wants
                // the eight.)
                let part = if straight && !fine {
                    gauss_2(at(k), at(k + 1), &mut element)
                } else if straight {
                    gauss_4(at(k), at(k + 1), &mut element)
                } else if many && bends {
                    gauss(at(k), at(k + 1), &mut element)
                } else if many {
                    gauss_4(at(k), at(k + 1), &mut element)
                } else {
                    let mut sized = |s: f64| {
                        let e = element(s);
                        (e, e[1] + least)
                    };
                    gauss_close(at(k), at(k + 1), &mut sized, depth)
                };
                add(&mut sum, part);
            }
        }
        sum
    };
    // The inside knots of the surface: where a loop crosses one, what
    // is integrated along it has a kink, and the rule that integrates
    // it wants its pieces smooth.
    let inside = |knots: &[f64]| -> Vec<f64> {
        let (lo, hi) = (knots[0], knots[knots.len() - 1]);
        let mut out: Vec<f64> = knots
            .iter()
            .copied()
            .filter(|k| *k > lo && *k < hi)
            .collect();
        out.dedup();
        out
    };
    let (lines_u, lines_v) = (inside(knots_u), inside(surface.knots().1));
    let level_within = {
        let (_, (e0, e1)) = surface.domain();
        (e1 - e0).abs() * 1e-12
    };
    let mut total = [0.0; N];
    for (step, cut) in face.loops.iter().flatten().zip(&pieces_of) {
        let (a, b) = solid.span(step);
        let (lo, hi) = (a.min(b), a.max(b));
        let mut stops = vec![lo, hi];
        // Where the step runs along a line of one v — a rim of a
        // cylinder, a side of a flat face laid out square — nothing is
        // summed: what is summed is by how v changes along it.
        let mut level: Vec<(f64, f64)> = Vec::new();
        for piece in cut {
            let (p0, p1) = piece.range[0];
            stops.push(p0);
            // (Along it to within rounding: a line of one v fitted to
            // where it was found is that line to a part in 1e12 or so,
            // and what is summed along it is that little.)
            let v = piece.cp[0][iv] / piece.cp[0][3];
            if piece
                .cp
                .iter()
                .all(|h| (h[iv] / h[3] - v).abs() <= level_within)
            {
                level.push((p0, p1));
            }
            for (axis, lines) in [(iu, &lines_u), (iv, &lines_v)] {
                for &line in lines {
                    let c: Vec<f64> = piece.cp.iter().map(|h| h[axis] - line * h[3]).collect();
                    if c.iter().all(|x| *x > 0.0) || c.iter().all(|x| *x < 0.0) {
                        continue;
                    }
                    for t in roots(&c, 0.0).unwrap_or_default() {
                        stops.push(p0 + (p1 - p0) * t);
                    }
                }
            }
        }
        stops.sort_by(f64::total_cmp);
        stops.dedup_by(|x, y| (*x - *y).abs() <= 1e-14 * (hi - lo));
        if a > b {
            stops.reverse();
        }
        // A straight step across a flat face: what is summed along it
        // is a cubic.
        let plain =
            affine && step.pcurve.degree() == 1 && step.pcurve.weights().iter().all(|w| *w == 1.0);
        // Long pieces in two, for the loops that are one arc.
        let pieces = if stops.len() > 8 || plain { 1 } else { 2 } * finer as u32;
        for pair in stops.windows(2) {
            let middle = (pair[0] + pair[1]) / 2.0;
            if level.iter().any(|(p0, p1)| middle > *p0 && middle < *p1) {
                continue;
            }
            for k in 0..pieces {
                let at = |f: f64| pair[0] + (pair[1] - pair[0]) * f / f64::from(pieces);
                let mut along = |t: f64| {
                    let (uv, d) = step.pcurve.derivative_at(t);
                    let (uv, d) = (placed(uv), placed(d));
                    upto(uv.x, uv.y).map(|inner| inner * d.y)
                };
                // (Many stops: each stretch between two is short.)
                let (a, b) = (at(f64::from(k)), at(f64::from(k + 1)));
                let part = if plain && !fine {
                    gauss_2(a, b, &mut along)
                } else if plain {
                    gauss_4(a, b, &mut along)
                } else if stops.len() > 64 {
                    // (Very many: a curve fitted piece by piece to
                    // where two surfaces cross, each piece next to
                    // straight.)
                    gauss_4(a, b, &mut along)
                } else if stops.len() > 16 && bends {
                    gauss(a, b, &mut along)
                } else if stops.len() > 16 {
                    gauss_4(a, b, &mut along)
                } else {
                    // (Along a line of the surface the area part is
                    // nothing but rounding: not what to go by.)
                    let mut sized = |t: f64| {
                        let (uv, d) = step.pcurve.derivative_at(t);
                        let (uv, d) = (placed(uv), placed(d));
                        let inner = upto(uv.x, uv.y);
                        (
                            inner.map(|i| i * d.y),
                            (inner[1].abs() + least * (d1 - d0)) * d.length(),
                        )
                    };
                    gauss_close(a, b, &mut sized, depth)
                };
                add(&mut total, part);
            }
        }
    }
    // Where a step ends a little off where the next starts (edges are
    // on their faces only within their tolerance), the loop is closed
    // straight across: left open, it would bound no area.
    for lp in &face.loops {
        for (k, step) in lp.iter().enumerate() {
            let next = &lp[(k + 1) % lp.len()];
            let here = placed(step.pcurve.point_at(solid.span(step).1));
            let there = placed(next.pcurve.point_at(solid.span(next).0));
            if here == there || here.y == there.y {
                continue;
            }
            // (In pieces between the surface's knot lines it crosses,
            // as the steps themselves are: across a point a surface
            // comes to, this is the whole of a side of it.)
            let mut stops = vec![0.0, 1.0];
            for (lines, from, to) in [(&lines_u, here.x, there.x), (&lines_v, here.y, there.y)] {
                for &line in lines {
                    let t = (line - from) / (to - from);
                    if t.is_finite() && t > 0.0 && t < 1.0 {
                        stops.push(t);
                    }
                }
            }
            stops.sort_by(f64::total_cmp);
            for pair in stops.windows(2) {
                let middle = (pair[0] + pair[1]) / 2.0;
                for (from, to) in [(pair[0], middle), (middle, pair[1])] {
                    let part = gauss(from, to, &mut |t| {
                        let uv = here.lerp(there, t);
                        upto(uv.x, uv.y).map(|inner| inner * (there.y - here.y))
                    });
                    add(&mut total, part);
                }
            }
        }
    }
    // (Summed with the parameters changed over, the loops run the
    // other way around and the normal points the other way: which the
    // area is the one not to make up for.)
    if over {
        total[1] = -total[1];
    }
    // (All but the area turn around with the face.)
    if !face.outward {
        for (k, sum) in total.iter_mut().enumerate() {
            if k != 1 {
                *sum = -*sum;
            }
        }
    }
    total
}

/// The area of face `face` of `solid` (also of a sheet's).
pub fn face_area(solid: &Solid, face_index: usize) -> f64 {
    face::<5>(solid, &solid.faces[face_index], Point3::ZERO)[1].abs()
}

/// The volume, area and centre of gravity of `solid`.
pub fn measure(solid: &Solid) -> Measures {
    // Seen from the middle of the solid: a thin solid's volume is then
    // a sum of small numbers, not the small difference of large ones.
    let mut lo = Point3::INFINITY;
    let mut hi = Point3::NEG_INFINITY;
    for v in &solid.vertices {
        lo = lo.min(v.point);
        hi = hi.max(v.point);
    }
    let from = if solid.vertices.is_empty() {
        Point3::ZERO
    } else {
        (lo + hi) / 2.0
    };
    let mut total = [0.0; 5];
    for f in &solid.faces {
        add(&mut total, face(solid, f, from));
    }
    let moments = Point3::new(total[2], total[3], total[4]);
    Measures {
        volume: total[0],
        area: total[1],
        centre: if total[0] != 0.0 {
            from + moments / total[0]
        } else {
            from
        },
    }
}

/// The volume of `solid`, where its centre of gravity is, and its
/// second moments `∫ q qᵀ dV` about that centre (`q` from the centre):
/// xx, yy, zz, xy, xz, yz — summed over its faces as its volume is.
pub fn second_moments(solid: &Solid) -> (f64, Point3, [f64; 6]) {
    let (lo, hi) = solid.extent();
    let from = if solid.vertices.is_empty() {
        Point3::ZERO
    } else {
        (lo + hi) / 2.0
    };
    let mut total = [0.0; WITH_SECOND];
    for f in &solid.faces {
        workpiece_core::cancel::checkpoint();
        add(&mut total, face(solid, f, from));
    }
    let volume = total[0];
    if volume == 0.0 {
        return (0.0, from, [0.0; 6]);
    }
    let off = Point3::new(total[2], total[3], total[4]) / volume;
    // About the centre: less what the whole of it, at the centre,
    // would be about where it was seen from.
    let o = off.to_array();
    let pairs = [(0, 0), (1, 1), (2, 2), (0, 1), (0, 2), (1, 2)];
    let mut second = [0.0; 6];
    for (k, (i, j)) in pairs.into_iter().enumerate() {
        second[k] = total[5 + k] - volume * o[i] * o[j];
    }
    (volume, from + off, second)
}

/// How far the faces of `solid` are from closing up, as a volume: how
/// much the volume they enclose changes with where it is seen from,
/// across the solid. Nothing for faces that meet exactly along their
/// edges; for those of a file whose edges are off its surfaces, what
/// its volume is not known closer than.
pub fn open(solid: &Solid) -> f64 {
    let (lo, hi) = solid.extent();
    let seen = |from: Point3| -> f64 {
        solid
            .faces
            .iter()
            .map(|f| face::<5>(solid, f, from)[0])
            .sum()
    };
    let middle = seen((lo + hi) / 2.0);
    [Point3::X, Point3::Y, Point3::Z]
        .into_iter()
        .map(|axis| (seen((lo + hi) / 2.0 + axis * (hi - lo)) - middle).abs())
        .fold(0.0, f64::max)
}
