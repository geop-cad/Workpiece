//! Curves fitted through points with known directions: the curve of an
//! edge in a face's (u, v), and what both need — where a point of space
//! is on a surface, and which way in (u, v) a direction of space goes.

use shapecraft_core::math::{Point2, Point3};
use shapecraft_core::world::topo::nurbs::{ControlPoint, NurbsCurve};
use shapecraft_core::world::topo::{Curve2, Curve3, NurbsSurface};

use crate::solve::{At, Over, closest};

/// The cubic through `nodes` — parameter, point and derivative of each,
/// parameters rising — with the derivatives it is given: between two
/// nodes a Hermite cubic, all of them one B-spline.
pub(crate) fn hermite<P: ControlPoint>(nodes: &[(f64, P, P)]) -> Option<NurbsCurve<P>> {
    let (first, last) = (nodes.first()?, nodes.last()?);
    if nodes.len() < 2 {
        return None;
    }
    let mut knots = vec![first.0; 4];
    let mut points = vec![first.1];
    for pair in nodes.windows(2) {
        let ((t0, p0, d0), (t1, p1, d1)) = (pair[0], pair[1]);
        let third = (t1 - t0) / 3.0;
        points.push(p0 + d0 * third);
        points.push(p1 + d1 * -third);
        knots.extend([t1, t1]);
    }
    points.push(last.1);
    knots.extend([last.0, last.0]);
    NurbsCurve::new(3, knots, points, None).ok()
}

/// The (u, v) direction that moves a point of `surface` at `uv` along
/// `d` (as nearly as the surface allows; where it collapses, the least
/// such).
pub(crate) fn pull(surface: &NurbsSurface, uv: Point2, d: Point3) -> Point2 {
    let (_, su, sv) = surface.derivatives_at(uv.x, uv.y);
    let (a, b, c) = (su.dot(su), su.dot(sv), sv.dot(sv));
    let (r, s) = (su.dot(d), sv.dot(d));
    let damp = (a + c) * 1e-14;
    let det = (a + damp) * (c + damp) - b * b;
    if det <= 0.0 || !det.is_finite() {
        return Point2::ZERO;
    }
    Point2::new(
        (r * (c + damp) - s * b) / det,
        (s * (a + damp) - r * b) / det,
    )
}

/// Where on `surface` the point nearest to `p` is, near `guess`.
pub(crate) fn foot(surface: &NurbsSurface, p: Point3, guess: Point2) -> (Point2, f64) {
    let (u, v) = surface.domain();
    let (_, y, _, q) = closest(&At(p), &Over(surface, [u, v]), [0.0; 2], guess.to_array());
    (Point2::from_array(y), q.distance(p))
}

/// A curve in the (u, v) of a surface, with how far its image in space
/// strays from what it stands for.
pub(crate) struct Fitted {
    /// The curve.
    pub curve: Curve2,
    /// The largest distance seen between its image and the curve in
    /// space.
    pub off: f64,
}

