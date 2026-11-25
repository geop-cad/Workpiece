//! The sketch as a constraint system: the equations of each constraint,
//! and solving, dragging and diagnosing with the shared solver.
//!
//! The variables are the coordinates of the points that are not fixed,
//! the radii of circles that are not projected, and the directions of
//! rotation axes. Besides the constraints the user made, every arc holds
//! one of its own (its ends are as far from its center), and every
//! rotation axis one (its direction is one long).

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use shapecraft_solver::{Pull, Scalar, System};

use crate::model::{Id, Kind, Shape, SketchArgs, SketchAxis};

/// One unknown of a sketch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Var {
    /// The x coordinate of a point.
    X(Id),
    /// The y coordinate of a point.
    Y(Id),
    /// The radius of a circle.
    R(Id),
    /// The u part of a rotation axis's direction.
    Du(Id),
    /// The v part of a rotation axis's direction.
    Dv(Id),
}

/// One constraint of the system.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Equation {
    /// An arc's ends are as far from its center.
    Arc(Id),
    /// A rotation axis's direction is one long.
    Unit(Id),
    /// A constraint of the sketch.
    Constraint(Id),
}

/// A 2D vector of scalars.
#[derive(Clone, Copy)]
struct V<S> {
    x: S,
    y: S,
}

impl<S: Scalar> V<S> {
    fn sub(self, o: Self) -> Self {
        V {
            x: self.x - o.x,
            y: self.y - o.y,
        }
    }
    fn dot(self, o: Self) -> S {
        self.x * o.x + self.y * o.y
    }
    fn cross(self, o: Self) -> S {
        self.x * o.y - self.y * o.x
    }
    fn len(self) -> S {
        self.dot(self).sqrt()
    }
}

/// The sketch's equations over its free variables.
pub(crate) struct Equations<'a> {
    args: &'a SketchArgs,
    pub(crate) vars: Vec<Var>,
    index: BTreeMap<Var, usize>,
    equations: Vec<Equation>,
}

impl<'a> Equations<'a> {
    pub(crate) fn new(args: &'a SketchArgs) -> Self {
        let mut vars = Vec::new();
        for (&id, p) in &args.points {
            if !p.fixed {
                vars.extend([Var::X(id), Var::Y(id)]);
            }
        }
        for (&id, e) in &args.entities {
            if matches!(e.shape, Shape::Circle { .. }) && e.projected.is_none() {
                vars.push(Var::R(id));
            }
            if matches!(e.shape, Shape::Axis { .. }) {
                vars.extend([Var::Du(id), Var::Dv(id)]);
            }
        }
        let index = vars.iter().enumerate().map(|(i, &v)| (v, i)).collect();
        // The arcs' and the axes' own equations first: they are never
        // the ones too many.
        let arcs = args.entities.iter().filter_map(|(&id, e)| match e.shape {
            Shape::Arc { .. } => Some(Equation::Arc(id)),
            Shape::Axis { .. } => Some(Equation::Unit(id)),
            _ => None,
        });
        // (A driven dimension holds nothing: it only says what the
        // rest leaves its value to be.)
        let constraints = args
            .constraints
            .iter()
            .filter(|(_, c)| c.dimension.as_ref().is_none_or(|d| !d.driven))
            .map(|(&id, _)| Equation::Constraint(id));
        Self {
            args,
            vars,
            index,
            equations: arcs.chain(constraints).collect(),
        }
    }

    /// The variables' current values.
    pub(crate) fn values(&self) -> Vec<f64> {
        self.vars.iter().map(|&v| self.value(v)).collect()
    }

    fn value(&self, var: Var) -> f64 {
        value_of(self.args, var)
    }

    /// The index of `var`, if it is free.
    pub(crate) fn index_of(&self, var: Var) -> Option<usize> {
        self.index.get(&var).copied()
    }

