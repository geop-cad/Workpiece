//! Following the curve where two surfaces cross, from a point of it to a
//! vertex.
//!
//! A step predicts along the curve's direction — across both normals —
//! and corrects by Newton's method onto both surfaces, held in the plane
//! through the prediction so that it cannot slide along the curve. The
//! step is as long as keeps a cubic through the points within the
//! accuracy asked for, measured at each step's middle against the curve
//! itself, so a straight section takes a step or two and a tight one as
//! many as it needs.

use workpiece_core::math::{Point2, Point3};
use workpiece_core::world::topo::{Curve2, Curve3, NurbsSurface};

use crate::fit::{foot, hermite, pull};
use crate::query::normal;
use crate::solve::solve;

/// A point of the curve where two surfaces cross.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Node {
    /// The point.
    pub p: Point3,
    /// Its (u, v) on the first surface.
    pub a: Point2,
    /// Its (u, v) on the second.
    pub b: Point2,
    /// The unit direction of the curve there; zero where the surfaces
    /// touch.
    pub tangent: Point3,
}

/// Why following stopped.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Stop {
    /// It reached this vertex.
    Vertex(usize),
    /// It could not go on.
    Lost(&'static str),
}

/// Two surfaces and how well to follow where they cross.
#[derive(Clone, Copy)]
pub(crate) struct Tracer<'a> {
    /// The first surface.
    pub a: &'a NurbsSurface,
    /// The second.
    pub b: &'a NurbsSurface,
    /// How far the fitted curve may be from the true one.
    pub target: f64,
    /// The size of what is being cut.
    pub scale: f64,
    /// The length of the first step tried.
    pub first: f64,
}

/// A vertex a curve can end at: its number, place and tolerance.
pub(crate) type Goal = (usize, Point3, f64);

