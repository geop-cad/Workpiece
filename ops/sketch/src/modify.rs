//! What the sketch's buttons do with what is selected: constraints,
//! dimensions, and changes to the geometry. Each says why it cannot be
//! done when the selection does not fit, so its button can.

use shapecraft_core::handles::CanvasView;
use shapecraft_core::math::Point2;

use crate::model::{Id, Item, Kind, Shape, SketchArgs, SketchAxis};
use crate::solve::Overdefined;
use crate::tools::SketchState;

/// Something a button does with the selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    /// Points coincide, a point lies on a curve, or circles share a
    /// center.
    Coincident,
    /// Level as the view shows it.
    Horizontal,
    /// Upright as the view shows it.
    Vertical,
    /// Lines are parallel.
    Parallel,
    /// Lines are perpendicular.
    Perpendicular,
    /// Lines lie on one line.
    Collinear,
    /// A line and a circle, or two circles, touch.
    Tangent,
    /// A spline runs on from a line, an arc or another spline with
    /// its direction and its curvature (G2).
    Curvature,
    /// Lines are as long, circles as large.
    Equal,
    /// A point is the middle of a line.
    Midpoint,
    /// Two points mirror each other in a line.
    Symmetric,
    /// Points stay where they are (or no longer do).
    Fix,
    /// Curves are construction geometry (or no longer are).
    Construction,
    /// Remove what is selected.
    Delete,
    /// Copy what is selected to the other side of a line, mirrored.
    Mirror,
    /// Copy the selected curves, a distance to their side.
    Offset,
    /// Round the corner where two lines meet.
    Fillet,
    /// Cut the corner where two lines meet off.
    Chamfer,
    /// Split a line or an arc at a point.
    Split,
    /// Run a line on until it meets another curve.
    Extend,
    /// Copy what is selected, a little to the side, to be dragged.
    Copy,
    /// Repeat what is selected along a line: a copy for each step of
    /// the line selected last.
    LinearPattern,
    /// Repeat what is selected around the point selected with it.
    CircularPattern,
    /// Make projected curves the sketch's own: free of the model.
    Detach,
    /// Turn what is selected about the point selected with it.
    Rotate,
    /// Make what is selected larger or smaller, from the point selected
    /// with it.
    Scale,
}

/// The selection, sorted by what things are.
#[derive(Debug, Default)]
pub(crate) struct Picked {
    pub(crate) points: Vec<Id>,
    pub(crate) lines: Vec<Id>,
    pub(crate) rounds: Vec<Id>,
    /// Splines and constraints: all but a curvature constraint leave
    /// them alone.
    pub(crate) others: usize,
    /// The splines among those.
    pub(crate) splines: Vec<Id>,
}

pub(crate) fn picked(args: &SketchArgs, selection: &[Item]) -> Picked {
    let mut picked = Picked::default();
    for item in selection {
        match *item {
            Item::Point(p) if args.points.contains_key(&p) => picked.points.push(p),
            Item::Entity(e) => match args.entities.get(&e).map(|e| &e.shape) {
                Some(Shape::Point(p)) => picked.points.push(*p),
                Some(Shape::Line(..) | Shape::Axis { .. }) => picked.lines.push(e),
                Some(Shape::Circle { .. } | Shape::Arc { .. }) => picked.rounds.push(e),
                Some(Shape::Spline { .. }) => {
                    picked.splines.push(e);
                    picked.others += 1;
                }
                _ => picked.others += 1,
            },
            _ => picked.others += 1,
        }
    }
    picked
}

/// What an action would do.
enum Plan {
    Constrain(Vec<Kind>),
    Fix(Vec<Id>),
    Construction(Vec<Id>),
    Delete,
    Mirror(Id),
    Offset,
    Fillet(Id),
    Chamfer(Id),
    Split(Id, Id),
    Extend(Id, Id),
    Copy(Vec<Id>),
    Linear(Vec<Id>, Id),
    Circular(Vec<Id>, Id),
    Detach(Vec<Id>),
    Turn(Vec<Id>, Id, bool),
}