    /// Every variable equation `k` reads, free or not.
    fn reads(&self, k: usize) -> Vec<Var> {
        let args = self.args;
        let mut vars = Vec::new();
        let point = |vars: &mut Vec<Var>, p: Id| vars.extend([Var::X(p), Var::Y(p)]);
        let (points, entities) = match self.equations[k] {
            Equation::Arc(e) | Equation::Unit(e) => (Vec::new(), vec![e]),
            Equation::Constraint(c) => args.constraints[&c].kind.operands(),
        };
        for p in points {
            point(&mut vars, p);
        }
        for e in entities {
            for p in args.points_of(e) {
                point(&mut vars, p);
            }
            match args.entities.get(&e).map(|e| &e.shape) {
                Some(Shape::Circle { .. }) => vars.push(Var::R(e)),
                Some(Shape::Axis { .. }) => vars.extend([Var::Du(e), Var::Dv(e)]),
                _ => {}
            }
        }
        vars.sort_unstable();
        vars.dedup();
        vars
    }

    /// The constraint of the sketch equation `k` is, if it is one.
    pub(crate) fn constraint(&self, k: usize) -> Option<Id> {
        match self.equations[k] {
            Equation::Constraint(id) => Some(id),
            Equation::Arc(_) | Equation::Unit(_) => None,
        }
    }
}

fn value_of(args: &SketchArgs, var: Var) -> f64 {
    match var {
        Var::X(p) => args.at(p).x,
        Var::Y(p) => args.at(p).y,
        Var::R(e) => args.circle(e).map_or(0.0, |c| c.2),
        Var::Du(e) | Var::Dv(e) => match args.entities.get(&e).map(|e| &e.shape) {
            Some(Shape::Axis { direction, .. }) => {
                if matches!(var, Var::Du(_)) {
                    direction.x
                } else {
                    direction.y
                }
            }
            _ => 0.0,
        },
    }
}

impl System for Equations<'_> {
    fn variables(&self) -> usize {
        self.vars.len()
    }

    fn constraints(&self) -> usize {
        self.equations.len()
    }

    fn uses(&self, k: usize) -> Vec<usize> {
        self.reads(k)
            .into_iter()
            .filter_map(|v| self.index_of(v))
            .collect()
    }

    fn residuals<S: Scalar>(&self, k: usize, x: &[S], out: &mut Vec<S>) {
        // The free variables come in the order `uses` lists them.
        let free: Vec<Var> = self
            .reads(k)
            .into_iter()
            .filter(|v| self.index.contains_key(v))
            .collect();
        let get = |var: Var| match free.iter().position(|v| *v == var) {
            Some(i) => x[i],
            None => S::constant(self.value(var)),
        };
        let eval = Eval {
            args: self.args,
            get: &get,
        };
        match self.equations[k] {
            Equation::Arc(e) => {
                if let Some(Shape::Arc { center, start, end }) =
                    self.args.entities.get(&e).map(|e| &e.shape)
                {
                    let c = eval.point(*center);
                    out.push(eval.point(*start).sub(c).len() - eval.point(*end).sub(c).len());
                }
            }
            Equation::Unit(e) => {
                let d = eval.direction(e);
                out.push(d.len() - S::constant(1.0));
            }
            Equation::Constraint(c) => {
                let constraint = &self.args.constraints[&c];
                let (value, diameter) = constraint
                    .dimension
                    .as_ref()
                    .map_or((0.0, false), |d| (d.value, d.diameter));
                eval.constraint(&constraint.kind, value, diameter, out);
            }
        }
    }
}

/// Evaluates constraints over scalars.
struct Eval<'a, S> {
    args: &'a SketchArgs,
    get: &'a dyn Fn(Var) -> S,
}

