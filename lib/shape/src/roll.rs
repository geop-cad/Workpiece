//! The geometry of a blend along an edge, whatever the edge and the two
//! faces it lies between are: nothing here knows of lines, circles,
//! planes or cylinders.
//!
//! At any place along the edge a **station** is worked out in the plane
//! square to the edge there. For a round, a ball of the round's radius
//! is set against both faces: where a point of each face, moved off the
//! face by the radius, is one and the same point in that plane (found by
//! Newton's method on the two points' (u, v)). The two points are where
//! the round leaves the faces, its rails; its section runs from one to
//! the other. For a chamfer the rails are the points of the faces a
//! distance across them from the edge.
//!
//! The blend's tool (what is taken from the body along an outside edge,
//! or added along an inside one) has a five-sided section at each
//! station: the blend from rail to rail, and a way around the edge a
//! margin outside the faces (inside, for an inside edge). It is swept
//! through as many stations as it takes for the surfaces between them
//! to be within a tolerance of the stations there ([`sweep::loft`]).

use std::sync::Arc;

use anyhow::{Context, bail, ensure};
use workpiece_boolean::Solid;
use workpiece_boolean::model::Walk;
use workpiece_core::math::{Point2, Point3};
use workpiece_core::world::topo::{Curve3, NurbsSurface, Topo};

use crate::blend::{By, Section, Shape};
use crate::sweep::{self, Loft};

/// One of the two faces along the edge.
struct Side {
    surface: Arc<NurbsSurface>,
    /// The edge in the face's (u, v), with the edge's parameters.
    /// Where on the surface the edge is, at some places along it: to
    /// look from.
    along: Vec<(f64, Point2)>,
    /// Whether the surface's own normal points out of the body.
    outward: bool,
    /// Whether the face walks the edge against its curve.
    reversed: bool,
    /// For a surface run on past its rim ([`run_on`]): the (u, v) the
    /// face's own piece of it takes up. What is on the surface past
    /// that is on the face only beside the edge's ends — along the
    /// edge from the face's own piece — not past the face's far side.
    own: Option<[(f64, f64); 2]>,
    /// For a surface that closes on itself one way and comes to a point
    /// at an end the other way (a sphere): whether that way is v, and
    /// at which ends it comes to a point ([`Side::over`]).
    wrap: Option<(bool, [bool; 2])>,
}

/// Whether `surface` closes on itself one way (u with the first value
/// true, else v), and at which ends of the other way it comes to a
/// point: what [`Side::over`] goes on around and over.
fn wrap_of(surface: &NurbsSurface) -> Option<(bool, [bool; 2])> {
    let (nu, nv) = surface.counts();
    let c = surface.control_points();
    let size = {
        let (mut lo, mut hi) = (c[0], c[0]);
        for p in c {
            (lo, hi) = (lo.min(*p), hi.max(*p));
        }
        (hi - lo).length()
    };
    let near = size * 1e-9;
    let row = |v: bool, last: bool| -> Vec<Point3> {
        if v {
            let j = if last { nv - 1 } else { 0 };
            (0..nu).map(|i| c[i * nv + j]).collect()
        } else {
            let i = if last { nu - 1 } else { 0 };
            (0..nv).map(|j| c[i * nv + j]).collect()
        }
    };
    let closes = |v: bool| {
        let (a, b) = (row(v, false), row(v, true));
        a.iter().zip(&b).all(|(p, q)| p.distance(*q) <= near)
    };
    let point = |v: bool, last: bool| {
        let r = row(v, last);
        r.iter().all(|p| p.distance(r[0]) <= near)
    };
    for pole_v in [true, false] {
        let poles = [point(pole_v, false), point(pole_v, true)];
        if closes(!pole_v) {
            return Some((pole_v, poles));
        }
    }
    None
}

/// `surface` run on past its rim, where that is certain: a flat
/// surface laid out evenly (a parallelogram) goes on as its plane does,
/// the same places at the same (u, v), by its own size each way. A
/// ball rolled along an edge that leans rests on the faces beside the
/// edge's ends a little past where the faces end: on their planes,
/// which a face's own piece of them need not reach. Any other surface
/// is run on past each side of it that is a rim — not where it closes
/// on itself, nor where it comes to a point — as it itself goes on
/// there ([`crate::thick::run_on_surface`]: a cylinder around, a sweep
/// straight on), by its last piece's length there.
fn run_on(surface: &Arc<NurbsSurface>) -> Arc<NurbsSurface> {
    if surface.degrees() != (1, 1) || surface.counts() != (2, 2) {
        return run_on_curved(surface);
    }
    let w = surface.weights();
    if w.iter().any(|x| (x - w[0]).abs() > 1e-12 * w[0].abs()) {
        return surface.clone();
    }
    let c = surface.control_points();
    let (along_u, along_v) = (c[2] - c[0], c[1] - c[0]);
    let size = along_u.length() + along_v.length();
    if (c[0] + along_u + along_v).distance(c[3]) > 1e-12 * size {
        return surface.clone();
    }
    let ((u0, u1), (v0, v1)) = surface.domain();
    let (du, dv) = (u1 - u0, v1 - v0);
    let corner = |i: f64, j: f64| c[0] + along_u * i + along_v * j;
    let wider = NurbsSurface::new(
        1,
        1,
        vec![u0 - du, u0 - du, u1 + du, u1 + du],
        vec![v0 - dv, v0 - dv, v1 + dv, v1 + dv],
        2,
        2,
        vec![
            corner(-1.0, -1.0),
            corner(-1.0, 2.0),
            corner(2.0, -1.0),
            corner(2.0, 2.0),
        ],
        None,
    );
    wider.map_or_else(|_| surface.clone(), Arc::new)
}

/// [`run_on`] of a surface that is not a parallelogram.
fn run_on_curved(surface: &Arc<NurbsSurface>) -> Arc<NurbsSurface> {
    let (nu, nv) = surface.counts();
    let c = surface.control_points();
    let size = {
        let (mut lo, mut hi) = (c[0], c[0]);
        for p in c {
            (lo, hi) = (lo.min(*p), hi.max(*p));
        }
        (hi - lo).length()
    };
    if size <= 0.0 || !size.is_finite() {
        return surface.clone();
    }
    let near = size * 1e-9;
    // A row of control points across the side: along u at a v (`v`
    // false: the side at the first or the last u).
    let row = |v: bool, last: bool| -> Vec<Point3> {
        if v {
            let j = if last { nv - 1 } else { 0 };
            (0..nu).map(|i| c[i * nv + j]).collect()
        } else {
            let i = if last { nu - 1 } else { 0 };
            (0..nv).map(|j| c[i * nv + j]).collect()
        }
    };
    let mut out = (**surface).clone();
    for v in [false, true] {
        let (first, last) = (row(v, false), row(v, true));
        let closes = first.iter().zip(&last).all(|(a, b)| a.distance(*b) <= near);
        if closes {
            continue;
        }
        for low in [true, false] {
            let side = if low { &first } else { &last };
            if side.iter().all(|p| p.distance(side[0]) <= near) {
                continue;
            }
            let ((u0, u1), (v0, v1)) = out.domain();
            let (a, b) = if v { (v0, v1) } else { (u0, u1) };
            let knots = if v { out.knots().1 } else { out.knots().0 };
            // (Its last piece's length there, at most the whole.)
            let piece = if low {
                knots.iter().find(|k| **k > a).map_or(b - a, |k| k - a)
            } else {
                knots
                    .iter()
                    .rev()
                    .find(|k| **k < b)
                    .map_or(b - a, |k| b - k)
            };
            let piece = piece.min(b - a);
            let to = if low { a - piece } else { b + piece };
            match crate::thick::run_on_surface(&out, v, low, to) {
                // (Where its weights stay positive: else it does not
                // go on as itself.)
                Ok(longer) if longer.weights().iter().all(|w| *w > 0.0) => out = longer,
                _ => {}
            }
        }
    }
    Arc::new(out)
}

impl Side {
    /// Where on the surface the edge's point `at` (at `t` along it) is.
    fn on(&self, t: f64, at: Point3) -> Point2 {
        let near = self
            .along
            .iter()
            .min_by(|a, b| (a.0 - t).abs().total_cmp(&(b.0 - t).abs()))
            .map_or(Point2::ZERO, |s| s.1);
        self.foot(at, near)
    }

    /// The (u, v) of the point of the surface nearest to `p`, looked
    /// for from `guess`: down the slope to it, where that comes to rest
    /// well inside the surface (as it does at once for a point on it),
    /// and else by the slower search that holds at its rim and where
    /// it comes to a point.
    fn foot(&self, p: Point3, guess: Point2) -> Point2 {
        self.slid(p, guess, 8)
            .unwrap_or_else(|| workpiece_boolean::geometry::foot(&self.surface, p, guess).0)
    }

    /// [`Side::slide`], if it came to rest.
    fn slid(&self, p: Point3, guess: Point2, steps: usize) -> Option<Point2> {
        self.slide(p, guess, steps)
            .and_then(|(uv, rests)| rests.then_some(uv))
    }

    /// At most `steps` steps from `guess` towards the point of the
    /// surface nearest to `p`, and whether that came to rest. `None`
    /// where a step leads to the surface's rim or to where it has no
    /// slope: that is not for this.
    fn slide(&self, p: Point3, guess: Point2, steps: usize) -> Option<(Point2, bool)> {
        let ((u0, u1), (v0, v1)) = self.surface.domain();
        // (On the rim itself is on the surface: an edge of a face often
        // is its surface's rim. Past it by more than rounding is not.)
        let (su, sv) = ((u1 - u0) * 1e-12, (v1 - v0) * 1e-12);
        let inside = |uv: Point2| {
            let on = uv.x >= u0 - su && uv.x <= u1 + su && uv.y >= v0 - sv && uv.y <= v1 + sv;
            on.then(|| Point2::new(uv.x.clamp(u0, u1), uv.y.clamp(v0, v1)))
        };
        let mut uv = inside(guess)?;
        for _ in 0..steps {
            let (q, du, dv) = self.surface.derivatives_at(uv.x, uv.y);
            let r = p - q;
            let (a, b, c) = (du.dot(du), du.dot(dv), dv.dot(dv));
            let det = a * c - b * b;
            if det.is_nan() || det <= 1e-12 * a * c {
                return None;
            }
            let (g, h) = (du.dot(r), dv.dot(r));
            let step = Point2::new(c * g - b * h, a * h - b * g) / det;
            uv = inside(uv + step)?;
            if (du * step.x).length() + (dv * step.y).length() <= 1e-15 * (1.0 + p.length()) {
                return Some((uv, true));
            }
        }
        Some((uv, false))
    }

    /// The point at `uv`, and the unit normal out of the body there.
    fn at(&self, uv: Point2) -> (Point3, Point3) {
        let (p, du, dv) = self.surface.derivatives_at(uv.x, uv.y);
        let n = self.natural(uv, du, dv);
        (p, if self.outward { n } else { -n })
    }

    /// The surface's own unit normal at `uv`, where it has the slopes
    /// `du` and `dv` — and where it collapses to a point there (a
    /// pole), the normal a little way in from it.
    fn natural(&self, uv: Point2, du: Point3, dv: Point3) -> Point3 {
        let n = du.cross(dv);
        let ((u0, u1), (v0, v1)) = self.surface.domain();
        let span = Point2::new(u1 - u0, v1 - v0);
        if n.length()
            > 1e-9 * (du * span.x).length().max((dv * span.y).length()).powi(2) / (span.x * span.y)
        {
            return n.normalize();
        }
        let middle = Point2::new(u0 + u1, v0 + v1) / 2.0;
        let near = uv + (middle - uv) * 1e-6;
        let (_, du, dv) = self.surface.derivatives_at(near.x, near.y);
        du.cross(dv).normalize_or_zero()
    }

    /// As [`Side::at`], and past the surface's rim as if it ran on
    /// straight from there: so that a solver finds what lies on the rim
    /// as readily as anything else.
    fn past(&self, uv: Point2) -> (Point3, Point3) {
        let uv = self.over(uv);
        let on = self.clamp(uv);
        let (p, du, dv) = self.surface.derivatives_at(on.x, on.y);
        let n = self.natural(on, du, dv);
        (
            p + du * (uv.x - on.x) + dv * (uv.y - on.y),
            if self.outward { n } else { -n },
        )
    }

    /// `uv`, or where the surface is there going on around the way it
    /// closes on itself, and past a point it comes to over that point:
    /// on the far side of it, half way around.
    fn over(&self, uv: Point2) -> Point2 {
        let Some((pole_v, poles)) = self.wrap else {
            return uv;
        };
        let ((u0, u1), (v0, v1)) = self.surface.domain();
        let ((c0, c1), (o0, o1)) = if pole_v {
            ((v0, v1), (u0, u1))
        } else {
            ((u0, u1), (v0, v1))
        };
        let (mut c, mut o) = if pole_v { (uv.y, uv.x) } else { (uv.x, uv.y) };
        let half = (o1 - o0) / 2.0;
        if poles[0] && c < c0 {
            (c, o) = (2.0 * c0 - c, o + half);
        } else if poles[1] && c > c1 {
            (c, o) = (2.0 * c1 - c, o + half);
        } else if o >= o0 && o <= o1 {
            return uv;
        }
        let o = o0 + (o - o0).rem_euclid(o1 - o0);
        if pole_v {
            Point2::new(o, c)
        } else {
            Point2::new(c, o)
        }
    }

    fn clamp(&self, uv: Point2) -> Point2 {
        let ((u0, u1), (v0, v1)) = self.surface.domain();
        Point2::new(uv.x.clamp(u0, u1), uv.y.clamp(v0, v1))
    }

    /// `uv` if it is on the surface (or as good as: a solver stops a
    /// hair past its rim as readily as on it), there.
    fn within(&self, uv: Point2) -> Option<Point2> {
        self.within_by(uv, false)
    }

    /// As [`Side::within`]; `loose` for a place by one of the edge's
    /// ends that its blend is cut back at, where anything on the
    /// surface run on past the face's own piece of it is beside the end: an edge that leaves its end
    /// along a face's rim (where the face becomes tangent to the one
    /// it ends on) has its ball rest past the rim, not beside it.
    fn within_by(&self, uv: Point2, loose: bool) -> Option<Point2> {
        let uv = self.over(uv);
        // (By how far in space that is: around a point the surface
        // collapses to, any (u, v) is that point.)
        let on = self.clamp(uv);
        let (_, du, dv) = self.surface.derivatives_at(on.x, on.y);
        let ((u0, u1), (v0, v1)) = self.surface.domain();
        let size = (du * (u1 - u0)).length() + (dv * (v1 - v0)).length();
        let off = du * (uv.x - on.x) + dv * (uv.y - on.y);
        if off.length() > size * 1e-8 {
            return None;
        }
        // On a surface run on past the face's own piece of it: beside
        // one of the edge's ends, where going along the edge leads back
        // onto that piece — not past the face's far side, which no
        // going along the edge comes back from.
        if let Some([(a0, a1), (b0, b1)]) = self.own.filter(|_| !loose) {
            let slack = Point2::new(a1 - a0, b1 - b0) * 1e-8;
            let near = self.along.iter().enumerate().min_by(|a, b| {
                (a.1.1 - on)
                    .length_squared()
                    .total_cmp(&(b.1.1 - on).length_squared())
            });
            let (i, _) = near?;
            let last = self.along.len() - 1;
            let way = self.along[(i + 1).min(last)].1 - self.along[i.saturating_sub(1)].1;
            // Where the line through it along the edge is within the
            // face's own (u, v): between these two shares of `way`.
            let (mut lo, mut hi) = (f64::NEG_INFINITY, f64::INFINITY);
            for (at, d, from, to, give) in [
                (on.x, way.x, a0, a1, slack.x),
                (on.y, way.y, b0, b1, slack.y),
            ] {
                if d.abs() < 1e-300 {
                    if at < from - give || at > to + give {
                        return None;
                    }
                } else {
                    let (s, t) = ((from - give - at) / d, (to + give - at) / d);
                    lo = lo.max(s.min(t));
                    hi = hi.min(s.max(t));
                }
            }
            if lo > hi {
                return None;
            }
        }
        Some(on)
    }

    /// Whether `uv` is in the face's own piece of its surface.
    fn owns(&self, uv: Point2) -> bool {
        let Some([(a0, a1), (b0, b1)]) = self.own else {
            return true;
        };
        let slack = Point2::new(a1 - a0, b1 - b0) * 1e-8;
        uv.x >= a0 - slack.x && uv.x <= a1 + slack.x && uv.y >= b0 - slack.y && uv.y <= b1 + slack.y
    }

    /// Steps in u and v that are small beside the surface.
    fn steps(&self) -> Point2 {
        let ((u0, u1), (v0, v1)) = self.surface.domain();
        Point2::new(u1 - u0, v1 - v0) * 1e-6
    }
}

/// Two faces asked what they are at places ([`Side::past`]), each
/// place once: a solver asks about one face again and again while it
/// moves over the other.
struct Asked<'a> {
    sides: [&'a Side; 2],
    seen: [std::cell::Cell<(Places, usize)>; 2],
}

/// The last few places a face was asked at, with what it is there.
type Places = [(Point2, (Point3, Point3)); 6];