fn plan(args: &SketchArgs, state: &SketchState, action: Action) -> Result<Plan, String> {
    let p = picked(args, &state.selection);
    let counts = (p.points.len(), p.lines.len(), p.rounds.len(), p.others);
    let need = |what: &str| Err(format!("Select {what}"));
    let horizontal = state.horizontal();
    let pairs = |ids: &[Id], make: &dyn Fn(Id, Id) -> Kind| -> Vec<Kind> {
        ids[1..].iter().map(|&b| make(ids[0], b)).collect()
    };
    // (A rotation axis has no ends, no length and no middle.)
    let endless = p.lines.iter().any(|&l| args.is_endless(l));
    Ok(Plan::Constrain(match action {
        Action::Coincident => match counts {
            (2, 0, 0, 0) => vec![Kind::Coincident(p.points[0], p.points[1])],
            (1, 1, 0, 0) => vec![Kind::PointOn {
                point: p.points[0],
                entity: p.lines[0],
            }],
            (1, 0, 1, 0) => vec![Kind::PointOn {
                point: p.points[0],
                entity: p.rounds[0],
            }],
            (0, 0, 2, 0) => {
                let center = |e| args.circle(e).map(|c| c.0);
                let (Some(a), Some(b)) = (center(p.rounds[0]), center(p.rounds[1])) else {
                    return need("two circles");
                };
                vec![Kind::Coincident(a, b)]
            }
            _ => return need("two points, a point and a line or circle, or two circles"),
        },
        Action::Horizontal | Action::Vertical => {
            let axis = if action == Action::Horizontal {
                horizontal
            } else {
                horizontal.other()
            };
            match counts {
                (2, 0, 0, 0) => vec![Kind::Along {
                    a: p.points[0],
                    b: p.points[1],
                    axis,
                }],
                (0, n, 0, 0) if n > 0 => p
                    .lines
                    .iter()
                    .filter_map(|&l| match args.entities[&l].shape {
                        Shape::Line(a, b) if !args.is_axis(l) => Some(Kind::Along { a, b, axis }),
                        // A rotation axis is along the plane's axis.
                        Shape::Axis { .. } => Some(Kind::Parallel(l, args.plane_axis(axis)?)),
                        _ => None,
                    })
                    .collect(),
                _ => return need("lines, or two points"),
            }
        }
        Action::Parallel | Action::Perpendicular | Action::Collinear => match counts {
            (0, 2, 0, 0) => {
                let (a, b) = (p.lines[0], p.lines[1]);
                vec![match action {
                    Action::Parallel => Kind::Parallel(a, b),
                    Action::Perpendicular => Kind::Perpendicular(a, b),
                    _ => Kind::Collinear(a, b),
                }]
            }
            _ => return need("two lines"),
        },
        Action::Tangent => match counts {
            (0, 1, 1, 0) => vec![Kind::Tangent {
                a: p.lines[0],
                b: p.rounds[0],
                internal: false,
            }],
            (0, 0, 2, 0) => {
                let (Some((_, c1, r1)), Some((_, c2, r2))) =
                    (args.circle(p.rounds[0]), args.circle(p.rounds[1]))
                else {
                    return need("two circles");
                };
                vec![Kind::Tangent {
                    a: p.rounds[0],
                    b: p.rounds[1],
                    internal: c1.distance(c2) < r1.max(r2),
                }]
            }
            _ => return need("a line and a circle or arc, or two circles or arcs"),
        },
        Action::Curvature => {
            let other = match (counts, &p.splines[..]) {
                ((0, 1, 0, 1), [_]) => p.lines[0],
                ((0, 0, 1, 1), [_]) => p.rounds[0],
                ((0, 0, 0, 2), [_, second]) => *second,
                _ => return need("a spline and the line, arc or spline it ends on"),
            };
            vec![Kind::Smooth {
                spline: p.splines[0],
                other,
            }]
        }
        Action::Equal => match counts {
            (0, n, 0, 0) if n >= 2 && !endless => pairs(&p.lines, &Kind::Equal),
            (0, 0, n, 0) if n >= 2 => pairs(&p.rounds, &Kind::Equal),
            _ => return need("two or more lines, or two or more circles or arcs"),
        },
        Action::Midpoint => match counts {
            (1, 1, 0, 0) if !endless => vec![Kind::Midpoint {
                point: p.points[0],
                line: p.lines[0],
            }],
            _ => return need("a point and a line"),
        },
        Action::Symmetric => match counts {
            (2, 1, 0, 0) => vec![Kind::Symmetric {
                a: p.points[0],
                b: p.points[1],
                line: p.lines[0],
            }],
            _ => return need("two points and the line to mirror them in"),
        },
        Action::Fix => {
            let mut points = p.points.clone();
            for &e in p.lines.iter().chain(&p.rounds) {
                points.extend(args.points_of(e));
            }
            points.retain(|p| {
                !args
                    .entities_at(*p)
                    .iter()
                    .any(|&e| args.is_projected(e) || args.is_axis(e))
            });
            if points.is_empty() {
                return need("points or curves that are not projected");
            }
            return Ok(Plan::Fix(points));
        }
        Action::Construction => {
            let entities: Vec<Id> = state
                .selection
                .iter()
                .filter_map(|item| match item {
                    Item::Entity(e) if args.entities.contains_key(e) && !args.is_endless(*e) => {
                        Some(*e)
                    }
                    _ => None,
                })
                .collect();
            if entities.is_empty() {
                return need("curves");
            }
            return Ok(Plan::Construction(entities));
        }
        Action::Delete => {
            if state.selection.is_empty() {
                return need("what to delete");
            }
            return Ok(Plan::Delete);
        }
        Action::Mirror => {
            // The line selected last is the mirror.
            let mirror = state.selection.iter().rev().find_map(|item| match item {
                Item::Entity(e) if args.is_line(*e) => Some(*e),
                _ => None,
            });
            let copies = state
                .selection
                .iter()
                .filter(|i| matches!(i, Item::Entity(e) if Some(*e) != mirror))
                .count();
            match mirror {
                Some(mirror) if copies > 0 => return Ok(Plan::Mirror(mirror)),
                _ => return need("the curves to mirror, then the line to mirror them in"),
            }
        }
        Action::Offset => {
            if p.lines.len() + p.rounds.len() == 0 || p.points.len() + p.others > 0 || endless {
                return need("lines, arcs or circles to offset");
            }
            return Ok(Plan::Offset);
        }
        Action::Fillet | Action::Chamfer => {
            // The corner itself, or the two lines that meet in it.
            let corner = match counts {
                (1, 0, 0, 0) => Some(p.points[0]),
                (0, 2, 0, 0) => {
                    let ends = |line: Id| args.points_of(line);
                    let (first, second) = (ends(p.lines[0]), ends(p.lines[1]));
                    first.into_iter().find(|point| second.contains(point))
                }
                _ => None,
            };
            match corner.filter(|c| crate::corners::is_corner(args, *c)) {
                Some(corner) if action == Action::Fillet => return Ok(Plan::Fillet(corner)),
                Some(corner) => return Ok(Plan::Chamfer(corner)),
                None => return need("a corner where two lines meet, or the two lines"),
            }
        }
        Action::Split => {
            let curve = match counts {
                (1, 1, 0, 0) if !endless => Some(p.lines[0]),
                (1, 0, 1, 0)
                    if !matches!(args.entities[&p.rounds[0]].shape, Shape::Circle { .. }) =>
                {
                    Some(p.rounds[0])
                }
                _ => None,
            };
            match curve {
                Some(curve) if !args.points_of(curve).contains(&p.points[0]) => {
                    return Ok(Plan::Split(curve, p.points[0]));
                }
                _ => return need("a line or an arc, and the point on it to split it at"),
            }
        }
        Action::Rotate | Action::Scale => {
            let chosen = crate::copy::copyable(args, &state.selection);
            let about: Vec<Id> = state
                .selection
                .iter()
                .filter_map(|item| match item {
                    Item::Point(p) if args.points.contains_key(p) => Some(*p),
                    _ => None,
                })
                .collect();
            match about[..] {
                [about] if !chosen.is_empty() => {
                    return Ok(Plan::Turn(chosen, about, action == Action::Scale));
                }
                _ => return need("the curves, and the point to turn or scale them about"),
            }
        }
        Action::Detach => {
            let projected: Vec<Id> = state
                .selection
                .iter()
                .filter_map(|item| match item {
                    Item::Entity(e) if args.is_projected(*e) && !args.is_axis(*e) => Some(*e),
                    _ => None,
                })
                .collect();
            if projected.is_empty() {
                return need("projected curves");
            }
            return Ok(Plan::Detach(projected));
        }
        Action::Copy => {
            let chosen = crate::copy::copyable(args, &state.selection);
            if chosen.is_empty() {
                return need("the curves to copy");
            }
            return Ok(Plan::Copy(chosen));
        }
        Action::LinearPattern => {
            // The line selected last is one step.
            let chosen = crate::copy::copyable(args, &state.selection);
            match chosen.split_last() {
                Some((step, rest))
                    if args.is_line(*step) && !args.is_endless(*step) && !rest.is_empty() =>
                {
                    return Ok(Plan::Linear(rest.to_vec(), *step));
                }
                _ => return need("the curves to repeat, then a line as long as a step"),
            }
        }
        Action::CircularPattern => {
            let chosen = crate::copy::copyable(args, &state.selection);
            let centres: Vec<Id> = state
                .selection
                .iter()
                .filter_map(|item| match item {
                    Item::Point(p) if args.points.contains_key(p) => Some(*p),
                    _ => None,
                })
                .collect();
            match centres[..] {
                [centre] if !chosen.is_empty() => return Ok(Plan::Circular(chosen, centre)),
                _ => return need("the curves to repeat, and the point to repeat them around"),
            }
        }
        Action::Extend => {
            // The line selected first runs on to what is selected with it.
            let chosen: Vec<Id> = state
                .selection
                .iter()
                .filter_map(|item| match item {
                    Item::Entity(e) => Some(*e),
                    _ => None,
                })
                .collect();
            match chosen[..] {
                [line, target]
                    if args.is_line(line)
                        && !args.is_endless(line)
                        && (args.is_line(target) || args.circle(target).is_some()) =>
                {
                    return Ok(Plan::Extend(line, target));
                }
                _ => return need("the line to run on, then the line or circle it runs on to"),
            }
        }
    }))
}