impl<S: Scalar> Eval<'_, S> {
    fn point(&self, id: Id) -> V<S> {
        V {
            x: (self.get)(Var::X(id)),
            y: (self.get)(Var::Y(id)),
        }
    }

    /// The direction of rotation axis `id`.
    fn direction(&self, id: Id) -> V<S> {
        V {
            x: (self.get)(Var::Du(id)),
            y: (self.get)(Var::Dv(id)),
        }
    }

    /// The ends of line `id`; of a rotation axis, the point it passes
    /// through and that point moved along it by its direction.
    fn line(&self, id: Id) -> Option<(V<S>, V<S>)> {
        match self.args.entities.get(&id)?.shape {
            Shape::Line(a, b) => Some((self.point(a), self.point(b))),
            Shape::Axis { through, .. } => {
                let (p, d) = (self.point(through), self.direction(id));
                Some((
                    p,
                    V {
                        x: p.x + d.x,
                        y: p.y + d.y,
                    },
                ))
            }
            _ => None,
        }
    }

    /// The center and radius of circle or arc `id`.
    fn circle(&self, id: Id) -> Option<(V<S>, S)> {
        match self.args.entities.get(&id)?.shape {
            Shape::Circle { center, .. } => Some((self.point(center), (self.get)(Var::R(id)))),
            Shape::Arc { center, start, .. } => {
                let c = self.point(center);
                Some((c, self.point(start).sub(c).len()))
            }
            _ => None,
        }
    }

    /// Of spline `id`, the end that is nearest to entity `other`: its
    /// end point and the two control points after it, and what its
    /// curvature there is of the triangle they make (by its degree and
    /// its knots). `None` for what is no spline, has weights, or is too
    /// short to bend.
    fn spline_end(&self, id: Id, other: Id) -> Option<([V<S>; 3], f64)> {
        let Shape::Spline {
            control,
            degree,
            knots,
            weights,
        } = &self.args.entities.get(&id)?.shape
        else {
            return None;
        };
        let (n, d, last) = (control.len(), *degree, knots.len().checked_sub(1)?);
        let plain = weights.iter().all(|w| (*w - 1.0).abs() < 1e-12);
        if n < 3 || d < 2 || !plain || last < d + 1 {
            return None;
        }
        let far = |end: Id| self.args.distance_to(other, self.args.at(end));
        let from_start = far(control[0]) <= far(control[n - 1]);
        let (ids, a, b) = if from_start {
            let b = knots[(d + 2).min(last)] - knots[0];
            (
                [control[0], control[1], control[2]],
                knots[d + 1] - knots[0],
                b,
            )
        } else {
            let b = knots[last] - knots[last.saturating_sub(d + 2)];
            (
                [control[n - 1], control[n - 2], control[n - 3]],
                knots[last] - knots[last - d - 1],
                b,
            )
        };
        if a <= 0.0 || b <= 0.0 {
            return None;
        }
        let factor = (d as f64 - 1.0) / d as f64 * a / b;
        Some((ids.map(|p| self.point(p)), factor))
    }

    /// The signed distance of `p` from the line through `a` and `b`.
    fn offset(p: V<S>, (a, b): (V<S>, V<S>)) -> S {
        let d = b.sub(a);
        d.cross(p.sub(a)) / d.len()
    }

    /// Pushes the equations of `kind` (with `value`, for a dimension;
    /// twice what the kind measures for a `diameter`). Nothing for a
    /// constraint on what is not there, or of the wrong shape.
    fn constraint(&self, kind: &Kind, value: f64, diameter: bool, out: &mut Vec<S>) {
        let value = S::constant(value);
        let sized = |measure: S| {
            if diameter {
                measure * S::constant(2.0) - value
            } else {
                measure - value
            }
        };
        let _: Option<()> = (|| {
            match *kind {
                Kind::Coincident(a, b) => {
                    let d = self.point(b).sub(self.point(a));
                    out.extend([d.x, d.y]);
                }
                Kind::PointOn { point, entity } => {
                    let p = self.point(point);
                    if let Some(line) = self.line(entity) {
                        out.push(Self::offset(p, line));
                    } else {
                        let (c, r) = self.circle(entity)?;
                        out.push(p.sub(c).len() - r);
                    }
                }
                Kind::Along { a, b, axis } => {
                    let d = self.point(b).sub(self.point(a));
                    out.push(match axis {
                        SketchAxis::U => d.y,
                        SketchAxis::V => d.x,
                    });
                }
                Kind::Parallel(a, b) | Kind::Perpendicular(a, b) => {
                    let (a, b) = (self.line(a)?, self.line(b)?);
                    let (da, db) = (a.1.sub(a.0), b.1.sub(b.0));
                    let scale = da.len() * db.len();
                    out.push(if matches!(kind, Kind::Parallel(..)) {
                        da.cross(db) / scale
                    } else {
                        da.dot(db) / scale
                    });
                }
                Kind::Collinear(a, b) => {
                    let (a, b) = (self.line(a)?, self.line(b)?);
                    out.extend([Self::offset(b.0, a), Self::offset(b.1, a)]);
                }
                Kind::Tangent { a, b, internal } => {
                    let (line, round) = match (self.line(a), self.line(b)) {
                        (Some(l), None) => (Some(l), b),
                        (None, Some(l)) => (Some(l), a),
                        _ => (None, a),
                    };
                    // Curves that share an end touch there. "As far as the
                    // radius" has no slope at such a solution (the curve
                    // cannot get nearer), so the equation is about
                    // directions instead.
                    let shared = self.args.points_of(a).into_iter().find(|p| {
                        let ends = |e: Id| match self.args.entities.get(&e).map(|e| &e.shape) {
                            Some(Shape::Line(s, t)) => vec![*s, *t],
                            Some(Shape::Arc { start, end, .. }) => vec![*start, *end],
                            _ => Vec::new(),
                        };
                        ends(a).contains(p) && ends(b).contains(p)
                    });
                    if let Some(line) = line {
                        let (c, r) = self.circle(round)?;
                        if let Some(p) = shared {
                            // The radius to the shared end is square to the line.
                            let (radial, d) = (self.point(p).sub(c), line.1.sub(line.0));
                            out.push(radial.dot(d) / (r * d.len()));
                        } else {
                            out.push(Self::offset(c, line).abs() - r);
                        }
                    } else {
                        let ((c1, r1), (c2, r2)) = (self.circle(a)?, self.circle(b)?);
                        if let Some(p) = shared {
                            // The centers and the shared end are in line.
                            let p = self.point(p);
                            out.push(p.sub(c1).cross(p.sub(c2)) / (r1 * r2));
                        } else {
                            let apart = c2.sub(c1).len();
                            out.push(if internal {
                                apart - (r1 - r2).abs()
                            } else {
                                apart - (r1 + r2)
                            });
                        }
                    }
                }
                Kind::Smooth { spline, other } => {
                    let (p, factor) = self.spline_end(spline, other)?;
                    let t = p[1].sub(p[0]);
                    let long = t.len();
                    // How it bends at its end, to the left of its way.
                    let bend = t.cross(p[2].sub(p[0])) * S::constant(factor) / (long * long * long);
                    if let Some(line) = self.line(other) {
                        // Straight on along the line, without a bend.
                        out.extend([Self::offset(p[1], line), Self::offset(p[2], line)]);
                    } else if let Some((c, r)) = self.circle(other) {
                        // Square to the radius, bent as the circle is
                        // (to the side its centre is on).
                        let radial = c.sub(p[0]);
                        out.push(radial.dot(t) / (r * long));
                        out.push(bend * r - t.cross(radial) / (long * r));
                    } else {
                        // Straight through the end of the other, which
                        // bends the same way: to its right, as it
                        // leaves the other way.
                        let (q, other_factor) = self.spline_end(other, spline)?;
                        let u = q[1].sub(q[0]);
                        let other_long = u.len();
                        let other_bend = u.cross(q[2].sub(q[0])) * S::constant(other_factor)
                            / (other_long * other_long * other_long);
                        out.push(t.cross(u) / (long * other_long));
                        out.push((bend + other_bend) * long);
                    }
                }
                Kind::Equal(a, b) => {
                    if let (Some(a), Some(b)) = (self.line(a), self.line(b)) {
                        out.push(a.1.sub(a.0).len() - b.1.sub(b.0).len());
                    } else {
                        out.push(self.circle(a)?.1 - self.circle(b)?.1);
                    }
                }
                Kind::Midpoint { point, line } => {
                    let (p, (a, b)) = (self.point(point), self.line(line)?);
                    let two = S::constant(2.0);
                    out.extend([p.x * two - a.x - b.x, p.y * two - a.y - b.y]);
                }
                Kind::Symmetric { a, b, line } => {
                    let (p, q, (la, lb)) = (self.point(a), self.point(b), self.line(line)?);
                    let d = lb.sub(la);
                    let half = S::constant(0.5);
                    let middle = V {
                        x: (p.x + q.x) * half,
                        y: (p.y + q.y) * half,
                    };
                    out.push(d.cross(middle.sub(la)) / d.len());
                    out.push(d.dot(q.sub(p)) / d.len());
                }
                Kind::Distance(a, b) => {
                    out.push(self.point(b).sub(self.point(a)).len() - value);
                }
                Kind::DistanceAlong { a, b, axis } => {
                    let d = self.point(b).sub(self.point(a));
                    let along = match axis {
                        SketchAxis::U => d.x,
                        SketchAxis::V => d.y,
                    };
                    out.push(along.abs() - value);
                }
                Kind::PointLine { point, line } => {
                    out.push(sized(
                        Self::offset(self.point(point), self.line(line)?).abs(),
                    ));
                }
                Kind::PointRound { point, round, far } => {
                    let (c, r) = self.circle(round)?;
                    let d = self.point(point).sub(c).len();
                    out.push(if far {
                        d + r - value
                    } else {
                        (d - r).abs() - value
                    });
                }
                Kind::LineRound { line, round, far } => {
                    let (c, r) = self.circle(round)?;
                    let d = Self::offset(c, self.line(line)?).abs();
                    out.push(if far {
                        d + r - value
                    } else {
                        (d - r).abs() - value
                    });
                }
                Kind::Gap(a, b) => {
                    let ((ca, ra), (cb, rb)) = (self.circle(a)?, self.circle(b)?);
                    let d = ca.sub(cb).len();
                    // One inside the other: the wall between them.
                    let inside = d.value() < (ra.value() - rb.value()).abs();
                    out.push(if inside {
                        (ra - rb).abs() - d - value
                    } else {
                        d - ra - rb - value
                    });
                }
                Kind::Size(e) => out.push(sized(self.circle(e)?.1)),
                Kind::Offset { a, b, flip, .. } => {
                    let signed = if flip { -value } else { value };
                    if let (Some(la), Some(lb)) = (self.line(a), self.line(b)) {
                        let (da, db) = (la.1.sub(la.0), lb.1.sub(lb.0));
                        out.push(da.cross(db) / (da.len() * db.len()));
                        out.push(Self::offset(lb.0, la) - signed);
                    } else {
                        let ((c1, r1), (c2, r2)) = (self.circle(a)?, self.circle(b)?);
                        let d = c2.sub(c1);
                        out.extend([d.x, d.y, r2 - r1 - signed]);
                    }
                }
                Kind::Angle(a, b) => {
                    let (a, b) = (self.line(a)?, self.line(b)?);
                    let (da, db) = (a.1.sub(a.0), b.1.sub(b.0));
                    let angle = da.cross(db).atan2(da.dot(db));
                    out.push(angle - value * S::constant(std::f64::consts::PI / 180.0));
                }
            }
            Some(())
        })();
    }
}

