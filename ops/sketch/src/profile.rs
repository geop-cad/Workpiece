//! The profile a sketch leaves in the world: its curves as NURBS, cut
//! where they cross, with names and tolerances, and the regions they
//! enclose. Construction geometry stays behind.
//!
//! Lines, arcs and circles are cut where they cross each other; a spline
//! is one edge, end to end (where it crosses something, the regions on
//! either side of the crossing are not told apart).

use std::collections::BTreeMap;
use std::f64::consts::TAU;

use workpiece_core::math::Point2;
use workpiece_core::world::topo::Curve2;
use workpiece_core::world::{DatumPlane, Profile, ProfileEdge, ProfileRegion, ProfileVertex};

use crate::geometry::{angle, arc_curve, unit};
use crate::model::{Id, Kind, Shape, SketchArgs};
use crate::trim::{Carrier, carrier, cuts_by, parameter};

/// Builds a profile's vertices and edges.
struct Pieces<'a> {
    args: &'a SketchArgs,
    profile: Profile,
    /// Points tied by coincidences, by the lowest id of each group.
    same: BTreeMap<Id, Id>,
    /// The vertex of each group of points.
    of_point: BTreeMap<Id, usize>,
    /// How near a crossing has to be to a vertex to be that vertex.
    merge: f64,
    /// The ends of the drawn curves (by the lowest id of each group).
    ends: std::collections::BTreeSet<Id>,
    /// How far apart curves may be and still touch.
    touch: f64,
}

