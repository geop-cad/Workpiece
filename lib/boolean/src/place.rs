//! Where points lie against a solid: inside it, on its boundary, or
//! outside. What is searched of the solid is worked out once, for as
//! many points as are asked about.

use std::borrow::Cow;

use shapecraft_core::math::Point3;

use crate::model::Solid;
use crate::query::{self, SolidData};

/// Where a point lies against a solid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// In the solid, clear of its boundary.
    In,
    /// On its boundary: on a face, an edge or a vertex.
    On,
    /// Outside it.
    Out,
}

/// A solid made ready to be asked where points lie against it.
pub struct Placer<'a> {
    /// The solid; one far from the origin, brought to it (see
    /// [`recentred`]).
    solid: Cow<'a, Solid>,
    /// How far that was brought: what a point asked about is moved by,
    /// and a point answered moved back by.
    shift: Point3,
    data: SolidData,
    all: Vec<usize>,
    /// Cells around the solid that are clear of its boundary, for the
    /// nearness they were laid for: laid when many points are placed
    /// at once.
    grid: std::sync::OnceLock<(f64, Option<crate::grid::Grid>)>,
    /// The places the nearest point of the boundary is looked for
    /// from: points along each edge, and points over each face.
    starts: std::sync::OnceLock<Starts>,
}

/// Points along each edge, and points over each face.
type Starts = (Vec<Vec<Point3>>, Vec<Vec<Point3>>);

/// How many steps along an edge, and each way over a face, the nearest
/// point is looked for from.
const STARTS: usize = 16;

/// How far to move what lies within `lo` and `hi` to have it about the
/// origin, if it is far from it for its size: coordinates are kept to a
/// part in 1e16 of how large they are, and what is searched and solved
/// here is settled to a part in 1e11 or less of the solid's size —
/// which a solid a thousand sizes from the origin no longer has.
pub(crate) fn recentred(solids: &[&Solid]) -> Option<Point3> {
    // (The box of the surfaces' control points: no smaller than the
    // solids.)
    let (mut lo, mut hi) = (Point3::INFINITY, Point3::NEG_INFINITY);
    for face in solids.iter().flat_map(|s| &s.faces) {
        for p in face.surface.control_points() {
            (lo, hi) = (lo.min(*p), hi.max(*p));
        }
    }
    let middle = (lo + hi) / 2.0;
    let size = lo.distance(hi);
    (middle.is_finite() && middle.abs().max_element() > size * 4.0).then_some(middle)
}