/// What of a sketch can still move, and which of its constraints are too
/// many.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Diagnosis {
    /// Whether the constraints hold.
    pub solved: bool,
    /// The degrees of freedom left.
    pub dof: usize,
    /// The points that can still move.
    pub free_points: BTreeSet<Id>,
    /// The entities whose own value can still change: a circle's
    /// radius, a rotation axis's direction.
    pub free_entities: BTreeSet<Id>,
    /// Constraints that add nothing.
    pub redundant: BTreeSet<Id>,
    /// Constraints that contradict earlier ones.
    pub conflicting: BTreeSet<Id>,
}

impl Diagnosis {
    /// Whether nothing can move and nothing is in conflict.
    pub fn is_fully_constrained(&self) -> bool {
        self.solved && self.dof == 0 && self.conflicting.is_empty()
    }
}

/// How constraints over-define a sketch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overdefined {
    /// They say what the sketch says already.
    Redundant,
    /// They contradict what it says.
    Conflicting,
}

impl Overdefined {
    /// What it is called to the user, of a dimension or a constraint.
    pub fn text(self, what: &str) -> String {
        match self {
            Overdefined::Redundant => {
                format!("Over-defined: the rest already fixes this {what}")
            }
            Overdefined::Conflicting => {
                format!("Over-defined: this {what} conflicts with the rest")
            }
        }
    }
}

