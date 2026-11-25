//! Questions of curves and surfaces that others building B-reps need
//! answered the way booleans answer them: where a point is on a surface
//! or a curve, and the curve in a surface's (u, v) under a curve that
//! lies on it.

use workpiece_core::math::{Point2, Point3};
use workpiece_core::world::topo::{Curve2, Curve3, NurbsSurface};

use crate::bezier::{Piece, curve_pieces, surface_pieces};
use crate::solve::{Along, At, Over, contacts};

/// Every (u, v) at which `surface` is within `tol` of `p` (more than one
/// where it meets itself, as along a seam).
pub fn on_surface(surface: &NurbsSurface, p: Point3, tol: f64) -> Vec<Point2> {
    let (u, v) = surface.domain();
    contacts(
        &At(p),
        &[Piece::point(p)],
        &Over(surface, [u, v]),
        &surface_pieces(surface, u, v),
        tol,
    )
    .map(|found| found.iter().map(|c| Point2::new(c.b[0], c.b[1])).collect())
    .unwrap_or_default()
}

/// Every parameter between `range`'s ends at which `curve` is within
/// `tol` of `p`.
pub fn on_curve(curve: &Curve3, range: (f64, f64), p: Point3, tol: f64) -> Vec<f64> {
    contacts(
        &At(p),
        &[Piece::point(p)],
        &Along(curve, range),
        &curve_pieces(curve, range.0, range.1),
        tol,
    )
    .map(|found| found.iter().map(|c| c.b[0]).collect())
    .unwrap_or_default()
}

/// The (u, v) of the point of `surface` nearest to `p`, looked for from
/// `guess`, and how far that point is.
pub fn foot(surface: &NurbsSurface, p: Point3, guess: Point2) -> (Point2, f64) {
    crate::fit::foot(surface, p, guess)
}

/// The curve in the (u, v) of `surface` under `curve` over `range`,
/// which lies on the surface, with `curve`'s parameters; `seed` is a
/// parameter of the curve and the (u, v) to look for it near. With it,
/// how far its image on the surface strays from the curve. Fitted to
/// within `accuracy` of the curve's feet on the surface.
pub fn pcurve(
    surface: &NurbsSurface,
    curve: &Curve3,
    range: (f64, f64),
    seed: (f64, Point2),
    accuracy: f64,
) -> Option<(Curve2, f64)> {
    crate::fit::project(surface, curve, range, seed, accuracy).map(|f| (f.curve, f.off))
}

/// Points along `curve` over `range`, from its first end to its second,
/// so close together that no chord between two of them is farther than
/// `tolerance` from the curve between them. Its ends and its knots are
/// among them.
pub fn chords(curve: &Curve3, range: (f64, f64), tolerance: f64) -> Vec<Point3> {
    let (lo, hi) = (range.0.min(range.1), range.0.max(range.1));
    let mut stops = vec![lo];
    for &k in curve.knots() {
        if k > lo && k < hi && stops.last().is_none_or(|&l| k > l) {
            stops.push(k);
        }
    }
    stops.push(hi);
    let straight = curve.degree() <= 1 && !curve.is_rational();
    // (Three places between two points say how far the curve leaves
    // their chord; it may do so a little more between those.)
    let near = tolerance.max(0.0) * 0.7;
    fn between(
        curve: &Curve3,
        (t0, p0): (f64, Point3),
        (t1, p1): (f64, Point3),
        near: f64,
        depth: u32,
        out: &mut Vec<Point3>,
    ) {
        let chord = p1 - p0;
        let off = |t: f64| {
            let q = curve.point_at(t) - p0;
            let along = (q.dot(chord) / chord.length_squared().max(1e-300)).clamp(0.0, 1.0);
            (q - chord * along).length()
        };
        let mid = (t0 + t1) / 2.0;
        let far = [0.25, 0.5, 0.75]
            .into_iter()
            .map(|f| off(t0 + (t1 - t0) * f))
            .fold(0.0, f64::max);
        if depth > 0 && far > near {
            let pm = curve.point_at(mid);
            between(curve, (t0, p0), (mid, pm), near, depth - 1, out);
            between(curve, (mid, pm), (t1, p1), near, depth - 1, out);
        } else {
            out.push(p1);
        }
    }
    let mut points = vec![curve.point_at(lo)];
    for pair in stops.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if straight {
            points.push(curve.point_at(b));
            continue;
        }
        // (In two halves first: a span that swings to both sides of
        // its chord may pass through it at all three places.)
        let mid = (a + b) / 2.0;
        let (pa, pm, pb) = (curve.point_at(a), curve.point_at(mid), curve.point_at(b));
        between(curve, (a, pa), (mid, pm), near, 24, &mut points);
        between(curve, (mid, pm), (b, pb), near, 24, &mut points);
    }
    if range.0 > range.1 {
        points.reverse();
    }
    points
}

/// The area the line from the origin of (u, v) to a point of `curve`
/// sweeps as the point goes from the parameter `from` to `to`,
/// counter-clockwise positive: the sum of it around a loop is the area
/// the loop goes around (Green's theorem). Span by span of the curve's
/// knots by Gauss's rule, which is exact for a polynomial curve up to
/// the fifth degree: a sliver between two curves that all but lie on
/// each other has an area far smaller than chords along them would
/// tell.
pub fn swept(curve: &Curve2, from: f64, to: f64) -> f64 {
    const NODES: [(f64, f64); 5] = [
        (0.0, 0.568_888_888_888_888_9),
        (-0.538_469_310_105_683_1, 0.478_628_670_499_366_5),
        (0.538_469_310_105_683_1, 0.478_628_670_499_366_5),
        (-0.906_179_845_938_664, 0.236_926_885_056_189_1),
        (0.906_179_845_938_664, 0.236_926_885_056_189_1),
    ];
    let (a, b) = (from.min(to), from.max(to));
    let mut stops = vec![a];
    stops.extend(curve.knots().iter().copied().filter(|k| *k > a && *k < b));
    stops.push(b);
    stops.dedup();
    // (A rational span is not a polynomial: in pieces.)
    let pieces = if curve.is_rational() { 8 } else { 1 };
    let mut sum = 0.0;
    for pair in stops.windows(2) {
        for k in 0..pieces {
            let at = |i: usize| pair[0] + (pair[1] - pair[0]) * i as f64 / pieces as f64;
            let (middle, reach) = ((at(k) + at(k + 1)) / 2.0, (at(k + 1) - at(k)) / 2.0);
            for (x, w) in NODES {
                let (p, d) = curve.derivative_at(middle + reach * x);
                sum += p.perp_dot(d) * w * reach;
            }
        }
    }
    if to >= from { sum / 2.0 } else { -sum / 2.0 }
}