impl Tracer<'_> {
    /// The sine of the angle the surfaces cross at, at `n` — no less
    /// than a tenth: how much nearer a point is to each surface than to
    /// their crossing.
    pub(crate) fn lean(&self, n: &Node) -> f64 {
        let normal = |s: &NurbsSurface, q: Point2| {
            let (_, du, dv) = s.derivatives_at(q.x, q.y);
            du.cross(dv).normalize_or_zero()
        };
        normal(self.a, n.a)
            .cross(normal(self.b, n.b))
            .length()
            .clamp(0.1, 1.0)
    }

    fn clamp(&self, a: Point2, b: Point2) -> (Point2, Point2) {
        let fit = |s: &NurbsSurface, q: Point2| {
            let ((u0, u1), (v0, v1)) = s.domain();
            Point2::new(q.x.clamp(u0, u1), q.y.clamp(v0, v1))
        };
        (fit(self.a, a), fit(self.b, b))
    }

    /// `nodes` with the ends that lie on a point a surface collapses to
    /// (the middle of a disk, a sphere's pole) given the (u, v) the
    /// crossing comes to it with. All along the collapsed side is that
    /// one point, and where on it a vertex was found says nothing: the
    /// curve in (u, v) ends where the places before it head for.
    fn settled_at_poles(&self, nodes: &[Node]) -> Vec<Node> {
        let mut out = nodes.to_vec();
        let n = out.len();
        if n < 2 {
            return out;
        }
        let limit = |s: &NurbsSurface, end: Point2, near: Point2, farther: Option<Point2>| {
            let (_, du, dv) = s.derivatives_at(end.x, end.y);
            let size = du.length().max(dv.length());
            // Which of u and v runs along the collapsed side.
            let free = if du.length() <= 1e-9 * size {
                0
            } else if dv.length() <= 1e-9 * size {
                1
            } else {
                return end;
            };
            let (e, a) = (end.to_array(), near.to_array());
            // On straight from the two places before it, by the other
            // parameter.
            let mut value = a[free];
            if let Some(b) = farther.map(|b| b.to_array())
                && (a[1 - free] - b[1 - free]).abs() > 0.0
            {
                let slope = (a[free] - b[free]) / (a[1 - free] - b[1 - free]);
                let on = a[free] + slope * (e[1 - free] - a[1 - free]);
                // (Not past the two by more than they are apart.)
                if (on - a[free]).abs() <= (a[free] - b[free]).abs() * 2.0 {
                    value = on;
                }
            }
            let mut settled = e;
            settled[free] = value;
            Point2::from_array(settled)
        };
        // And an end on a seam, where a surface closes on itself, the
        // side of it the crossing comes from: the one a place a little
        // way back along the crossing is found on from there.
        let side = |s: &NurbsSurface, end: &Node, uv: Point2, near: &Node| -> Point2 {
            let (u, v) = s.domain();
            let back = end.p.lerp(near.p, 1e-3);
            let mut settled = uv;
            for (k, (lo, hi)) in [u, v].into_iter().enumerate() {
                let x = settled.to_array()[k];
                let span = hi - lo;
                if x - lo > span * 1e-9 && hi - x > span * 1e-9 {
                    continue;
                }
                let at = |x: f64| {
                    let mut q = settled.to_array();
                    q[k] = x;
                    Point2::from_array(q)
                };
                let same = s
                    .point_at(at(lo).x, at(lo).y)
                    .distance(s.point_at(at(hi).x, at(hi).y))
                    <= self.scale * 1e-12;
                if !same {
                    continue;
                }
                let (from_lo, from_hi) = (foot(s, back, at(lo)).1, foot(s, back, at(hi)).1);
                // (From the far side the place is as far off as it is
                // from the seam; from the near one, on the surface.)
                if from_lo < from_hi * 1e-3 {
                    settled = at(lo);
                } else if from_hi < from_lo * 1e-3 {
                    settled = at(hi);
                }
            }
            settled
        };
        let (first, near_first) = (out[0], out[1]);
        let (last, near_last) = (out[n - 1], out[n - 2]);
        out[0].a = side(self.a, &first, first.a, &near_first);
        out[0].b = side(self.b, &first, first.b, &near_first);
        out[n - 1].a = side(self.a, &last, last.a, &near_last);
        out[n - 1].b = side(self.b, &last, last.b, &near_last);
        let farther = |k: usize| (n > 2).then_some(k);
        let (first, last) = (out[0], out[n - 1]);
        out[0].a = limit(self.a, first.a, out[1].a, farther(2).map(|k| out[k].a));
        out[0].b = limit(self.b, first.b, out[1].b, farther(2).map(|k| out[k].b));
        out[n - 1].a = limit(
            self.a,
            last.a,
            out[n - 2].a,
            farther(n.saturating_sub(3)).map(|k| out[k].a),
        );
        out[n - 1].b = limit(
            self.b,
            last.b,
            out[n - 2].b,
            farther(n.saturating_sub(3)).map(|k| out[k].b),
        );
        out
    }

    /// `n`, a point of the crossing a step on from `from`, with its
    /// (u, v) on the side of a seam that `from` is on. Where a surface
    /// closes on itself, its two sides are one curve in space, and a
    /// long step up to it may be solved onto the far one — whose (u, v)
    /// is then across the whole surface from the step before it.
    fn same_side(&self, from: &Node, mut n: Node) -> Node {
        let fix = |s: &NurbsSurface, was: Point2, now: Point2| -> Point2 {
            let (u, v) = s.domain();
            let mut out = now;
            for (k, (lo, hi)) in [u, v].into_iter().enumerate() {
                let span = hi - lo;
                let (w, x) = (was.to_array()[k], now.to_array()[k]);
                if (x - w).abs() <= span * 0.5 {
                    continue;
                }
                let other = if x - lo <= span * 1e-6 {
                    hi
                } else if hi - x <= span * 1e-6 {
                    lo
                } else {
                    continue;
                };
                let mut there = out.to_array();
                there[k] = other;
                let there = Point2::from_array(there);
                let same = s
                    .point_at(there.x, there.y)
                    .distance(s.point_at(out.x, out.y))
                    <= self.scale * 1e-12;
                if same {
                    out = there;
                }
            }
            out
        };
        n.a = fix(self.a, from.a, n.a);
        n.b = fix(self.b, from.b, n.b);
        n
    }

    /// The point of the crossing nearest to the guesses `a` and `b`, in
    /// the plane through `anchor` across `along`.
    pub(crate) fn correct(
        &self,
        a: Point2,
        b: Point2,
        anchor: Point3,
        along: Point3,
    ) -> Option<Node> {
        let (mut a, mut b) = self.clamp(a, b);
        let eval = |a: Point2, b: Point2| {
            let (pa, au, av) = self.a.derivatives_at(a.x, a.y);
            let (pb, bu, bv) = self.b.derivatives_at(b.x, b.y);
            let gap = pa - pb;
            let f = [gap.x, gap.y, gap.z, along.dot(pa - anchor)];
            (f, pa, pb, [au, av, -bu, -bv])
        };
        let norm = |f: &[f64; 4]| f.iter().map(|x| x * x).sum::<f64>();
        let (mut f, mut pa, mut pb, mut j) = eval(a, b);
        let done = 1e-15 * self.scale;
        for _ in 0..50 {
            let n = norm(&f);
            if n.sqrt() <= done {
                break;
            }
            let rows: [[f64; 4]; 4] = [
                [j[0].x, j[1].x, j[2].x, j[3].x],
                [j[0].y, j[1].y, j[2].y, j[3].y],
                [j[0].z, j[1].z, j[2].z, j[3].z],
                [along.dot(j[0]), along.dot(j[1]), 0.0, 0.0],
            ];
            // Newton's step where it helps.
            let mut moved = false;
            if let Some(d) = solve(rows, f.map(|x| -x), 4) {
                let (na, nb) = self.clamp(a + Point2::new(d[0], d[1]), b + Point2::new(d[2], d[3]));
                let next = eval(na, nb);
                let m = norm(&next.0);
                if m < n {
                    (a, b) = (na, nb);
                    (f, pa, pb, j) = next;
                    // Barely better: held at the edge of a surface.
                    if m > n * 0.98 {
                        break;
                    }
                    moved = true;
                }
            }
            if moved {
                continue;
            }
            if n.sqrt() <= 1e-12 * self.scale {
                // As near as rounding lets it get.
                break;
            }
            // Damped least squares where the system is singular (a
            // pole) or the step overshoots.
            let mut h = [[0.0; 4]; 4];
            let mut g = [0.0; 4];
            for i in 0..4 {
                for k in 0..4 {
                    h[i][k] = (0..4).map(|r| rows[r][i] * rows[r][k]).sum();
                }
                g[i] = -(0..4).map(|r| rows[r][i] * f[r]).sum::<f64>();
            }
            let trace: f64 = (0..4).map(|i| h[i][i]).sum();
            let mut damping = 1e-9;
            while damping < 1e4 {
                let mut hd = h;
                for (i, row) in hd.iter_mut().enumerate() {
                    row[i] += damping * (row[i] + trace * 1e-9);
                }
                if let Some(d) = solve(hd, g, 4) {
                    let (na, nb) =
                        self.clamp(a + Point2::new(d[0], d[1]), b + Point2::new(d[2], d[3]));
                    let next = eval(na, nb);
                    let m = norm(&next.0);
                    if m < n {
                        (a, b) = (na, nb);
                        (f, pa, pb, j) = next;
                        moved = m <= n * 0.98;
                        break;
                    }
                }
                damping *= 100.0;
            }
            if !moved {
                break;
            }
        }
        if pa.distance(pb) > 1e-10 * self.scale {
            return None;
        }
        let across = normal(self.a, a).cross(normal(self.b, b));
        let tangent = if across.length() > 1e-7 {
            across.normalize()
        } else {
            Point3::ZERO
        };
        Some(Node {
            p: (pa + pb) / 2.0,
            a,
            b,
            tangent,
        })
    }

    /// Follows the crossing from `start` the way its tangent points,
    /// until it comes to one of `goals` — `own`, the vertex it starts
    /// at, only once it has been away — or cannot go on. `inside` says
    /// whether a point is still in both faces.
    pub(crate) fn follow(
        &self,
        start: Node,
        own: usize,
        goals: &[Goal],
        inside: &dyn Fn(&Node) -> bool,
    ) -> (Vec<Node>, Stop) {
        // With long steps along what turns out to be a line or a circle;
        // and, should it leave that after such steps, again without.
        match self.follow_by(start, own, goals, inside, true) {
            Some(found) => found,
            None => self
                .follow_by(start, own, goals, inside, false)
                .unwrap_or((vec![start], Stop::Lost("it cannot be followed"))),
        }
    }

    /// [`follow`](Self::follow), with `strides` or without: once the
    /// first few places lie on a line or a circle, the steps are as
    /// long as that allows, each still corrected onto both surfaces and
    /// checked against the line or circle. `None` if the crossing
    /// leaves the line or circle after such steps: what was stepped
    /// over is then not known closely enough.
    fn follow_by(
        &self,
        start: Node,
        own: usize,
        goals: &[Goal],
        inside: &dyn Fn(&Node) -> bool,
        strides: bool,
    ) -> Option<(Vec<Node>, Stop)> {
        let exactly = self.scale * 1e-11;
        let mut shape: Option<Shape> = None;
        let mut strode = false;
        let mut nodes = vec![start];
        let mut h = self.first;
        let least = (self.scale * 1e-10).min(self.first * 1e-6);
        let mut travelled = 0.0;
        let own_reach = goals
            .iter()
            .find(|g| g.0 == own)
            .map_or(0.0, |g| g.2 * 8.0 + self.target * 8.0)
            .max(self.scale * 1e-7);
        loop {
            workpiece_core::cancel::checkpoint();
            if nodes.len() > 200_000 {
                return Some((nodes, Stop::Lost("it goes on for ever")));
            }
            let cur = *nodes.last().expect("never empty");
            if h < least {
                return Some((nodes, Stop::Lost("it cannot be followed further")));
            }
            // No farther than the next vertex ahead: the step ends on it,
            // or — where the surfaces end short of its middle — just
            // inside its ball.
            let mut go = h;
            let mut short = None;
            let mut aim: Option<Goal> = None;
            for &(id, q, r) in goals {
                if id == own && travelled < own_reach {
                    continue;
                }
                let along = (q - cur.p).dot(cur.tangent);
                let beside = (q - cur.p).reject_from_normalized(cur.tangent).length();
                // (One a step's length ahead to within its own reach is
                // ahead of the step, not beyond it: a step halved on
                // the way to it ends on it again.)
                if along > least
                    && along < go + r + self.target
                    && beside <= along * 0.05 + r + self.target
                {
                    go = go.min(along);
                    short = Some(along - r * 0.5).filter(|s| *s > least);
                    aim = Some((id, q, r));
                }
            }
            let stepped = [Some(go), short].into_iter().flatten().find_map(|go| {
                let guess_a = cur.a + pull(self.a, cur.a, cur.tangent) * go;
                let guess_b = cur.b + pull(self.b, cur.b, cur.tangent) * go;
                self.correct(guess_a, guess_b, cur.p + cur.tangent * go, cur.tangent)
                    .map(|next| (self.same_side(&cur, next), go))
            });
            let Some((mut next, go)) = stepped else {
                // A short way from a vertex where a surface collapses
                // (a pole), no step can be corrected onto the crossing:
                // the vertex itself is on both surfaces, and ends it.
                if let Some((id, q, r)) = aim.filter(|_| go < self.scale * 1e-4) {
                    let (ua, off_a) = foot(self.a, q, cur.a);
                    let (ub, off_b) = foot(self.b, q, cur.b);
                    let within = r + self.target * 2.0;
                    let collapsed = |s: &NurbsSurface, q: Point2| {
                        let (_, du, dv) = s.derivatives_at(q.x, q.y);
                        du.cross(dv).length() <= 1e-9 * du.length().max(dv.length()).powi(2)
                    };
                    let at_pole = collapsed(self.a, ua) || collapsed(self.b, ub);
                    if at_pole && off_a <= within && off_b <= within {
                        nodes.push(Node {
                            p: q,
                            a: ua,
                            b: ub,
                            tangent: cur.tangent,
                        });
                        return Some((nodes, Stop::Vertex(id)));
                    }
                }
                // Or a surface ends here, short of an edge that is off
                // it: the crossing ends within a vertex's reach of it.
                if go < self.scale * 1e-4 && nodes.len() > 1 {
                    let wide = 2.0 / self.lean(&cur);
                    let near = goals
                        .iter()
                        .filter(|g| g.0 != own || travelled >= own_reach)
                        .filter(|g| cur.p.distance(g.1) <= g.2 * wide + self.target * 2.0)
                        .min_by(|x, y| cur.p.distance(x.1).total_cmp(&cur.p.distance(y.1)));
                    if let Some(&(id, _, _)) = near {
                        return Some((nodes, Stop::Vertex(id)));
                    }
                }
                h = go / 2.0;
                continue;
            };
            let step = next.p - cur.p;
            let length = step.length();
            // (Where the surfaces touch, a point is on both of them to
            // within rounding for some way about the place: the step
            // that was aimed at a vertex there and came down as near
            // as that is at it.)
            if let Some((id, q, r)) = aim
                && go < self.scale * 1e-3
                && next.p.distance(q) <= self.scale * 1e-6
                && next.p.distance(q) > r + self.target * 2.0
            {
                let (ua, off_a) = foot(self.a, q, next.a);
                let (ub, off_b) = foot(self.b, q, next.b);
                let within = r + self.target * 2.0;
                let flat = normal(self.a, ua).cross(normal(self.b, ub)).length() <= 1e-6;
                if flat && off_a <= within && off_b <= within {
                    nodes.push(Node {
                        p: q,
                        a: ua,
                        b: ub,
                        tangent: cur.tangent,
                    });
                    return Some((nodes, Stop::Vertex(id)));
                }
            }
            if let Some((id, q, r)) = aim
                && next.tangent == Point3::ZERO
                && next.p.distance(q) <= r + self.target * 2.0
            {
                // At a vertex where the surfaces touch: branches of
                // their crossing meet there, and this one ends.
                nodes.push(Node {
                    tangent: cur.tangent,
                    ..next
                });
                return Some((nodes, Stop::Vertex(id)));
            }
            if next.tangent == Point3::ZERO {
                // The surfaces touch ahead: creep up to it, then stop.
                if go > self.scale * 1e-6 {
                    h = go / 2.0;
                    continue;
                }
                // Or they touch at one point only, where branches of
                // their crossing pass through each other (two vaults
                // at their crowns): this one goes straight on, a step
                // beyond it.
                let beyond = [1e-3, 3e-3, 1e-2].into_iter().find_map(|far| {
                    let far = self.scale * far;
                    let guess_a = cur.a + pull(self.a, cur.a, cur.tangent) * far;
                    let guess_b = cur.b + pull(self.b, cur.b, cur.tangent) * far;
                    let found =
                        self.correct(guess_a, guess_b, cur.p + cur.tangent * far, cur.tangent)?;
                    let onward = found.tangent.dot(cur.tangent);
                    let gone = (found.p - cur.p).dot(cur.tangent);
                    (onward.abs() > 0.9 && gone > far * 0.5 && inside(&found)).then(|| Node {
                        tangent: found.tangent * onward.signum(),
                        ..found
                    })
                });
                if let Some(next) = beyond {
                    travelled += next.p.distance(cur.p);
                    nodes.push(next);
                    h = self.first;
                    continue;
                }
                return Some((nodes, Stop::Lost("the faces touch along where they cross")));
            }
            if next.tangent.dot(cur.tangent) < 0.0 {
                next.tangent = -next.tangent;
            }
            if length < go * 0.5 || length > go * 2.0 || next.tangent.dot(cur.tangent) < 0.98 {
                h = go / 2.0;
                continue;
            }
            // Along a line or a circle: on it still, or no longer.
            let striding = shape.as_ref().is_some_and(|s| s.holds(next.p, exactly));
            if shape.is_some() && !striding {
                if strode {
                    return None;
                }
                shape = None;
            }
            // The cubic through the two, at its middle, against the
            // crossing itself.
            let middle = (cur.p + next.p) / 2.0 + (cur.tangent - next.tangent) * (length / 8.0);
            let way = (cur.tangent + next.tangent).normalize();
            let off = if striding {
                Some(0.0)
            } else {
                self.correct((cur.a + next.a) / 2.0, (cur.b + next.b) / 2.0, middle, way)
                    .map(|m| m.p.distance(middle))
            };
            let Some(off) = off else {
                h = go / 2.0;
                continue;
            };
            // And the cubics through the two in each surface's (u, v),
            // carried to space: a straight section can be far from
            // straight there.
            let flat = |s: &NurbsSurface, from: Point2, to: Point2| {
                let (d0, d1) = (pull(s, from, cur.tangent), pull(s, to, next.tangent));
                let at = (from + to) / 2.0 + (d0 - d1) * (length / 8.0);
                s.point_at(at.x, at.y).distance(middle)
            };
            let off = if striding {
                0.0
            } else {
                off.max(flat(self.a, cur.a, next.a))
                    .max(flat(self.b, cur.b, next.b))
            };
            if off > self.target && go > least * 4.0 {
                h = go / 2.0;
                continue;
            }
            // A vertex on the way: the nearest to here that the step
            // passes.
            let mut reached: Option<(f64, usize, Node)> = None;
            for &(id, q, r) in goals {
                if id == own && travelled < own_reach {
                    continue;
                }
                let along = (q - cur.p).dot(step) / (length * length);
                if !(-0.01..=1.01).contains(&along) {
                    continue;
                }
                // (One behind the step by more than its own reach was
                // passed, or left, already: a hundredth of a long
                // stride back is a long way.)
                if along < 0.0 && cur.p.distance(q) > r * 10.0 + self.target {
                    continue;
                }
                let beside = (q - (cur.p + step * along)).length();
                if beside > length * 0.25 + r * 10.0 + self.target {
                    continue;
                }
                let f = along.clamp(0.0, 1.0);
                let within = r + self.target * 2.0;
                // The crossing's point nearest the vertex; where the
                // surfaces end short of it, the step's own end in its
                // ball. A vertex within its tolerance of both surfaces
                // is further than that from where they cross, the
                // shallower they cross — and further again where a
                // surface ends short of an edge that is off it.
                let wide = |at: &Node| 2.0 * r / self.lean(at) + self.target * 2.0;
                let snapped = self
                    .correct(cur.a.lerp(next.a, f), cur.b.lerp(next.b, f), q, way)
                    .map(|at| self.same_side(&cur, at))
                    .filter(|at| at.p.distance(q) <= wide(at));
                let mut at = match snapped {
                    Some(at) => at,
                    None if next.p.distance(q) <= within.max(wide(&next)) => next,
                    None => continue,
                };
                if at.tangent.dot(way) < 0.0 {
                    at.tangent = -at.tangent;
                }
                if at.tangent == Point3::ZERO {
                    at.tangent = way;
                }
                if reached.as_ref().is_none_or(|x| along < x.0) {
                    reached = Some((along, id, at));
                }
            }
            if let Some((_, id, at)) = reached {
                // A last step too short to fit through takes the place
                // of the one before.
                if nodes.len() > 1 && at.p.distance(cur.p) < length * 1e-3 {
                    nodes.pop();
                }
                nodes.push(at);
                return Some((nodes, Stop::Vertex(id)));
            }
            if !inside(&next) {
                return Some((
                    nodes,
                    Stop::Lost("it leaves a face where nothing was found to cross"),
                ));
            }
            nodes.push(next);
            travelled += length;
            strode |= striding;
            if strides && nodes.len() == 5 {
                let places: Vec<Point3> = nodes.iter().map(|n| n.p).collect();
                shape = Shape::of(&places, exactly);
            }
            match &shape {
                // (As far as a vertex ahead is still seen from here.)
                Some(shape) => h = shape.stride().min(self.scale * 0.25),
                None if off < self.target / 32.0 => h = (h * 2.0).min(self.scale * 0.25),
                None => {}
            }
        }
    }

    /// The curve through `nodes` and its images in the two surfaces'
    /// (u, v), all with one parameter (the length along the chords), and
    /// how far they stray from each other.
    pub(crate) fn curves(&self, nodes: &[Node]) -> Option<(Curve3, Curve2, Curve2, f64)> {
        // A crossing that is a straight line or an arc of a circle (a
        // plane through a cylinder, two planes) is that line or arc
        // itself, not a spline through the places found along it.
        let nodes = &self.settled_at_poles(nodes);
        let places: Vec<Point3> = nodes.iter().map(|n| n.p).collect();
        if let Some((curve, at)) = simplest(&places, self.scale * 1e-11) {
            let mut in_a = Vec::with_capacity(nodes.len());
            let mut in_b = Vec::with_capacity(nodes.len());
            for (n, &t) in nodes.iter().zip(&at) {
                let way = curve.derivative_at(t).1;
                in_a.push((t, n.a, pull(self.a, n.a, way)));
                in_b.push((t, n.b, pull(self.b, n.b, way)));
            }
            // In (u, v), exactly where that is plain: on a flat face,
            // or where the crossing is a straight line there (around a
            // cylinder, along it); else through the places found.
            let range = curve.domain();
            let near = self.scale * 1e-10;
            let (first, last) = (&nodes[0], &nodes[nodes.len() - 1]);
            let exact = |surface, from: Point2, to: Point2| {
                let flat = crate::fit::on_flat(surface, &curve, range).filter(|f| f.off <= near);
                let found =
                    flat.or_else(|| crate::fit::straight(surface, &curve, range, from, to, near));
                found.map(|f| f.curve)
            };
            // (The places may be far apart along a line or a circle:
            // between them, more are found on the curve itself, each
            // from where its neighbours are in (u, v), until the curve
            // through them all is as close as asked.)
            let places_a: Vec<Point2> = nodes.iter().map(|n| n.a).collect();
            let places_b: Vec<Point2> = nodes.iter().map(|n| n.b).collect();
            let filled = |surface: &NurbsSurface, uv: &[Point2]| -> Option<Curve2> {
                let sample = |t: f64, here: Point2| {
                    let way = curve.derivative_at(t).1;
                    (t, here, pull(surface, here, way))
                };
                let mut all: Vec<(f64, Point2, Point2)> = at
                    .iter()
                    .zip(uv)
                    .map(|(t, here)| sample(*t, *here))
                    .collect();
                // A place more wherever the curve through them strays
                // from the surface's own image of the crossing between
                // two of them: many where it swings round in (u, v)
                // within a short stretch (by a point the surface comes
                // to), few along the rest.
                let mut fitted = hermite(&all)?;
                for _ in 0..48 {
                    let mut more = Vec::with_capacity(all.len() * 2);
                    let mut added = false;
                    for pair in all.windows(2) {
                        more.push(pair[0]);
                        let t = (pair[0].0 + pair[1].0) / 2.0;
                        let p = curve.point_at(t);
                        let q = fitted.point_at(t);
                        if surface.point_at(q.x, q.y).distance(p) > self.target
                            && pair[1].0 - pair[0].0 > (range.1 - range.0) * 1e-9
                        {
                            let here = foot(surface, p, pair[0].1.lerp(pair[1].1, 0.5)).0;
                            more.push(sample(t, here));
                            added = true;
                        }
                    }
                    more.push(all[all.len() - 1]);
                    if !added || more.len() > 4096 {
                        break;
                    }
                    all = more;
                    fitted = hermite(&all)?;
                }
                Some(fitted)
            };
            // Or, quicker where it works, the curve's image as it is
            // fitted from its start: taken if it passes through where
            // the places are in (u, v) — around a seam or by a point
            // the surface comes to, it may run elsewhere.
            let image = |surface: &NurbsSurface, uv: &[Point2]| -> Option<Curve2> {
                let ((u0, u1), (v0, v1)) = surface.domain();
                let span = Point2::new(u1 - u0, v1 - v0);
                // (From its start; and, should that run elsewhere — a
                // start on a seam is on either side of it — from a
                // place along it, which is on one.)
                let middle = at.len() / 2;
                [0, middle].into_iter().find_map(|k| {
                    let seed = (at[k], uv[k]);
                    let fitted =
                        crate::fit::project(surface, &curve, range, seed, self.target)?.curve;
                    let through = at
                        .iter()
                        .zip(uv)
                        .all(|(t, here)| ((fitted.point_at(*t) - *here) / span).length() <= 1e-6);
                    through.then_some(fitted)
                })
            };
            let pa = exact(self.a, first.a, last.a)
                .or_else(|| image(self.a, &places_a))
                .or_else(|| filled(self.a, &places_a));
            let pb = exact(self.b, first.b, last.b)
                .or_else(|| image(self.b, &places_b))
                .or_else(|| filled(self.b, &places_b));
            if let (Some(pa), Some(pb)) = (pa, pb) {
                let mut off = 0.0_f64;
                for pair in at.windows(2) {
                    for f in [0.25, 0.5, 0.75] {
                        let t = pair[0] + (pair[1] - pair[0]) * f;
                        let p = curve.point_at(t);
                        let (qa, qb) = (pa.point_at(t), pb.point_at(t));
                        off = off
                            .max(self.a.point_at(qa.x, qa.y).distance(p))
                            .max(self.b.point_at(qb.x, qb.y).distance(p));
                    }
                }
                return Some((curve, pa, pb, off));
            }
        }
        let mut t = 0.0;
        let mut space = Vec::with_capacity(nodes.len());
        let mut in_a = Vec::with_capacity(nodes.len());
        let mut in_b = Vec::with_capacity(nodes.len());
        for (k, n) in nodes.iter().enumerate() {
            if k > 0 {
                let step = n.p.distance(nodes[k - 1].p);
                if step <= 0.0 {
                    return None;
                }
                t += step;
            }
            space.push((t, n.p, n.tangent));
            in_a.push((t, n.a, pull(self.a, n.a, n.tangent)));
            in_b.push((t, n.b, pull(self.b, n.b, n.tangent)));
        }
        let (curve, pa, pb) = (hermite(&space)?, hermite(&in_a)?, hermite(&in_b)?);
        let mut off = 0.0_f64;
        for pair in space.windows(2) {
            for f in [0.25, 0.5, 0.75] {
                let t = pair[0].0 + (pair[1].0 - pair[0].0) * f;
                let p = curve.point_at(t);
                let (qa, qb) = (pa.point_at(t), pb.point_at(t));
                off = off
                    .max(self.a.point_at(qa.x, qa.y).distance(p))
                    .max(self.b.point_at(qb.x, qb.y).distance(p));
            }
        }
        Some((curve, pa, pb, off))
    }
}