thread_local! {
    /// The last sketch diagnosed: views ask every frame.
    static LAST: RefCell<Option<(SketchArgs, Diagnosis)>> = const { RefCell::new(None) };
}

impl SketchArgs {
    /// How close to zero the solver brings every equation.
    pub fn solve_tolerance(&self) -> f64 {
        1e-11 * self.scale()
    }

    fn store(&mut self, vars: &[Var], x: &[f64]) {
        for (&var, &value) in vars.iter().zip(x) {
            match var {
                Var::X(p) => {
                    if let Some(p) = self.points.get_mut(&p) {
                        p.at.x = value;
                    }
                }
                Var::Y(p) => {
                    if let Some(p) = self.points.get_mut(&p) {
                        p.at.y = value;
                    }
                }
                Var::R(e) => {
                    if let Some(Shape::Circle { radius, .. }) =
                        self.entities.get_mut(&e).map(|e| &mut e.shape)
                    {
                        *radius = value;
                    }
                }
                Var::Du(e) | Var::Dv(e) => {
                    if let Some(Shape::Axis { direction, .. }) =
                        self.entities.get_mut(&e).map(|e| &mut e.shape)
                    {
                        if matches!(var, Var::Du(_)) {
                            direction.x = value;
                        } else {
                            direction.y = value;
                        }
                    }
                }
            }
        }
    }