/// The curve in (u, v) that `at` describes over `[t0, t1]`: `at(t, near)`
/// gives the (u, v) at `t` (looked for near `near`, the (u, v) of a
/// neighbour) and its derivative. Nodes are added until the fit is
/// within `target`, on the surface, of what `at` gives between them (as
/// far as that can be had). `space(t)` is the curve in space the fit
/// stands for; how far its image strays from that is reported.
pub(crate) fn fit(
    surface: &NurbsSurface,
    space: &dyn Fn(f64) -> Point3,
    at: &dyn Fn(f64, Point2) -> (Point2, Point2),
    seed: (f64, Point2),
    (t0, t1): (f64, f64),
    breaks: &[f64],
    target: f64,
) -> Option<Fitted> {
    // The first nodes: the ends, the curve's own breaks, and a few
    // between — more are added where the fit needs them — found
    // outwards from the seed.
    let mut ts = vec![t0, t1];
    ts.extend(breaks.iter().copied().filter(|&t| t > t0 && t < t1));
    ts.sort_by(f64::total_cmp);
    ts.dedup_by(|a, b| (*a - *b).abs() <= 1e-12 * (t1 - t0));
    let per = (8 / (ts.len() - 1)).clamp(1, 4);
    let mut first: Vec<f64> = Vec::new();
    for pair in ts.windows(2) {
        for k in 0..per {
            first.push(pair[0] + (pair[1] - pair[0]) * k as f64 / per as f64);
        }
    }
    first.push(t1);
    let from = first
        .iter()
        .enumerate()
        .min_by(|a, b| (a.1 - seed.0).abs().total_cmp(&(b.1 - seed.0).abs()))?
        .0;
    let mut nodes: Vec<(f64, Point2, Point2)> =
        vec![(0.0, Point2::ZERO, Point2::ZERO); first.len()];
    // From node to node: each foot is looked for from the one before.
    // Where that loses the curve (on a surface that waves, the foot of
    // a point a long way on is not found from here), in short steps
    // instead, each looked for from where the last two say it will be.
    const STEPS: usize = 32;
    let gap = |t: f64, uv: Point2| surface.point_at(uv.x, uv.y).distance(space(t));
    let ((u0, u1), (v0, v1)) = surface.domain();
    let (low, high) = (Point2::new(u0, v0), Point2::new(u1, v1));
    let walk = |from: f64, to: f64, start: Point2| {
        let begun = gap(from, start);
        let whole = at(to, start);
        if gap(to, whole.0) <= begun * 16.0 + target * 1e3 {
            return whole;
        }
        let (mut before, mut near, mut off) = (start, start, begun);
        let mut found = whole;
        for i in 1..=STEPS {
            let t = from + (to - from) * i as f64 / STEPS as f64;
            let ahead = (near + (near - before)).clamp(low, high);
            found = at(t, ahead);
            let mut now = gap(t, found.0);
            // (Further off than it was: from the last foot itself.)
            if now.is_nan() || now > off * 4.0 + target {
                let again = at(t, near);
                let other = gap(t, again.0);
                if other < now || !now.is_finite() {
                    (found, now) = (again, other);
                }
            }
            (before, near, off) = (near, found.0, now);
        }
        found
    };
    let (uv, d) = at(first[from], seed.1);
    nodes[from] = (first[from], uv, d);
    for k in from + 1..first.len() {
        let (uv, d) = walk(first[k - 1], first[k], nodes[k - 1].1);
        nodes[k] = (first[k], uv, d);
    }
    for k in (0..from).rev() {
        let (uv, d) = walk(first[k + 1], first[k], nodes[k + 1].1);
        nodes[k] = (first[k], uv, d);
    }
    let on = |uv: Point2| surface.point_at(uv.x, uv.y);
    let mut curve = hermite(&nodes)?;
    // A curve that is not quite on the surface has feet that the
    // directions given do not quite follow; nothing is gained by
    // fitting them closer than a small part of how far off it is.
    let apart = nodes
        .iter()
        .map(|n| on(n.1).distance(space(n.0)))
        .fold(0.0, f64::max);
    // And the directions are then taken from the feet themselves, node
    // to node (but at the curve's own breaks): with directions the feet
    // do not follow, the fit closes in on them only a node at a time.
    let loose = apart > target * 100.0;
    let target = target.max(apart * 0.1);
    // How far the fit is, on the surface, from where it should be: its
    // image against the foot of the curve's point. (A point as near
    // the surface as the fit comes at its end has its foot straight
    // down the surface's normal there: what of the way between the two
    // lies along the surface is how far apart they are, with no foot
    // looked for.)
    let astray = |curve: &Curve2, t: f64| {
        let uv = curve.point_at(t);
        if !loose {
            let (s, su, sv) = surface.derivatives_at(uv.x, uv.y);
            let off = s - space(t);
            let n = su.cross(sv);
            if off.length() <= target * 64.0 && n.length() > 1e-6 * su.length() * sv.length() {
                return off.reject_from(n).length();
            }
        }
        on(uv).distance(on(at(t, uv).0))
    };
    let follow = |nodes: &mut Vec<(f64, Point2, Point2)>| {
        if !loose {
            return;
        }
        for i in 1..nodes.len() - 1 {
            if ts.iter().any(|t| *t == nodes[i].0) {
                continue;
            }
            let (h0, h1) = (nodes[i].0 - nodes[i - 1].0, nodes[i + 1].0 - nodes[i].0);
            let before = (nodes[i].1 - nodes[i - 1].1) / h0;
            let after = (nodes[i + 1].1 - nodes[i].1) / h1;
            nodes[i].2 = before * (h1 / (h0 + h1)) + after * (h0 / (h0 + h1));
        }
    };
    if loose {
        follow(&mut nodes);
        curve = hermite(&nodes)?;
    }
    // The nearest each stretch has been to its curve, and how many
    // times running it was then halved for nothing, by where it starts:
    // a stretch that is no nearer for having been halved twice over (the
    // feet themselves are no smoother than that, where a curve runs
    // along the surface's edge or off it) is left as it is.
    let mut was: Vec<(f64, f64, u8)> = Vec::new();
    // The stretches found near enough, by where they start: between two
    // nodes the fit is theirs alone, so they stay so (unless the
    // directions are taken from the neighbours, which change).
    let mut settled: std::collections::HashSet<u64> = std::collections::HashSet::new();
    for _ in 0..12 {
        let mut more = Vec::new();
        let mut now: Vec<(f64, f64, u8)> = Vec::new();
        for pair in nodes.windows(2) {
            let (a, b) = (pair[0].0, pair[1].0);
            if !loose && settled.contains(&a.to_bits()) {
                now.push((a, 0.0, 0));
                continue;
            }
            // (A fit whose image is within half the target of the curve
            // in space is within the target of the curve's feet: no
            // foot need be looked for to know the stretch is near
            // enough.)
            let near = !loose
                && [0.25, 0.5, 0.75].into_iter().all(|f| {
                    let t = a + (b - a) * f;
                    on(curve.point_at(t)).distance(space(t)) <= target * 0.5
                });
            if near {
                now.push((a, 0.0, 0));
                settled.insert(a.to_bits());
                continue;
            }
            let worst = [0.25, 0.5, 0.75]
                .into_iter()
                .map(|f| astray(&curve, a + (b - a) * f))
                .fold(0.0, f64::max);
            // The stretch this one is a half of.
            let (best, idle) = was
                .iter()
                .rev()
                .find(|w| w.0 <= a)
                .map_or((f64::INFINITY, 0), |w| (w.1, w.2));
            let idle = if worst < best * 0.9 { 0 } else { idle + 1 };
            if worst > target && idle < 2 && (b - a) > 1e-9 * (t1 - t0) {
                let t = (a + b) / 2.0;
                let (uv, d) = at(t, (pair[0].1 + pair[1].1) / 2.0);
                more.push((t, uv, d));
                let best = best.min(worst);
                now.extend([(a, best, idle), (t, best, idle)]);
            } else {
                now.push((a, 0.0, 0));
                settled.insert(a.to_bits());
            }
        }
        if more.is_empty() || nodes.len() > 4096 {
            break;
        }
        was = now;
        nodes.extend(more);
        nodes.sort_by(|a, b| a.0.total_cmp(&b.0));
        follow(&mut nodes);
        curve = hermite(&nodes)?;
    }
    let mut off = 0.0_f64;
    for pair in nodes.windows(2) {
        for f in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let t = pair[0].0 + (pair[1].0 - pair[0].0) * f;
            off = off.max(on(curve.point_at(t)).distance(space(t)));
        }
    }
    Some(Fitted { curve, off })
}