/// A line or a circle that the first places of a crossing lie on.
enum Shape {
    /// The line through a point, along a direction.
    Line(Point3, Point3),
    /// The circle about a centre, square to an axis, of a radius.
    Circle(Point3, Point3, f64),
}

impl Shape {
    /// The line or the circle that all of `places` (five of them) lie
    /// on to within `within`, if there is one.
    fn of(places: &[Point3], within: f64) -> Option<Self> {
        let (a, b, c) = (
            places[0],
            places[places.len() / 2],
            places[places.len() - 1],
        );
        let way = (c - a).try_normalize()?;
        let line = Shape::Line(a, way);
        if places.iter().all(|p| line.holds(*p, within)) {
            return Some(line);
        }
        let (ab, ac) = (b - a, c - a);
        let normal = ab.cross(ac);
        let area = normal.length_squared();
        if area <= 1e-18 * ab.length_squared() * ac.length_squared() {
            return None;
        }
        let centre = a
            + (normal.cross(ab) * ac.length_squared() + ac.cross(normal) * ab.length_squared())
                / (2.0 * area);
        let circle = Shape::Circle(centre, normal.normalize(), centre.distance(a));
        places
            .iter()
            .all(|p| circle.holds(*p, within))
            .then_some(circle)
    }

    /// Whether `p` is on it, to within `within`.
    fn holds(&self, p: Point3, within: f64) -> bool {
        match *self {
            Shape::Line(a, way) => (p - a).reject_from_normalized(way).length() <= within,
            Shape::Circle(centre, axis, radius) => {
                let d = p - centre;
                (d.length() - radius).abs() <= within && d.dot(axis).abs() <= within
            }
        }
    }