    /// Solves the constraints, moving as little as possible, and pulling
    /// each of `pulls` towards its value. Returns whether they hold now;
    /// what cannot be solved stays as it was.
    pub fn pull(&mut self, pulls: &[(Var, f64)]) -> bool {
        let tolerance = self.solve_tolerance();
        let equations = Equations::new(self);
        let mut x = equations.values();
        let mut pull = Pull::none(&x);
        for &(var, target) in pulls {
            if let Some(i) = equations.index_of(var)
                && target.is_finite()
            {
                pull = pull.hold(i, target);
            }
        }
        let report = shapecraft_solver::solve(&equations, &mut x, &pull, tolerance);
        let sane = x.iter().all(|v| v.is_finite());
        let vars = equations.vars.clone();
        if sane {
            self.store(&vars, &x);
            self.refresh_driven();
        }
        report.converged && sane
    }

    /// Solves the constraints, moving as little as possible. Returns
    /// whether they hold now.
    pub fn solve(&mut self) -> bool {
        self.pull(&[])
    }

    /// Sets the value of dimension `id` and solves, going there in steps
    /// so the sketch does not flip over. Returns whether it could; if not,
    /// the value is set but the geometry stays.
    pub fn set_value(&mut self, id: Id, value: f64) -> bool {
        const STEPS: usize = 8;
        // A driven dimension has no value to set: it measures.
        let Some(from) = self
            .constraints
            .get(&id)
            .and_then(|c| c.dimension.as_ref())
            .filter(|d| !d.driven)
            .map(|d| d.value)
        else {
            return false;
        };
        // The pairs of an offset share its value.
        let group = match self.constraints[&id].kind {
            Kind::Offset { group, .. } => Some(group),
            _ => None,
        };
        let ids: Vec<Id> = self
            .constraints
            .iter()
            .filter(|(k, c)| {
                **k == id || matches!(c.kind, Kind::Offset { group: g, .. } if Some(g) == group)
            })
            .map(|(&k, _)| k)
            .collect();
        let set = |args: &mut Self, value: f64| {
            for id in &ids {
                if let Some(d) = args
                    .constraints
                    .get_mut(id)
                    .and_then(|c| c.dimension.as_mut())
                {
                    d.value = value;
                }
            }
        };
        let before = self.clone();
        for step in 1..=STEPS {
            set(self, from + (value - from) * step as f64 / STEPS as f64);
            if !self.solve() {
                *self = before;
                set(self, value);
                return false;
            }
        }
        set(self, value);
        true
    }

    /// What dimension `id` measures as the sketch is: its kind's value
    /// (an angle in degrees, positive); `None` for what is no dimension
    /// or has no single value (an offset's pairs share theirs).
    pub fn measured(&self, id: Id) -> Option<f64> {
        let constraint = self.constraints.get(&id)?;
        let kind = constraint.kind;
        let diameter = constraint.dimension.as_ref()?.diameter;
        let get = |var: Var| value_of(self, var);
        let eval = Eval {
            args: self,
            get: &get,
        };
        let mut out: Vec<f64> = Vec::new();
        // With the value zero, what is left of the equation is the
        // measure itself.
        eval.constraint(&kind, 0.0, diameter, &mut out);
        let [measure] = out[..] else {
            return None;
        };
        Some(match kind {
            Kind::Angle(..) => measure.abs().to_degrees(),
            _ => measure,
        })
    }