impl<'a> Asked<'a> {
    fn of(sides: [&'a Side; 2]) -> Self {
        let none = (Point2::NAN, (Point3::ZERO, Point3::ZERO));
        Asked {
            sides,
            seen: [0, 1].map(|_| std::cell::Cell::new(([none; 6], 0))),
        }
    }

    /// [`Side::past`] of side `k` at `uv`.
    fn past(&self, k: usize, uv: Point2) -> (Point3, Point3) {
        let (mut seen, mut next) = self.seen[k].get();
        if let Some(found) = seen.iter().find(|s| s.0 == uv) {
            return found.1;
        }
        let found = self.sides[k].past(uv);
        seen[next] = (uv, found);
        next = (next + 1) % seen.len();
        self.seen[k].set((seen, next));
        found
    }
}

/// How something goes along an edge without changing: straight on, or
/// around an axis.
#[derive(Debug, Clone, Copy)]
enum Motion {
    /// Straight on, from where the edge is at `from`.
    Along {
        /// Where the edge is, to start with.
        from: Point3,
    },
    /// Around `axis` through `centre`, from where the edge is at
    /// `from`.
    Around {
        /// Where the edge is, to start with.
        from: Point3,
        /// A point of the axis: the middle of the edge's circle.
        centre: Point3,
        /// The axis' direction.
        axis: Point3,
    },
}

impl Motion {
    /// What takes everything from where it is when the edge is at the
    /// start to where it is when the edge is at `at`.
    fn to(&self, at: Point3) -> Box<dyn Fn(Point3) -> Point3> {
        match *self {
            Motion::Along { from } => {
                let by = at - from;
                Box::new(move |p| p + by)
            }
            Motion::Around { from, centre, axis } => {
                let (a, b) = (from - centre, at - centre);
                let r2 = a.length_squared().max(1e-300);
                let (cos, sin) = (a.dot(b) / r2, a.cross(b).dot(axis) / r2);
                Box::new(move |p| {
                    let v = p - centre;
                    centre + v * cos + axis.cross(v) * sin + axis * (axis.dot(v) * (1.0 - cos))
                })
            }
        }
    }

    /// Whether `surface` is the same wherever it is moved to: all of
    /// its unbounded self. (`way` is the way straight on goes.)
    fn keeps(&self, surface: &NurbsSurface, way: Point3, within: f64) -> bool {
        let flat = crate::tweak::flat_way(surface, within);
        match *self {
            Motion::Along { .. } => {
                if let Some(n) = flat {
                    return n.dot(way).abs() <= 1e-12;
                }
                // Swept straight along the way: two rows of control
                // points, each of the one the other's moved along it.
                let straight = |s: &NurbsSurface| {
                    let (nu, nv) = s.counts();
                    let w = s.weights();
                    s.degrees().1 == 1
                        && nv == 2
                        && (0..nu).all(|i| {
                            let by = s.control_point(i, 1) - s.control_point(i, 0);
                            w[i * nv] == w[i * nv + 1] && by.cross(way).length() <= within
                        })
                };
                straight(surface) || crate::tweak::swapped(surface).is_some_and(|s| straight(&s))
            }
            Motion::Around { centre, axis, .. } => {
                if let Some(n) = flat {
                    return n.cross(axis).length() <= 1e-12;
                }
                // Turned about the axis: every row of control points
                // the first row's, nearer the axis or farther and
                // along it, the first around a circle about it; and
                // the weights a row's times a column's.
                let turned = |s: &NurbsSurface| {
                    let (nu, nv) = s.counts();
                    let (p, _) = s.degrees();
                    let w = s.weights();
                    let ((u0, u1), _) = s.domain();
                    let off = |q: Point3| {
                        let v = q - centre;
                        (v - axis * v.dot(axis), v.dot(axis))
                    };
                    let row = Curve3::new(
                        p,
                        s.knots().0.to_vec(),
                        (0..nu).map(|i| s.control_point(i, 0)).collect(),
                        Some((0..nu).map(|i| w[i * nv]).collect()),
                    );
                    let Some((c, k, _)) = row
                        .ok()
                        .and_then(|row| crate::tweak::circle_of(&row, (u0, u1), within))
                    else {
                        return false;
                    };
                    if k.cross(axis).length() > 1e-12 || off(c).0.length() > within {
                        return false;
                    }
                    let (first, _) = off(s.control_point(0, 0));
                    let far = first.length();
                    (0..nv).all(|j| {
                        let (own, high) = off(s.control_point(0, j));
                        // (How much farther from the axis than the
                        // first row, and how far along it.)
                        let by = own.dot(first) / (far * far);
                        (0..nu).all(|i| {
                            let (a, _) = off(s.control_point(i, 0));
                            let (b, h) = off(s.control_point(i, j));
                            (b - a * by).length() <= within
                                && (h - high).abs() <= within
                                && (w[i * nv + j] * w[0] - w[i * nv] * w[j]).abs() <= 1e-12
                        })
                    }) && far > within
                };
                turned(surface) || crate::tweak::swapped(surface).is_some_and(|s| turned(&s))
            }
        }
    }
}

/// The blend at one place along the edge.
#[derive(Debug, Clone)]
pub struct Station {
    /// The edge's parameter.
    pub t: f64,
    /// The edge's point there, and its unit direction.
    pub at: Point3,
    /// The edge's unit direction there.
    pub along: Point3,
    /// Where the blend leaves the two faces.
    pub rails: [Point3; 2],
    /// The unit normals out of the body at the rails, square to the
    /// edge.
    pub outs: [Point3; 2],
    /// The normals out of the body at the edge itself, square to it.
    pub edge_outs: [Point3; 2],
    /// A round's ball: its centre and radius.
    pub centre: Option<Point3>,
    /// The ball's radius.
    pub radius: f64,
    /// The ways the tool goes on from the rails into the body: the
    /// normals at the rails, but where the ball rolls on the rim of a
    /// face ([`RunOut`]) a way that turns from that face's normal
    /// towards the other's, so that the tool ends in a flat section
    /// where the round comes to nothing.
    sinks: [Point3; 2],
    /// Where the ball touches the rim of a face it has rolled off onto
    /// ([`RunOut`]): the rim's parameter.
    rim: Option<f64>,
    /// The rails in the faces' (u, v): where to look from next.
    uv: [Point2; 2],
    /// Of each rail, whether it is on its face (0) or on a face that
    /// runs on from it smoothly (the how manyeth of those, from 1):
    /// where the ball rolls off the face onto that one.
    on: [usize; 2],
}

impl Station {
    /// Where the rails are in the two faces' (u, v).
    pub fn uv(&self) -> [Point2; 2] {
        self.uv
    }

    /// Whether both rails are on the edge's own two faces.
    pub fn on_own(&self) -> bool {
        self.on == [0, 0]
    }

    /// Of each rail, on which face of `rolled`'s it is.
    pub fn faces(&self, rolled: &Rolled) -> [usize; 2] {
        [0, 1].map(|k| match self.on[k] {
            0 => rolled.faces[k],
            j => rolled.next_faces[k][j - 1],
        })
    }
}

/// An edge to blend, between its two faces.
pub struct Rolled {
    curve: Arc<Curve3>,
    /// The edge's curve run on past its ends ([`run_on_curve`]): where a
    /// blend rolled on past an end is.
    onward: Arc<Curve3>,
    /// The part of the edge's curve it is.
    pub range: (f64, f64),
    sides: [Side; 2],
    /// Of each side, the faces it runs on into smoothly, across an
    /// edge of it other than the blended one: where a ball that rolls
    /// off it is to touch.
    next: [Vec<Side>; 2],
    /// Those faces of the solid.
    pub next_faces: [Vec<usize>; 2],
    shape: Shape,
    /// How large the body is: what tolerances are of.
    size: f64,
    /// Whether the edge is an outside one: the blend takes material
    /// away.
    pub convex: bool,
    /// The edge of the solid it runs along: the edge it blends, or of
    /// a blend between two faces that do not meet, the edge of the
    /// first with the face between them.
    pub edge: usize,
    /// The solid's faces on its two sides, and its end vertices.
    pub faces: [usize; 2],
    /// Its end vertices.
    pub vertices: [usize; 2],
    /// The edge's ends, and at each whether the tool may run on past
    /// it: the body turns away from the edge there.
    pub ends: [(Point3, bool); 2],
    /// At each end, whether there is nothing of the body on beyond it
    /// between the two faces.
    pub air: [bool; 2],
    /// Whether it closes on itself, smoothly: there is no end to it.
    pub closed: bool,
    /// How long it is, and its length if it is straight with the same
    /// blend all along (what a corner patch can be set against).
    pub length: f64,
    /// Whether it is straight with the same blend all along.
    pub straight: bool,
    /// Whether the edge is a straight line.
    line: bool,
    /// Places along the edge, evenly in its parameter: where a point
    /// beside it is looked for from ([`Rolled::beside`]).
    spots: std::cell::OnceCell<Vec<Point3>>,
    /// Towards each end, how its blend grows to a corner that is set
    /// back there.
    pub swell: [Option<Swell>; 2],
    /// At each end, how far its tool is to run on straight past it
    /// whatever is there: into a stretch its ball is too large to roll
    /// along ([`Rolled::tight`]).
    pub on: [Option<f64>; 2],
    /// The stations found from the edge itself, each with what it was
    /// found for: a blend is asked for the same places again and again
    /// (its ends, its middle), by whoever looks at it.
    seen: std::cell::RefCell<Vec<(Seen, Station)>>,
    /// How the blend moves along the edge, if that is all it does, and
    /// its station at the edge's start ([`Rolled::steady`]): told once
    /// for each way the edge is taken (outside or inside).
    moved: std::cell::RefCell<Vec<(bool, Option<Steady>)>>,
    /// At each end, whether its tool is run on and cut back to the
    /// faces it ends on ([`Rolled::check_end`]): beside such an end the
    /// ball may rest on a face run on past its own piece.
    cut_back: std::cell::Cell<[bool; 2]>,
    /// At each end, where the ball rolls on over the rim of one of the
    /// faces as the round fades out there ([`RunOut`]).
    run_out: std::cell::RefCell<[Option<RunOut>; 2]>,
}

/// A tool's sections swept along its edge: the degree and knots along
/// it, and the control sections (each the control points of all its
/// sides, in weighted coordinates).
type Swept = (usize, Vec<f64>, Vec<Vec<[f64; 4]>>);

/// How a blend that only moves along its edge moves, and its station at
/// the edge's start.
type Steady = (Motion, Station);

/// Where a ball touches the two faces: in their (u, v), and on which
/// face of each side ([`Station`]'s `on`).
type Touch = ([Point2; 2], [usize; 2]);

/// What a station was found for: the place along the edge, and what of
/// the blend it depends on that may be changed after.
type Seen = (u64, bool, [Option<Swell>; 2]);

/// How a blend grows towards an end of its edge, to a corner set back
/// from it: `by` times its size up to `near` the end, its own size from
/// `far` on, and between the two eased from the one to the other with
/// no break in how it bends.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Swell {
    /// As far from the end, all of it.
    pub near: f64,
    /// As far from the end, none of it any more.
    pub far: f64,
    /// How many times the size.
    pub by: f64,
}

/// What of an edge a tool is to blend.
#[derive(Debug, Clone, Copy, Default)]
pub struct Stretch {
    /// How far short of each end it stops, where that is more than
    /// nothing.
    pub inset: [f64; 2],
    /// At each end, how far it is to run on straight past it whatever
    /// is there (to be cut back to what another blend leaves it).
    pub on: [Option<f64>; 2],
    /// At each end, how far it is to run on past it along the edge's
    /// curve run on there, the ball rolled on along it between the
    /// faces run on (to be cut back to the faces the edge ends on).
    pub rolled_on: [Option<f64>; 2],
    /// The half of the edge it is to blend, if not all of it: the first
    /// or the second.
    pub half: Option<usize>,
    /// At each end, whether the blend of another edge goes on from it
    /// there, smoothly, as one tool with it ([`Chain`]): it just ends.
    pub joined: [bool; 2],
    /// At each end, whether it ends in its own last section, laid flat,
    /// though it would be cut back there ([`EndCut::smooth`]).
    pub square: [bool; 2],
}

/// Where a blend's tool, run on past an end of its edge, is cut back
/// to the faces the edge ends on ([`Rolled::check_end`]).
#[derive(Debug, Clone)]
pub struct EndCut {
    /// The edge's end: a point of each face's plane.
    pub at: Point3,
    /// Each flat face's unit normal towards the edge: what is kept.
    pub keep: Vec<Point3>,
    /// Each curved face, with its unit normal at the end towards the
    /// edge: what is kept is on that side of its surface.
    pub curved: Vec<(usize, Point3)>,
    /// How far the tool is to run on past the end — if it runs on: a
    /// blend that fades out where curved faces become tangent to one
    /// of its own ends square there, cut back to them.
    pub long: Option<f64>,
    /// Where the ball rolls on over a rim to the end ([`RunOut`]): the
    /// point where its rail reaches the rim, and runs along it on to
    /// the end. The tool then ends where the round comes to nothing,
    /// and is not cut back.
    pub over: Option<Point3>,
    /// Whether it is cut back only because the faces go on smoothly
    /// past the end: to the plane through the end square to the edge
    /// — which, where that cannot be made, its own last section is
    /// nearly ([`Stretch::square`]).
    pub smooth: bool,
}

/// Where a round fades out at an end of its edge on a face that only
/// touches one of the edge's two faces there ([`Rolled::check_end`]):
/// short of the end the ball's rail on the other face reaches that
/// face's rim (its edge with the face the round ends on) and the ball
/// rolls on over the rim — touching the face it stays on and the rim
/// itself — until it touches both at the end, and the round is nothing.
#[derive(Debug, Clone)]
struct RunOut {
    /// The side whose face the ball stays on.
    stay: usize,
    /// The other face's rim: its curve.
    rim: Arc<Curve3>,
    /// The rim's parameter at the blended edge's end.
    tip: f64,
    /// Where along the blended edge the ball's rail reaches the rim.
    from: f64,
    /// And where along the rim.
    rim_from: f64,
    /// The face the edge ends on: its surface, whether its normal points
    /// out of the body, and where on it the end is.
    face: (Arc<NurbsSurface>, bool, Point2),
    /// The vertex the edge ends at: where the round comes to nothing.
    corner: Point3,
}

impl RunOut {
    /// The rim's point at `rim`, its end taken to the corner: a rim's
    /// curve ends within its tolerance of its vertex, not on it, and the
    /// round is to come to nothing at the vertex itself — else its two
    /// rails end that far apart, and what its tool is made of there
    /// depends on how far, as the noise in a body's values has it. (The
    /// way from the one to the other shared out along the rim, from
    /// where the ball reaches it to the end.)
    fn rim_point(&self, rim: f64) -> Point3 {
        let span = self.tip - self.rim_from;
        let share = if span == 0.0 {
            1.0
        } else {
            ((rim - self.rim_from) / span).clamp(0.0, 1.0)
        };
        self.rim.point_at(rim) + (self.corner - self.rim.point_at(self.tip)) * share
    }
}

/// `ring`, a tool's section whose first `count` control points are the
/// blend's (from rail to rail), laid into one plane — as a flat cap
/// over it has to be: the plane the blend's own side lies in (a round's
/// arc and its ball's centre), the other sides' points moved into it
/// square to it; or, where the blend's side is straight or not in one
/// plane, the plane through its two rails that is nearest square to
/// `way`, all but the rails moved into that. Nothing is moved where all
/// is within `within` of the plane already.
fn flatten(ring: &mut [[f64; 4]], count: usize, way: Point3, within: f64) {
    let points: Vec<Point3> = ring
        .iter()
        .map(|h| Point3::new(h[0], h[1], h[2]) / h[3])
        .collect();
    let (a, b) = (points[0], points[count - 1]);
    let newell: Point3 = points[..count]
        .windows(2)
        .map(|w| (w[0] - a).cross(w[1] - a))
        .sum();
    let chord = (b - a).length();
    let own = newell
        .try_normalize()
        .filter(|_| newell.length() > chord * chord * 1e-6)
        .filter(|n| {
            points[..count]
                .iter()
                .all(|p| (*p - a).dot(*n).abs() <= within)
        });
    let (normal, from) = match own {
        Some(normal) => (normal, count),
        None => match way.reject_from(b - a).try_normalize() {
            Some(normal) => (normal, 1),
            None => return,
        },
    };
    if points.iter().all(|p| (*p - a).dot(normal).abs() <= within) {
        return;
    }
    for (k, (h, p)) in ring.iter_mut().zip(&points).enumerate() {
        // (The rails stay where they are.)
        if k < from || k == count - 1 || k == points.len() - 1 {
            continue;
        }
        let q = *p - normal * (*p - a).dot(normal);
        *h = [q.x * h[3], q.y * h[3], q.z * h[3], h[3]];
    }
}

/// The parameter of the point of `curve` over `range` nearest to `p`:
/// from the nearest of a few places on it, by Newton's method on the way
/// there being square to it.
pub(crate) fn curve_foot(curve: &Curve3, range: (f64, f64), p: Point3) -> f64 {
    const N: usize = 64;
    let (t0, t1) = range;
    let mut t = t0;
    let mut least = f64::INFINITY;
    for i in 0..=N {
        let at = t0 + (t1 - t0) * i as f64 / N as f64;
        let far = curve.point_at(at).distance(p);
        if far < least {
            (least, t) = (far, at);
        }
    }
    let h = (t1 - t0) * 1e-7;
    let lean = |t: f64| {
        let (c, d) = curve.derivative_at(t);
        (c - p).dot(d)
    };
    for _ in 0..40 {
        let slope = (lean(t + h) - lean(t - h)) / (2.0 * h);
        if slope.abs() < 1e-300 {
            break;
        }
        let next = (t - lean(t) / slope).clamp(t0, t1);
        let rests = (next - t).abs() <= (t1 - t0) * 1e-15;
        t = next;
        if rests {
            break;
        }
    }
    t
}