/// Why `action` cannot be done with the selection; `None` if it can.
pub(crate) fn unavailable(
    args: &SketchArgs,
    state: &SketchState,
    action: Action,
) -> Option<String> {
    plan(args, state, action).err()
}

/// The sketch after `action` on the selection, and what is selected then;
/// `None` if it cannot be done.
pub(crate) fn apply(
    args: &SketchArgs,
    state: &SketchState,
    action: Action,
) -> Result<(SketchArgs, Vec<Item>), String> {
    let none = || "That cannot be done here".to_owned();
    let mut new = args.clone();
    let mut selection = state.selection.clone();
    match plan(args, state, action)? {
        Plan::Constrain(kinds) => {
            // What would over-define the sketch is left out: all of it
            // left out, nothing is done and the user is told why.
            let mut why = None;
            selection.clear();
            for kind in kinds {
                let mut trial = new.clone();
                let id = trial.constrain(kind);
                trial.solve();
                match new.overdefined_by(&trial, &[id]) {
                    Some(w) => why = why.or(Some(w)),
                    None => {
                        new = trial;
                        selection.push(Item::Constraint(id));
                    }
                }
            }
            if let (true, Some(why)) = (selection.is_empty(), why) {
                return Err(why.text("constraint"));
            }
        }
        Plan::Fix(points) => {
            let all = points.iter().all(|p| new.points[p].fixed);
            for p in points {
                if let Some(point) = new.points.get_mut(&p) {
                    point.fixed = !all;
                }
            }
        }
        Plan::Construction(entities) => {
            let all = entities.iter().all(|e| new.entities[e].construction);
            for e in entities {
                if let Some(entity) = new.entities.get_mut(&e) {
                    entity.construction = !all;
                }
            }
        }
        Plan::Delete => {
            new.remove(&state.selection);
            selection.clear();
        }
        Plan::Mirror(line) => selection = crate::copy::mirror(&mut new, &state.selection, line),
        Plan::Offset => {
            let distance = 20.0 * state.view.pixel;
            selection =
                crate::copy::offset(&mut new, &state.selection, distance).ok_or_else(none)?;
        }
        // (As large as shows well; a dimension says how large it is to be.)
        Plan::Fillet(corner) => {
            selection = crate::corners::fillet(&mut new, corner, 24.0 * state.view.pixel)
                .ok_or_else(none)?;
            new.solve();
        }
        Plan::Chamfer(corner) => {
            selection = crate::corners::chamfer(&mut new, corner, 24.0 * state.view.pixel)
                .ok_or_else(none)?;
        }
        Plan::Split(curve, point) => {
            selection = crate::corners::split(&mut new, curve, point).ok_or_else(none)?
        }
        Plan::Turn(chosen, about, scale) => {
            // Their points, each once; a circle's radius with them.
            let centre = new.at(about);
            let turn = Point2::from_angle(state.angle.to_radians());
            let factor = if scale { state.factor } else { 1.0 };
            let mut moved = std::collections::BTreeSet::new();
            for &e in &chosen {
                for p in new.points_of(e) {
                    if p == about || !moved.insert(p) {
                        continue;
                    }
                    if let Some(point) = new.points.get_mut(&p) {
                        let from = point.at - centre;
                        point.at = centre
                            + if scale {
                                from * factor
                            } else {
                                turn.rotate(from)
                            };
                    }
                }
                if let Some(Shape::Circle { radius, .. }) =
                    new.entities.get_mut(&e).map(|e| &mut e.shape)
                {
                    *radius *= factor;
                }
            }
            new.solve();
        }
        Plan::Detach(projected) => {
            // A copy that is the sketch's own, where the projection is:
            // the projection itself stays the model's.
            let copies = crate::copy::copied(&mut new, &projected, &|p| p, 1.0);
            for copy in &copies {
                if let Some(entity) = new.entities.get_mut(copy) {
                    entity.construction = false;
                }
            }
            selection = copies.into_iter().map(Item::Entity).collect();
        }
        Plan::Copy(chosen) => {
            let by = Point2::splat(24.0 * state.view.pixel);
            let copies = crate::copy::copied(&mut new, &chosen, &|p| p + by, 1.0);
            selection = copies.into_iter().map(Item::Entity).collect();
        }
        Plan::Linear(chosen, step) => {
            let (a, b) = new.line(step).ok_or_else(none)?;
            let mut all = Vec::new();
            for k in 1..state.copies.clamp(2, 200) {
                let by = (b - a) * k as f64;
                all.extend(crate::copy::copied(&mut new, &chosen, &|p| p + by, 1.0));
            }
            selection = all.into_iter().map(Item::Entity).collect();
        }
        Plan::Circular(chosen, centre) => {
            let about = new.at(centre);
            let n = state.copies.clamp(2, 200);
            let mut all = Vec::new();
            for k in 1..n {
                let turn = Point2::from_angle(std::f64::consts::TAU * k as f64 / n as f64);
                let to = |p: Point2| about + turn.rotate(p - about);
                all.extend(crate::copy::copied(&mut new, &chosen, &to, 1.0));
            }
            selection = all.into_iter().map(Item::Entity).collect();
        }
        Plan::Extend(line, target) => {
            selection = crate::corners::extend(&mut new, line, target).ok_or_else(none)?;
            new.solve();
        }
    }
    Ok((new, selection))
}