    /// Sets what driven dimensions measure to what the sketch is now.
    pub fn refresh_driven(&mut self) {
        let driven: Vec<(Id, f64)> = self
            .constraints
            .keys()
            .filter(|&&id| self.is_driven(id))
            .filter_map(|&id| Some((id, self.measured(id)?)))
            .collect();
        for (id, value) in driven {
            if let Some(d) = self
                .constraints
                .get_mut(&id)
                .and_then(|c| c.dimension.as_mut())
            {
                d.value = value;
            }
        }
    }

    /// Why `with` — this sketch with constraints `added` — is
    /// over-defined by them, if it is: one of them says what the sketch
    /// says already (redundant), or something it cannot (in conflict).
    /// Judged as the solver judges the sketch itself, once solved.
    pub fn overdefined_by(&self, with: &SketchArgs, added: &[Id]) -> Option<Overdefined> {
        let before = self.diagnose();
        let mut solved = with.clone();
        let converged = solved.solve();
        let after = solved.diagnose();
        let new = |set: &BTreeSet<Id>, was: &BTreeSet<Id>| set.iter().any(|id| !was.contains(id));
        let conflicting = (!converged && before.solved)
            || new(&after.conflicting, &before.conflicting)
            || added.iter().any(|id| after.conflicting.contains(id));
        if conflicting {
            Some(Overdefined::Conflicting)
        } else if new(&after.redundant, &before.redundant)
            || added.iter().any(|id| after.redundant.contains(id))
        {
            Some(Overdefined::Redundant)
        } else {
            None
        }
    }

    /// Whether every constraint holds as the sketch is.
    pub fn holds(&self) -> bool {
        let equations = Equations::new(self);
        let x = equations.values();
        let mut out = Vec::new();
        let tolerance = self.solve_tolerance();
        (0..equations.constraints()).all(|k| {
            let local: Vec<f64> = equations.uses(k).iter().map(|&v| x[v]).collect();
            out.clear();
            equations.residuals(k, &local, &mut out);
            out.iter().all(|r: &f64| r.abs() <= tolerance)
        })
    }

    /// What can still move, and which constraints are too many.
    pub fn diagnose(&self) -> Diagnosis {
        if let Some(hit) = LAST.with_borrow(|last| {
            last.as_ref()
                .filter(|(args, _)| args == self)
                .map(|(_, d)| d.clone())
        }) {
            return hit;
        }
        let tolerance = self.solve_tolerance() * 1e3;
        let equations = Equations::new(self);
        let x = equations.values();
        let found = shapecraft_solver::diagnose(&equations, &x, tolerance);
        let mut diagnosis = Diagnosis {
            dof: found.dof,
            ..Diagnosis::default()
        };
        for (i, &var) in equations.vars.iter().enumerate() {
            if found.free[i] {
                match var {
                    Var::X(p) | Var::Y(p) => {
                        diagnosis.free_points.insert(p);
                    }
                    Var::R(e) | Var::Du(e) | Var::Dv(e) => {
                        diagnosis.free_entities.insert(e);
                    }
                }
            }
        }
        let ids = |ks: &[usize]| -> BTreeSet<Id> {
            ks.iter().filter_map(|&k| equations.constraint(k)).collect()
        };
        diagnosis.redundant = ids(&found.redundant);
        diagnosis.conflicting = ids(&found.conflicting);
        // Solved when every equation holds.
        let mut out = Vec::new();
        let mut worst: f64 = 0.0;
        let mut broken = BTreeSet::new();
        for k in 0..equations.constraints() {
            let local: Vec<f64> = equations.uses(k).iter().map(|&v| x[v]).collect();
            out.clear();
            equations.residuals(k, &local, &mut out);
            let own = out.iter().fold(0.0, |w: f64, r: &f64| w.max(r.abs()));
            worst = worst.max(own);
            if own > tolerance {
                broken.extend(equations.constraint(k));
            }
        }
        diagnosis.solved = worst <= tolerance;
        // Where the constraints do not hold though none of them is
        // found to depend on the others (they cannot be met from where
        // the sketch is — a dimension on the wrong side, an angle the
        // other way round), those that do not hold are what is wrong.
        if !diagnosis.solved && diagnosis.conflicting.is_empty() {
            diagnosis.conflicting = broken;
        }
        LAST.with_borrow_mut(|last| *last = Some((self.clone(), diagnosis.clone())));
        diagnosis
    }
}
