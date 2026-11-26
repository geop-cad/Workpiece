//! Where a plane cuts a solid's boundary, followed directly: no block
//! is cut from the solid for it.
//!
//! Every edge is asked where it goes through the plane; a flat face is
//! then cut along the straight line between such places, a bent one by
//! walking its surface along the plane from one of them to the next —
//! and, where the plane cuts a closed curve out of the middle of a
//! face, around that. The points given are on the plane and on the
//! surface as closely as numbers go; between them the true curve is
//! within what was asked.
//!
//! Whatever is doubtful — the plane through a vertex, touching an edge
//! or a face, along a face — is not decided here: nothing is returned,
//! and whoever asked falls back to cutting the solid.

use workpiece_core::math::angle_between;
use workpiece_core::math::{Point2, Point3};

use crate::model::{Face, Solid, Walk};

/// A place where an edge of a face goes through the plane.
struct Crossing {
    /// The place.
    point: Point3,
    /// It, in the face's (u, v).
    uv: Point2,
    /// The way into the face from there, in (u, v): square to the
    /// edge's curve there, to the face's side of it.
    inward: Point2,
    /// Whether a cut already starts or ends there.
    used: bool,
}

/// The plane, and how fine the cut is followed.
struct Plane {
    origin: Point3,
    normal: Point3,
    /// The size of the solid.
    size: f64,
    /// How far the true curve may be from the line between two points
    /// given of it.
    sag: f64,
}

impl Plane {
    fn height(&self, p: Point3) -> f64 {
        (p - self.origin).dot(self.normal)
    }

    /// Below this, a height is not told from nothing.
    fn nothing(&self) -> f64 {
        self.size * 1e-9
    }
}

/// Where the plane through `origin` across `normal` cuts the boundary
/// of `solid`: lines of points, each from one edge of the solid to
/// another (or closed, its last point its first, where the cut stays
/// within one face). They join end to end into the closed outlines of
/// the cut. The true cut is within `sag` of them, and never shorter
/// than they are. `None` where that cannot be told here for sure: the
/// plane through a vertex, touching the solid, or lying in a face.
pub fn plane_section(
    solid: &Solid,
    origin: Point3,
    normal: Point3,
    sag: f64,
) -> Option<Vec<Vec<Point3>>> {
    let normal = normal.try_normalize()?;
    if solid.is_empty() || sag.is_nan() || sag <= 0.0 {
        return None;
    }
    let (lo, hi) = solid.extent();
    let plane = Plane {
        origin,
        normal,
        size: (hi - lo).length().max(1e-12),
        sag,
    };
    // Where each edge goes through the plane.
    let mut through = Vec::with_capacity(solid.edges.len());
    for edge in &solid.edges {
        through.push(roots(&plane, &edge.curve, edge.range)?);
    }
    let mut out = Vec::new();
    for (f, face) in solid.faces.iter().enumerate() {
        let mut crossings = Vec::new();
        for step in face.loops.iter().flatten() {
            let Walk::Edge { edge, reversed } = step.walk else {
                continue;
            };
            for &t in &through[edge] {
                let (uv, along) = step.pcurve.derivative_at(t);
                let along = if reversed { -along } else { along };
                crossings.push(Crossing {
                    point: solid.edges[edge].point(t),
                    uv,
                    inward: Point2::new(-along.y, along.x),
                    used: false,
                });
            }
        }
        if crossings.len() % 2 != 0 {
            return None;
        }
        match flat(face, &plane) {
            Some(across) => straight(&plane, across, &crossings, &mut out)?,
            None => bent(solid, f, &plane, &mut crossings, &mut out)?,
        }
    }
    Some(out)
}