/// Sweeps of one degree, each starting where the one before ends, as
/// one: its sections where two meet once, there with nothing of either
/// beyond them (a knot as many times over as the degree).
#[allow(clippy::type_complexity)]
fn end_to_end(
    pieces: Vec<(usize, Vec<f64>, Vec<Vec<[f64; 4]>>)>,
) -> (usize, Vec<f64>, Vec<Vec<[f64; 4]>>) {
    let mut pieces = pieces.into_iter();
    let (degree, mut knots, mut rings) = pieces.next().expect("a sweep");
    for (other, theirs, more) in pieces {
        debug_assert_eq!(other, degree);
        knots.truncate(knots.len() - 1);
        knots.extend_from_slice(&theirs[degree + 1..]);
        rings.extend(more.into_iter().skip(1));
    }
    (degree, knots, rings)
}

/// The normals out of the body of the face `f` of `solid` at its
/// vertex `vertex`: one for each time its outline comes to it.
fn normals_at(solid: &Solid, f: usize, vertex: usize) -> Vec<Point3> {
    let face = &solid.faces[f];
    face.loops
        .iter()
        .flatten()
        .filter(|step| solid.ends(step).0 == vertex)
        .map(|step| {
            let uv = solid.start(step);
            let (_, du, dv) = face.surface.derivatives_at(uv.x, uv.y);
            let mut n = du.cross(dv);
            // (Where the surface comes to a point: a hair in from it.)
            if n.length() <= 1e-12 * du.length().max(dv.length()).powi(2) {
                let ((u0, u1), (v0, v1)) = face.surface.domain();
                let middle = Point2::new(u0 + u1, v0 + v1) / 2.0;
                let near = uv + (middle - uv) * 1e-8;
                let (_, du, dv) = face.surface.derivatives_at(near.x, near.y);
                n = du.cross(dv);
            }
            let n = n.normalize_or_zero();
            if face.outward { n } else { -n }
        })
        .collect()
}

/// The point of the face `f` of `solid` nearest to `p` (on its surface,
/// looked for from the nearest place of its outline), and its normal
/// out of the body there.
fn face_foot(solid: &Solid, f: usize, p: Point3) -> (Point3, Point3) {
    let face = &solid.faces[f];
    let start = face
        .loops
        .iter()
        .flatten()
        .map(|step| step.pcurve.point_at(solid.span(step).0))
        .min_by(|a, b| {
            let d = |uv: &Point2| face.surface.point_at(uv.x, uv.y).distance(p);
            d(a).total_cmp(&d(b))
        })
        .unwrap_or(Point2::ZERO);
    let uv = workpiece_boolean::geometry::foot(&face.surface, p, start).0;
    let (q, du, dv) = face.surface.derivatives_at(uv.x, uv.y);
    let n = du.cross(dv).normalize_or_zero();
    (q, if face.outward { n } else { -n })
}

/// How many places along an edge are looked at to know it.
const SAMPLES: usize = 96;

/// The line or the arc of a circle that runs through all of `places`
/// in their order, within `within` — if there is one — and where along
/// it each place is.
fn simplest(places: &[Point3], closed: bool, within: f64) -> Option<(Curve3, Vec<f64>)> {
    let n = places.len() - 1;
    let (a, end) = (places[0], places[n]);
    if !closed {
        let chord = end - a;
        let long = chord.length();
        let way = chord.try_normalize()?;
        let at: Vec<f64> = places.iter().map(|p| (*p - a).dot(way) / long).collect();
        if places
            .iter()
            .all(|p| (*p - a).reject_from_normalized(way).length() <= within)
            && at.windows(2).all(|w| w[1] > w[0])
        {
            let line = Curve3::line(a, end).ok()?;
            let (t0, t1) = line.domain();
            return Some((line, at.iter().map(|f| t0 + (t1 - t0) * f).collect()));
        }
    }
    // The circle through three of them, well apart.
    let (b, c) = if closed {
        (places[n / 3], places[2 * n / 3])
    } else {
        (places[n / 2], end)
    };
    let (ab, ac) = (b - a, c - a);
    let normal = ab.cross(ac);
    let area = normal.length_squared();
    if area <= 1e-18 * ab.length_squared() * ac.length_squared() {
        return None;
    }
    let centre = a
        + (normal.cross(ab) * ac.length_squared() + ac.cross(normal) * ab.length_squared())
            / (2.0 * area);
    let normal = normal.normalize();
    let radius = centre.distance(a);
    let x = (a - centre) / radius;
    let y = normal.cross(x);
    let mut angles = vec![0.0_f64];
    for p in &places[1..] {
        let d = *p - centre;
        if (d.length() - radius).abs() > within || d.dot(normal).abs() > within {
            return None;
        }
        // On around from the last one, by less than half a turn.
        let last = *angles.last()?;
        let turn = d.dot(y).atan2(d.dot(x));
        let on = (turn - last).rem_euclid(std::f64::consts::TAU);
        if on <= 0.0 || on >= std::f64::consts::PI {
            return None;
        }
        angles.push(last + on);
    }
    let all = if closed {
        std::f64::consts::TAU
    } else {
        *angles.last()?
    };
    if all > std::f64::consts::TAU + 1e-9 || (closed && (angles[n] - all).abs() > 1e-6) {
        return None;
    }
    let arc = crate::arcs::ellipse(centre, x * radius, y * radius, all);
    // (Its pieces run evenly in angle only about: each place is where
    // the arc comes nearest to it.)
    let (t0, t1) = arc.domain();
    let at = places
        .iter()
        .zip(&angles)
        .map(|(p, angle)| {
            let guess = t0 + (t1 - t0) * angle / all;
            // (Down the slope to it, where that comes to rest: the place
            // is on the arc. Else, halving.)
            let mut t = guess;
            for _ in 0..12 {
                let (at, d) = arc.derivative_at(t);
                let speed = d.length_squared();
                if speed <= 0.0 {
                    break;
                }
                let step = (*p - at).dot(d) / speed;
                let next = (t + step).clamp(t0, t1);
                let moved = (next - t).abs();
                t = next;
                if moved <= 1e-15 * (t1 - t0) {
                    return t;
                }
            }
            let mut best = guess;
            let mut step = (t1 - t0) / n as f64;
            for _ in 0..40 {
                for t in [best - step, best + step] {
                    let t = t.clamp(t0, t1);
                    if arc.point_at(t).distance(*p) < arc.point_at(best).distance(*p) {
                        best = t;
                    }
                }
                step *= 0.5;
            }
            best
        })
        .collect();
    Some((arc, at))
}

/// A smooth curve through all of `places` in their order (a cubic
/// spline, its parameter the way gone along them), and where along it
/// each place is.
fn threaded(places: &[Point3]) -> anyhow::Result<(Curve3, Vec<f64>)> {
    let n = places.len();
    ensure!(n >= 4, "too few places for a curve through them");
    let mut at = vec![0.0];
    for w in places.windows(2) {
        at.push(at[at.len() - 1] + w[0].distance(w[1]));
    }
    let long = at[n - 1];
    ensure!(long > 0.0, "the places are all one");
    for t in &mut at {
        *t /= long;
    }
    // Knots where the places are, by threes (and all four of each end).
    let degree = 3;
    let mut knots = vec![0.0; degree + 1];
    for k in 1..n - degree {
        knots.push(at[k..k + degree].iter().sum::<f64>() / degree as f64);
    }
    knots.extend(std::iter::repeat_n(1.0, degree + 1));
    let matrix: Vec<Vec<f64>> = at
        .iter()
        .map(|&t| {
            let (first, values) = sweep::basis(degree, &knots, t);
            let mut row = vec![0.0; n];
            row[first..first + values.len()].copy_from_slice(&values);
            row
        })
        .collect();
    let wanted = places.iter().map(|p| vec![[p.x, p.y, p.z, 1.0]]).collect();
    let controls = sweep::through(matrix, wanted).context("no curve runs through the places")?;
    let points = controls
        .iter()
        .map(|c| Point3::new(c[0][0], c[0][1], c[0][2]))
        .collect();
    Ok((Curve3::new(degree, knots, points, None)?, at))
}

/// `curve` run on past each end of its domain that `range` reaches, as
/// it itself goes on there ([`crate::thick::run_out`]: a line straight
/// on, an arc around), by as much as `range` is long in its parameter
/// — where its weights stay positive; else that end as it is.
fn run_on_curve(curve: &Curve3, range: (f64, f64)) -> Curve3 {
    let (lo, hi) = curve.domain();
    let long = range.1 - range.0;
    let slack = (hi - lo) * 1e-12;
    let mut out = curve.clone();
    for low in [true, false] {
        if (low && range.0 > lo + slack) || (!low && range.1 < hi - slack) {
            continue;
        }
        let degree = out.degree();
        let mut knots = out.knots().to_vec();
        let mut row: Vec<[f64; 4]> = out
            .control_points()
            .iter()
            .zip(out.weights())
            .map(|(p, w)| [p.x * w, p.y * w, p.z * w, *w])
            .collect();
        let (a, b) = out.domain();
        let to = if low { -(a - long) } else { b + long };
        if low {
            knots = knots.iter().rev().map(|k| -k).collect();
            row.reverse();
        }
        let (mut knots, mut row) = crate::thick::run_out(degree, &knots, &row, to);
        if low {
            knots = knots.iter().rev().map(|k| -k).collect();
            row.reverse();
        }
        if row.iter().any(|h| h[3] <= 0.0) {
            continue;
        }
        let points = row
            .iter()
            .map(|h| Point3::new(h[0], h[1], h[2]) / h[3])
            .collect();
        let weights = row.iter().map(|h| h[3]).collect();
        if let Ok(longer) = Curve3::new(degree, knots, points, Some(weights)) {
            out = longer;
        }
    }
    out
}

/// Of each of the faces `faces` of `solid` beside its edge `edge`, the
/// faces it runs on into smoothly across another of its edges (tangent
/// to it there), each as a [`Side`] of the blend along `curve` over
/// `range`: where a ball rolled along the edge that leaves the face
/// touches next.
fn smoothly_on(
    solid: &Solid,
    edge: usize,
    faces: [usize; 2],
    curve: &Curve3,
    range: (f64, f64),
) -> ([Vec<Side>; 2], [Vec<usize>; 2]) {
    let normal = |f: usize, uv: Point2| {
        let face = &solid.faces[f];
        let (_, du, dv) = face.surface.derivatives_at(uv.x, uv.y);
        let n = du.cross(dv).normalize_or_zero();
        if face.outward { n } else { -n }
    };
    let mut sides: [Vec<Side>; 2] = [Vec::new(), Vec::new()];
    let mut found: [Vec<usize>; 2] = [Vec::new(), Vec::new()];
    for k in 0..2 {
        let f = faces[k];
        for step in solid.faces[f].loops.iter().flatten() {
            let Walk::Edge { edge: e, .. } = step.walk else {
                continue;
            };
            if e == edge {
                continue;
            }
            let (t0, t1) = solid.edges[e].range;
            let mid = (t0 + t1) / 2.0;
            let other = solid.faces.iter().enumerate().find_map(|(g, face)| {
                if g == f || faces.contains(&g) || found[k].contains(&g) {
                    return None;
                }
                face.loops.iter().flatten().find_map(|s| match s.walk {
                    Walk::Edge { edge: x, .. } if x == e => Some((g, s.pcurve.point_at(mid))),
                    _ => None,
                })
            });
            let Some((g, at)) = other else {
                continue;
            };
            let (n, m) = (normal(f, step.pcurve.point_at(mid)), normal(g, at));
            if n.dot(m) <= 0.0 || n.cross(m).length() > 1e-6 {
                continue;
            }
            let face = &solid.faces[g];
            let mut near = at;
            let along = (0..=SAMPLES)
                .map(|i| {
                    let t = range.0 + (range.1 - range.0) * i as f64 / SAMPLES as f64;
                    near =
                        workpiece_boolean::geometry::foot(&face.surface, curve.point_at(t), near).0;
                    (t, near)
                })
                .collect();
            found[k].push(g);
            sides[k].push(Side {
                wrap: wrap_of(&face.surface),
                surface: run_on(&face.surface),
                along,
                outward: face.outward,
                reversed: false,
                own: {
                    let (u, v) = face.surface.domain();
                    Some([u, v])
                },
            });
        }
    }
    (sides, found)
}

/// Whether a point is inside the solid ([`workpiece_boolean::holds`]).
pub(crate) type Holds<'a> = &'a dyn Fn(Point3) -> Option<bool>;

/// The edge at a place along it ([`Rolled::frame`]), with the place.
type Frame = (f64, (Point3, Point3, [Point3; 2], [Point3; 2]));

/// What a solution leans towards, where the equations leave it open.
type Lean<'a, const N: usize> = &'a dyn Fn(&[f64; N]) -> [f64; 2];

/// Solves `f(x) = 0` for `N` unknowns by Newton's method from `x`, the
/// slopes taken by differences with `steps`. Where that leaves
/// something open (a ball that touches a face all along a curve), it is
/// what brings `lean` nearest to nothing; and what neither says a word
/// about (which way around a point a surface collapses to) stays as it
/// is. `None` if it does not come to rest.
fn solve<const N: usize>(
    f: &dyn Fn(&[f64; N]) -> [f64; N],
    lean: Option<Lean<'_, N>>,
    mut x: [f64; N],
    steps: [f64; N],
    rest: f64,
) -> Option<[f64; N]> {
    // How little the leaning counts beside the equations themselves.
    const LIGHT: f64 = 1e-5;
    let norm = |r: &[f64; N]| r.iter().map(|v| v * v).sum::<f64>().sqrt();
    let mut r = f(&x);
    for _ in 0..60 {
        let settled = norm(&r) <= rest;
        if settled && lean.is_none() {
            return Some(x);
        }
        // The slopes, column by column.
        let mut j = [[0.0; N]; N];
        let mut l = [[0.0; N]; 2];
        for c in 0..N {
            let mut ahead = x;
            ahead[c] += steps[c];
            let mut behind = x;
            behind[c] -= steps[c];
            let (fa, fb) = (f(&ahead), f(&behind));
            for row in 0..N {
                j[row][c] = (fa[row] - fb[row]) / (2.0 * steps[c]);
            }
            if let Some(lean) = lean {
                let (la, lb) = (lean(&ahead), lean(&behind));
                for row in 0..2 {
                    l[row][c] = (la[row] - lb[row]) / (2.0 * steps[c]) * LIGHT;
                }
            }
        }
        let leaning = lean.map_or([0.0; 2], |lean| lean(&x).map(|v| v * LIGHT));
        // The least step that does it, each unknown measured in its own
        // steps.
        let mut normal = [[0.0; N]; N];
        let mut rhs = [0.0; N];
        for a in 0..N {
            for b in 0..N {
                let main: f64 = (0..N).map(|row| j[row][a] * j[row][b]).sum();
                let light: f64 = (0..2).map(|row| l[row][a] * l[row][b]).sum();
                normal[a][b] = (main + light) * steps[a] * steps[b];
            }
            let main: f64 = (0..N).map(|row| j[row][a] * r[row]).sum();
            let light: f64 = (0..2).map(|row| l[row][a] * leaning[row]).sum();
            rhs[a] = -(main + light) * steps[a];
        }
        let most = (0..N).map(|a| normal[a][a]).fold(0.0, f64::max);
        for (a, row) in normal.iter_mut().enumerate() {
            row[a] += most * 1e-14;
        }
        let mut j = normal;
        for c in 0..N {
            let pivot = (c..N).max_by(|a, b| j[*a][c].abs().total_cmp(&j[*b][c].abs()))?;
            if j[pivot][c].abs() < 1e-300 {
                return None;
            }
            j.swap(c, pivot);
            rhs.swap(c, pivot);
            let top = j[c];
            for row in c + 1..N {
                let m = j[row][c] / top[c];
                for (below, above) in j[row].iter_mut().zip(&top).skip(c) {
                    *below -= m * above;
                }
                rhs[row] -= m * rhs[c];
            }
        }
        let mut dx = [0.0; N];
        for c in (0..N).rev() {
            let known: f64 = (c + 1..N).map(|k| j[c][k] * dx[k]).sum();
            dx[c] = (rhs[c] - known) / j[c][c];
        }
        // (At rest, and nothing left to lean on.)
        if settled && dx.iter().all(|d| d.abs() < 1e-4) {
            return Some(x);
        }
        for c in 0..N {
            dx[c] *= steps[c];
        }
        // A full step, or as much of one as makes things better (or,
        // at rest, no worse).
        let mut scale = 1.0;
        loop {
            workpiece_core::cancel::checkpoint();
            let next: [f64; N] = std::array::from_fn(|i| x[i] + dx[i] * scale);
            let rn = f(&next);
            if norm(&rn) < norm(&r) || (settled && norm(&rn) <= rest) {
                (x, r) = (next, rn);
                break;
            }
            if scale < 1e-4 {
                if settled {
                    return Some(x);
                }
                (x, r) = (next, rn);
                break;
            }
            scale *= 0.5;
        }
        if !x.iter().all(|v| v.is_finite()) {
            return None;
        }
    }
    (norm(&r) <= rest * 1e3).then_some(x)
}