impl<'a> Placer<'a> {
    /// What answers for `solid`, holding it: kept for as long as there
    /// are questions about the solid, so that what is made ready for
    /// them is made once.
    pub fn holding(solid: Solid) -> Placer<'static> {
        let near = recentred(&[&solid]).and_then(|by| Some((solid.moved(&|p| p - by).ok()?, by)));
        let (solid, shift) = near.unwrap_or((solid, Point3::ZERO));
        Placer {
            data: SolidData::new(&solid),
            all: (0..solid.faces.len()).collect(),
            solid: Cow::Owned(solid),
            shift,
            grid: std::sync::OnceLock::new(),
            starts: std::sync::OnceLock::new(),
        }
    }

    /// `solid`, ready for [`side`](Self::side).
    pub fn new(solid: &'a Solid) -> Self {
        let near = recentred(&[solid]).and_then(|by| Some((solid.moved(&|p| p - by).ok()?, by)));
        let (solid, shift) = match near {
            Some((moved, by)) => (Cow::Owned(moved), by),
            None => (Cow::Borrowed(solid), Point3::ZERO),
        };
        Self {
            data: SolidData::new(&solid),
            all: (0..solid.faces.len()).collect(),
            solid,
            shift,
            grid: std::sync::OnceLock::new(),
            starts: std::sync::OnceLock::new(),
        }
    }

    /// Where `p` lies: on the boundary if it is within `tol` of a face
    /// (inside its loops, or on them), else inside or outside. `None`
    /// if that cannot be told: every ray tried from the point grazes a
    /// face or passes too near an edge.
    pub fn side(&self, p: Point3, tol: f64) -> Option<Side> {
        self.side_here(p - self.shift, tol)
    }

    /// Where each of `points` lies, as [`side`](Self::side) tells it
    /// of one. Many points are told faster than one by one: cells
    /// around the solid that are clear of its boundary are found once,
    /// and a point in one of them is where the cell is; any other is
    /// told from a clear cell nearby, by what the short way there
    /// crosses.
    pub fn sides(&self, points: &[Point3], tol: f64) -> Vec<Option<Side>> {
        /// Fewer points than this are told one by one.
        const MANY: usize = 256;
        if points.len() < MANY || self.solid.is_empty() {
            return points.iter().map(|&p| self.side(p, tol)).collect();
        }
        // (What is on a face is within the nearness asked for, or the
        // face's own if that is more.)
        let pad = self
            .solid
            .faces
            .iter()
            .map(|f| f.tolerance)
            .fold(tol, f64::max);
        let (laid_for, grid) = self.grid.get_or_init(|| {
            let inside = |p: Point3| query::inside(&self.solid, &self.data, &self.all, p);
            (pad, crate::grid::Grid::new(&self.data, pad * 1.01, &inside))
        });
        let Some(grid) = grid.as_ref().filter(|_| *laid_for >= pad) else {
            return points.iter().map(|&p| self.side(p, tol)).collect();
        };
        points
            .iter()
            .enumerate()
            .map(|(k, &p)| {
                if k % 256 == 0 {
                    shapecraft_core::cancel::checkpoint();
                }
                let p = p - self.shift;
                if !self.data.bounds.holds(p, tol) {
                    return Some(Side::Out);
                }
                if let Some(inside) = grid.holds(p) {
                    return Some(if inside { Side::In } else { Side::Out });
                }
                if self.on_here(p, tol) {
                    return Some(Side::On);
                }
                // From a clear cell nearby: on the other side of the
                // boundary for every face crossed on the way.
                for (clear, inside) in grid.clear_near(p) {
                    let crossed =
                        query::crossings_between(&self.solid, &self.data, &self.all, p, clear);
                    if let Some(n) = crossed {
                        let inside = inside ^ (n % 2 == 1);
                        return Some(if inside { Side::In } else { Side::Out });
                    }
                }
                query::inside(&self.solid, &self.data, &self.all, p)
                    .map(|inside| if inside { Side::In } else { Side::Out })
            })
            .collect()
    }

    /// Whether `p` is within `tol` of a face: inside its loops, or on
    /// them.
    fn on_here(&self, p: Point3, tol: f64) -> bool {
        self.solid
            .faces
            .iter()
            .zip(&self.data.faces)
            .any(|(face, data)| query::on_face(face, data, p, tol.max(face.tolerance)).is_some())
    }

    /// The least box with sides along the axes that holds the solid:
    /// its low and high corners, `None` for an empty solid.
    pub fn bounds(&self) -> Option<(Point3, Point3)> {
        let (lo, hi) = self.bounds_here()?;
        Some((lo + self.shift, hi + self.shift))
    }

    /// The point of the solid nearest `p`: `p` itself if the solid
    /// holds it, else the nearest point of its boundary. `None` if the
    /// solid is empty or `p` cannot be placed against it.
    pub fn nearest(&self, p: Point3, tol: f64) -> Option<Point3> {
        Some(self.nearest_here(p - self.shift, tol)? + self.shift)
    }

    /// The point of the solid's boundary nearest `p`, wherever `p` is;
    /// `None` for an empty solid.
    pub fn nearest_on_boundary(&self, p: Point3) -> Option<Point3> {
        Some(self.nearest_on_boundary_here(p - self.shift)? + self.shift)
    }

    /// How far from `origin` the ray along `direction` meets each of
    /// the solid's faces (a point on an edge once for each face there).
    /// `None` for no direction, or if a meeting could not be looked for.
    pub fn meets(&self, origin: Point3, direction: Point3) -> Option<Vec<f64>> {
        if self.solid.is_empty() {
            return Some(Vec::new());
        }
        let d = direction.try_normalize()?;
        query::meetings(&self.solid, &self.data, origin - self.shift, d)
    }

    /// The principal curvatures of the boundary at its point nearest
    /// `p` (see `curvature_here`).
    pub fn curvature(&self, p: Point3, tol: f64) -> Option<[f64; 2]> {
        self.curvature_here(p - self.shift, tol)
    }

    /// The boundary's outward normal at `p` (see `normal_here`).
    pub fn normal(&self, p: Point3, tol: f64) -> Option<Point3> {
        self.normal_here(p - self.shift, tol)
    }

    /// Where `p` lies: on the boundary if it is within `tol` of a face
    /// (inside its loops, or on them), else inside or outside. `None`
    /// if that cannot be told: every ray tried from the point grazes a
    /// face or passes too near an edge.
    fn side_here(&self, p: Point3, tol: f64) -> Option<Side> {
        if self.solid.is_empty() || !self.data.bounds.holds(p, tol) {
            return Some(Side::Out);
        }
        if self.on_here(p, tol) {
            return Some(Side::On);
        }
        query::inside(&self.solid, &self.data, &self.all, p)
            .map(|inside| if inside { Side::In } else { Side::Out })
    }

    /// The least box with sides along the axes that holds the solid:
    /// its low and high corners, `None` for an empty solid. Along each
    /// axis the solid reaches farthest at a vertex, on an edge or where
    /// a face lies square to the axis: every edge and every face is
    /// searched for those places (from a grid of starts, each followed
    /// to where it is farthest), and a place on a face counts if it is
    /// within the face's loops.
    fn bounds_here(&self) -> Option<(Point3, Point3)> {
        if self.solid.is_empty() {
            return None;
        }
        let mut lo = Point3::INFINITY;
        let mut hi = Point3::NEG_INFINITY;
        for v in &self.solid.vertices {
            lo = lo.min(v.point);
            hi = hi.max(v.point);
        }
        const STARTS: usize = 16;
        use shapecraft_core::math::Point2;
        for axis in 0..3 {
            for sign in [1.0, -1.0] {
                // (What the vertices reach is known: an edge or a face
                // is looked at only if it may reach farther — its
                // control points do, which it lies among.)
                let mut best = if sign > 0.0 { hi[axis] } else { -lo[axis] };
                let slack = |best: f64| best + 1e-13 * (1.0 + best.abs());
                for edge in &self.solid.edges {
                    let (a, b) = edge.range;
                    let curve = &edge.curve;
                    // A straight edge reaches no farther than its ends.
                    if curve.degree() <= 1 && !curve.is_rational() {
                        continue;
                    }
                    let positive = curve.weights().iter().all(|w| *w > 0.0);
                    let most = curve
                        .control_points()
                        .iter()
                        .map(|p| sign * p[axis])
                        .fold(f64::NEG_INFINITY, f64::max);
                    if positive && most <= slack(best) {
                        continue;
                    }
                    // Where it runs level along the axis, piece by
                    // piece: the roots of a polynomial.
                    if let Some(most) = farthest(curve, (a, b), axis, sign, slack(best)) {
                        best = best.max(most);
                        continue;
                    }
                    let reach = |t: f64| sign * edge.point(t.clamp(a, b))[axis];
                    let step = (b - a) / STARTS as f64;
                    // Where the edge runs level along the axis, by
                    // Newton's steps: the place the search below comes
                    // to, sooner.
                    let rise = |t: f64| sign * curve.derivative_at(t).1[axis];
                    let level = |from: f64| -> Option<f64> {
                        let mut t = from;
                        let mut g = rise(t);
                        for _ in 0..16 {
                            let e = (b - a) * 1e-6;
                            let bend = (rise((t + e).clamp(a.min(b), a.max(b))) - g) / e;
                            if bend == 0.0 || bend.is_nan() {
                                return None;
                            }
                            let next = t - g / bend;
                            if !(next > a.min(b) && next < a.max(b)) {
                                return None;
                            }
                            let moved = ((next - t) / (b - a)).abs();
                            t = next;
                            g = rise(t);
                            if moved <= 1e-13 {
                                return (bend < 0.0).then_some(t);
                            }
                        }
                        None
                    };
                    for k in 0..=STARTS {
                        let (mut t, mut h) = (a + step * k as f64, step);
                        let climb = |t: &mut f64, h: &mut f64, fine: f64| {
                            while *h > (b - a) * fine {
                                let here = reach(*t);
                                let next = [*t - *h, *t + *h]
                                    .into_iter()
                                    .map(|s| s.clamp(a, b))
                                    .find(|s| reach(*s) > here);
                                match next {
                                    Some(s) => *t = s,
                                    None => *h /= 2.0,
                                }
                            }
                        };
                        // Roughly first: at an end of the edge it is a
                        // vertex, which is known; else from near where
                        // it runs level, to there.
                        climb(&mut t, &mut h, 1e-4);
                        if t == a || t == b {
                            continue;
                        }
                        if let Some(s) = level(t)
                            && reach(s) >= reach(t)
                        {
                            (t, h) = (s, (b - a) * 1e-12);
                        }
                        climb(&mut t, &mut h, 1e-15);
                        best = best.max(reach(t));
                    }
                }
                for (face, data) in self.solid.faces.iter().zip(&self.data.faces) {
                    let [u, v] = data.uv;
                    let surface = &face.surface;
                    // A flat face reaches no farther than its edges.
                    let flat = surface.degrees() == (1, 1)
                        && surface.weights().iter().all(|w| *w == 1.0)
                        && {
                            let p = surface.control_points();
                            p.len() == 4
                                && (p[3] - p[2] - p[1] + p[0]).length()
                                    <= 1e-12 * p[0].distance(p[3])
                        };
                    if flat {
                        continue;
                    }
                    // Nor does one that runs straight along one of its
                    // parameters (a cylinder's wall, a cone's, what a
                    // curve sweeps going straight): along each of its
                    // lines it is farthest at an end, on an edge.
                    let ((pu, pv), (nu, nv)) = (surface.degrees(), surface.counts());
                    if (pu == 1 && nu == 2) || (pv == 1 && nv == 2) {
                        continue;
                    }
                    let clamp = |p: Point2| Point2::new(p.x.clamp(u.0, u.1), p.y.clamp(v.0, v.1));
                    let reach = |p: Point2| sign * surface.point_at(p.x, p.y)[axis];
                    let size = Point2::new(u.1 - u.0, v.1 - v.0);
                    for patch in &data.patches {
                        let positive = patch.cp.iter().all(|h| h[3] > 0.0);
                        let bounds = patch.bounds();
                        let most = if sign > 0.0 {
                            bounds.hi[axis]
                        } else {
                            -bounds.lo[axis]
                        };
                        if positive && most <= slack(best) {
                            continue;
                        }
                        // From places all over the patch, each followed
                        // to where it is farthest.
                        // (As many as the patch may have humps: by its
                        // degree.)
                        let over = patch.n[0].max(patch.n[1]).saturating_sub(1).clamp(2, 6);
                        let [(p0, p1), (q0, q1)] = patch.range;
                        // Where the surface is level along the axis, by
                        // Newton's steps on how it rises: the same
                        // place the search below comes to, sooner.
                        let rise = |p: Point2| {
                            let (_, du, dv) = surface.derivatives_at(p.x, p.y);
                            Point2::new(sign * du[axis], sign * dv[axis])
                        };
                        let level = |from: Point2| -> Option<Point2> {
                            let mut p = from;
                            let mut g = rise(p);
                            for _ in 0..16 {
                                let e = size * 1e-6;
                                let (gu, gv) = (
                                    (rise(clamp(p + Point2::new(e.x, 0.0))) - g) / e.x,
                                    (rise(clamp(p + Point2::new(0.0, e.y))) - g) / e.y,
                                );
                                let det = gu.x * gv.y - gv.x * gu.y;
                                if det == 0.0 || det.is_nan() {
                                    return None;
                                }
                                let step = Point2::new(
                                    (-g.x * gv.y + g.y * gv.x) / det,
                                    (-g.y * gu.x + g.x * gu.y) / det,
                                );
                                let next = p + step;
                                // (Not out of the patch's own parameters:
                                // what is farthest at their rim is left
                                // to the search.)
                                if !(next.x > u.0 && next.x < u.1 && next.y > v.0 && next.y < v.1) {
                                    return None;
                                }
                                let moved = (step / size).length();
                                p = next;
                                g = rise(p);
                                if moved <= 1e-13 {
                                    // A place it is farthest at, not
                                    // one it is nearest at or a saddle.
                                    return (gu.x < 0.0 && det > 0.0).then_some(p);
                                }
                            }
                            None
                        };
                        for i in 0..=over {
                            for j in 0..=over {
                                let share = Point2::new(i as f64, j as f64) / over as f64;
                                let mut p = clamp(Point2::new(
                                    p0 + (p1 - p0) * share.x,
                                    q0 + (q1 - q0) * share.y,
                                ));
                                let mut h = 1.0 / STARTS as f64;
                                let climb = |p: &mut Point2, h: &mut f64, fine: f64| {
                                    while *h > fine {
                                        let here = reach(*p);
                                        let ways = [(*h, 0.0), (-*h, 0.0), (0.0, *h), (0.0, -*h)];
                                        let next = ways
                                            .into_iter()
                                            .map(|(du, dv)| clamp(*p + size * Point2::new(du, dv)))
                                            .find(|q| reach(*q) > here);
                                        match next {
                                            Some(q) => *p = q,
                                            None => *h /= 2.0,
                                        }
                                    }
                                };
                                // Roughly first. At the rim of what the
                                // face's loops lie in it is on a loop —
                                // an edge, which is known — or outside
                                // them; else from near where the
                                // surface is level, to there.
                                climb(&mut p, &mut h, 1e-4);
                                if p.x == u.0 || p.x == u.1 || p.y == v.0 || p.y == v.1 {
                                    continue;
                                }
                                if let Some(q) = level(p)
                                    && reach(q) >= reach(p)
                                {
                                    // (A last look around it, closely.)
                                    (p, h) = (q, 1e-12);
                                }
                                climb(&mut p, &mut h, 1e-15);
                                if reach(p) > best && data.trim.holds(p) != Some(false) {
                                    best = reach(p);
                                }
                            }
                        }
                    }
                }
                if sign > 0.0 {
                    hi[axis] = hi[axis].max(best);
                } else {
                    lo[axis] = lo[axis].min(-best);
                }
            }
        }
        Some((lo, hi))
    }

    /// The point of the solid nearest `p`: `p` itself if the solid
    /// holds it, else the nearest point of its boundary — a vertex, a
    /// point of an edge, or a point of a face within its loops.
    /// `None` if the solid is empty or `p` cannot be placed against it.
    fn nearest_here(&self, p: Point3, tol: f64) -> Option<Point3> {
        if self.solid.is_empty() {
            return None;
        }
        if self.side_here(p, tol)? != Side::Out {
            return Some(p);
        }
        self.nearest_on_boundary_here(p)
    }

    /// The point of the solid's boundary nearest `p`, wherever `p` is;
    /// `None` for an empty solid.
    fn nearest_on_boundary_here(&self, p: Point3) -> Option<Point3> {
        use shapecraft_core::math::Point2;
        if self.solid.is_empty() {
            return None;
        }
        let mut best: Option<(f64, Point3)> = None;
        let take = |best: &mut Option<(f64, Point3)>, q: Point3| {
            let d = q.distance_squared(p);
            if best.is_none_or(|b| d < b.0) {
                *best = Some((d, q));
            }
        };
        for v in &self.solid.vertices {
            take(&mut best, v.point);
        }
        // Along each edge and over each face: looked at in a few
        // places (the same for every point asked about: found once),
        // and from every one that is nearer than those beside it, the
        // nearest place is come to downhill.
        let (along_edges, over_faces) = self.starts.get_or_init(|| {
            let edges = self.solid.edges.iter().map(|edge| {
                let (a, b) = edge.range;
                (0..=STARTS)
                    .map(|k| edge.point(a + (b - a) * k as f64 / STARTS as f64))
                    .collect()
            });
            let faces = self
                .solid
                .faces
                .iter()
                .zip(&self.data.faces)
                .map(|(face, data)| {
                    let [u, v] = data.uv;
                    let mut points = Vec::with_capacity((STARTS + 1) * (STARTS + 1));
                    for i in 0..=STARTS {
                        for j in 0..=STARTS {
                            let share = Point2::new(i as f64, j as f64) / STARTS as f64;
                            let q =
                                Point2::new(u.0, v.0) + Point2::new(u.1 - u.0, v.1 - v.0) * share;
                            points.push(face.surface.point_at(q.x, q.y));
                        }
                    }
                    points
                });
            (edges.collect(), faces.collect())
        });
        // Whether a search has come to rest where the way to `p` is
        // square to the way `d` it can still go — to within what the
        // place is not known closer than anyway — or is held at an end
        // of its range it would leave: there it is nearest.
        let fine = self.data.bounds.size() * 1e-13;
        let rests = |r: Point3, d: Point3, at: f64, (lo, hi): (f64, f64)| {
            let down = r.dot(d);
            // (Where going on along it leads nowhere — around a pole,
            // where all of a surface's side is one point — it rests
            // too: no place is nearer that way.)
            d.length() <= fine
                || down.abs() <= fine * d.length()
                || at <= lo && down > 0.0
                || at >= hi && down < 0.0
        };
        for (edge, points) in self.solid.edges.iter().zip(along_edges) {
            let (a, b) = edge.range;
            let at = |k: usize| a + (b - a) * k as f64 / STARTS as f64;
            let far: Vec<f64> = points.iter().map(|q| q.distance_squared(p)).collect();
            for k in 0..=STARTS {
                let low = (k == 0 || far[k] <= far[k - 1]) && (k == STARTS || far[k] <= far[k + 1]);
                if low {
                    let mut t = at(k);
                    // (A few steps more, each to where the curve's own
                    // way there is square to the way to the point: the
                    // search before stops a little short of that.)
                    // (Each step by how the curve bends there too,
                    // where that leads nearer: far from a curve that
                    // bends, steps without it come to rest slowly.)
                    for _ in 0..40 {
                        let (q, d) = edge.curve.derivative_at(t);
                        let down = (q - p).dot(d);
                        if rests(q - p, d, t, (a, b)) {
                            break;
                        }
                        let h = (b - a) * if t + (b - a) * 1e-6 <= b { 1e-6 } else { -1e-6 };
                        let bend = (edge.curve.derivative_at(t + h).1 - d) / h;
                        let steep = d.length_squared() + (q - p).dot(bend);
                        let here = q.distance_squared(p);
                        let nearer = |next: f64| {
                            // (No farther, to within what a distance
                            // is known to: near where it rests, a step
                            // on changes it by less than that.)
                            next.is_finite()
                                && next != t
                                && edge.point(next).distance_squared(p) <= here * (1.0 + 1e-14)
                        };
                        let bent = (t - down / steep).clamp(a, b);
                        let plain = (t - down / d.length_squared()).clamp(a, b);
                        // (Or, where neither leads nearer, a shorter
                        // step the same way.)
                        let shorter = || {
                            let by = plain - t;
                            (1..12)
                                .map(|k| t + by * 0.5_f64.powi(k))
                                .find(|next| nearer(*next))
                        };
                        if steep > 0.0 && nearer(bent) {
                            t = bent;
                        } else if nearer(plain) {
                            t = plain;
                        } else if let Some(next) = shorter() {
                            t = next;
                        } else {
                            break;
                        }
                    }
                    let (q, d) = edge.curve.derivative_at(t);
                    // (And from there, if it has not come to rest, by
                    // ever smaller steps to where none is nearer: far
                    // from a curve that bends, the steps before come to
                    // it slowly.)
                    if !rests(q - p, d, t, (a, b)) {
                        let far = |t: f64| edge.point(t.clamp(a, b)).distance_squared(p);
                        let mut h = (b - a) / STARTS as f64;
                        while h > (b - a) * 1e-15 {
                            let here = far(t);
                            let next = [t - h, t + h]
                                .into_iter()
                                .map(|s| s.clamp(a, b))
                                .find(|s| far(*s) < here);
                            match next {
                                Some(s) => t = s,
                                None => h /= 2.0,
                            }
                        }
                    }
                    take(&mut best, edge.point(t));
                }
            }
        }
        let faces = self.solid.faces.iter().zip(&self.data.faces);
        for ((face, data), points) in faces.zip(over_faces) {
            // (Not a face all of which is farther than what there is.)
            let apart = (data.bounds.lo - p)
                .max(p - data.bounds.hi)
                .max(Point3::ZERO);
            if best.is_some_and(|b| apart.length_squared() > b.0) {
                continue;
            }
            let [u, v] = data.uv;
            let at = |i: usize, j: usize| {
                let share = Point2::new(i as f64, j as f64) / STARTS as f64;
                Point2::new(u.0, v.0) + Point2::new(u.1 - u.0, v.1 - v.0) * share
            };
            let n = STARTS + 1;
            let far: Vec<f64> = points.iter().map(|q| q.distance_squared(p)).collect();
            for i in 0..n {
                for j in 0..n {
                    let here = far[i * n + j];
                    let low = (i.saturating_sub(1)..=(i + 1).min(STARTS)).all(|k| {
                        (j.saturating_sub(1)..=(j + 1).min(STARTS)).all(|l| here <= far[k * n + l])
                    });
                    if !low {
                        continue;
                    }
                    let start = at(i, j);
                    let clamp = |q: Point2| Point2::new(q.x.clamp(u.0, u.1), q.y.clamp(v.0, v.1));
                    let place = |q: Point2| face.surface.point_at(q.x, q.y);
                    let size = Point2::new(u.1 - u.0, v.1 - v.0);
                    let mut q = start;
                    // (A few steps more, each to where the surface's
                    // own ways there are square to the way to the
                    // point: the search before stops a little short of
                    // that.)
                    // (Each step by how the surface bends there too,
                    // where that leads nearer — far from a surface that
                    // bends, steps without it come to rest slowly — and
                    // along a side of the surface it is held at.)
                    for _ in 0..40 {
                        let (s, su, sv) = face.surface.derivatives_at(q.x, q.y);
                        let r = s - p;
                        let (ru, rv) = (rests(r, su, q.x, u), rests(r, sv, q.y, v));
                        if ru && rv {
                            break;
                        }
                        let (e, f, g) = (su.dot(su), su.dot(sv), sv.dot(sv));
                        let (bu, bv) = (-r.dot(su), -r.dot(sv));
                        let held = |at: f64, (lo, hi): (f64, f64), by: f64| {
                            (at <= lo && by < 0.0) || (at >= hi && by > 0.0)
                        };
                        // (Nor does it go on a way that leads nowhere:
                        // around a pole.)
                        let (hu, hv) = (
                            held(q.x, u, bu) || su.length() <= fine,
                            held(q.y, v, bv) || sv.length() <= fine,
                        );
                        if hu && hv {
                            break;
                        }
                        // How the surface bends: from its ways a little
                        // on (to the side there is room on).
                        let aside = |at: f64, (lo, hi): (f64, f64)| {
                            let h = (hi - lo) * 1e-6;
                            if at + h <= hi { h } else { -h }
                        };
                        let (du, dv) = (aside(q.x, u), aside(q.y, v));
                        let (_, su_u, sv_u) = face.surface.derivatives_at(q.x + du, q.y);
                        let (_, _, sv_v) = face.surface.derivatives_at(q.x, q.y + dv);
                        let (a, b, c) = (
                            e + r.dot((su_u - su) / du),
                            f + r.dot((sv_u - sv) / du),
                            g + r.dot((sv_v - sv) / dv),
                        );
                        let here = r.length_squared();
                        let nearer = |step: Point2| {
                            let next = clamp(q + step);
                            // (No farther, to within what a distance
                            // is known to: near where it rests, a step
                            // on changes it by less than that.)
                            (step.is_finite()
                                && next != q
                                && place(next).distance_squared(p) <= here * (1.0 + 1e-14))
                                .then_some(next)
                        };
                        let bent = if hu {
                            (c > 0.0).then(|| Point2::new(0.0, bv / c))
                        } else if hv {
                            (a > 0.0).then(|| Point2::new(bu / a, 0.0))
                        } else {
                            let det = a * c - b * b;
                            (det > 0.0 && a > 0.0)
                                .then(|| Point2::new(c * bu - b * bv, a * bv - b * bu) / det)
                        };
                        let plain = if hu {
                            Point2::new(0.0, bv / g)
                        } else if hv {
                            Point2::new(bu / e, 0.0)
                        } else {
                            Point2::new(g * bu - f * bv, e * bv - f * bu) / (e * g - f * f)
                        };
                        // (Or, where neither leads nearer — the place
                        // is no hollow of the distance yet — a shorter
                        // step the same way.)
                        let shorter = || (1..12).find_map(|k| nearer(plain * 0.5_f64.powi(k)));
                        match bent
                            .and_then(nearer)
                            .or_else(|| nearer(plain))
                            .or_else(shorter)
                        {
                            Some(next) => q = next,
                            None => break,
                        }
                    }
                    let (s, su, sv) = face.surface.derivatives_at(q.x, q.y);
                    let rested = rests(s - p, su, q.x, u) && rests(s - p, sv, q.y, v);
                    let mut h = if rested { 0.0 } else { 1.0 / STARTS as f64 };
                    while h > 1e-15 {
                        let here = place(q).distance_squared(p);
                        let ways = [
                            (h, 0.0),
                            (-h, 0.0),
                            (0.0, h),
                            (0.0, -h),
                            (h, h),
                            (h, -h),
                            (-h, h),
                            (-h, -h),
                        ];
                        let next = ways
                            .into_iter()
                            .map(|(du, dv)| clamp(q + size * Point2::new(du, dv)))
                            .find(|r| place(*r).distance_squared(p) < here);
                        match next {
                            Some(r) => q = r,
                            None => h /= 2.0,
                        }
                    }
                    if data.trim.holds(q) != Some(false) {
                        take(&mut best, place(q));
                    }
                }
            }
        }
        best.map(|b| b.1)
    }

    /// The principal curvatures of the boundary at its point nearest
    /// `p`, the greater first: more than nothing where the boundary
    /// bends away from its outward normal (a ball), less where it bends
    /// towards it (a hole). `None` where that point is on a crease (see
    /// [`normal`](Self::normal)), or the solid is empty.
    fn curvature_here(&self, p: Point3, tol: f64) -> Option<[f64; 2]> {
        use shapecraft_core::math::Point2;
        // (A point on the boundary already is its own nearest: found
        // on a face, without the search for the nearest of all. Where
        // it is on each face is found once, for its normal as for its
        // curvature.)
        let mut on = self.on_faces(p, tol);
        let q = if on.is_empty() {
            let q = self.nearest_on_boundary_here(p)?;
            on = self.on_faces(q, tol);
            q
        } else {
            p
        };
        let out = self.normal_among(q, tol, &on)?;
        let &(f, uv) = on.first()?;
        let (face, data) = (&self.solid.faces[f], &self.data.faces[f]);
        let [u, v] = data.uv;
        let size = Point2::new(u.1 - u.0, v.1 - v.0);
        let middle = Point2::new((u.0 + u.1) / 2.0, (v.0 + v.1) / 2.0);
        let s = &face.surface;
        // Where the surface collapses (a pole) its curvature is that of
        // the points beside it: taken a little way in.
        let mut uv = uv;
        for _ in 0..3 {
            let (_, du, dv) = s.derivatives_at(uv.x, uv.y);
            if du.cross(dv).length() > 1e-6 * du.length().max(dv.length()).powi(2) {
                break;
            }
            uv += (middle - uv) * 1e-4;
        }
        // The second derivatives, from the first a little to each side
        // (within the surface's domain).
        let h = size * 1e-5;
        let (ua, ub) = ((uv.x - h.x).max(u.0), (uv.x + h.x).min(u.1));
        let (va, vb) = ((uv.y - h.y).max(v.0), (uv.y + h.y).min(v.1));
        let (_, su, sv) = s.derivatives_at(uv.x, uv.y);
        let (_, su_a, sv_ua) = s.derivatives_at(ua, uv.y);
        let (_, su_b, sv_ub) = s.derivatives_at(ub, uv.y);
        let (_, su_va, sv_a) = s.derivatives_at(uv.x, va);
        let (_, su_vb, sv_b) = s.derivatives_at(uv.x, vb);
        let suu = (su_b - su_a) / (ub - ua);
        let svv = (sv_b - sv_a) / (vb - va);
        let suv = ((sv_ub - sv_ua) / (ub - ua) + (su_vb - su_va) / (vb - va)) / 2.0;
        let (e, f, g) = (su.dot(su), su.dot(sv), sv.dot(sv));
        // (Bending away from the outward normal counts as more than
        // nothing: against the surface's second derivatives along it.)
        let (l, m, n) = (-suu.dot(out), -suv.dot(out), -svv.dot(out));
        let det = e * g - f * f;
        if det <= 0.0 {
            return None;
        }
        let mean = (e * n - 2.0 * f * m + g * l) / (2.0 * det);
        let gauss = (l * n - m * m) / det;
        let spread = (mean * mean - gauss).max(0.0).sqrt();
        Some([mean + spread, mean - spread])
    }

    /// The unit normal of the boundary at `p`, out of the solid: `None`
    /// if `p` is not within `tol` of a face, or lies where faces meet
    /// at an angle (on an edge between two that do not run on smoothly
    /// into each other, or at a corner).
    fn normal_here(&self, p: Point3, tol: f64) -> Option<Point3> {
        self.normal_among(p, tol, &self.on_faces(p, tol))
    }

    /// The faces `p` is on, within `tol` (or their own tolerance), each
    /// with where on it: its (u, v).
    fn on_faces(&self, p: Point3, tol: f64) -> Vec<(usize, shapecraft_core::math::Point2)> {
        let faces = self.solid.faces.iter().zip(&self.data.faces);
        faces
            .enumerate()
            .filter_map(|(f, (face, data))| {
                Some((f, query::on_face(face, data, p, tol.max(face.tolerance))?))
            })
            .collect()
    }

    /// [`normal_here`](Self::normal_here), the faces `p` is on (as
    /// [`on_faces`](Self::on_faces) finds them) given.
    fn normal_among(
        &self,
        p: Point3,
        tol: f64,
        on: &[(usize, shapecraft_core::math::Point2)],
    ) -> Option<Point3> {
        let mut found: Option<Point3> = None;
        // The normals of the faces `p` is on — and, where it is on an
        // edge, of every face along that edge there, by the edge's own
        // curve in that face (a point on a face's rim is not always
        // found within its loops).
        let mut normals = Vec::new();
        for (f, face) in self.solid.faces.iter().enumerate() {
            let out = if face.outward { 1.0 } else { -1.0 };
            if let Some(&(_, uv)) = on.iter().find(|(g, _)| *g == f) {
                normals.push(query::normal(&face.surface, uv) * out);
            }
            for step in face.loops.iter().flatten() {
                let crate::model::Walk::Edge { edge, .. } = step.walk else {
                    continue;
                };
                let e = &self.solid.edges[edge];
                let within = tol.max(e.tolerance);
                // (An edge the point is not among the control points
                // of, it is not on.)
                let pieces = &self.data.edges[edge];
                if !pieces.iter().any(|piece| piece.bounds().holds(p, within)) {
                    continue;
                }
                for at in crate::geometry::on_curve(&e.curve, e.range, p, within) {
                    normals.push(query::normal(&face.surface, step.pcurve.point_at(at)) * out);
                }
            }
        }
        for n in normals {
            match found {
                // (Smooth: the two normals are one, to a millionth of a
                // radian.)
                Some(other) if other.cross(n).length() > 1e-6 || other.dot(n) < 0.0 => {
                    return None;
                }
                Some(_) => {}
                None => found = Some(n),
            }
        }
        found
    }
}