/// `args` with dimension `kind` at `value` (a diameter if `diameter`),
/// its text at `label`, solved; and its id. One that would over-define
/// the sketch is added driven (it only measures): then also what the
/// user is told of that.
pub(crate) fn place_dimension(
    args: &SketchArgs,
    kind: Kind,
    value: f64,
    label: Point2,
    diameter: bool,
) -> (SketchArgs, Id, Option<String>) {
    let mut trial = args.clone();
    let id = trial.dimension_of(kind, value, label, diameter);
    trial.solve();
    match args.overdefined_by(&trial, &[id]) {
        None => (trial, id, None),
        Some(why) => {
            let mut driven = args.clone();
            let id = driven.dimension_of(kind, value, label, diameter);
            driven.set_driven(id, true);
            let said = match why {
                Overdefined::Redundant => "the rest fixes it already",
                Overdefined::Conflicting => "it would conflict with the rest",
            };
            (driven, id, Some(format!("Added driven: {said}")))
        }
    }
}

/// Makes dimension `id` driven or driving. Driving is refused, saying
/// why, if it would over-define the sketch.
pub(crate) fn set_driven(args: &SketchArgs, id: Id, driven: bool) -> Result<SketchArgs, String> {
    let mut new = args.clone();
    if driven {
        new.set_driven(id, true);
        return Ok(new);
    }
    // As the others see it: without it, then with it holding what it
    // measures.
    let mut trial = args.clone();
    trial.set_driven(id, false);
    // (Its id is the newest, as the solver ranks it, if it is moved to
    // the end.)
    let moved = trial.fresh();
    if let Some(c) = trial.constraints.remove(&id) {
        trial.constraints.insert(moved, c);
    }
    let mut without = args.clone();
    without.constraints.remove(&id);
    trial.solve();
    match without.overdefined_by(&trial, &[moved]) {
        None => {
            new.set_driven(id, false);
            Ok(new)
        }
        Some(why) => Err(match why {
            Overdefined::Redundant => "Stays driven: the rest fixes it already",
            Overdefined::Conflicting => "Stays driven: it would conflict with the rest",
        }
        .to_owned()),
    }
}