impl Pieces<'_> {
    fn root(&self, mut p: Id) -> Id {
        while self.same[&p] != p {
            p = self.same[&p];
        }
        p
    }

    /// The vertex of sketch point `p`.
    fn vertex(&mut self, p: Id) -> usize {
        let p = self.root(p);
        if let Some(&v) = self.of_point.get(&p) {
            return v;
        }
        self.profile.vertices.push(ProfileVertex {
            name: format!("p{p}"),
            point: self.args.at(p),
            tolerance: self.args.points[&p].tolerance,
        });
        let v = self.profile.vertices.len() - 1;
        self.of_point.insert(p, v);
        v
    }

    /// The vertex at `at`, where entities `a` and `b` cross: one that is
    /// there already, or a new one named after the two.
    fn crossing(&mut self, at: Point2, a: Id, b: Id) -> usize {
        let near = |v: &ProfileVertex| v.point.distance(at) <= self.merge.max(v.tolerance);
        if let Some(v) = self.profile.vertices.iter().position(near) {
            return v;
        }
        let base = format!("x(e{},e{})", a.min(b), a.max(b));
        let taken = |name: &str| self.profile.vertices.iter().any(|v| v.name == name);
        // Curves that cross twice: the crossings are numbered in the
        // order they are met.
        let name = (0..)
            .map(|n| {
                if n == 0 {
                    base.clone()
                } else {
                    format!("{base}#{n}")
                }
            })
            .find(|name| !taken(name))
            .expect("unbounded range");
        self.profile.vertices.push(ProfileVertex {
            name,
            point: at,
            tolerance: self.merge,
        });
        self.profile.vertices.len() - 1
    }

    /// The vertex of an end of `a` or `b` that their crossing at `at`
    /// is, if one is: within the vertex's tolerance of it, or where they
    /// touch (their directions the same there), within how far along them
    /// the touch is uncertain — a gap of `g` between a curve of radius `r`
    /// and one tangent to it leaves the touch `sqrt(2 r g)` unsure.
    fn end_at(&mut self, at: Point2, a: Id, b: Id, own: &Carrier) -> Option<usize> {
        let args = self.args;
        let theirs = carrier(args, b)?;
        let radius = |c: &Carrier| match *c {
            Carrier::Line(..) => f64::INFINITY,
            Carrier::Round(_, r, _) => r,
        };
        let (da, db) = (direction(own, at), direction(&theirs, at));
        let touching = da.perp_dot(db).abs() <= 1e-3;
        let r = radius(own).min(radius(&theirs));
        let unsure = if touching && r.is_finite() {
            4.0 * (2.0 * r * self.touch).sqrt()
        } else {
            0.0
        };
        let ends: Vec<Id> = [a, b]
            .into_iter()
            .flat_map(|e| curve_ends(&args.entities[&e].shape))
            .map(|p| self.root(p))
            .collect();
        let near = ends
            .into_iter()
            .filter_map(|p| self.of_point.get(&p).copied())
            .map(|v| (v, self.profile.vertices[v].point.distance(at)))
            .filter(|&(v, d)| {
                d <= self
                    .merge
                    .max(self.profile.vertices[v].tolerance)
                    .max(unsure)
            })
            .min_by(|x, y| x.1.total_cmp(&y.1))?;
        Some(near.0)
    }

    /// Adds the edge of `entity` from `start` to `end` along `curve`, the
    /// stretch `span` of it; the vertices grow to reach the curve's ends.
    fn edge(
        &mut self,
        entity: Id,
        whole: bool,
        curve: Curve2,
        start: usize,
        end: usize,
        span: (f64, f64),
    ) {
        let (t0, t1) = curve.domain();
        let tolerance = self.args.entities[&entity].tolerance + self.args.solve_tolerance();
        for (t, v) in [(t0, start), (t1, end)] {
            let vertex = &mut self.profile.vertices[v];
            let needed = curve.point_at(t).distance(vertex.point) + tolerance;
            vertex.tolerance = vertex.tolerance.max(needed);
        }
        let name = if whole {
            format!("e{entity}")
        } else {
            let vertices = &self.profile.vertices;
            format!(
                "e{entity}[{}..{}]",
                vertices[start].name, vertices[end].name
            )
        };
        self.profile.edges.push(ProfileEdge {
            name,
            entity: format!("e{entity}"),
            span,
            curve,
            t0,
            t1,
            start,
            end,
            tolerance,
        });
    }

    /// The edges of entity `id`: one, or its pieces between the curves
    /// that cross it.
    fn entity(&mut self, id: Id) {
        let args = self.args;
        let drawn = |e: Id| args.entities.get(&e).is_some_and(|e| !e.construction);
        let ends = match &args.entities[&id].shape {
            Shape::Point(p) => {
                self.vertex(*p);
                return;
            }
            Shape::Line(a, b) => Some((*a, *b)),
            Shape::Arc { start, end, .. } => Some((*start, *end)),
            Shape::Spline { control, .. } => control.first().copied().zip(control.last().copied()),
            Shape::Circle { .. } | Shape::Axis { .. } => None,
        };
        let Some(own) = carrier(args, id) else {
            // A spline: one edge.
            if let (Some(curve), Some((a, b))) = (args.curve(id), ends) {
                let (start, end) = (self.vertex(a), self.vertex(b));
                self.edge(id, true, curve, start, end, (0.0, 1.0));
            }
            return;
        };
        let cuts = cuts_by(args, id, &drawn);
        let (length, at): (f64, Box<dyn Fn(f64) -> Point2>) = match own {
            Carrier::Line(a, b) => (1.0, Box::new(move |t| a + (b - a) * t)),
            Carrier::Round(c, r, None) => (TAU, Box::new(move |t| c + unit(t) * r)),
            Carrier::Round(c, r, Some((start, total))) => {
                (total, Box::new(move |t| c + unit(angle(start) + t) * r))
            }
        };
        let piece = |from: f64, to: f64| match own {
            Carrier::Line(..) => Curve2::line(at(from), at(to)).ok(),
            Carrier::Round(c, ..) => arc_curve(c, at(from), to - from),
        };
        // The stops along the curve: its ends and the cuts, each a vertex.
        let closed = ends.is_none();
        let mut stops: Vec<(f64, usize)> = Vec::new();
        for &(t, other, p) in &cuts {
            // A curve's end where it crosses or touches this one is the
            // place, not the crossing found a little way off.
            let stop = match self.end_at(p, id, other, &own) {
                Some(v) => {
                    let t = parameter(&own, self.profile.vertices[v].point).0;
                    (if closed { t } else { t.clamp(0.0, length) }, v)
                }
                None => (t, self.crossing(p, id, other)),
            };
            stops.push(stop);
        }
        // The ends of other curves held on this one (a T where a curve
        // ends on this one): cut there, whether its crossing is found or
        // not (a curve that ends on one it is tangent to touches it, and
        // rounding may leave them a hair apart, not crossing).
        for constraint in args.constraints.values() {
            let Kind::PointOn { point, entity } = constraint.kind else {
                continue;
            };
            if entity != id || !self.ends.contains(&self.root(point)) {
                continue;
            }
            let v = self.vertex(point);
            let t = parameter(&own, self.profile.vertices[v].point).0;
            if closed || (t > 0.0 && t < length) {
                stops.push((t, v));
            }
        }
        stops.sort_by(|a, b| a.0.total_cmp(&b.0));
        match ends {
            Some((a, b)) => {
                stops.insert(0, (0.0, self.vertex(a)));
                stops.push((length, self.vertex(b)));
            }
            // A circle: round from its first cut back to it; uncut, from
            // a vertex of its own where it starts.
            None => {
                if stops.is_empty() {
                    self.profile.vertices.push(ProfileVertex {
                        name: format!("e{id}/seam"),
                        point: at(0.0),
                        tolerance: args.entities[&id].tolerance,
                    });
                    stops.push((0.0, self.profile.vertices.len() - 1));
                }
                let (first, vertex) = stops[0];
                stops.push((first + TAU, vertex));
            }
        }
        // A cut where the stop before it already is (a curve that
        // touches this one at its end, found a hair inside it; an end held
        // on it that is also found crossing it) is that stop: there is no
        // piece between them.
        let unit_length = match own {
            Carrier::Line(a, b) => a.distance(b),
            Carrier::Round(_, r, _) => r,
        };
        let last = stops.len() - 1;
        let mut kept: Vec<(f64, usize)> = Vec::with_capacity(stops.len());
        for (k, stop) in stops.into_iter().enumerate() {
            let reach = self.merge.max(self.profile.vertices[stop.1].tolerance);
            match kept.last_mut() {
                Some(before)
                    if before.1 == stop.1 && (stop.0 - before.0) * unit_length <= reach =>
                {
                    // At an end, the end's place.
                    if k == last {
                        before.0 = stop.0;
                    }
                }
                _ => kept.push(stop),
            }
        }
        let stops = kept;
        let whole = stops.len() == 2;
        for pair in stops.windows(2) {
            let ((from, start), (to, end)) = (pair[0], pair[1]);
            if let Some(curve) = piece(from, to).filter(|_| to > from) {
                let span = (from / length, (to / length).min(1.0));
                // A circle's piece across its start wraps: it is the end
                // of the circle's range.
                let span = if span.0 >= 1.0 {
                    (span.0 - 1.0, span.1)
                } else {
                    span
                };
                self.edge(id, whole, curve, start, end, span);
            }
        }
    }
}