impl Rolled {
    /// What is to be known of the edge `edge` of `solid`, blended to
    /// `shape`.
    pub fn of(solid: &Solid, edge: usize, shape: &Shape) -> anyhow::Result<Rolled> {
        let size = {
            let (lo, hi) = solid.extent();
            (hi - lo).length()
        };
        let holds = |p| workpiece_boolean::holds(solid, p);
        Rolled::of_in(solid, edge, shape, Some(&holds), size)
    }

    /// As [`Rolled::of`], asking `holds` whether a point is inside the
    /// solid (one made ready for many edges of it). Without it, the
    /// ends are not looked at: only which faces the edge lies between,
    /// and whether it is an outside edge, are to be had of it. `size`
    /// is how large the solid is ([`Solid::extent`], corner to corner).
    pub(crate) fn of_in(
        solid: &Solid,
        edge: usize,
        shape: &Shape,
        holds: Option<Holds<'_>>,
        size: f64,
    ) -> anyhow::Result<Rolled> {
        let e = &solid.edges[edge];
        // The two faces along it, each with how it walks the edge.
        let mut found = Vec::new();
        for (f, face) in solid.faces.iter().enumerate() {
            for step in face.loops.iter().flatten() {
                if let Walk::Edge {
                    edge: walked,
                    reversed,
                } = step.walk
                    && walked == edge
                {
                    found.push((
                        f,
                        Side {
                            wrap: wrap_of(&face.surface),
                            surface: run_on(&face.surface),
                            along: (0..=SAMPLES)
                                .map(|k| {
                                    let t = e.range.0
                                        + (e.range.1 - e.range.0) * k as f64 / SAMPLES as f64;
                                    (t, step.pcurve.point_at(t))
                                })
                                .collect(),
                            outward: face.outward,
                            reversed,
                            own: {
                                let (u, v) = face.surface.domain();
                                Some([u, v])
                            },
                        },
                    ));
                }
            }
        }
        ensure!(found.len() == 2, "the edge does not lie between two faces");
        ensure!(
            found[0].0 != found[1].0,
            "the edge is a seam of one face: there is nothing to blend"
        );
        let second = found.pop().context("two faces")?;
        let first = found.pop().context("two faces")?;
        let places: Vec<Point3> = (0..=SAMPLES)
            .map(|k| e.point(e.range.0 + (e.range.1 - e.range.0) * k as f64 / SAMPLES as f64))
            .collect();
        // (One that comes back to its start at an angle ends there,
        // twice.)
        let closed = e.ends[0] == e.ends[1] && {
            let (_, d0) = e.curve.derivative_at(e.range.0);
            let (_, d1) = e.curve.derivative_at(e.range.1);
            d0.normalize_or_zero().dot(d1.normalize_or_zero()) > 1.0 - 1e-8
        };
        let within = e.tolerance.max(size * 1e-10);
        let own = Some((e.curve.clone(), e.range));
        Rolled::along(
            solid,
            shape,
            edge,
            [first, second],
            &places,
            own,
            (size, within),
            closed,
            holds,
        )
    }

    /// What is to be known of a blend between the faces `faces` of
    /// `solid` that do not meet in an edge, but lie either side of one
    /// face narrow enough for the blend to reach across: it runs along
    /// where the two faces would meet, run on past that face.
    pub fn between(solid: &Solid, faces: [usize; 2], shape: &Shape) -> anyhow::Result<Rolled> {
        ensure!(faces[0] != faces[1], "a blend is between two faces");
        // The faces along each edge.
        let mut beside: Vec<Vec<usize>> = vec![Vec::new(); solid.edges.len()];
        for (f, face) in solid.faces.iter().enumerate() {
            for step in face.loops.iter().flatten() {
                if let Walk::Edge { edge, .. } = step.walk {
                    beside[edge].push(f);
                }
            }
        }
        // An edge of the first face whose other face has an edge with
        // the second: along it the two lie side by side.
        let other = |e: usize, f: usize| -> Option<usize> {
            (beside[e].len() == 2 && beside[e].contains(&f))
                .then(|| beside[e].iter().copied().find(|g| *g != f))
                .flatten()
        };
        // (Of several such edges, the one the two faces would meet
        // nearest to: a face may lie between the two at their ends
        // too, across them.)
        let mut best: Option<(f64, Rolled)> = None;
        let mut why = None;
        for step in solid.faces[faces[0]].loops.iter().flatten() {
            let Walk::Edge { edge, reversed } = step.walk else {
                continue;
            };
            let Some(narrow) = other(edge, faces[0]) else {
                continue;
            };
            if narrow == faces[1] {
                return Rolled::of(solid, edge, shape);
            }
            let far = (0..solid.edges.len()).find(|&e| other(e, narrow) == Some(faces[1]));
            let Some(far) = far else {
                continue;
            };
            match Rolled::bridged(solid, faces, shape, (edge, reversed, step), far) {
                Ok((off, rolled)) => {
                    if best.as_ref().is_none_or(|b| off < b.0) {
                        best = Some((off, rolled));
                    }
                }
                Err(e) => why = Some(e),
            }
        }
        match (best, why) {
            (Some((_, rolled)), _) => Ok(rolled),
            (None, Some(why)) => Err(why),
            (None, None) => bail!("the two faces neither meet nor lie either side of one face"),
        }
    }

    /// As [`Rolled::between`], beside one edge of the first face (how
    /// that face walks it: `guide`) whose other face has the edge `far`
    /// with the second; and how far from that edge the two faces would
    /// meet, at most.
    fn bridged(
        solid: &Solid,
        faces: [usize; 2],
        shape: &Shape,
        guide: (usize, bool, &workpiece_boolean::model::Use),
        far: usize,
    ) -> anyhow::Result<(f64, Rolled)> {
        let size = {
            let (lo, hi) = solid.extent();
            (hi - lo).length()
        };
        let (edge, reversed, step) = guide;
        let e = &solid.edges[edge];
        ensure!(
            e.ends[0] != e.ends[1],
            "a blend across a face that closes on itself is not made yet"
        );
        let at = |k: usize| e.range.0 + (e.range.1 - e.range.0) * k as f64 / SAMPLES as f64;
        let first = Side {
            wrap: None,
            surface: solid.faces[faces[0]].surface.clone(),
            along: (0..=SAMPLES)
                .map(|k| (at(k), step.pcurve.point_at(at(k))))
                .collect(),
            outward: solid.faces[faces[0]].outward,
            reversed,
            own: None,
        };
        // The second face, as it is beside the same places.
        let surface = solid.faces[faces[1]].surface.clone();
        let mut near = {
            let ((u0, u1), (v0, v1)) = surface.domain();
            Point2::new(u0 + u1, v0 + v1) / 2.0
        };
        let mut along = Vec::new();
        for k in 0..=SAMPLES {
            near = workpiece_boolean::geometry::foot(&surface, e.point(at(k)), near).0;
            along.push((at(k), near));
        }
        let mut second = Side {
            wrap: None,
            surface,
            along,
            outward: solid.faces[faces[1]].outward,
            reversed: !reversed,
            own: None,
        };
        // Where the two would meet, in the plane square to the edge at
        // each place: a point of both, each run on straight past its
        // rim.
        let mut places = Vec::new();
        let mut off = 0.0_f64;
        let (sa, sb) = (first.steps(), second.steps());
        let mut from = [
            first.along[0].1.x,
            first.along[0].1.y,
            second.along[0].1.x,
            second.along[0].1.y,
        ];
        // (Each looked for from where the two before it lead on to:
        // between flat faces that is where it is.)
        let mut before: Option<[f64; 4]> = None;
        for k in 0..=SAMPLES {
            workpiece_core::cancel::checkpoint();
            let (p, d) = e.curve.derivative_at(at(k));
            let way = d.normalize_or_zero();
            let asked = Asked::of([&first, &second]);
            let f = |x: &[f64; 4]| {
                let (a, _) = asked.past(0, Point2::new(x[0], x[1]));
                let (b, _) = asked.past(1, Point2::new(x[2], x[3]));
                let apart = a - b;
                [apart.x, apart.y, apart.z, ((a + b) / 2.0 - p).dot(way)]
            };
            let rest = 1e-12 * (1.0 + size);
            let steps = [sa.x, sa.y, sb.x, sb.y];
            let on: Option<[f64; 4]> =
                before.map(|b| std::array::from_fn(|c| 2.0 * from[c] - b[c]));
            let last = from;
            let found = on
                .and_then(|on| solve(&f, None, on, steps, rest))
                .or_else(|| solve(&f, None, from, steps, rest));
            from = found.context("the two faces do not meet, run on past the face between them")?;
            before = Some(last);
            let (a, _) = first.past(Point2::new(from[0], from[1]));
            ensure!(
                a.distance(p) <= size,
                "the two faces do not meet near the face between them"
            );
            off = off.max(a.distance(p));
            places.push(a);
        }
        // Which way the second face lies from there: towards its edge
        // with the face between.
        let towards = solid.edges[far].point(solid.edges[far].middle());
        let middle = SAMPLES / 2;
        let (_, d) = e.curve.derivative_at(at(middle));
        let (_, du, dv) = {
            let uv = second.along[middle].1;
            second.surface.derivatives_at(uv.x, uv.y)
        };
        let walk = d.normalize_or_zero() * if second.reversed { -1.0 } else { 1.0 };
        if second
            .natural(second.along[middle].1, du, dv)
            .cross(walk)
            .dot(towards - places[middle])
            < 0.0
        {
            second.reversed = !second.reversed;
        }
        let within = e.tolerance.max(size * 1e-10);
        let rolled = Rolled::along(
            solid,
            shape,
            edge,
            [(faces[0], first), (faces[1], second)],
            &places,
            None,
            (size, within),
            false,
            Some(&|p| workpiece_boolean::holds(solid, p)),
        )?;
        Ok((off, rolled))
    }

    /// The blend along the curve through `places`, between the two
    /// faces `sides` (each with where on it the places are), beside the
    /// edge `edge` of `solid`: `own` is the curve the places are of, if
    /// there is one.
    #[allow(clippy::too_many_arguments)]
    fn along(
        solid: &Solid,
        shape: &Shape,
        edge: usize,
        sides: [(usize, Side); 2],
        places: &[Point3],
        own: Option<(Arc<Curve3>, (f64, f64))>,
        (size, within): (f64, f64),
        closed: bool,
        holds: Option<Holds<'_>>,
    ) -> anyhow::Result<Rolled> {
        let vertices = solid.edges[edge].ends;
        let [mut first, mut second] = sides;
        // Along the simplest curve the edge is: what is swept along a
        // line or a circle is of few pieces and exact, and an edge two
        // bodies were found to cross in is given as many small ones.
        let (curve, range) = match simplest(places, closed, within) {
            Some((curve, at)) => {
                for side in [&mut first.1, &mut second.1] {
                    for (place, t) in side.along.iter_mut().zip(&at) {
                        place.0 = *t;
                    }
                }
                let range = curve.domain();
                (Arc::new(curve), range)
            }
            None => match own {
                Some(own) => own,
                None => {
                    let (curve, at) = threaded(places)?;
                    for side in [&mut first.1, &mut second.1] {
                        for (place, t) in side.along.iter_mut().zip(&at) {
                            place.0 = *t;
                        }
                    }
                    let range = curve.domain();
                    (Arc::new(curve), range)
                }
            },
        };
        // (Run on past its ends, for a blend rolled on past them.)
        let onward = if closed {
            curve.clone()
        } else {
            Arc::new(run_on_curve(&curve, range))
        };
        let e_point = |t: f64| curve.point_at(t);
        let (next, next_faces) = smoothly_on(solid, edge, [first.0, second.0], &curve, range);
        let mut rolled = Rolled {
            curve: curve.clone(),
            onward,
            range,
            sides: [first.1, second.1],
            next,
            next_faces,
            shape: *shape,
            size,
            convex: true,
            edge,
            faces: [first.0, second.0],
            vertices,
            ends: [(e_point(range.0), false), (e_point(range.1), false)],
            air: [false; 2],
            closed,
            length: 0.0,
            straight: false,
            line: false,
            spots: std::cell::OnceCell::new(),
            swell: [None; 2],
            on: [None; 2],
            seen: std::cell::RefCell::new(Vec::new()),
            moved: std::cell::RefCell::new(Vec::new()),
            cut_back: std::cell::Cell::new([false; 2]),
            run_out: std::cell::RefCell::new([None, None]),
        };
        // An outside edge where going into one face leads behind the
        // other.
        let (t0, t1) = range;
        let middle = (t0 + t1) / 2.0;
        let (_, along, into, outs) = rolled.frame(middle);
        let lean = outs[1].dot(into[0]);
        ensure!(
            lean.abs() > 1e-6,
            "the faces along the edge run on smoothly into each other: there is nothing to blend"
        );
        rolled.convex = lean < 0.0;
        ensure!(along != Point3::ZERO, "the edge has no direction");
        // Its length, and whether it is straight.
        let points: Vec<Point3> = (0..=64)
            .map(|k| e_point(t0 + (t1 - t0) * f64::from(k) / 64.0))
            .collect();
        rolled.length = points.windows(2).map(|w| w[0].distance(w[1])).sum();
        let chord = points[64] - points[0];
        let line = !rolled.closed
            && points.iter().all(|p| {
                (*p - points[0])
                    .reject_from(chord.normalize_or_zero())
                    .length()
                    <= 1e-9 * size
            });
        rolled.line = line;
        let Some(holds) = holds else {
            return Ok(rolled);
        };
        // At each end: is there material on beyond it, in the corner?
        let reach = match *shape {
            Shape::Fillet { size, until, .. } => size.max(until.unwrap_or(size)),
            Shape::Chamfer { first, second } => first.max(second),
        };
        for (k, t) in [t0, t1].into_iter().enumerate() {
            let (at, along, into, _) = rolled.frame(t);
            let outward = if k == 0 { -along } else { along };
            let step = reach.min(size * 0.05) * 0.2;
            let probe = at + outward * step + (into[0] + into[1]) * (step * 0.5);
            rolled.air[k] = !rolled.closed && holds(probe) == Some(false);
            rolled.ends[k].1 = rolled.convex && rolled.air[k];
        }
        // The same blend all along a straight edge: its two ends'
        // stations are one another, moved.
        if line {
            let (a, b) = (rolled.station(t0, None)?, rolled.station(t1, None)?);
            let moved = b.at - a.at;
            let same = |p: Point3, q: Point3| (q - p - moved).length() <= 1e-9 * size;
            rolled.straight = same(a.rails[0], b.rails[0]) && same(a.rails[1], b.rails[1]);
        }
        Ok(rolled)
    }

    /// The edge's curve as it is at `t`: its own, or run on past an end.
    fn curve_at(&self, t: f64) -> &Curve3 {
        if t < self.range.0 || t > self.range.1 {
            &self.onward
        } else {
            &self.curve
        }
    }

    /// The edge at `t`: its point and unit direction, and of each face
    /// the way into it across the edge and the normal out of the body,
    /// both square to the edge.
    fn frame(&self, t: f64) -> (Point3, Point3, [Point3; 2], [Point3; 2]) {
        let (at, d) = self.curve_at(t).derivative_at(t);
        let along = d.normalize_or_zero();
        let mut into = [Point3::ZERO; 2];
        let mut outs = [Point3::ZERO; 2];
        for (k, side) in self.sides.iter().enumerate() {
            let uv = side.on(t, at);
            let (_, du, dv) = side.surface.derivatives_at(uv.x, uv.y);
            let natural = side.natural(uv, du, dv);
            let walk = if side.reversed { -along } else { along };
            into[k] = natural.cross(walk).normalize_or_zero();
            let out = if side.outward { natural } else { -natural };
            outs[k] = out.reject_from(along).normalize_or_zero();
        }
        (at, along, into, outs)
    }

    /// How far along the edge `t` is, from 0 at its start to 1.
    fn share(&self, t: f64) -> f64 {
        ((t - self.range.0) / (self.range.1 - self.range.0)).clamp(0.0, 1.0)
    }

    /// Whether the ball is too large to roll along the edge touching
    /// both faces: somewhere its centre would not go on with it, but
    /// stand still or back up — along an edge that bends, outside,
    /// more tightly than the ball is round.
    pub fn tight(&self) -> bool {
        self.tight_at(0.0).is_some()
    }

    /// Where the ball's centre backs up by more than `slack` from one
    /// of a few places along the edge to the next ([`Rolled::tight`]):
    /// the edge's point there. (One that only stands still — a round
    /// of a rim as large as the rim, which closes it in a dome — does
    /// not cut into itself.)
    pub fn tight_at(&self, slack: f64) -> Option<Point3> {
        const N: usize = 12;
        // (Along a straight line the ball's centre goes on with the
        // plane it is in, whatever the faces do.)
        if self.line {
            return None;
        }
        let (t0, t1) = self.range;
        let mut stations = Vec::with_capacity(N + 1);
        for k in 0..=N {
            let t = t0 + (t1 - t0) * k as f64 / N as f64;
            let station = self.station(t, None).ok()?;
            let centre = station.centre?;
            stations.push((centre, station.along, station.at));
        }
        // (A centre that stays where it is, all along: the ball turns
        // on the spot.)
        let still = stations
            .iter()
            .all(|s| s.0.distance(stations[0].0) <= slack.max(self.size * 1e-9));
        if still && slack > 0.0 {
            return None;
        }
        stations.windows(2).find_map(|w| {
            let ((from, along, _), (centre, way, at)) = (w[0], w[1]);
            ((centre - from).dot(along + way) <= -slack).then_some(at)
        })
    }