/// The dimension that fits `selection`, measured, when its text is put at
/// `label`, and whether it is a diameter: between two points or along a
/// line (level, upright or aligned, by where the text is), from a point
/// to a line, between parallel lines, the angle of two lines, a
/// circle's diameter, an arc's radius. To a rotation axis with the text
/// across it from what is measured, it is a diameter.
pub(crate) fn dimension_for(
    args: &SketchArgs,
    selection: &[Item],
    label: Point2,
    view: &CanvasView,
) -> Option<(Kind, f64, bool)> {
    let p = picked(args, selection);
    let between = |a: Id, b: Id| {
        let (pa, pb) = (args.at(a), args.at(b));
        let horizontal = SketchAxis::nearest(view.right);
        // Beside the points, between them along one axis: measure along
        // that axis.
        let inside = |axis: SketchAxis| {
            let (u, l) = (axis.unit(), label);
            let (lo, hi) = (pa.dot(u).min(pb.dot(u)), pa.dot(u).max(pb.dot(u)));
            l.dot(u) >= lo && l.dot(u) <= hi
        };
        let along = |axis: SketchAxis| (pb - pa).dot(axis.unit()).abs();
        let axis = match (inside(horizontal), inside(horizontal.other())) {
            (true, false) => Some(horizontal),
            (false, true) => Some(horizontal.other()),
            _ => None,
        };
        match axis.filter(|&axis| along(axis) > 1e-9 && along(axis.other()) > 1e-9) {
            Some(axis) => (Kind::DistanceAlong { a, b, axis }, along(axis), false),
            None => (Kind::Distance(a, b), pa.distance(pb), false),
        }
    };
    let off = |point: Id, line: Id| {
        let (a, b) = args.line(line)?;
        let d = (b - a).try_normalize()?;
        let side = d.perp_dot(args.at(point) - a);
        let off = side.abs();
        // To a rotation axis, with the text on the other side of it
        // than the point: the diameter, as a turned part is
        // dimensioned.
        let across =
            args.is_rotation_axis(line) && off > 1e-9 && side * d.perp_dot(label - a) < 0.0;
        let value = if across { 2.0 * off } else { off };
        Some((Kind::PointLine { point, line }, value, across))
    };
    let center = |e: Id| args.circle(e).map(|c| c.0);
    match (p.points.len(), p.lines.len(), p.rounds.len(), p.others) {
        (2, 0, 0, 0) => Some(between(p.points[0], p.points[1])),
        (0, 1, 0, 0) => match args.entities[&p.lines[0]].shape {
            // An axis has no length to give.
            Shape::Line(..) if args.is_endless(p.lines[0]) => None,
            Shape::Line(a, b) => Some(between(a, b)),
            _ => None,
        },
        (1, 1, 0, 0) => off(p.points[0], p.lines[0]),
        (0, 2, 0, 0) => {
            let ((a1, b1), (a2, b2)) = (args.line(p.lines[0])?, args.line(p.lines[1])?);
            let (d1, d2) = ((b1 - a1).try_normalize()?, (b2 - a2).try_normalize()?);
            let angle = d1.perp_dot(d2).atan2(d1.dot(d2)).to_degrees();
            if d1.perp_dot(d2).abs() < 1e-9 {
                // An endless one (a rotation axis, an axis of the plane)
                // is what is measured to.
                let endless = |l: Id| args.is_endless(l);
                let (from, to) = if endless(p.lines[1]) && !endless(p.lines[0]) {
                    (p.lines[0], p.lines[1])
                } else {
                    (p.lines[1], p.lines[0])
                };
                let start = *args.points_of(from).first()?;
                off(start, to)
            } else if angle > 0.0 {
                Some((Kind::Angle(p.lines[0], p.lines[1]), angle, false))
            } else {
                Some((Kind::Angle(p.lines[1], p.lines[0]), -angle, false))
            }
        }
        (0, 0, 1, 0) => {
            let e = p.rounds[0];
            let (_, _, r) = args.circle(e)?;
            Some(match args.entities[&e].shape {
                Shape::Circle { .. } => (Kind::Size(e), 2.0 * r, true),
                _ => (Kind::Size(e), r, false),
            })
        }
        (1, 0, 1, 0) => Some(between(p.points[0], center(p.rounds[0])?)),
        (0, 0, 2, 0) => Some(between(center(p.rounds[0])?, center(p.rounds[1])?)),
        (0, 1, 1, 0) => off(center(p.rounds[0])?, p.lines[0]),
        _ => None,
    }
}

/// Why no dimension fits the selection; `None` if one does.
pub(crate) fn no_dimension(args: &SketchArgs, state: &SketchState) -> Option<String> {
    let at = Point2::ZERO;
    match dimension_for(args, &state.selection, at, &state.view) {
        Some(_) => None,
        None => Some(
            "Select a line, a circle or arc, two points, two lines, or a point and a line"
                .to_owned(),
        ),
    }
}