/// The parameters at which `curve` may have a corner: its knots as
/// often repeated as its degree.
fn corners(curve: &Curve3) -> Vec<f64> {
    let (p, knots) = (curve.degree(), curve.knots());
    let mut out = Vec::new();
    let mut k = 0;
    while k < knots.len() {
        let same = knots[k..].iter().take_while(|x| **x == knots[k]).count();
        if same >= p {
            out.push(knots[k]);
        }
        k += same;
    }
    out
}

/// The curve in the (u, v) of `surface` under `curve` over `range`,
/// which lies on the surface: at `seed` (a parameter and its (u, v)) and
/// outwards from there, each point's foot on the surface.
pub(crate) fn project(
    surface: &NurbsSurface,
    curve: &Curve3,
    range: (f64, f64),
    seed: (f64, Point2),
    target: f64,
) -> Option<Fitted> {
    if let Some(exact) = on_flat(surface, curve, range) {
        return Some(exact);
    }
    let end = |t: f64| foot(surface, curve.point_at(t), seed.1).0;
    if let Some(line) = straight(surface, curve, range, end(range.0), end(range.1), target) {
        return Some(line);
    }
    let at = |t: f64, near: Point2| {
        let (p, d) = curve.derivative_at(t);
        let (mut uv, gap) = foot(surface, p, near);
        // (A search that ends on a point the surface collapses to, away
        // from the point looked for, was caught there: every way along
        // the collapsed side is level. From the nearest of a grid of
        // places about where it was looked for instead.)
        let collapsed = |uv: Point2| {
            let (_, su, sv) = surface.derivatives_at(uv.x, uv.y);
            let size = su.length().max(sv.length());
            su.length() <= 1e-9 * size || sv.length() <= 1e-9 * size
        };
        if gap > target * 1e3 && collapsed(uv) {
            // (About where the curve's neighbour is: the curve goes on
            // from there, not from the far side of a seam.)
            let ((u0, u1), (v0, v1)) = surface.domain();
            let reach = Point2::new(u1 - u0, v1 - v0) * 0.2;
            let (lo, hi) = (
                (near - reach).max(Point2::new(u0, v0)),
                (near + reach).min(Point2::new(u1, v1)),
            );
            const GRID: usize = 16;
            let mut best = (f64::INFINITY, uv);
            for i in 0..GRID {
                for j in 0..GRID {
                    let q = Point2::new(
                        lo.x + (hi.x - lo.x) * (i as f64 + 0.5) / GRID as f64,
                        lo.y + (hi.y - lo.y) * (j as f64 + 0.5) / GRID as f64,
                    );
                    let far = surface.point_at(q.x, q.y).distance(p);
                    if far < best.0 {
                        best = (far, q);
                    }
                }
            }
            let (again, off) = foot(surface, p, best.1);
            // (Much nearer, or the first was no worse a foot: at a
            // point the surface does collapse to, any place along the
            // collapsed side is as near, and the one found from the
            // curve's neighbour is the one that goes on from it.)
            if off < gap * 1e-3 {
                uv = again;
            }
        }
        (uv, pull(surface, uv, d))
    };
    fit(
        surface,
        &|t| curve.point_at(t),
        &at,
        seed,
        range,
        &corners(curve),
        target,
    )
}