    /// How many times its size the blend is at `t`, where it grows
    /// towards a corner set back ([`Swell`]).
    fn swollen(&self, t: f64) -> f64 {
        let mut by = 1.0;
        for (end, swell) in self.swell.iter().enumerate() {
            let Some(swell) = swell else {
                continue;
            };
            let share = self.share(t);
            let far = self.length * if end == 0 { share } else { 1.0 - share };
            let x = ((far - swell.near) / (swell.far - swell.near)).clamp(0.0, 1.0);
            // (Flat to the third order at both ends: where it starts to
            // grow it bends as the round beside it does.)
            let eased = x.powi(4) * (35.0 - 84.0 * x + 70.0 * x * x - 20.0 * x.powi(3));
            by *= swell.by + (1.0 - swell.by) * eased;
        }
        by
    }

    /// The ball of `radius` set against both faces in the plane square
    /// to the edge at `t`: the (u, v) of where it touches each.
    fn ball(&self, frame: &Frame, radius: f64, guess: Option<Touch>) -> Option<Touch> {
        // On the faces themselves, or where the station close by had it
        // (on a face one of them runs on into smoothly); else on such a
        // face where the ball rolls off one of them onto it.
        let mut tries: Vec<[usize; 2]> = Vec::new();
        if let Some((_, on)) = guess {
            tries.push(on);
        }
        tries.push([0, 0]);
        for k in 0..2 {
            for j in 1..=self.next[k].len() {
                let mut on = [0, 0];
                on[k] = j;
                tries.push(on);
            }
        }
        let mut seen: Vec<[usize; 2]> = Vec::new();
        // (One that touches a face run on past its own piece, where a
        // face it runs on into is touched instead: that one.)
        let mut past: Option<Touch> = None;
        let mut best: Option<usize> = None;
        for on in tries {
            if seen.contains(&on) {
                continue;
            }
            seen.push(on);
            let guess = guess.filter(|g| g.1 == on).map(|g| g.0);
            let sides = [self.side(0, on[0]), self.side(1, on[1])];
            // (Where it is not found from the station close by, on the
            // same faces, it is looked for from the edge.)
            // (And on a face run on into smoothly, from where it touches
            // the face it runs on from run on past its own piece: by
            // the two faces' common edge, the two places are close.)
            let from_past = || {
                let (puv, pon) = past?;
                let (t, at) = (frame.0, frame.1.0);
                let start = [0, 1].map(|k| {
                    let p = self.side(k, pon[k]).at(puv[k]).0;
                    sides[k].foot(p, sides[k].on(t, at))
                });
                self.ball_on(sides, frame, radius, Some(start))
            };
            let found = self
                .ball_on(sides, frame, radius, guess)
                .or_else(|| guess.and_then(|_| self.ball_on(sides, frame, radius, None)))
                .or_else(|| (on != [0, 0]).then(from_past).flatten());
            if let Some(uv) = found {
                let off = (0..2)
                    .filter(|&k| !self.next[k].is_empty() && !sides[k].owns(uv[k]))
                    .count();
                if off == 0 {
                    return Some((uv, on));
                }
                // (Of those, one past as few of them as may be.)
                if best.is_none_or(|b| off < b) {
                    (past, best) = (Some((uv, on)), Some(off));
                }
            }
        }
        past
    }

    /// Side `k`, or with `on` more than nothing the face it runs on into
    /// smoothly that is `on`th of those.
    fn side(&self, k: usize, on: usize) -> &Side {
        if on == 0 {
            &self.sides[k]
        } else {
            &self.next[k][on - 1]
        }
    }

    /// [`Rolled::ball`] against the faces `sides`.
    fn ball_on(
        &self,
        sides: [&Side; 2],
        frame: &Frame,
        radius: f64,
        guess: Option<[Point2; 2]>,
    ) -> Option<[Point2; 2]> {
        let (t, (at, along, into, outs)) = (frame.0, frame.1);
        // Into the material for an outside edge, out of it for an
        // inside one.
        let off = if self.convex { -radius } else { radius };
        // From where a station close by touches, brought into this
        // plane; or else:
        let brought = guess.map(|uv| {
            [0, 1].map(|k| {
                let side = &sides[k];
                let (p, _) = side.at(uv[k]);
                let here = p - along * (p - at).dot(along);
                side.foot(here, uv[k])
            })
        });
        let start = brought.unwrap_or_else(|| {
            // About where it touches flat faces at this angle.
            let turn = outs[0].angle_between(outs[1]);
            let reach = radius * (turn / 2.0).tan().abs().min(8.0);
            [0, 1].map(|k| {
                let side = &sides[k];
                let uv = side.on(t, at);
                side.foot(at + into[k] * reach, uv)
            })
        });
        let asked = Asked::of(sides);
        let past = |k: usize, uv: Point2| asked.past(k, uv);
        let centre = |k: usize, uv: Point2| {
            let (p, n) = past(k, uv);
            p + n * off
        };
        let f = |x: &[f64; 4]| {
            let (a, b) = (
                centre(0, Point2::new(x[0], x[1])),
                centre(1, Point2::new(x[2], x[3])),
            );
            let d = a - b;
            [d.x, d.y, d.z, ((a + b) / 2.0 - at).dot(along)]
        };
        let (sa, sb) = (sides[0].steps(), sides[1].steps());
        // (A ball that touches a face all along a curve touches it in
        // this plane.)
        let lean = |x: &[f64; 4]| {
            let (a, _) = past(0, Point2::new(x[0], x[1]));
            let (b, _) = past(1, Point2::new(x[2], x[3]));
            [(a - at).dot(along), (b - at).dot(along)]
        };
        let x = solve(
            &f,
            Some(&lean),
            [start[0].x, start[0].y, start[1].x, start[1].y],
            [sa.x, sa.y, sb.x, sb.y],
            1e-12 * (1.0 + self.size),
        );
        let x = x?;
        // (From a station too far along, a ball that touches a face
        // all along a curve is left touching it out of this plane.)
        let askew = lean(&x).iter().any(|off| off.abs() > 0.1 * radius);
        if askew && guess.is_some() {
            return None;
        }
        // (And a ball that came to rest far from the edge rests against
        // some other part of the faces: not the blend of this edge.)
        // (Twenty radii, where the faces meet squarely or more bluntly;
        // between faces that meet like a knife's the ball rests farther
        // from the edge, by how flat faces at this angle would have it.)
        let sharp = (outs[0].angle_between(outs[1]) / 2.0).cos().abs().max(1e-3);
        let far = 20.0 * radius * (std::f64::consts::FRAC_1_SQRT_2 / sharp).max(1.0);
        let (a, n) = sides[0].past(Point2::new(x[0], x[1]));
        if (a + n * off).distance(at) > far {
            return None;
        }
        // On the faces' surfaces, not past where they end — but for
        // by an end of the edge, on the surfaces run on beside it.
        let loose = !self.closed
            && (t < self.range.0
                || t > self.range.1
                || (0..2).any(|k| {
                    self.cut_back.get()[k] && self.ends[k].0.distance(at) <= 4.0 * radius
                }));
        Some([
            sides[0].within_by(Point2::new(x[0], x[1]), loose)?,
            sides[1].within_by(Point2::new(x[2], x[3]), loose)?,
        ])
    }

    /// The point of face `k` in the plane square to the edge at `t`, a
    /// distance `far` from the edge.
    fn across(&self, k: usize, frame: &Frame, far: f64, guess: Option<Point2>) -> Option<Point2> {
        let (t, (at, along, into, _)) = (frame.0, frame.1);
        let side = &self.sides[k];
        let start = guess.unwrap_or_else(|| {
            let uv = side.on(t, at);
            side.foot(at + into[k] * far, uv)
        });
        let f = |x: &[f64; 2]| {
            let (p, _) = side.past(Point2::new(x[0], x[1]));
            [(p - at).dot(along), (p - at).length() - far]
        };
        let s = side.steps();
        let x = solve(
            &f,
            None,
            [start.x, start.y],
            [s.x, s.y],
            1e-12 * (1.0 + self.size),
        );
        let x = x?;
        let uv = side.within(Point2::new(x[0], x[1]))?;
        // (The way into the face, not back out of it.)
        let (p, _) = side.at(uv);
        ((p - at).dot(into[k]) > 0.0).then_some(uv)
    }

    /// The blend at `t`; `near` is a station close by, to look from.
    pub fn station(&self, t: f64, near: Option<&Station>) -> anyhow::Result<Station> {
        if let Some((motion, first)) = self.steady() {
            return Ok(self.carried(&motion, &first, t));
        }
        if near.is_some() {
            return self.station_at(t, near);
        }
        let key: Seen = (t.to_bits(), self.convex, self.swell);
        if let Some(seen) = self.seen.borrow().iter().find(|s| s.0 == key) {
            return Ok(seen.1.clone());
        }
        let found = self.station_at(t, None)?;
        self.seen.borrow_mut().push((key, found.clone()));
        Ok(found)
    }

    fn station_at(&self, t: f64, near: Option<&Station>) -> anyhow::Result<Station> {
        if let Some(station) = self.rolled_over(t, near)? {
            return Ok(station);
        }
        let frame: Frame = (t, self.frame(t));
        let (at, along, _, edge_outs) = frame.1;
        // (From a station close by: one far along the edge is no better
        // a place to look from than the edge itself.)
        let reach = match self.shape {
            Shape::Fillet { size, until, .. } => size.max(until.unwrap_or(size)),
            Shape::Chamfer { first, second } => first.max(second),
        };
        let guess = near
            .filter(|s| s.at.distance(at) <= 4.0 * reach)
            .map(|s| (s.uv, s.on));
        let fits = || "a blend of this size does not fit between the faces here";
        let make = |(uv, on): Touch, centre: Option<Point3>, radius: f64| {
            let (a, b) = (self.side(0, on[0]).at(uv[0]), self.side(1, on[1]).at(uv[1]));
            let mut sinks = [
                a.1.reject_from(along).normalize_or_zero(),
                b.1.reject_from(along).normalize_or_zero(),
            ];
            // Beside where the ball goes on to roll over a rim, the tool
            // goes into the body from the rail on the face it leaves
            // between that face's normal and the face the edge ends
            // on's: as it does where the ball comes to the rim.
            for out in self.run_out.borrow().iter().flatten() {
                let j = 1 - out.stay;
                let rail = if j == 0 { a } else { b };
                let uv = workpiece_boolean::geometry::foot(&out.face.0, rail.0, out.face.2).0;
                let (p, du, dv) = out.face.0.derivatives_at(uv.x, uv.y);
                let n = du.cross(dv).normalize_or_zero();
                let n = if out.face.1 { n } else { -n };
                let x = (1.0 - p.distance(rail.0) / radius.max(1e-300)).clamp(0.0, 1.0);
                let eased = x.powi(4) * (35.0 - 84.0 * x + 70.0 * x * x - 20.0 * x.powi(3));
                if eased > 0.0 {
                    sinks[j] = (rail.1 + n * eased)
                        .reject_from(along)
                        .try_normalize()
                        .unwrap_or(sinks[j]);
                }
            }
            Station {
                t,
                at,
                along,
                rails: [a.0, b.0],
                outs: [
                    a.1.reject_from(along).normalize_or_zero(),
                    b.1.reject_from(along).normalize_or_zero(),
                ],
                edge_outs,
                centre,
                radius,
                sinks,
                rim: None,
                uv,
                on,
            }
        };
        match self.shape {
            Shape::Chamfer { first, second } => {
                ensure!(first > 0.0 && second > 0.0, "a chamfer needs a distance");
                // (Looked for from the station close by, or — if that
                // is not close enough — from the edge itself.)
                let across = |k: usize, far: f64| {
                    guess
                        .filter(|g| g.1 == [0, 0])
                        .and_then(|g| self.across(k, &frame, far, Some(g.0[k])))
                        .or_else(|| self.across(k, &frame, far, None))
                        .with_context(fits)
                };
                let uv = [across(0, first)?, across(1, second)?];
                Ok(make((uv, [0, 0]), None, 0.0))
            }
            Shape::Fillet {
                by, size, until, ..
            } => {
                ensure!(size > 0.0 && size.is_finite(), "a round needs a size");
                // (Growing evenly along the edge, where it does.)
                let size =
                    (size + (until.unwrap_or(size) - size) * self.share(t)) * self.swollen(t);
                let rolled = |radius: f64, guess: Option<Touch>| {
                    let uv = self
                        .ball(&frame, radius, guess)
                        .or_else(|| self.ball(&frame, radius, None))?;
                    let (p, n) = self.side(0, uv.1[0]).at(uv.0[0]);
                    let centre = p + n * if self.convex { -radius } else { radius };
                    Some((uv, centre))
                };
                let radius = if by == By::Radius {
                    size
                } else {
                    // The radius whose round has the size asked for.
                    let measured = |radius: f64| {
                        let ((uv, on), _) = rolled(radius, None)?;
                        let rails = [
                            self.side(0, on[0]).at(uv[0]).0,
                            self.side(1, on[1]).at(uv[1]).0,
                        ];
                        Some(match by {
                            By::Width => rails[0].distance(rails[1]),
                            _ => rails[0].distance(at),
                        })
                    };
                    let (mut lo, mut hi) = (0.0, size);
                    while measured(hi).is_some_and(|m| m < size) && hi < size * 1e6 {
                        hi *= 2.0;
                    }
                    for _ in 0..60 {
                        let mid = (lo + hi) / 2.0;
                        match measured(mid) {
                            Some(m) if m < size => lo = mid,
                            _ => hi = mid,
                        }
                    }
                    (lo + hi) / 2.0
                };
                let (uv, centre) = rolled(radius, guess).with_context(fits)?;
                Ok(make(uv, Some(centre), radius))
            }
        }
    }

    /// Whether, where the edge's end `end` is on the face `face` alone
    /// and that only touches one of the edge's two faces there, the
    /// ball rolls on over the other one's rim (its edge with `face`) as
    /// the round fades out: a round of a radius, and a rim the ball
    /// goes around — one with nothing of the body beyond it on the
    /// ball's side (outside the body for an inside edge, inside it for
    /// an outside one) — and how ([`RunOut`]). `None` where it does
    /// not; an error where it does and the ball cannot be rolled so (a
    /// round cut back there instead would leave more than it is to).
    fn run_out_over(
        &self,
        solid: &Solid,
        end: usize,
        face: usize,
    ) -> anyhow::Result<Option<RunOut>> {
        let (t0, t1) = self.range;
        let Some(radius) = self.ball_radius() else {
            return Ok(None);
        };
        if self.closed {
            return Ok(None);
        }
        let vertex = self.vertices[end];
        let Some(&n_face) = normals_at(solid, face, vertex).first() else {
            return Ok(None);
        };
        let Some(stay) = (0..2).find(|&k| {
            normals_at(solid, self.faces[k], vertex)
                .iter()
                .any(|m| m.cross(n_face).length() <= 1e-6)
        }) else {
            return Ok(None);
        };
        let leave = 1 - stay;
        // The rim: the edge the face left and the end face have at the
        // vertex.
        let edges_of = |f: usize| -> Vec<usize> {
            solid.faces[f]
                .loops
                .iter()
                .flatten()
                .filter_map(|step| match step.walk {
                    Walk::Edge { edge, .. } if solid.edges[edge].ends.contains(&vertex) => {
                        Some(edge)
                    }
                    _ => None,
                })
                .collect()
        };
        let theirs = edges_of(face);
        let Some(rim) = edges_of(self.faces[leave])
            .into_iter()
            .find(|e| theirs.contains(e) && *e != self.edge)
        else {
            return Ok(None);
        };
        let e = &solid.edges[rim];
        if e.ends[0] == e.ends[1] {
            return Ok(None);
        }
        let tip = if e.ends[0] == vertex {
            e.range.0
        } else {
            e.range.1
        };
        let other = if e.ends[0] == vertex {
            e.range.1
        } else {
            e.range.0
        };
        // (Its point a little way from the end: between the two faces,
        // off the end face, on into the face left — inside the body
        // there only if the rim is an inside one.)
        let q = e.point(tip + (other - tip) * 0.25);
        let (_, n_left) = face_foot(solid, self.faces[leave], q);
        let (_, n_end) = face_foot(solid, face, q);
        let probe = q + (n_end - n_left).normalize_or_zero() * (radius * 1e-3);
        if workpiece_boolean::holds(solid, probe) != Some(self.convex) {
            return Ok(None);
        }
        // Where along the edge the ball's rail on the face left reaches
        // the rim: where it goes past the end face.
        let inner = self.curve.point_at(if end == 0 {
            t0 + (t1 - t0) * 0.05
        } else {
            t1 - (t1 - t0) * 0.05
        });
        let sense = {
            let (p, n) = face_foot(solid, face, inner);
            -(inner - p).dot(n).signum()
        };
        let beyond = |t: f64| -> anyhow::Result<bool> {
            let rail = self.station(t, None)?.rails[leave];
            let (p, n) = face_foot(solid, face, rail);
            Ok((rail - p).dot(n) * sense > 0.0)
        };
        let tip_t = if end == 0 { t0 } else { t1 };
        if !beyond(tip_t)? {
            return Ok(None);
        }
        const N: usize = 48;
        let mut inside = None;
        let mut out = tip_t;
        for i in 1..=N {
            let t = tip_t + ((t0 + t1) / 2.0 - tip_t) * i as f64 / N as f64;
            match beyond(t)? {
                true => out = t,
                false => {
                    inside = Some(t);
                    break;
                }
            }
        }
        let mut inside = inside.context(
            "the round's rail does not come back off the face it ends on: it cannot fade out there",
        )?;
        for _ in 0..60 {
            let mid = (inside + out) / 2.0;
            if beyond(mid)? {
                out = mid;
            } else {
                inside = mid;
            }
        }
        let from = (inside + out) / 2.0;
        let first = self.station(from, None)?;
        let made = RunOut {
            stay,
            rim: e.curve.clone(),
            tip,
            from,
            rim_from: curve_foot(&e.curve, e.range, first.rails[leave]),
            face: {
                let f = &solid.faces[face];
                let start = f
                    .loops
                    .iter()
                    .flatten()
                    .find(|step| solid.ends(step).0 == vertex)
                    .map_or(Point2::ZERO, |step| solid.start(step));
                (f.surface.clone(), f.outward, start)
            },
            corner: solid.vertices[vertex].point,
        };
        // The ball rolls over the rim all the way to the end, touching
        // it from beside both faces there: else the round does not fade
        // out so.
        self.run_out.borrow_mut()[end] = Some(made.clone());
        self.seen.borrow_mut().clear();
        let mut last: Option<Station> = None;
        let holds = (1..=16).try_for_each(|i| -> anyhow::Result<()> {
            let t = made.from + (tip_t - made.from) * f64::from(i) / 16.0;
            last = Some(self.station(t, last.as_ref())?);
            Ok(())
        });
        self.run_out.borrow_mut()[end] = None;
        self.seen.borrow_mut().clear();
        holds.context("the ball does not roll on over the rim to where the round fades out")?;
        Ok(Some(made))
    }