/// The parameters within `range` at which `curve` goes through the
/// plane. `None` if it touches the plane, ends in it or lies in it.
fn roots(
    plane: &Plane,
    curve: &workpiece_core::world::topo::Curve3,
    range: (f64, f64),
) -> Option<Vec<f64>> {
    let nothing = plane.nothing();
    let height = |t: f64| plane.height(curve.point_at(t));
    let slope = |t: f64| curve.derivative_at(t).1.dot(plane.normal);
    let n = if curve.degree() == 1 && curve.control_points().len() == 2 {
        1
    } else {
        (curve.control_points().len() * 4).max(8)
    };
    let at = |k: usize| range.0 + (range.1 - range.0) * k as f64 / n as f64;
    let heights: Vec<f64> = (0..=n).map(|k| height(at(k))).collect();
    if heights.iter().any(|h| h.abs() <= nothing) {
        return None;
    }
    // Between two parameters the curve is on either side of the plane
    // at: where it goes through.
    let between = |mut a: f64, mut b: f64, mut ha: f64| {
        for _ in 0..200 {
            let m = (a + b) / 2.0;
            if m <= a || m >= b {
                break;
            }
            let hm = height(m);
            if (hm > 0.0) == (ha > 0.0) {
                (a, ha) = (m, hm);
            } else {
                b = m;
            }
        }
        (a + b) / 2.0
    };
    let mut found = Vec::new();
    for k in 0..n {
        let (a, b) = (heights[k], heights[k + 1]);
        if (a > 0.0) != (b > 0.0) {
            found.push(between(at(k), at(k + 1), a));
            continue;
        }
        // Coming nearer the plane and going away again within the
        // stretch, it may dip through it and back: where it is nearest.
        let (sa, sb) = (slope(at(k)), slope(at(k + 1)));
        let nearing = |h: f64, s: f64| (h > 0.0) == (s < 0.0);
        if n > 1 && nearing(a, sa) && !nearing(b, sb) {
            let (mut p, mut q) = (at(k), at(k + 1));
            for _ in 0..100 {
                let m = (p + q) / 2.0;
                if m <= p || m >= q {
                    break;
                }
                if nearing(a, slope(m)) {
                    p = m;
                } else {
                    q = m;
                }
            }
            let turn = (p + q) / 2.0;
            let h = height(turn);
            if h.abs() <= nothing {
                return None;
            }
            if (h > 0.0) != (a > 0.0) {
                found.push(between(at(k), turn, a));
                found.push(between(turn, at(k + 1), h));
            }
        }
    }
    Some(found)
}

/// The way across `face` if it is flat: its plane's normal.
fn flat(face: &Face, plane: &Plane) -> Option<Point3> {
    let points = face.surface.control_points();
    let first = *points.first()?;
    let mut widest = (0.0, Point3::ZERO);
    for a in points {
        for b in points {
            let across = (*a - first).cross(*b - first);
            if across.length_squared() > widest.0 {
                widest = (across.length_squared(), across);
            }
        }
        // (A few are enough to find three well apart.)
        if widest.0 > plane.size * plane.size * 1e-6 {
            break;
        }
    }
    let across = widest.1.try_normalize()?;
    let near = plane.size * 1e-11;
    points
        .iter()
        .all(|p| (*p - first).dot(across).abs() <= near)
        .then_some(across)
}

/// The cut of a flat face: straight from each place its border goes
/// through the plane to the next along the line the two planes share.
fn straight(
    plane: &Plane,
    across: Point3,
    crossings: &[Crossing],
    out: &mut Vec<Vec<Point3>>,
) -> Option<()> {
    if crossings.is_empty() {
        return Some(());
    }
    // (A face in the plane, or nearly: no one line.)
    let along = plane.normal.cross(across);
    if along.length() < 1e-6 {
        return None;
    }
    let along = along.normalize();
    let mut places: Vec<(f64, Point3)> = crossings
        .iter()
        .map(|c| (c.point.dot(along), c.point))
        .collect();
    places.sort_by(|a, b| a.0.total_cmp(&b.0));
    for pair in places.chunks(2) {
        if pair[1].0 - pair[0].0 <= plane.size * 1e-9 {
            return None;
        }
        out.push(vec![pair[0].1, pair[1].1]);
    }
    Some(())
}

/// A point of a bent face's cut being walked.
#[derive(Clone, Copy)]
struct At {
    uv: Point2,
    point: Point3,
    du: Point3,
    dv: Point3,
    /// The way the cut runs there, a length of one (either way of the
    /// two).
    way: Point3,
}