    /// How long a step along it may be: any length along a line; along
    /// a circle a twentieth of a radian short of a tenth, within which
    /// a vertex ahead is still seen along the tangent.
    fn stride(&self) -> f64 {
        match *self {
            Shape::Line(..) => f64::INFINITY,
            Shape::Circle(_, _, radius) => radius * 0.085,
        }
    }
}

/// The line, or the arc of a circle, that runs through all of `places`
/// in their order to within `within` — if there is one — and where along
/// it each place is (rising). A crossing that comes back to where it
/// started is the whole circle.
fn simplest(places: &[Point3], within: f64) -> Option<(Curve3, Vec<f64>)> {
    let n = places.len().checked_sub(1)?;
    if n < 1 {
        return None;
    }
    let (a, end) = (places[0], places[n]);
    let closed = n >= 3 && a.distance(end) <= within;
    if !closed {
        let chord = end - a;
        let long = chord.length();
        let way = chord.try_normalize()?;
        let mut at: Vec<f64> = places.iter().map(|p| (*p - a).dot(way) / long).collect();
        // (The ends are the ends, to the last bit.)
        at[0] = 0.0;
        at[n] = 1.0;
        let straight = places
            .iter()
            .all(|p| (*p - a).reject_from_normalized(way).length() <= within);
        if straight && at.windows(2).all(|w| w[1] > w[0]) {
            return Some((Curve3::line(a, end).ok()?, at));
        }
    }
    if n < 3 {
        return None;
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
    // In pieces of a quarter turn at most, each a rational quadratic.
    let pieces = (all / std::f64::consts::FRAC_PI_2 - 1e-9).ceil().max(1.0) as usize;
    let each = all / pieces as f64;
    let on = |angle: f64, far: f64| centre + (x * angle.cos() + y * angle.sin()) * far;
    let w = (each / 2.0).cos();
    let mut points = vec![on(0.0, radius)];
    let mut weights = vec![1.0];
    let mut knots = vec![0.0; 3];
    for k in 0..pieces {
        let from = each * k as f64;
        points.extend([on(from + each / 2.0, radius / w), on(from + each, radius)]);
        weights.extend([w, 1.0]);
        knots.extend([(k + 1) as f64; 2]);
    }
    knots.push(pieces as f64);
    if closed {
        // (Back to the very place it left.)
        let last = points.len() - 1;
        points[last] = points[0];
    }
    let arc = Curve3::new(2, knots, points, Some(weights)).ok()?;
    // Where the arc is at an angle: within its piece, the tangent of
    // half the angle from the piece's middle runs evenly with the
    // parameter (the piece is a circle seen from a point on it).
    let half = (each / 4.0).tan();
    let mut at: Vec<f64> = angles
        .iter()
        .map(|&angle| {
            let angle = angle.min(all);
            let piece = ((angle / each).floor() as usize).min(pieces - 1);
            let from_middle = angle - each * piece as f64 - each / 2.0;
            let within = (1.0 + (from_middle / 2.0).tan() / half) / 2.0;
            piece as f64 + within.clamp(0.0, 1.0)
        })
        .collect::<Vec<f64>>();
    // (The ends are the ends, to the last bit.)
    at[0] = 0.0;
    at[n] = pieces as f64;
    if !at.windows(2).all(|w| w[1] > w[0]) {
        return None;
    }
    Some((arc, at))
}