    /// The radius of the round, where it is one radius all along (what
    /// a ball rolled over a rim is of).
    fn ball_radius(&self) -> Option<f64> {
        match self.shape {
            Shape::Fillet {
                by: By::Radius,
                size,
                until: None,
                section: Section::Circular,
                ..
            } if self.swell.iter().all(Option::is_none) => Some(size),
            _ => None,
        }
    }

    /// The end whose [`RunOut`] `t` is in, if it is in one: past where
    /// the ball's rail reaches the rim, towards the end.
    fn run_out_of(&self, t: f64) -> Option<(usize, RunOut)> {
        let outs = self.run_out.borrow();
        outs.iter().enumerate().find_map(|(end, out)| {
            let out = out.as_ref()?;
            let past = if end == 1 { t > out.from } else { t < out.from };
            past.then(|| (end, out.clone()))
        })
    }

    /// The station at `t` where the ball rolls on over a rim there
    /// ([`RunOut`]), if `t` is where it does. It touches the face of
    /// side `stay` (or a face that runs on from it smoothly) and the rim,
    /// its centre square to the rim from where it touches that — at a
    /// place along the rim, from where the rail reaches it to the end,
    /// as far along as `t` is along the edge from the one to the other
    /// (the ball's centre need not go on along the edge as it nears the
    /// end, but goes on along the rim). Its section is in the plane of
    /// the ball's centre and the two places it touches.
    fn rolled_over(&self, t: f64, near: Option<&Station>) -> anyhow::Result<Option<Station>> {
        let Some((end, out)) = self.run_out_of(t) else {
            return Ok(None);
        };
        let radius = self
            .ball_radius()
            .context("a round that rolls over a rim has a radius")?;
        let (k, j) = (out.stay, 1 - out.stay);
        let (at, along, _, edge_outs) = self.frame(t);
        let off = if self.convex { -radius } else { radius };
        let tip = if end == 0 { self.range.0 } else { self.range.1 };
        // (Within the tolerance of the end, at it: there the two places
        // the ball touches come together, and where they are told apart
        // by less than the tolerance the ball is not found more closely.)
        let share = if at.distance(self.curve.point_at(tip)) <= self.size * 1e-9 {
            1.0
        } else {
            ((t - out.from) / (tip - out.from)).clamp(0.0, 1.0)
        };
        let rim_at = |share: f64| out.rim_from + (out.tip - out.rim_from) * share;
        // The ball touching side `k` at `uv` (on its `on`th face) and the
        // rim at `rim`; and whether it does so from beside both faces
        // there, its side of them (else it is in one of them).
        let make = |uv: Point2, on: usize, rim: f64| -> (Station, bool) {
            let (a, n) = self.side(k, on).at(uv);
            // (At the end the round is nothing: the two places it
            // touches are one.)
            let b = if share >= 1.0 { a } else { out.rim_point(rim) };
            let centre = a + n * off;
            let mut rails = [Point3::ZERO; 2];
            (rails[k], rails[j]) = (a, b);
            let mut outs = [Point3::ZERO; 2];
            outs[k] = n.reject_from(along).normalize_or_zero();
            // (At the rim, the ball's own normal there; where it touches
            // both at the end, the face's.)
            outs[j] = ((centre - b) / off)
                .reject_from(along)
                .try_normalize()
                .unwrap_or(outs[k]);
            let side = &self.sides[j];
            let guess = side.on(t, at);
            let foot = side.foot(b, guess);
            let (_, normal) = side.at(foot);
            let face = {
                let uv = workpiece_boolean::geometry::foot(&out.face.0, b, out.face.2).0;
                let (_, du, dv) = out.face.0.derivatives_at(uv.x, uv.y);
                let n = du.cross(dv).normalize_or_zero();
                if out.face.1 { n } else { -n }
            };
            // Into the body from the rim: between the two faces' normals
            // there (as the rail on the face left went in, where it
            // comes to the rim — beside it, more of the face's the
            // nearer — and the faces' normals at the end, where the
            // section comes to nothing: its plane there).
            let mut sinks = outs;
            sinks[j] = (normal + face)
                .reject_from(along)
                .try_normalize()
                .unwrap_or(normal);
            let mut uvs = [Point2::ZERO; 2];
            (uvs[k], uvs[j]) = (uv, foot);
            let mut ons = [0, 0];
            ons[k] = on;
            // (Of the two normals, how much of each the way to the
            // centre is: of neither less than nothing.)
            let to = (centre - b) * off.signum();
            let c = normal.dot(face);
            let (x, y) = (to.dot(normal) / radius, to.dot(face) / radius);
            let det = 1.0 - c * c;
            let beside = share >= 1.0
                || (det > 1e-12 && x - c * y >= -1e-7 * det && y - c * x >= -1e-7 * det);
            let station = Station {
                t,
                at,
                along,
                rails,
                outs,
                edge_outs,
                centre: Some(centre),
                radius,
                sinks,
                rim: Some(rim),
                uv: uvs,
                on: ons,
            };
            (station, beside)
        };
        // At the end itself it touches both there.
        if share >= 1.0 {
            let side = &self.sides[k];
            let uv = side.foot(at, side.on(t, at));
            return Ok(Some(make(uv, 0, out.tip).0));
        }
        // The ball at `rim`, from `uv` on the `on`th face of side `k`.
        let ball = |rim: f64, uv0: Point2, on0: usize| -> Option<Station> {
            let b = out.rim_point(rim);
            let (_, d) = out.rim.derivative_at(rim);
            let d = d.normalize_or_zero();
            let mut tries = vec![on0, 0];
            tries.extend(1..=self.next[k].len());
            let mut best: Option<Station> = None;
            let mut seen = Vec::new();
            for on in tries {
                if seen.contains(&on) {
                    continue;
                }
                seen.push(on);
                let side = self.side(k, on);
                let start = if on == on0 {
                    uv0
                } else {
                    side.foot(self.side(k, on0).past(uv0).0, side.on(t, at))
                };
                // (The ball through the rim's point: |a + n·off - b|² is
                // off², as how far the two places it touches are apart
                // measures it — near the end, where they come together,
                // as closely as anywhere.)
                let f = |x: &[f64; 2]| {
                    let (a, n) = side.past(Point2::new(x[0], x[1]));
                    let apart = a - b;
                    let far = apart.length().max(1e-300);
                    [
                        (apart + n * off).dot(d),
                        far + 2.0 * off * n.dot(apart) / far,
                    ]
                };
                let steps = side.steps();
                // (And from across the rim's point, where that comes
                // to the ball on the wrong side of it.)
                let across = {
                    let (p, _) = side.past(start);
                    let foot = side.foot(b, start);
                    let (q, _) = side.at(foot);
                    side.foot(q * 2.0 - p, foot)
                };
                for from in [start, across] {
                    let Some(x) = solve(
                        &f,
                        None,
                        [from.x, from.y],
                        [steps.x, steps.y],
                        1e-12 * (1.0 + self.size),
                    ) else {
                        continue;
                    };
                    let Some(uv) = side.within_by(Point2::new(x[0], x[1]), true) else {
                        continue;
                    };
                    let (station, beside) = make(uv, on, rim);
                    if !beside {
                        continue;
                    }
                    if self.next[k].is_empty() || side.owns(uv) {
                        return Some(station);
                    }
                    best = best.or(Some(station));
                    break;
                }
            }
            best
        };
        // From the station close by (not the end's, where the ball
        // touches both in one place: from there it may as well go to
        // where it touches the rim from inside the face it ends on);
        // else rolled on from where it starts to roll over, a little at
        // a time.
        let close = near.filter(|s| {
            s.rim.is_some_and(|rim| {
                let gone = (rim - out.rim_from) / (out.tip - out.rim_from);
                gone < 1.0 && (gone - share).abs() <= 0.05
            })
        });
        let found = match close {
            Some(s) => ball(rim_at(share), s.uv[k], s.on[k]),
            None => {
                let first = self.station(out.from, None)?;
                let steps = (share / 0.02).ceil().max(1.0) as usize;
                let mut got = None;
                let (mut uv, mut on) = (first.uv[k], first.on[k]);
                for i in 1..=steps {
                    let Some(station) = ball(rim_at(share * i as f64 / steps as f64), uv, on)
                    else {
                        got = None;
                        break;
                    };
                    (uv, on) = (station.uv[k], station.on[k]);
                    got = Some(station);
                }
                got
            }
        };
        found
            .map(Some)
            .context("the ball does not roll on over the rim where its round fades out")
    }

    /// The blend's section at `station`, from the first rail to the
    /// second: its degree, its knots, and its control points with their
    /// weights.
    pub fn section(&self, station: &Station) -> anyhow::Result<(usize, Vec<f64>, Vec<[f64; 4]>)> {
        let [a, b] = station.rails;
        let one = |p: Point3| [p.x, p.y, p.z, 1.0];
        let Shape::Fillet { section, rho, .. } = self.shape else {
            return Ok((1, vec![0.0, 0.0, 1.0, 1.0], vec![one(a), one(b)]));
        };
        // Where the tangents at the rails meet: the corner the section
        // rounds. A tangent is square to the edge and to the face's
        // normal.
        // (A circle's of a ball of one size all along, in the plane of
        // its centre and the two places it touches: square to the radii
        // there — the plane square to the edge where the ball rolls
        // along a line or around a circle, and not quite elsewhere.)
        let even = matches!(self.shape, Shape::Fillet { until: None, .. })
            && self.swell.iter().all(Option::is_none);
        let plane = station
            .centre
            .filter(|_| section == Section::Circular && even)
            .map(|c| ((a - c).cross(b - c), c))
            .filter(|(normal, _)| normal.length() > 1e-9 * station.radius * station.radius);
        let ways = [0, 1].map(|k| {
            let way = match plane {
                Some((normal, c)) => normal.cross(station.rails[k] - c).normalize_or_zero(),
                None => station.along.cross(station.outs[k]),
            };
            let other = station.rails[1 - k] - station.rails[k];
            if way.dot(other) < 0.0 { -way } else { way }
        });
        let (dd, between) = (ways[0].dot(ways[1]), b - a);
        let det = 1.0 - dd * dd;
        // (Where the round comes to nothing, its tangents at the two
        // rails run on into each other: the corner it rounds is between
        // the rails, as straight as they are.)
        let apex = if (det <= 1e-12 && dd < 0.0) || between.length() <= self.size * 1e-12 {
            (a + b) / 2.0
        } else {
            ensure!(
                det > 1e-12,
                "the round would be half a circle or more: the faces do not meet at an angle"
            );
            a + ways[0] * ((between.dot(ways[0]) - between.dot(ways[1]) * dd) / det)
        };
        Ok(match section {
            Section::Circular | Section::Conic => {
                let weight = if section == Section::Circular {
                    let centre = station.centre.context("a round has a centre")?;
                    ((a - centre).angle_between(b - centre) / 2.0).cos()
                } else {
                    ensure!(rho > 0.0 && rho < 1.0, "a conic's rho is between 0 and 1");
                    rho / (1.0 - rho)
                };
                (
                    2,
                    vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                    vec![one(a), [apex.x, apex.y, apex.z, weight], one(b)],
                )
            }
            Section::Curvature | Section::Centred => {
                // As straight as each face where it leaves it (a flat
                // face has no curvature): as full as takes off what the
                // round does, or as bends by `rho` at its middle.
                let w = if section == Section::Curvature {
                    crate::gn::even((a - apex).angle_between(b - apex))
                } else {
                    ensure!(rho > 0.0 && rho.is_finite(), "its middle bends by a radius");
                    crate::gn::centred(a, apex, b, rho)
                };
                (
                    5,
                    vec![0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0],
                    crate::gn::section(a, apex, b, w),
                )
            }
        })
    }

    /// The tool's five sides at `station`: the blend, and around the
    /// edge `margin` outside the faces (inside them, for an inside
    /// edge).
    fn ring(&self, station: &Station, margin: f64) -> anyhow::Result<Vec<Vec<[f64; 4]>>> {
        let side = if self.convex { margin } else { -margin };
        let [a, b] = station.rails;
        let (ga, gb) = (a + station.sinks[0] * side, b + station.sinks[1] * side);
        // Around the edge: the margin from both faces as they are
        // there.
        let [na, nb] = station.edge_outs;
        let around = station.at + (na + nb) * (side / (1.0 + na.dot(nb)).max(0.05));
        let one = |p: Point3| [p.x, p.y, p.z, 1.0];
        let (_, _, blend) = self.section(station)?;
        Ok(vec![
            blend,
            vec![one(b), one(gb)],
            vec![one(gb), one(around)],
            vec![one(around), one(ga)],
            vec![one(ga), one(a)],
        ])
    }

    /// The parameter a distance `far` along the edge from its end `end`.
    pub(crate) fn inset(&self, end: usize, far: f64) -> f64 {
        let (t0, t1) = self.range;
        if far <= 0.0 {
            return if end == 0 { t0 } else { t1 };
        }
        // By its length, in small steps.
        const N: usize = 512;
        let at = |k: usize| {
            let f = k as f64 / N as f64;
            if end == 0 {
                t0 + (t1 - t0) * f
            } else {
                t1 - (t1 - t0) * f
            }
        };
        let mut gone = 0.0;
        for k in 0..N {
            let step = self
                .curve
                .point_at(at(k))
                .distance(self.curve.point_at(at(k + 1)));
            if gone + step >= far {
                let part = (far - gone) / step.max(1e-300);
                return at(k) + (at(k + 1) - at(k)) * part;
            }
            gone += step;
        }
        at(N)
    }

    /// How the blend moves along the edge, if that is all it does, with
    /// its station at the edge's start: the edge is straight or runs
    /// around a circle, both faces are the same wherever the edge has
    /// gone along them (flat along the line, or swept straight along
    /// it; turned about the circle's axis, or flat across it), the
    /// blend is of one size all along. Every station is then the one
    /// at the start moved, and not looked for.
    fn steady(&self) -> Option<Steady> {
        let even = match self.shape {
            Shape::Fillet { until, .. } => until.is_none(),
            Shape::Chamfer { .. } => true,
        };
        if !even
            || self.swell.iter().any(Option::is_some)
            || self.run_out.borrow().iter().any(Option::is_some)
        {
            return None;
        }
        if let Some(told) = self.moved.borrow().iter().find(|m| m.0 == self.convex) {
            return told.1.clone();
        }
        let told = self.steady_looked();
        self.moved.borrow_mut().push((self.convex, told.clone()));
        told
    }

    fn steady_looked(&self) -> Option<Steady> {
        let within = self.size * 1e-11;
        let (t0, t1) = self.range;
        let first = self.station_at(t0, None).ok()?;
        let motion = if self.line {
            Motion::Along { from: first.at }
        } else {
            let (centre, axis, _) = crate::tweak::circle_of(&self.curve, self.range, within)?;
            Motion::Around {
                from: first.at,
                centre,
                axis,
            }
        };
        let way = self.curve.derivative_at(t0).1.normalize_or_zero();
        if !self
            .sides
            .iter()
            .all(|side| motion.keeps(&side.surface, way, within))
        {
            return None;
        }
        // (And a way along the edge the faces are where that puts the
        // rails, and face as it has them face.)
        let carried = self.carried(&motion, &first, t0 + (t1 - t0) * 0.37);
        let same = (0..2).all(|k| {
            let (p, n) = self.sides[k].at(carried.uv[k]);
            let out = n.reject_from(carried.along).normalize_or_zero();
            p.distance(carried.rails[k]) <= within && out.distance(carried.outs[k]) <= 1e-9
        });
        same.then_some((motion, first))
    }