/// What walks a face's surface along the plane.
struct Walker<'a> {
    face: &'a Face,
    plane: &'a Plane,
    domain: ((f64, f64), (f64, f64)),
}

impl Walker<'_> {
    /// The surface at `uv`, if the cut runs one way there.
    fn at(&self, uv: Point2) -> Option<At> {
        let (point, du, dv) = self.face.surface.derivatives_at(uv.x, uv.y);
        let across = du.cross(dv);
        let way = self.plane.normal.cross(across);
        // (Where the surface lies along the plane, or comes to a point,
        // the cut runs no one way.)
        if way.length() <= 1e-7 * du.length().max(dv.length()).powi(2) {
            return None;
        }
        Some(At {
            uv,
            point,
            du,
            dv,
            way: way.normalize(),
        })
    }

    /// `uv` brought onto the plane: moved the shortest way in (u, v).
    fn onto(&self, mut uv: Point2) -> Option<At> {
        let ((u0, u1), (v0, v1)) = self.domain;
        for _ in 0..12 {
            uv = Point2::new(uv.x.clamp(u0, u1), uv.y.clamp(v0, v1));
            let at = self.at(uv)?;
            let h = self.plane.height(at.point);
            if h.abs() <= self.plane.size * 1e-14 {
                return Some(at);
            }
            let mut slope = Point2::new(at.du.dot(self.plane.normal), at.dv.dot(self.plane.normal));
            // (At a side of the surface, and wanted beyond it: along
            // the side.)
            let out = |at: f64, lo: f64, hi: f64, by: f64| {
                (at <= lo && by < 0.0) || (at >= hi && by > 0.0)
            };
            if out(uv.x, u0, u1, -slope.x * h) {
                slope.x = 0.0;
            }
            if out(uv.y, v0, v1, -slope.y * h) {
                slope.y = 0.0;
            }
            let steep = slope.length_squared();
            if steep <= 0.0 {
                return None;
            }
            uv -= slope * (h / steep);
        }
        let at = self.at(uv)?;
        (self.plane.height(at.point).abs() <= self.plane.size * 1e-11).then_some(at)
    }

    /// What `by`, a step in space along the surface, is in (u, v).
    fn step(&self, at: &At, by: Point3) -> Option<Point2> {
        let (a, b, c) = (at.du.dot(at.du), at.du.dot(at.dv), at.dv.dot(at.dv));
        let det = a * c - b * b;
        if det <= 1e-14 * a * c || det <= 0.0 {
            return None;
        }
        let (p, q) = (at.du.dot(by), at.dv.dot(by));
        Some(Point2::new((c * p - b * q) / det, (a * q - b * p) / det))
    }

    /// How far apart two places are in (u, v), each way as a share of
    /// all there is of it.
    fn apart(&self, a: Point2, b: Point2) -> f64 {
        let ((u0, u1), (v0, v1)) = self.domain;
        ((a.x - b.x).abs() / (u1 - u0)).max((a.y - b.y).abs() / (v1 - v0))
    }

    /// Walks the cut from `from` the way `sign` says, until it comes to
    /// one of `ends` (given as places in space and (u, v)): the points
    /// passed, without `from`'s own, and which end it came to.
    fn walk(
        &self,
        from: At,
        sign: f64,
        ends: &[(usize, Point3, Point2)],
    ) -> Option<(Vec<Point3>, usize)> {
        let plane = self.plane;
        let ((u0, u1), (v0, v1)) = self.domain;
        let slack = Point2::new(u1 - u0, v1 - v0) * 1e-9;
        let mut points = Vec::new();
        let mut here = from;
        let mut sign = sign;
        let mut h = plane.size / 64.0;
        let longest = plane.size / 6.0;
        for count in 0..100_000 {
            if count % 256 == 255 {
                workpiece_core::cancel::checkpoint();
            }
            let way = here.way * sign;
            // An end just ahead: the cut is there.
            let mut ahead: Option<(f64, usize, Point3)> = None;
            for &(k, point, uv) in ends {
                let to = point - here.point;
                let along = to.dot(way);
                if along <= plane.size * 1e-12 || along > 1.3 * h {
                    continue;
                }
                let aside = (to - way * along).length();
                if aside <= 0.5 * along && self.apart(uv, here.uv) <= 0.6 {
                    // (Not the place it has only just left.)
                    if count == 0 && to.length() <= plane.size * 1e-9 {
                        continue;
                    }
                    if ahead.is_none_or(|(least, ..)| along < least) {
                        ahead = Some((along, k, point));
                    }
                }
            }
            if let Some((_, k, point)) = ahead {
                points.push(point);
                return Some((points, k));
            }
            // A step on, as long as keeps the true cut near the line
            // to it.
            let mut next = None;
            for _ in 0..40 {
                let by = self.step(&here, way * h)?;
                let uv = here.uv + by;
                let outside = uv.x < u0 - slack.x
                    || uv.x > u1 + slack.x
                    || uv.y < v0 - slack.y
                    || uv.y > v1 + slack.y;
                if !outside && let Some(there) = self.onto(uv) {
                    let chord = there.point - here.point;
                    let length = chord.length();
                    // (It has to have gone on, about as far as meant.)
                    if length > 0.25 * h && length < 2.0 * h && chord.dot(way) > 0.0 {
                        let onward = if there.way.dot(way) < 0.0 { -1.0 } else { 1.0 };
                        let turned = angle_between(there.way * onward, way);
                        let bulge = length * turned / 8.0;
                        if bulge <= plane.sag {
                            next = Some((there, onward, bulge));
                            break;
                        }
                    }
                }
                h /= 2.0;
                if h < plane.size * 1e-9 {
                    return None;
                }
            }
            let (there, onward, bulge) = next?;
            // (`way` already has the sign in it: what is kept is the
            // sign that takes the new place's own way onward.)
            sign = onward;
            points.push(there.point);
            here = there;
            let wider = (0.5 * plane.sag / bulge.max(plane.sag * 1e-6)).sqrt();
            h = (h * wider.clamp(0.5, 2.0)).min(longest);
        }
        None
    }
}