/// The ends of a curve: none for a point or a circle.
fn curve_ends(shape: &Shape) -> Vec<Id> {
    match shape {
        Shape::Point(_) | Shape::Circle { .. } | Shape::Axis { .. } => Vec::new(),
        Shape::Line(a, b) => vec![*a, *b],
        Shape::Arc { start, end, .. } => vec![*start, *end],
        Shape::Spline { control, .. } => control
            .first()
            .into_iter()
            .chain(control.last())
            .copied()
            .collect(),
    }
}

/// The direction of `carrier` at `p` (on it), one long, either way.
fn direction(carrier: &Carrier, p: Point2) -> Point2 {
    match *carrier {
        Carrier::Line(a, b) => (b - a).normalize_or_zero(),
        Carrier::Round(c, ..) => (p - c).normalize_or_zero().perp(),
    }
}

/// One way along an edge: its index, and whether against its direction.
type Use = (usize, bool);

/// A loop walked counter-clockwise: its edges, its outline and its area.
type Outer = (Vec<Use>, Vec<Point2>, f64);

/// Points along edge `use_` of `profile`, in the direction walked.
fn walk(profile: &Profile, (edge, reversed): (usize, bool)) -> Vec<Point2> {
    let e = &profile.edges[edge];
    let mut points = e.curve.polyline(e.t0, e.t1);
    if reversed {
        points.reverse();
    }
    points
}

/// Whether `p` is inside the polygon `points` (even-odd).
fn inside(points: &[Point2], p: Point2) -> bool {
    let mut inside = false;
    for k in 0..points.len() {
        let (a, b) = (points[k], points[(k + 1) % points.len()]);
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            inside = !inside;
        }
    }
    inside
}