    /// The station `first`, moved by `motion` to where the edge is at
    /// `t`.
    fn carried(&self, motion: &Motion, first: &Station, t: f64) -> Station {
        let at = self.curve_at(t).point_at(t);
        let to = motion.to(at);
        let turn = |v: Point3| to(first.at + v) - to(first.at);
        let rails = first.rails.map(&to);
        let uv = [0, 1].map(|k| {
            let side = &self.sides[k];
            side.foot(rails[k], side.on(t, at))
        });
        Station {
            t,
            at,
            along: turn(first.along),
            rails,
            outs: first.outs.map(turn),
            edge_outs: first.edge_outs.map(turn),
            centre: first.centre.map(&to),
            radius: first.radius,
            sinks: first.sinks.map(turn),
            rim: None,
            uv,
            on: [0, 0],
        }
    }

    /// The margin a tool of the stretch from `from` to `to` needs — at
    /// least `margin`, and enough to hold the faces between the edge
    /// and the rails where they bend away from the straight way there —
    /// and a few stations along the stretch.
    fn held(&self, margin: f64, from: f64, to: f64) -> anyhow::Result<(f64, Vec<Station>)> {
        let probes: Vec<Station> = {
            let mut out: Vec<Station> = Vec::new();
            for k in 0..=4 {
                let t = from + (to - from) * f64::from(k) / 4.0;
                // (Each from the edge itself: looked for from the one
                // before, a quarter of the way back along an edge that
                // closes on itself, the ball may be found across the
                // circle from where it is.)
                out.push(self.station(t, None)?);
            }
            out
        };
        let mut sag = 0.0_f64;
        for station in &probes {
            for k in 0..2 {
                let side = &self.sides[k];
                // (Halfway there in space: halfway in (u, v) may be
                // the far side of a surface that closes on itself.)
                let halfway = (station.at + station.rails[k]) / 2.0;
                let middle = side.foot(halfway, side.on(station.t, station.at));
                let (p, _) = side.at(side.clamp(middle));
                let chord = (station.rails[k] - station.at).normalize_or_zero();
                sag = sag.max((p - station.at).reject_from(chord).length());
            }
        }
        Ok((margin.max(sag * 2.5), probes))
    }

    /// How far a tool runs on past an end: twice as far as the blend is
    /// wide at `stations` (from the edge to its rails — which a round
    /// between faces that all but run on into each other is much less
    /// than its radius: run on by that, the tool would be far larger
    /// than what it is to cut, and what it cuts no finer than a tool
    /// that large is told from another).
    fn past(&self, margin: f64, stations: &[Station]) -> f64 {
        let wide = stations
            .iter()
            .flat_map(|s| s.rails.map(|rail| rail.distance(s.at)))
            .fold(0.0, f64::max);
        margin * 4.0 + wide * 2.0
    }

    /// The tool that blends the edge: a solid to take from the body (an
    /// outside edge) or to add to it (an inside one). It stops `inset`
    /// short of each end, where that is more than nothing, and else
    /// runs on past an end the body turns away from.
    pub fn tool(&self, id: &str, margin: f64, stretch: &Stretch) -> anyhow::Result<Solid> {
        let (from, to) = self.reach(stretch)?;
        let (margin, probes) = self.held(margin, from, to)?;
        let swept = self.sections(margin, &probes, stretch, (from, to), true)?;
        let closed = self.closed && stretch.inset == [0.0, 0.0] && stretch.half.is_none();
        self.lofted(id, margin, &probes[0], swept, closed)
    }

    /// Whether the tool of `stretch` just ends at the edge's end `end`:
    /// in the middle of the edge, where it is in halves, or where
    /// another piece of its chain goes on from it.
    fn inner(stretch: &Stretch, end: usize) -> bool {
        stretch.half.is_some_and(|half| half != end) || stretch.joined[end]
    }

    /// How far a tool of `stretch` is to run on past the end `end`
    /// along the edge's curve run on, if it is.
    fn rolled_on(&self, stretch: &Stretch, end: usize) -> Option<f64> {
        stretch.rolled_on[end]
            .filter(|_| !Self::inner(stretch, end) && stretch.inset[end] <= 0.0 && !self.closed)
    }

    /// From where to where along the edge's curve (run on past its
    /// ends) the tool of `stretch` goes.
    fn reach(&self, stretch: &Stretch) -> anyhow::Result<(f64, f64)> {
        let inset = stretch.inset;
        let (from, to) = (self.inset(0, inset[0]), self.inset(1, inset[1]));
        let middle = (self.range.0 + self.range.1) / 2.0;
        let (from, to) = match stretch.half {
            Some(0) => (from, middle),
            Some(_) => (middle, to),
            None => (from, to),
        };
        let (from, to) = {
            let (lo, hi) = self.onward.domain();
            let by = |end: usize, t: f64| {
                self.rolled_on(stretch, end).map_or(0.0, |long| {
                    long / self.curve_at(t).derivative_at(t).1.length().max(1e-300)
                })
            };
            // (Not in from the edge's own ends, where the curve run on
            // ends a hair short of them: a run-out's tip is at its end.)
            (
                (from - by(0, from)).max(lo).min(from),
                (to + by(1, to)).min(hi).max(to),
            )
        };
        ensure!(to > from, "nothing is left of the edge to blend");
        Ok((from, to))
    }

    /// The tool's sections from `from` to `to` (see [`Rolled::reach`])
    /// as a spline along the edge: its degree, knots and control
    /// sections, with what the ends of `stretch` ask for. With `plain`,
    /// of the edge's own degree where that is all the blend is.
    fn sections(
        &self,
        margin: f64,
        probes: &[Station],
        stretch: &Stretch,
        (from, to): (f64, f64),
        plain: bool,
    ) -> anyhow::Result<Swept> {
        let inset = stretch.inset;
        let inner = |end: usize| Self::inner(stretch, end);
        let rolled_on = |end: usize| self.rolled_on(stretch, end);
        let tolerance = self.size * 1e-9;
        // The sections change along the edge as a spline over the
        // edge's own knots, of its degree or the third: what the blend
        // of a line or of a circle between flat and round faces is
        // exactly, and anything else as closely as wanted with more
        // knots. (In weighted coordinates, scaled as the edge's own
        // curve is: where it is a polynomial if the edge is.)
        // (Each section looked for from the one before.)
        let last: std::cell::RefCell<Option<Station>> = std::cell::RefCell::new(None);
        // (Along an edge the blend only moves along — straight on, or
        // around an axis — it is the same everywhere, moved: its
        // section at the start, taken to wherever it is asked for.)
        let steady = self.steady().map(|(motion, first)| -> anyhow::Result<_> {
            let ring = self.ring(&first, margin)?;
            Ok((
                motion,
                ring.into_iter().flatten().collect::<Vec<[f64; 4]>>(),
            ))
        });
        let steady = steady.transpose()?;
        let mut flat = |t: f64| -> anyhow::Result<Vec<[f64; 4]>> {
            if let Some((motion, ring)) = &steady {
                let to = motion.to(self.curve_at(t).point_at(t));
                return Ok(ring
                    .iter()
                    .map(|h| {
                        let p = to(Point3::new(h[0], h[1], h[2]));
                        [p.x, p.y, p.z, h[3]]
                    })
                    .collect());
            }
            let station = self.station(t, last.borrow().as_ref())?;
            // Rolled on past an end: the ball rolls on from where it is
            // at the end, not off to some other place it may rest.
            if t < self.range.0 || t > self.range.1 {
                let which = usize::from(t > self.range.1);
                let end = self.end(which)?;
                let gone = station.at.distance(end.at);
                // (And on away from it: an edge's curve run on far may
                // turn back towards the edge.)
                ensure!(
                    (station.at - end.at).dot(-self.away(which)) >= gone * 0.7,
                    "the edge's curve does not run on away from its end"
                );
                let wide = end.rails[0].distance(end.rails[1]);
                for k in 0..2 {
                    ensure!(
                        station.rails[k].distance(end.rails[k]) <= gone * 2.0 + wide * 0.25,
                        "the ball does not roll on past the end"
                    );
                }
            }
            let ring = self.ring(&station, margin)?;
            *last.borrow_mut() = Some(station);
            Ok(ring.into_iter().flatten().collect())
        };
        // (Of the edge's own degree if that is all the blend is; else
        // swept anew, from the start, with nothing left from the try.)
        // (Along the curve run on, where it rolls on past an end.)
        let curve: &Curve3 = if from < self.range.0 || to > self.range.1 {
            &self.onward
        } else {
            &self.curve
        };
        // (Where the ball starts to roll over a rim, the blend turns
        // there: swept in two, end to end.)
        let mut breaks: Vec<f64> = (0..2)
            .filter(|&end| !inner(end) && inset[end] <= 0.0 && rolled_on(end).is_none())
            .filter_map(|end| self.run_out.borrow()[end].as_ref().map(|out| out.from))
            .filter(|t| *t > from && *t < to)
            .collect();
        breaks.sort_by(f64::total_cmp);
        let (degree, mut knots, mut rings) = if breaks.is_empty() {
            let plain = if plain {
                sweep::along_plain(curve, (from, to), tolerance, self.size, &mut flat)
                    .ok()
                    .flatten()
            } else {
                None
            };
            match plain {
                Some(swept) => swept,
                None => {
                    *last.borrow_mut() = None;
                    sweep::along(curve, (from, to), tolerance, self.size, &mut flat)
                        .context("the blend could not be swept")?
                }
            }
        } else {
            let mut cuts = vec![from];
            cuts.extend(breaks);
            cuts.push(to);
            let mut pieces = Vec::new();
            for w in cuts.windows(2) {
                *last.borrow_mut() = None;
                pieces.push(
                    sweep::along(curve, (w[0], w[1]), tolerance, self.size, &mut flat)
                        .context("the blend could not be swept")?,
                );
            }
            end_to_end(pieces)
        };
        let closed = self.closed && inset == [0.0, 0.0] && stretch.half.is_none();
        if closed {
            // All the way around: it ends as it starts.
            let first = rings[0].clone();
            *rings.last_mut().expect("never empty") = first;
        } else {
            // Past an end the body turns away from: the section there,
            // moved on along the edge, and the sweep run on to it as
            // smoothly as it arrives.
            for end in [1, 0] {
                let asked = stretch.on[end].filter(|_| !inner(end));
                if inset[end] > 0.0
                    || inner(end)
                    || rolled_on(end).is_some()
                    || !(self.ends[end].1 || asked.is_some())
                {
                    continue;
                }
                let past = self.past(margin, probes).max(asked.unwrap_or(0.0));
                let n = rings.len();
                let t = if end == 0 { from } else { to };
                let (_, d) = self.curve_at(t).derivative_at(t);
                let speed = d.length().max(1e-300);
                let by = d / speed * if end == 0 { -past } else { past };
                let long = past / speed;
                let (at, next, span) = if end == 0 {
                    (&rings[0], &rings[1], knots[degree + 1] - knots[degree])
                } else {
                    (&rings[n - 1], &rings[n - 2], knots[n] - knots[n - 1])
                };
                // The far section, and the one before it that keeps
                // the sweep's direction where the two join.
                let far: Vec<[f64; 4]> = at
                    .iter()
                    .map(|h| {
                        [
                            h[0] + by.x * h[3],
                            h[1] + by.y * h[3],
                            h[2] + by.z * h[3],
                            h[3],
                        ]
                    })
                    .collect();
                let near: Vec<[f64; 4]> = at
                    .iter()
                    .zip(next)
                    .map(|(h, g)| std::array::from_fn(|c| h[c] + (h[c] - g[c]) * (long / span)))
                    .collect();
                // (Straight on as it comes, of the first degree: its
                // last section moved on is all of it.)
                let same = |a: &[f64; 4], b: &[f64; 4]| {
                    let w = a[3].abs().max(1e-300);
                    (0..3).all(|c| (a[c] - b[c]).abs() <= tolerance * w)
                };
                if degree == 1 && far.iter().zip(&near).all(|(a, b)| same(a, b)) {
                    if end == 0 {
                        rings[0] = far;
                        knots[0] = from - long;
                        knots[1] = from - long;
                    } else {
                        rings[n - 1] = far;
                        let k = knots.len();
                        knots[k - 1] = to + long;
                        knots[k - 2] = to + long;
                    }
                    continue;
                }
                let mut added: Vec<Vec<[f64; 4]>> = (0..degree)
                    .map(|k| {
                        // (Straight on, of the first degree: the far
                        // section is all there is to add.)
                        let f = if degree > 1 {
                            k as f64 / (degree - 1) as f64
                        } else {
                            0.0
                        };
                        far.iter()
                            .zip(&near)
                            .map(|(a, b)| std::array::from_fn(|c| a[c] + (b[c] - a[c]) * f))
                            .collect()
                    })
                    .collect();
                if end == 0 {
                    added.extend(std::mem::take(&mut rings));
                    rings = added;
                    knots.remove(0);
                    let mut all = vec![from - long; degree + 1];
                    all.extend(std::mem::take(&mut knots));
                    knots = all;
                } else {
                    added.reverse();
                    rings.extend(added);
                    knots.pop();
                    knots.extend(std::iter::repeat_n(to + long, degree + 1));
                }
            }
        }
        // An end it runs on past (to be cut back there) is cut off in
        // the plane square to the way it goes there: its section laid
        // into that plane, flat as its cap is. (On curved faces the
        // ball touches them a little out of the plane of its station.)
        let count = self.ring(&probes[0], margin)?[0].len();
        for (end, &short) in inset.iter().enumerate() {
            let runs_on = !closed
                && short <= 0.0
                && !inner(end)
                && !stretch.square[end]
                && (rolled_on(end).is_some()
                    || stretch.on[end].is_some()
                    || self.cut_back.get()[end]);
            if closed || stretch.joined[end] {
                continue;
            }
            let ring = if end == 0 {
                rings.first_mut()
            } else {
                rings.last_mut()
            };
            let Some(ring) = ring else {
                continue;
            };
            let t = if end == 0 { from } else { to };
            if !runs_on {
                // Where a half ends in the middle, the other starts with
                // the very same section: the one found there from the
                // edge itself, not from the section before it.
                if stretch.half.is_some_and(|half| half != end) {
                    let station = self.station(t, None)?;
                    // (In weighted coordinates, scaled as the sweep has
                    // them there: by the weight of a straight side's
                    // point, one in itself.)
                    let scale = ring[count][3];
                    *ring = self
                        .ring(&station, margin)?
                        .into_iter()
                        .flatten()
                        .map(|h| {
                            let w = h[3] * scale;
                            [h[0] * w, h[1] * w, h[2] * w, w]
                        })
                        .collect();
                }
                // An end where it just ends is the blend's own section:
                // the plane of that, the rest of the section laid into
                // it.
                let way = self.curve_at(t).derivative_at(t).1.normalize_or_zero();
                flatten(ring, count, way, self.size * 1e-12);
                continue;
            }
            let way = self.curve_at(t).derivative_at(t).1.normalize_or_zero();
            let points: Vec<Point3> = ring
                .iter()
                .map(|h| Point3::new(h[0], h[1], h[2]) / h[3])
                .collect();
            // (Into the plane through the point of it farthest on past
            // the end: none is moved back, towards the section before.)
            let on = if end == 0 { -way } else { way };
            let centre = points
                .iter()
                .copied()
                .max_by(|a, b| a.dot(on).total_cmp(&b.dot(on)))
                .unwrap_or(Point3::ZERO);
            for (h, p) in ring.iter_mut().zip(points) {
                let q = p - way * (p - centre).dot(way);
                *h = [q.x * h[3], q.y * h[3], q.z * h[3], h[3]];
            }
        }
        Ok((degree, knots, rings))
    }

    /// The tool of the sections `swept` (as [`Rolled::sections`] gives
    /// them, all the way round where `closed`), its section's sides as
    /// they are at `probe`.
    fn lofted(
        &self,
        id: &str,
        margin: f64,
        probe: &Station,
        (degree, knots, rings): Swept,
        closed: bool,
    ) -> anyhow::Result<Solid> {
        let tolerance = self.size * 1e-9;
        let (side, across, _) = self.section(probe)?;
        let line = || (1, vec![0.0, 0.0, 1.0, 1.0]);
        let loft = Loft {
            sides: vec![(side, across), line(), line(), line(), line()],
            counts: self.ring(probe, margin)?.iter().map(Vec::len).collect(),
            degree,
            knots,
            rings,
            closed,
            tolerance: tolerance.max(1e-9),
        };
        let mut scratch = Topo::new();
        let body = sweep::loft(&mut scratch, id, &loft)?;
        let mut tool = workpiece_boolean::read(&scratch, body)?;
        // A round runs on smoothly into the faces its rails lie in.
        if matches!(self.shape, Shape::Fillet { .. }) {
            let rails: Vec<usize> = (0..tool.edges.len())
                .filter(|&e| {
                    tool.edges[e].name.as_deref().is_some_and(|name| {
                        name == format!("{id}/along/v0") || name == format!("{id}/along/v1")
                    })
                })
                .collect();
            for face in &mut tool.faces {
                if face.name.as_deref() == Some(&format!("{id}/side/e0")) {
                    face.smooth = rails.clone();
                }
            }
        }
        Ok(tool)
    }

    /// The blend at the edge's end `end`.
    pub fn end(&self, end: usize) -> anyhow::Result<Station> {
        self.station(if end == 0 { self.range.0 } else { self.range.1 }, None)
    }

    /// The blend halfway along the edge.
    pub fn middle(&self) -> anyhow::Result<Station> {
        self.station((self.range.0 + self.range.1) / 2.0, None)
    }