/// The straight line in (u, v) from `from` to `to`, with the parameters
/// of `curve` over `range`, if `curve` is that line's image on
/// `surface` to within `target`: a circle around a cylinder is one, and
/// a line along it.
pub(crate) fn straight(
    surface: &NurbsSurface,
    curve: &Curve3,
    range: (f64, f64),
    from: Point2,
    to: Point2,
    target: f64,
) -> Option<Fitted> {
    let line = Curve2::new(
        1,
        vec![range.0, range.0, range.1, range.1],
        vec![from, to],
        None,
    )
    .ok()?;
    // Along one of the surface's own lines (u or v the same all along)
    // it is that line. Any other way, only where the surface is no
    // narrower one way than the other: near a point it comes to, places
    // far apart in (u, v) are close in space, and a line that is close
    // enough in space may cross its neighbours in (u, v).
    let ((u0, u1), (v0, v1)) = surface.domain();
    let own =
        (from.x - to.x).abs() <= 1e-12 * (u1 - u0) || (from.y - to.y).abs() <= 1e-12 * (v1 - v0);
    let mut off = 0.0_f64;
    let (mut least, mut most) = (f64::INFINITY, 0.0_f64);
    for k in 0..=24 {
        let t = range.0 + (range.1 - range.0) * f64::from(k) / 24.0;
        let uv = line.point_at(t);
        let (p, du, dv) = surface.derivatives_at(uv.x, uv.y);
        off = off.max(p.distance(curve.point_at(t)));
        if off > target {
            return None;
        }
        for d in [du.length() * (u1 - u0), dv.length() * (v1 - v0)] {
            (least, most) = (least.min(d), most.max(d));
        }
    }
    if !own && least <= most * 1e-3 {
        return None;
    }
    Some(Fitted { curve: line, off })
}