/// How far the piece of `curve` over `range` reaches along `axis`
/// (against it, with `sign` -1) where that is beyond `beyond`, else
/// anything not beyond it: at its ends, at the joints of its Bézier
/// pieces, or where it runs level along the axis — the roots of a
/// polynomial of each piece. `None` for a curve with weights that are
/// not all positive.
fn farthest(
    curve: &shapecraft_core::world::topo::Curve3,
    range: (f64, f64),
    axis: usize,
    sign: f64,
    beyond: f64,
) -> Option<f64> {
    let (a, b) = (range.0.min(range.1), range.0.max(range.1));
    let mut most = f64::NEG_INFINITY;
    for piece in crate::bezier::curve_pieces(curve, a, b) {
        if piece.cp.iter().any(|h| h[3] <= 0.0) {
            return None;
        }
        let n = piece.cp.len() - 1;
        let x: Vec<f64> = piece.cp.iter().map(|h| sign * h[axis]).collect();
        let w: Vec<f64> = piece.cp.iter().map(|h| h[3]).collect();
        most = most.max(x[0] / w[0]).max(x[n] / w[n]);
        // (It lies among its points: no farther than they are.)
        let hull = x
            .iter()
            .zip(&w)
            .fold(f64::NEG_INFINITY, |m, (x, w)| m.max(x / w));
        if n < 2 && w.iter().all(|v| *v == w[0]) || hull <= beyond {
            continue;
        }
        let slope =
            |c: &[f64]| -> Vec<f64> { c.windows(2).map(|p| (p[1] - p[0]) * n as f64).collect() };
        // Level where x'w = xw'.
        let rise: Vec<f64> = crate::known::product(&slope(&x), &w)
            .into_iter()
            .zip(crate::known::product(&x, &slope(&w)))
            .map(|(p, q)| p - q)
            .collect();
        for t in crate::bezier::roots(&rise, 0.0).unwrap_or_default() {
            let (mut px, mut pw) = (x.clone(), w.clone());
            for k in 1..=n {
                for i in 0..=n - k {
                    px[i] += (px[i + 1] - px[i]) * t;
                    pw[i] += (pw[i + 1] - pw[i]) * t;
                }
            }
            most = most.max(px[0] / pw[0]);
        }
    }
    Some(most)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn points_in_on_and_outside_a_block() {
        let block = crate::block(
            "b",
            Point3::ZERO,
            [Point3::X, Point3::Y, Point3::Z],
            [10.0, 20.0, 30.0],
        )
        .expect("a block");
        let placer = Placer::new(&block);
        let side = |x, y, z| placer.side(Point3::new(x, y, z), 1e-7);
        assert_eq!(side(5.0, 10.0, 15.0), Some(Side::In));
        assert_eq!(side(5.0, 10.0, 30.0), Some(Side::On));
        assert_eq!(side(0.0, 0.0, 15.0), Some(Side::On));
        assert_eq!(side(10.0, 20.0, 30.0), Some(Side::On));
        assert_eq!(side(5.0, 10.0, 30.01), Some(Side::Out));
        assert_eq!(side(5.0, 10.0, 29.99), Some(Side::In));
        assert_eq!(side(20.0, 0.0, 0.0), Some(Side::Out));
        let (lo, hi) = placer.bounds().expect("not empty");
        assert!(lo.distance(Point3::ZERO) < 1e-12);
        assert!(hi.distance(Point3::new(10.0, 20.0, 30.0)) < 1e-12);
        let near = placer
            .nearest(Point3::new(20.0, 10.0, 15.0), 1e-7)
            .expect("a point");
        assert!(near.distance(Point3::new(10.0, 10.0, 15.0)) < 1e-9);
        let near = placer
            .nearest(Point3::new(12.0, 23.0, 34.0), 1e-7)
            .expect("a point");
        assert!(near.distance(Point3::new(10.0, 20.0, 30.0)) < 1e-9);
        let normal = |x, y, z| placer.normal(Point3::new(x, y, z), 1e-6);
        assert!(normal(5.0, 10.0, 30.0).is_some_and(|n| n.distance(Point3::Z) < 1e-12));
        assert_eq!(normal(0.0, 0.0, 15.0), None);
        assert_eq!(normal(5.0, 10.0, 15.0), None);
    }
}