    /// How far inside what the blend takes of the face on side `k` the
    /// point `p` of that face is: how much nearer to the edge than the
    /// blend's rail is there (less than nothing: that far outside).
    /// `None` if it is not beside the edge but on past an end of it, or
    /// there is no blend where it is.
    pub fn inside(&self, k: usize, p: Point3) -> Option<f64> {
        let (t, far) = self.beside(p)?;
        let station = self.station(t, None).ok()?;
        Some(station.rails[k].distance(station.at) - far)
    }

    /// The edge's point at `t`.
    pub(crate) fn point(&self, t: f64) -> Point3 {
        self.curve.point_at(t)
    }

    /// Where along the edge the point `p` is beside it — where the
    /// edge is nearest to it — and how far from the edge it is there.
    /// `None` if it is on past an end of the edge.
    pub(crate) fn beside(&self, p: Point3) -> Option<(f64, f64)> {
        let (t0, t1) = self.range;
        // Where along the edge it is nearest: from the nearest of a few
        // places, by Newton's method on the way there being square to
        // the edge.
        const N: usize = 64;
        let mut t = t0;
        let mut least = f64::INFINITY;
        let spots = self.spots.get_or_init(|| {
            (0..=N)
                .map(|i| self.curve.point_at(t0 + (t1 - t0) * i as f64 / N as f64))
                .collect()
        });
        for (i, spot) in spots.iter().enumerate() {
            let at = t0 + (t1 - t0) * i as f64 / N as f64;
            let far = spot.distance(p);
            if far < least {
                (least, t) = (far, at);
            }
        }
        let h = (t1 - t0) * 1e-6;
        let lean = |t: f64| {
            let (c, d) = self.curve.derivative_at(t);
            (c - p).dot(d)
        };
        for _ in 0..30 {
            let slope = (lean(t + h) - lean(t - h)) / (2.0 * h);
            if slope.abs() < 1e-300 {
                break;
            }
            let next = t - lean(t) / slope;
            if !next.is_finite() {
                return None;
            }
            let rests = (next - t).abs() <= (t1 - t0) * 1e-13;
            t = next.clamp(t0 - (t1 - t0), t1 + (t1 - t0));
            if rests {
                break;
            }
        }
        let edge = (t1 - t0) * 1e-9;
        if !self.closed && (t <= t0 + edge || t >= t1 - edge) {
            return None;
        }
        let t = t.clamp(t0, t1);
        Some((t, p.distance(self.curve.point_at(t))))
    }

    /// Whether `p`, a point of the face on side `k`, lies in what the
    /// blend takes of that face ([`Rolled::inside`]).
    pub fn takes(&self, k: usize, p: Point3) -> bool {
        self.inside(k, p).is_some_and(|by| by > self.size * 1e-7)
    }

    /// The way along the edge away from its end `end`.
    pub fn away(&self, end: usize) -> Point3 {
        let t = if end == 0 { self.range.0 } else { self.range.1 };
        let along = self.curve.derivative_at(t).1.normalize_or_zero();
        if end == 0 { along } else { -along }
    }

    /// How the blend ends at the edge's end `end`, as far as the tool
    /// can be made to. Where it does not run on past the end, what the
    /// edge ends on is square to it, or runs on from one of its two
    /// faces, and the tool is cut off square there: `None`. Else the
    /// ends of its faces on flat faces at a slant to it, or on flat
    /// faces its own run on from that the edge leaves (where they
    /// become tangent to it, and its blend fades out): the tool is run
    /// on past the end and cut back to each of their planes
    /// ([`EndCut`]). (`margin` is the tool's.)
    pub fn check_end(
        &self,
        solid: &Solid,
        end: usize,
        margin: f64,
    ) -> anyhow::Result<Option<EndCut>> {
        if self.ends[end].1 || self.closed {
            return Ok(None);
        }
        let vertex = self.vertices[end];
        let at = solid.vertices[vertex].point;
        let way = self.away(end);
        // Each face's normal out of the body at the vertex.
        let normals_at = |f: usize| normals_at(solid, f, vertex);
        let beside: Vec<Point3> = self.faces.into_iter().flat_map(normals_at).collect();
        // The edges at the vertex each face has.
        let edges_at = |f: usize| -> Vec<usize> {
            solid.faces[f]
                .loops
                .iter()
                .flatten()
                .filter_map(|step| match step.walk {
                    Walk::Edge { edge, .. } if solid.edges[edge].ends.contains(&vertex) => {
                        Some(edge)
                    }
                    _ => None,
                })
                .collect()
        };
        let shares = |f: usize, g: usize| edges_at(f).iter().any(|e| edges_at(g).contains(e));
        let within = self.size * 1e-9;
        // A point of the edge a little in from the end: which side of
        // a face's plane the edge is on, where it leaves the plane
        // along it.
        let inner = {
            let (t0, t1) = self.range;
            let t = if end == 0 {
                t0 + (t1 - t0) * 0.05
            } else {
                t1 - (t1 - t0) * 0.05
            };
            self.curve.point_at(t)
        };
        // Each plane to cut back to: a point of it and its normal
        // towards the edge; and how far on the tool has to run.
        let mut planes: Vec<(usize, Point3)> = Vec::new();
        // (Curved ones: each with whether it only touches a face of
        // the edge there, where the blend fades out.)
        let mut curved: Vec<(usize, Point3, bool)> = Vec::new();
        let mut slant = 0.0_f64;
        // (The faces of those that only touch a face of the edge.)
        let mut touching: Vec<usize> = Vec::new();
        // Whether a face here runs on smoothly from one of the edge's
        // own, across an edge with it: the body goes on past the end
        // beside the edge, and so does its edge, most likely.
        let mut goes_on = false;
        let taken = |f: usize, planes: &[(usize, Point3)], curved: &[(usize, Point3, bool)]| {
            planes.iter().any(|p| p.0 == f) || curved.iter().any(|c| c.0 == f)
        };
        for f in 0..solid.faces.len() {
            if self.faces.contains(&f) || taken(f, &planes, &curved) {
                continue;
            }
            for n in normals_at(f) {
                let square = n.cross(way).length() <= 1e-6;
                let runs_on = beside.iter().any(|m| n.cross(*m).length() <= 1e-6);
                if square || taken(f, &planes, &curved) {
                    continue;
                }
                let flat = crate::tweak::flat_way(&solid.faces[f].surface, within).is_some();
                let lean = n.dot(way);
                if runs_on {
                    // Run on from a face of the edge: cut back to it
                    // where it is flat, touches that face only here
                    // (they have no edge in common at the end) and the
                    // edge leaves its plane — else ended square, as
                    // ever.
                    // (Not another piece of one of them, of its very
                    // surface.)
                    let piece = self
                        .faces
                        .iter()
                        .any(|&g| *solid.faces[g].surface == *solid.faces[f].surface);
                    let off = n.dot(inner - at);
                    if !piece
                        && lean.abs() <= 0.05
                        && off.abs() > self.size * 1e-7
                        && !self.faces.iter().any(|&g| {
                            normals_at(g).iter().any(|m| n.cross(*m).length() <= 1e-6)
                                && shares(f, g)
                        })
                    {
                        if flat {
                            planes.push((f, n * off.signum()));
                        } else {
                            curved.push((f, n * off.signum(), true));
                        }
                        touching.push(f);
                    } else {
                        goes_on = true;
                    }
                    continue;
                }
                ensure!(
                    lean.abs() > 0.05,
                    "the edge ends on a face it all but runs along: its blend cannot end there"
                );
                if flat {
                    planes.push((f, n * lean.signum()));
                } else {
                    curved.push((f, n * lean.signum(), false));
                }
                slant = slant.max((1.0 - lean * lean).max(0.0).sqrt() / lean.abs());
            }
        }
        // Where the faces go on smoothly past the end, the tool is
        // cut off in the plane through the end square to the edge: run
        // on past it and cut back there. (Its own last section is not
        // flat where the faces are curved: the ball touches them a
        // little out of the plane of its station, so a cap laid over
        // that section would not lie on its edges.)
        let smooth = planes.is_empty() && curved.is_empty() && goes_on;
        if smooth {
            planes.push((usize::MAX, way));
        }
        if planes.is_empty() && curved.is_empty() {
            return Ok(None);
        }
        let mut cut_back = self.cut_back.get();
        if !cut_back[end] {
            cut_back[end] = true;
            self.cut_back.set(cut_back);
            // (What was found before, not beside such an end, is
            // looked for again.)
            self.seen.borrow_mut().clear();
            self.moved.borrow_mut().clear();
        }
        // Where the end is on a face alone, which only touches one of
        // the edge's two faces: the ball rolls on over the other's rim
        // ([`RunOut`]), if it may — and the tool then ends at the end.
        let over = if planes.len() + curved.len() == 1 && touching.len() == 1 {
            self.run_out_over(solid, end, touching[0])?
        } else {
            None
        };
        let rolls_over = over.is_some();
        let along_rim = over.as_ref().map(|out| out.rim_point(out.rim_from));
        if let Some(out) = over {
            self.run_out.borrow_mut()[end] = Some(out);
            self.seen.borrow_mut().clear();
            self.moved.borrow_mut().clear();
        }
        // Far enough on for every point of the section to reach the
        // planes: as far from the edge as the rails are, and the
        // margin, at the slant.
        let station = self.end(end)?;
        let wide = station
            .rails
            .iter()
            .map(|rail| rail.distance(station.at))
            .fold(0.0, f64::max);
        let long = (wide + margin * 2.0) * slant.max(1.0) * 1.5 + margin * 4.0;
        // (Where it fades out on curved faces alone, it ends there: run
        // on, it would hug its own face on past them.)
        let runs_on = !rolls_over && (!planes.is_empty() || curved.iter().any(|c| !c.2));
        Ok(Some(EndCut {
            at,
            keep: planes.into_iter().map(|p| p.1).collect(),
            curved: curved.into_iter().map(|c| (c.0, c.1)).collect(),
            long: runs_on.then_some(long),
            over: along_rim,
            smooth,
        }))
    }
}

/// Blends of edges that run on into each other smoothly, end to end (a
/// tangent chain of them): one tool along them all, with no end between
/// two of them — two tools laid end to end there would meet in a section
/// each has only as closely as it is swept. Each piece is a blend and
/// whether it runs against the chain's way.
pub struct Chain<'a> {
    /// The blends, one after the other, each with whether it runs
    /// against the chain.
    pub pieces: Vec<(&'a Rolled, bool)>,
    /// Whether the last ends where the first starts, smoothly: the
    /// chain has no end.
    pub closed: bool,
}

impl<'a> Chain<'a> {
    /// The piece at the chain's end `end`, and which end of it that is.
    pub fn end(&self, end: usize) -> (&'a Rolled, usize) {
        let (rolled, reversed) = if end == 0 {
            self.pieces[0]
        } else {
            self.pieces[self.pieces.len() - 1]
        };
        (rolled, usize::from(reversed) ^ end)
    }

    /// The vertices the chain ends at.
    pub fn vertices(&self) -> [usize; 2] {
        [0, 1].map(|end| {
            let (rolled, own) = self.end(end);
            rolled.vertices[own]
        })
    }

    /// The way along the chain at its end `end`, away from that end.
    pub fn away(&self, end: usize) -> Point3 {
        let (rolled, own) = self.end(end);
        rolled.away(own)
    }

    /// At each end, how far its tool is to run on straight past it
    /// whatever is there ([`Rolled::on`]).
    pub fn on(&self) -> [Option<f64>; 2] {
        [0, 1].map(|end| {
            let (rolled, own) = self.end(end);
            rolled.on[own]
        })
    }

    /// The tool along the whole chain (see [`Rolled::tool`]), `stretch`
    /// told of the chain's ends; its halves are each from an end to the
    /// middle of the middle piece.
    pub fn tool(&self, id: &str, margin: f64, stretch: &Stretch) -> anyhow::Result<Solid> {
        let n = self.pieces.len();
        if n == 1 && !self.pieces[0].1 && !self.closed {
            return self.pieces[0].0.tool(id, margin, stretch);
        }
        // Each piece's own stretch: the chain's at its ends, joined to
        // the next piece elsewhere.
        let middle = n / 2;
        let which: Vec<usize> = match stretch.half {
            None => (0..n).collect(),
            Some(0) => (0..=middle).collect(),
            Some(_) => (middle..n).collect(),
        };
        let mut parts = Vec::new();
        for i in 0..n {
            let (rolled, reversed) = self.pieces[i];
            let own = |end: usize| usize::from(reversed) ^ end;
            let mut piece = Stretch::default();
            for end in 0..2 {
                let outer = if end == 0 { i == 0 } else { i == n - 1 };
                if outer && !self.closed {
                    piece.inset[own(end)] = stretch.inset[end];
                    piece.on[own(end)] = stretch.on[end];
                    piece.rolled_on[own(end)] = stretch.rolled_on[end];
                    piece.square[own(end)] = stretch.square[end];
                } else {
                    piece.joined[own(end)] = true;
                }
            }
            // (The margin is the whole chain's: the two halves meet
            // in one section.)
            let whole = rolled.reach(&piece)?;
            let (need, mut probes) = rolled.held(margin, whole.0, whole.1)?;
            let mut reach = whole;
            if let Some(half) = stretch.half
                && i == middle
            {
                // (It ends in its middle there, joined to nothing.)
                piece.half = Some(own(half));
                piece.joined[own(1 - half.min(1))] = false;
                reach = rolled.reach(&piece)?;
                probes = rolled.held(margin, reach.0, reach.1)?.1;
            }
            parts.push((rolled, reversed, piece, reach, need, probes));
        }
        // (One margin for all: their sections meet where they join.)
        let margin = parts.iter().map(|p| p.4).fold(margin, f64::max);
        let parts: Vec<_> = parts
            .into_iter()
            .enumerate()
            .filter(|(i, _)| which.contains(i))
            .map(|(_, part)| part)
            .collect();
        let blend_count = |rolled: &Rolled, probe: &Station| -> anyhow::Result<usize> {
            Ok(rolled.ring(probe, margin)?[0].len())
        };
        let count = blend_count(parts[0].0, &parts[0].5[0])?;
        let size = parts[0].0.size;
        let point = |h: &[f64; 4]| Point3::new(h[0], h[1], h[2]) / h[3];
        // (Two sections that are to be one: the balls either piece finds
        // where they join are one ball.)
        let meet = |one: &[[f64; 4]], other: &[[f64; 4]]| -> anyhow::Result<()> {
            let apart = one
                .iter()
                .zip(other)
                .map(|(p, q)| point(p).distance(point(q)))
                .fold(0.0, f64::max);
            let at = (point(&other[0]) + point(&other[count - 1])) / 2.0;
            ensure!(
                apart <= size * 1e-6,
                "the blends of two edges that run on into each other do not meet where the edges do, near ({:.3}, {:.3}, {:.3}): the balls rolled along the two are {apart:.2e} apart there",
                at.x,
                at.y,
                at.z
            );
            Ok(())
        };
        let mut swept: Vec<Swept> = Vec::new();
        for (rolled, reversed, piece, reach, _, probes) in &parts {
            ensure!(
                blend_count(rolled, &probes[0])? == count,
                "the blends of the edges of a chain are not of one kind of section"
            );
            let (degree, mut knots, mut rings) =
                rolled.sections(margin, probes, piece, *reach, false)?;
            if *reversed {
                rings.reverse();
                let (lo, hi) = (knots[0], knots[knots.len() - 1]);
                knots = knots.iter().rev().map(|k| lo + hi - k).collect();
            }
            if let Some((_, _, before)) = swept.last() {
                let last = &before[before.len() - 1];
                let first = &rings[0];
                // (Its faces taken the other way round: each section
                // turned to start where the last one's starts.)
                let (a, b) = (point(&last[0]), point(&last[count - 1]));
                if point(&first[0]).distance(a) > point(&first[0]).distance(b) {
                    let (_, across, _) = rolled.section(&probes[0])?;
                    let (lo, hi) = (across[0], across[across.len() - 1]);
                    ensure!(
                        across
                            .iter()
                            .zip(across.iter().rev())
                            .all(|(x, y)| (x - lo - (hi - y)).abs() <= 1e-12 * (hi - lo)),
                        "the blends of two edges that run on into each other have their faces the other way round, and a section that is not the same turned"
                    );
                    for ring in &mut rings {
                        ring.reverse();
                        ring.rotate_right(count);
                    }
                }
                // Where the two join, the one section both have.
                meet(&rings[0], last)?;
                rings[0] = last.clone();
            }
            swept.push((degree, knots, rings));
        }
        let degree = swept[0].0;
        ensure!(
            swept.iter().all(|s| s.0 == degree),
            "the blends of the edges of a chain are not of one degree along them"
        );
        // One after the other along one parameter.
        let mut at = swept[0].1[swept[0].1.len() - 1];
        for piece in swept.iter_mut().skip(1) {
            let by = at - piece.1[0];
            for k in &mut piece.1 {
                *k += by;
            }
            at = piece.1[piece.1.len() - 1];
        }
        let (degree, knots, mut rings) = end_to_end(swept);
        let closed = self.closed && stretch.half.is_none();
        if closed {
            let first = rings[0].clone();
            meet(&rings[rings.len() - 1], &first)?;
            *rings.last_mut().expect("never empty") = first;
        }
        let (rolled, _, _, _, _, probes) = &parts[0];
        rolled.lofted(id, margin, &probes[0], (degree, knots, rings), closed)
    }
}