/// The curve in the (u, v) of `surface` under `curve` over `range`, for
/// a surface that is a flat parallelogram, even in (u, v): there every
/// point's (u, v) is its place along the two sides, so the curve's own
/// control points, weights and knots give the curve in (u, v) exactly.
/// `None` for any other surface.
pub(crate) fn on_flat(surface: &NurbsSurface, curve: &Curve3, range: (f64, f64)) -> Option<Fitted> {
    if surface.degrees() != (1, 1) || surface.counts() != (2, 2) {
        return None;
    }
    if surface.weights().iter().any(|w| *w != 1.0) {
        return None;
    }
    let c = surface.control_points();
    // (Along u first: c[0], c[1] are at the first u; c[2], c[3] at the last.)
    let (origin, a, b) = (c[0], c[2] - c[0], c[1] - c[0]);
    let size = a.length().max(b.length());
    if (c[3] - (c[0] + a + b)).length() > size * 1e-14 {
        return None;
    }
    let (aa, ab, bb) = (a.dot(a), a.dot(b), b.dot(b));
    let det = aa * bb - ab * ab;
    if det <= aa * bb * 1e-12 {
        return None;
    }
    let ((u0, u1), (v0, v1)) = surface.domain();
    let place = |p: Point3| {
        let d = p - origin;
        let (da, db) = (d.dot(a), d.dot(b));
        Point2::new(
            u0 + (u1 - u0) * (da * bb - db * ab) / det,
            v0 + (v1 - v0) * (db * aa - da * ab) / det,
        )
    };
    let flat = Curve2::new(
        curve.degree(),
        curve.knots().to_vec(),
        curve.control_points().iter().map(|p| place(*p)).collect(),
        Some(curve.weights().to_vec()),
    )
    .ok()?
    .trimmed(range.0, range.1)
    .ok()?;
    let off = (0..=16)
        .map(|k| {
            let t = range.0 + (range.1 - range.0) * f64::from(k) / 16.0;
            let uv = flat.point_at(t);
            surface.point_at(uv.x, uv.y).distance(curve.point_at(t))
        })
        .fold(0.0, f64::max);
    Some(Fitted { curve: flat, off })
}

/// The curve `pcurve` (the image in the (u, v) of `surface` of `other`
/// over `other_range`) with the parameters of `curve` over `range`
/// instead: the two curves are the same in space. `flipped` says they
/// run opposite ways.
#[allow(clippy::too_many_arguments)]
pub(crate) fn reparametrize(
    surface: &NurbsSurface,
    pcurve: &Curve2,
    other: &Curve3,
    other_range: (f64, f64),
    curve: &Curve3,
    range: (f64, f64),
    flipped: bool,
    target: f64,
) -> Option<Fitted> {
    // Where on `other` the point of `curve` at `t` is: by Newton from a
    // straight guess.
    let guess = |t: f64| {
        let f = (t - range.0) / (range.1 - range.0);
        let f = if flipped { 1.0 - f } else { f };
        other_range.0 + (other_range.1 - other_range.0) * f
    };
    // Exactly, where that is plain: on a flat face, or where the curve
    // is a straight line in (u, v).
    if let Some(exact) = on_flat(surface, curve, range).filter(|f| f.off <= target) {
        return Some(exact);
    }
    let (from, to) = (
        pcurve.point_at(guess(range.0)),
        pcurve.point_at(guess(range.1)),
    );
    if let Some(line) = straight(surface, curve, range, from, to, target) {
        return Some(line);
    }
    let on_other = |t: f64| {
        let (p, d) = curve.derivative_at(t);
        let mut s = guess(t);
        for _ in 0..40 {
            let (q, dq) = other.derivative_at(s);
            let step = (p - q).dot(dq) / dq.length_squared().max(f64::MIN_POSITIVE);
            let next = (s + step).clamp(other_range.0, other_range.1);
            if (next - s).abs() <= 1e-15 * (1.0 + s.abs()) {
                s = next;
                break;
            }
            s = next;
        }
        let (_, dq) = other.derivative_at(s);
        // How fast `s` moves with `t`.
        (s, d.dot(dq) / dq.length_squared().max(f64::MIN_POSITIVE))
    };
    let at = |t: f64, _: Point2| {
        let (s, speed) = on_other(t);
        let (uv, d) = pcurve.derivative_at(s);
        (uv, d * speed)
    };
    fit(
        surface,
        &|t| curve.point_at(t),
        &at,
        (range.0, Point2::ZERO),
        range,
        &corners(curve),
        target,
    )
}