/// The bounded regions `profile`'s edges enclose: the loops walked with
/// the region on the left, counter-clockwise ones as outer boundaries and
/// clockwise ones as the holes of the region around them.
fn regions(profile: &Profile, scale: f64) -> Vec<ProfileRegion> {
    let edges = &profile.edges;
    // Every way along every edge leaves a vertex in some direction.
    let leaves = |(edge, reversed): (usize, bool)| {
        if reversed {
            edges[edge].end
        } else {
            edges[edge].start
        }
    };
    // The ways leave a vertex in the order they cross a small circle
    // around it. Their directions at the vertex alone tie where curves
    // leave it tangent to each other (a round tangent to a side, the two
    // pieces of a side it is tangent to): there the bend tells them apart,
    // and a chord of the same length along each turns from the tangent by
    // half its bend. The circle is smaller than any edge at the vertex, so
    // each edge crosses it once, close by.
    const SAMPLES: usize = 64;
    // The parameter `f` (0 to 1) of the way along its edge.
    let at_fraction = |(edge, reversed): (usize, bool), f: f64| {
        let e = &edges[edge];
        let f = if reversed { 1.0 - f } else { f };
        e.curve.point_at(e.t0 + (e.t1 - e.t0) * f)
    };
    // How far from where it leaves the way gets.
    let reach = |use_: (usize, bool)| {
        let from = at_fraction(use_, 0.0);
        (1..=SAMPLES)
            .map(|k| at_fraction(use_, k as f64 / SAMPLES as f64).distance(from))
            .fold(0.0, f64::max)
    };
    let mut radius: BTreeMap<usize, f64> = BTreeMap::new();
    for edge in 0..edges.len() {
        for use_ in [(edge, false), (edge, true)] {
            let r = radius.entry(leaves(use_)).or_insert(f64::INFINITY);
            *r = r.min(reach(use_) / 4.0);
        }
    }
    let heading = |use_: (usize, bool)| {
        let rho = radius[&leaves(use_)];
        let from = at_fraction(use_, 0.0);
        let beyond = (1..=SAMPLES)
            .find(|&k| at_fraction(use_, k as f64 / SAMPLES as f64).distance(from) >= rho);
        let (Some(k), true) = (beyond, rho > 0.0) else {
            // No way to speak of: its first step.
            let points = walk(profile, use_);
            return angle(points[points.len().min(2) - 1] - points[0]);
        };
        // Where it crosses the circle, by halving.
        let (mut lo, mut hi) = ((k - 1) as f64 / SAMPLES as f64, k as f64 / SAMPLES as f64);
        for _ in 0..60 {
            let mid = (lo + hi) / 2.0;
            if at_fraction(use_, mid).distance(from) < rho {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        angle(at_fraction(use_, hi) - from)
    };
    let mut around: BTreeMap<usize, Vec<(Use, f64)>> = BTreeMap::new();
    for edge in 0..edges.len() {
        for use_ in [(edge, false), (edge, true)] {
            around
                .entry(leaves(use_))
                .or_default()
                .push((use_, heading(use_)));
        }
    }
    for uses in around.values_mut() {
        uses.sort_by(|a, b| a.1.total_cmp(&b.1));
    }
    // After an edge, the next is the one clockwise of the way back: the
    // region stays on the left.
    let next = |(edge, reversed): (usize, bool)| {
        let back = (edge, !reversed);
        let uses = &around[&leaves(back)];
        let i = uses.iter().position(|u| u.0 == back).unwrap_or(0);
        uses[(i + uses.len() - 1) % uses.len()].0
    };
    let mut seen = vec![[false; 2]; edges.len()];
    let mut outers: Vec<Outer> = Vec::new();
    let mut inners: Vec<(Vec<(usize, bool)>, Point2)> = Vec::new();
    for edge in 0..edges.len() {
        for reversed in [false, true] {
            if seen[edge][usize::from(reversed)] {
                continue;
            }
            let mut walked = Vec::new();
            let mut outline = Vec::new();
            let mut use_ = (edge, reversed);
            while !seen[use_.0][usize::from(use_.1)] {
                seen[use_.0][usize::from(use_.1)] = true;
                walked.push(use_);
                let points = walk(profile, use_);
                outline.extend_from_slice(&points[..points.len() - 1]);
                use_ = next(use_);
            }
            let area: f64 = (0..outline.len())
                .map(|k| outline[k].perp_dot(outline[(k + 1) % outline.len()]) / 2.0)
                .sum();
            if area > 1e-12 * scale * scale {
                outers.push((walked, outline, area));
            } else if outline.len() >= 2 {
                // A point just to the left of the walk: in the region
                // around this loop.
                let (a, b) = (outline[0], outline[1]);
                let left = (b - a).normalize_or_zero().perp() * 1e-6 * scale;
                inners.push((walked, (a + b) / 2.0 + left));
            }
        }
    }
    let mut regions: Vec<ProfileRegion> = outers
        .iter()
        .map(|(walked, _, area)| ProfileRegion {
            outer: walked.clone(),
            holes: Vec::new(),
            area: *area,
        })
        .collect();
    // Each clockwise loop is a hole of the smallest region around it; the
    // ones around nothing bound the outside.
    for (walked, probe) in inners {
        let around = outers
            .iter()
            .enumerate()
            .filter(|(_, (_, outline, _))| inside(outline, probe))
            .min_by(|a, b| a.1.2.total_cmp(&b.1.2));
        if let Some((region, _)) = around {
            let hole: f64 = {
                let outline: Vec<Point2> = walked
                    .iter()
                    .flat_map(|&u| {
                        let points = walk(profile, u);
                        points[..points.len() - 1].to_vec()
                    })
                    .collect();
                (0..outline.len())
                    .map(|k| outline[k].perp_dot(outline[(k + 1) % outline.len()]) / 2.0)
                    .sum()
            };
            regions[region].area += hole;
            regions[region].holes.push(walked);
        }
    }
    regions
}

/// The profile of the (solved) sketch `args` on `plane`.
///
/// Points that coincide are one vertex, named after the lowest of their
/// ids (`"p3"`); a vertex where two curves cross is named after them
/// (`"x(e2,e7)"`). An edge is named after its entity (`"e7"`), a piece of
/// one also after the vertices it runs between. A vertex's tolerance
/// covers the ends of its edges, with their tolerances.
pub(crate) fn profile(args: &SketchArgs, plane: DatumPlane) -> Profile {
    // Points tied by coincidences are one vertex.
    let mut same: BTreeMap<Id, Id> = args.points.keys().map(|&p| (p, p)).collect();
    for constraint in args.constraints.values() {
        if let Kind::Coincident(a, b) = constraint.kind
            && same.contains_key(&a)
            && same.contains_key(&b)
        {
            let root = |mut p: Id| {
                while same[&p] != p {
                    p = same[&p];
                }
                p
            };
            let (a, b) = (root(a), root(b));
            same.insert(a.max(b), a.min(b));
        }
    }
    // So are the ends of drawn curves at one place, tied or not: a
    // corner is where two curves end, not only where the sketch says
    // they do.
    let merge = crate::model::DEFAULT_TOLERANCE * args.scale();
    let mut ends: Vec<Id> = args
        .entities
        .values()
        .filter(|e| !e.construction)
        .flat_map(|e| curve_ends(&e.shape))
        .collect();
    ends.sort_by(|a, b| args.at(*a).x.total_cmp(&args.at(*b).x));
    for (k, &a) in ends.iter().enumerate() {
        for &b in &ends[k + 1..] {
            if args.at(b).x - args.at(a).x > merge {
                break;
            }
            if args.at(a).distance(args.at(b)) <= merge {
                let root = |mut p: Id| {
                    while same[&p] != p {
                        p = same[&p];
                    }
                    p
                };
                let (ra, rb) = (root(a), root(b));
                same.insert(ra.max(rb), ra.min(rb));
            }
        }
    }
    let mut pieces = Pieces {
        args,
        profile: Profile::new(plane),
        same,
        of_point: BTreeMap::new(),
        merge,
        ends: std::collections::BTreeSet::new(),
        touch: crate::trim::touch_gap(args),
    };
    let drawn: Vec<Id> = args
        .entities
        .iter()
        .filter(|(_, e)| !e.construction)
        .map(|(&id, _)| id)
        .collect();
    // Ends first, so crossings at an end find its vertex.
    for &id in &drawn {
        let shape = &args.entities[&id].shape;
        if let Shape::Point(p) = shape {
            pieces.vertex(*p);
        }
        for p in curve_ends(shape) {
            pieces.vertex(p);
            let root = pieces.root(p);
            pieces.ends.insert(root);
        }
    }
    for &id in &drawn {
        pieces.entity(id);
    }
    let mut profile = pieces.profile;
    profile.regions = regions(&profile, args.scale());
    // The rotation axis drawn, for what turns the sketch about it.
    profile.axis = args.entities.values().find_map(|e| match e.shape {
        Shape::Axis { through, direction } => Some((args.at(through), direction)),
        _ => None,
    });
    profile
}

/// The ends of the sketch's drawn curves that join nothing: no other
/// curve's end is there, and no curve passes through (as far as the
/// profile merges them).
pub(crate) fn open_ends(args: &SketchArgs) -> Vec<Point2> {
    let merge = crate::model::DEFAULT_TOLERANCE * args.scale();
    let drawn: Vec<Id> = args
        .entities
        .iter()
        .filter(|(_, e)| !e.construction && e.axis.is_none())
        .map(|(&id, _)| id)
        .collect();
    let mut open = Vec::new();
    for &id in &drawn {
        for p in curve_ends(&args.entities[&id].shape) {
            let at = args.at(p);
            let joined = drawn.iter().any(|&other| {
                if other == id {
                    // (A curve closed on itself: both its ends are one.)
                    let ends = curve_ends(&args.entities[&id].shape);
                    return ends
                        .iter()
                        .filter(|&&q| q != p)
                        .any(|&q| args.at(q).distance(at) <= merge);
                }
                curve_ends(&args.entities[&other].shape)
                    .iter()
                    .any(|&q| args.at(q).distance(at) <= merge)
                    || args.distance_to(other, at) <= merge
            });
            if !joined {
                open.push(at);
            }
        }
    }
    open
}