/// The cut of a bent face: walked from each place its border goes
/// through the plane to another, and around what the plane cuts out of
/// its middle.
fn bent(
    solid: &Solid,
    f: usize,
    plane: &Plane,
    crossings: &mut [Crossing],
    out: &mut Vec<Vec<Point3>>,
) -> Option<()> {
    let face = &solid.faces[f];
    let walker = Walker {
        face,
        plane,
        domain: face.surface.domain(),
    };
    let first = out.len();
    while let Some(start) = crossings.iter().position(|c| !c.used) {
        crossings[start].used = true;
        let from = walker.onto(crossings[start].uv)?;
        // Into the face.
        let by = walker.step(&from, from.way)?;
        let into = by.dot(crossings[start].inward);
        if into.abs() <= 1e-6 * by.length() * crossings[start].inward.length() {
            return None;
        }
        let ends: Vec<(usize, Point3, Point2)> = crossings
            .iter()
            .enumerate()
            .filter(|(_, c)| !c.used)
            .map(|(k, c)| (k, c.point, c.uv))
            .collect();
        let (mut points, end) = walker.walk(from, into.signum(), &ends)?;
        crossings[end].used = true;
        points.insert(0, crossings[start].point);
        out.push(points);
    }
    // What the plane cuts out of the middle of the face: found where a
    // grid over the surface is on both sides of the plane, and where
    // the surface rises toward the plane between the grid's points.
    let ((u0, u1), (v0, v1)) = walker.domain;
    let spans = |knots: &[f64]| {
        let mut distinct = knots.to_vec();
        distinct.dedup();
        distinct.len().saturating_sub(1).max(1)
    };
    let (ku, kv) = face.surface.knots();
    let (nu, nv) = ((spans(ku) * 4).clamp(8, 32), (spans(kv) * 4).clamp(8, 32));
    let uv_at = |i: usize, j: usize| {
        Point2::new(
            u0 + (u1 - u0) * i as f64 / nu as f64,
            v0 + (v1 - v0) * j as f64 / nv as f64,
        )
    };
    let mut heights = vec![0.0; (nu + 1) * (nv + 1)];
    for i in 0..=nu {
        for j in 0..=nv {
            let uv = uv_at(i, j);
            heights[i * (nv + 1) + j] = plane.height(face.surface.point_at(uv.x, uv.y));
        }
    }
    let height = |i: usize, j: usize| heights[i * (nv + 1) + j];
    let mut seeds: Vec<Point2> = Vec::new();
    // (Where a side of the grid goes through the plane.)
    let mut through = |a: Point2, b: Point2, ha: f64, hb: f64| {
        if (ha > 0.0) != (hb > 0.0) {
            seeds.push(a + (b - a) * (ha / (ha - hb)));
        }
    };
    for i in 0..=nu {
        for j in 0..=nv {
            if i < nu {
                through(uv_at(i, j), uv_at(i + 1, j), height(i, j), height(i + 1, j));
            }
            if j < nv {
                through(uv_at(i, j), uv_at(i, j + 1), height(i, j), height(i, j + 1));
            }
        }
    }
    // (Where a point of the grid is nearer the plane than all around
    // it: the surface may reach through it between them.)
    for i in 1..nu {
        for j in 1..nv {
            let h = height(i, j);
            let around = [(i - 1, j), (i + 1, j), (i, j - 1), (i, j + 1)];
            let nearest = around
                .iter()
                .all(|&(a, b)| (height(a, b) > 0.0) == (h > 0.0) && height(a, b).abs() >= h.abs());
            if !nearest {
                continue;
            }
            let cell = Point2::new((u1 - u0) / nu as f64, (v1 - v0) / nv as f64);
            let (mut uv, mut best) = (uv_at(i, j), h);
            let mut reach = 0.5;
            for _ in 0..60 {
                let (_, du, dv) = face.surface.derivatives_at(uv.x, uv.y);
                let slope = Point2::new(du.dot(plane.normal), dv.dot(plane.normal));
                // (Toward the plane, in the grid's own measure.)
                let toward = Point2::new(slope.x * cell.x, slope.y * cell.y) * -h.signum();
                let Some(toward) = toward.try_normalize() else {
                    break;
                };
                let next = uv + Point2::new(toward.x * cell.x, toward.y * cell.y) * reach;
                let inside = (next.x - uv_at(i, j).x).abs() <= cell.x
                    && (next.y - uv_at(i, j).y).abs() <= cell.y;
                let there = plane.height(face.surface.point_at(next.x, next.y));
                if inside && there * h.signum() < best * h.signum() {
                    (uv, best) = (next, there);
                    reach *= 1.5;
                } else {
                    reach *= 0.5;
                    if reach < 1e-9 {
                        break;
                    }
                }
                if (best > 0.0) != (h > 0.0) {
                    break;
                }
            }
            if best.abs() <= plane.nothing() {
                // The surface touches the plane there, or all but.
                return None;
            }
            if (best > 0.0) != (h > 0.0) {
                let a = uv_at(i, j);
                seeds.push(a + (uv - a) * (h / (h - best)));
            }
        }
    }
    for seed in seeds {
        let Some(from) = walker.onto(seed) else {
            // (Not brought onto the plane from there: what it is a
            // seed of is not told.)
            return None;
        };
        // On a cut already walked?
        let near = 4.0 * plane.sag + plane.size * 1e-9;
        let walked = out[first..].iter().any(|line| {
            line.windows(2).any(|w| {
                let (a, b) = (w[0], w[1]);
                let along =
                    ((from.point - a).dot(b - a) / (b - a).length_squared()).clamp(0.0, 1.0);
                (a + (b - a) * along).distance(from.point) <= near
            })
        });
        if walked {
            continue;
        }
        // (Beside the face, where its surface runs on past its edges.)
        if !crate::lies_on(solid, f, from.point, plane.size * 1e-9) {
            continue;
        }
        let ends = [(0, from.point, from.uv)];
        let (mut points, _) = walker.walk(from, 1.0, &ends)?;
        if points.len() < 3 {
            return None;
        }
        points.insert(0, from.point);
        out.push(points);
    }
    Some(())
}
