//! Trimming: cutting away the piece of a curve between the curves that
//! cross it.
//!
//! Lines, arcs and circles are trimmed, where lines, arcs and circles
//! cross them. The piece under the pointer goes; what is left ends on the
//! curves that cut it, and keeps the entity's id (a second piece, when a
//! middle is cut out, is a new entity).

use std::f64::consts::TAU;

use shapecraft_core::handles::CanvasView;
use shapecraft_core::math::Point2;

use crate::geometry::{angle, sweep};
use crate::model::{Id, Item, Kind, Shape, SketchArgs};
use crate::tools::REACH_PX;

/// How close parameters (0–1 along a line, radians around a circle) have
/// to be to count as the same place.
const SAME: f64 = 1e-9;

/// How far from a circle's rim, in radii, half a chord has to be before
/// it is told from a touch: the height is the root of a difference of
/// squares, so rounding alone gives one of 1e-8 radii.
const TOUCH: f64 = 1e-6;

/// How far an axis reaches either way, in sizes of the sketch.
const AXIS_REACH: f64 = 1e4;

/// What a curve is, for crossing it with another.
#[derive(Clone, Copy)]
pub(crate) enum Carrier {
    Line(Point2, Point2),
    /// A circle; with the sweep's start direction and angle for an arc.
    Round(Point2, f64, Option<(Point2, f64)>),
}

pub(crate) fn carrier(args: &SketchArgs, id: Id) -> Option<Carrier> {
    match args.entities.get(&id)?.shape {
        // An axis goes on without end: far beyond anything drawn.
        Shape::Line(a, b) if args.is_axis(id) => {
            let (a, b) = (args.at(a), args.at(b));
            let far = (b - a) * AXIS_REACH * args.scale().max(1.0);
            Some(Carrier::Line(a - far, a + far))
        }
        Shape::Line(a, b) => Some(Carrier::Line(args.at(a), args.at(b))),
        Shape::Circle { center, radius } => Some(Carrier::Round(args.at(center), radius, None)),
        Shape::Arc { center, start, end } => {
            let (c, s, e) = (args.at(center), args.at(start), args.at(end));
            Some(Carrier::Round(
                c,
                c.distance(s),
                Some((s - c, sweep(s - c, e - c))),
            ))
        }
        _ => None,
    }
}

/// Where `p` (on the carrier) is along it: 0–1 on a line, the angle from
/// the start on an arc, the angle on a circle. And how long it is.
pub(crate) fn parameter(carrier: &Carrier, p: Point2) -> (f64, f64) {
    match *carrier {
        Carrier::Line(a, b) => ((p - a).dot(b - a) / (b - a).length_squared(), 1.0),
        Carrier::Round(c, _, None) => (angle(p - c), TAU),
        Carrier::Round(c, _, Some((start, total))) => {
            let s = sweep(start, p - c);
            // Just before the start is the start, not a full turn on.
            (
                if s > total + (TAU - total) / 2.0 {
                    s - TAU
                } else {
                    s
                },
                total,
            )
        }
    }
}

/// Whether `p` (on the carrier's full line or circle) is on the curve
/// itself, ends included.
pub(crate) fn within(carrier: &Carrier, p: Point2) -> bool {
    let (t, length) = parameter(carrier, p);
    matches!(carrier, Carrier::Round(_, _, None)) || (t >= -SAME && t <= length + SAME)
}

/// How far apart (in the sketch's lengths) a line and a circle, or two
/// circles, may be and still touch, for sketch `args`: well above what a
/// solved tangency is off by. Tangent curves that the solver left a
/// hair's breadth across each other meet once, at the tangent point, not
/// at two crossings far apart along them (the chord through a gap of `g`
/// is `2 sqrt(2 r g)` long: a gap of 1e-11 leaves crossings 1e-5 apart).
pub(crate) fn touch_gap(args: &SketchArgs) -> f64 {
    1e2 * args.solve_tolerance()
}

/// Where the full lines and circles of two carriers cross. Ones closer
/// than `gap` to touching (see [`touch_gap`]) touch: one place.
pub(crate) fn crossings(a: &Carrier, b: &Carrier, gap: f64) -> Vec<Point2> {
    match (*a, *b) {
        (Carrier::Line(a0, a1), Carrier::Line(b0, b1)) => {
            let (da, db) = (a1 - a0, b1 - b0);
            let det = da.perp_dot(db);
            if det.abs() <= 1e-12 * da.length() * db.length() {
                return Vec::new();
            }
            vec![a0 + da * ((b0 - a0).perp_dot(db) / det)]
        }
        (Carrier::Line(p, q), Carrier::Round(c, r, _))
        | (Carrier::Round(c, r, _), Carrier::Line(p, q)) => {
            let Some(d) = (q - p).try_normalize() else {
                return Vec::new();
            };
            let foot = p + d * (c - p).dot(d);
            // How far the line is inside the circle (outside, below 0).
            // (Rounding alone leaves a part in 1e9 of r.)
            let into = r - foot.distance(c);
            if into.abs() <= gap.max(5e-10 * r) {
                return vec![foot];
            }
            let h2 = r * r - foot.distance_squared(c);
            if into < 0.0 || h2 < 0.0 {
                return Vec::new();
            }
            let h = h2.sqrt();
            // (The difference of two squares is known to a part in 1e16
            // of r²: its root, to one in 1e8 of r. Below that the line
            // touches.)
            if h <= TOUCH * r {
                vec![foot]
            } else {
                vec![foot - d * h, foot + d * h]
            }
        }
        (Carrier::Round(c1, r1, _), Carrier::Round(c2, r2, _)) => {
            let d = c1.distance(c2);
            if d <= 1e-12 {
                return Vec::new();
            }
            let u = (c2 - c1) / d;
            // Touching from outside, or one inside the other: where they
            // meet is on the line through the centres.
            let gap = gap.max(1e-12 * (r1 + r2));
            if (r1 + r2 - d).abs() <= gap {
                return vec![c1 + u * r1];
            }
            if (d - (r1 - r2).abs()).abs() <= gap {
                return vec![if r1 >= r2 { c1 + u * r1 } else { c1 - u * r1 }];
            }
            if d > r1 + r2 || d < (r1 - r2).abs() {
                return Vec::new();
            }
            let along = (d * d + r1 * r1 - r2 * r2) / (2.0 * d);
            let h = (r1 * r1 - along * along).max(0.0).sqrt();
            let foot = c1 + u * along;
            if h <= TOUCH * r1 {
                vec![foot]
            } else {
                vec![foot - u.perp() * h, foot + u.perp() * h]
            }
        }
    }
}

/// Where the other curves cut `id`: parameter along it, the cutter, and
/// the point; sorted along the curve.
fn cuts(args: &SketchArgs, id: Id) -> Vec<(f64, Id, Point2)> {
    cuts_by(args, id, &|_| true)
}

/// [`cuts`] by the curves `cutter` takes only.
pub(crate) fn cuts_by(
    args: &SketchArgs,
    id: Id,
    cutter: &dyn Fn(Id) -> bool,
) -> Vec<(f64, Id, Point2)> {
    let Some(own) = carrier(args, id) else {
        return Vec::new();
    };
    let closed = matches!(own, Carrier::Round(_, _, None));
    let mut cuts = Vec::new();
    for &other in args.entities.keys().filter(|&&o| o != id && cutter(o)) {
        let Some(theirs) = carrier(args, other) else {
            continue;
        };
        for p in crossings(&own, &theirs, touch_gap(args)) {
            let (t, length) = parameter(&own, p);
            // Inside this curve (its ends cut nothing), on the other.
            let inside = closed || (t > SAME && t < length - SAME);
            if inside && within(&theirs, p) {
                cuts.push((t, other, p));
            }
        }
    }
    cuts.sort_by(|a, b| a.0.total_cmp(&b.0));
    cuts.dedup_by(|b, a| (b.0 - a.0).abs() <= SAME);
    cuts
}

/// A new point at `cut`, on the curve that cuts there.
fn cut_point(args: &mut SketchArgs, cut: &(f64, Id, Point2)) -> Id {
    let point = args.add_point(cut.2);
    args.constrain(Kind::PointOn {
        point,
        entity: cut.1,
    });
    point
}

/// After end `old` of `entity` became `new`: what leveled the entity's
/// two ends still does, what measured between them goes, and `old` goes
/// if nothing else uses it.
fn replace_end(args: &mut SketchArgs, entity: Id, old: Id, new: Id) {
    let ends = args.points_of(entity);
    let pair = |a: Id, b: Id| (a == old && ends.contains(&b)) || (b == old && ends.contains(&a));
    let mut gone = Vec::new();
    for (&id, constraint) in &mut args.constraints {
        match &mut constraint.kind {
            Kind::Along { a, b, .. } if pair(*a, *b) => {
                for p in [a, b] {
                    if *p == old {
                        *p = new;
                    }
                }
            }
            Kind::Distance(a, b) | Kind::DistanceAlong { a, b, .. } if pair(*a, *b) => {
                gone.push(id)
            }
            _ => {}
        }
    }
    for id in gone {
        args.constraints.remove(&id);
    }
    if args.entities_at(old).is_empty() {
        args.remove(&[Item::Point(old)]);
    }
}

/// Cuts away the piece of `id` around `p`. Returns whether anything was
/// cut.
fn trim_at(args: &mut SketchArgs, id: Id, p: Point2) -> bool {
    let Some(own) = carrier(args, id) else {
        return false;
    };
    let cuts = cuts(args, id);
    let (t, _) = parameter(&own, p);
    let below = cuts.iter().rev().find(|c| c.0 < t).copied();
    let above = cuts.iter().find(|c| c.0 > t).copied();
    let shape = args.entities[&id].shape.clone();
    match shape {
        Shape::Circle { center, .. } => {
            // What is left runs from the cut above, round to the cut below.
            let (Some(first), Some(last)) = (cuts.first().copied(), cuts.last().copied()) else {
                return false;
            };
            if cuts.len() < 2 {
                return false;
            }
            let (from, to) = (above.unwrap_or(first), below.unwrap_or(last));
            let (start, end) = (cut_point(args, &from), cut_point(args, &to));
            if let Some(entity) = args.entities.get_mut(&id) {
                entity.shape = Shape::Arc { center, start, end };
            }
        }
        Shape::Line(start, end) | Shape::Arc { start, end, .. } => {
            let rebuilt = |start: Id, end: Id| match &shape {
                Shape::Arc { center, .. } => Shape::Arc {
                    center: *center,
                    start,
                    end,
                },
                _ => Shape::Line(start, end),
            };
            match (below, above) {
                (None, None) => args.remove(&[Item::Entity(id)]),
                (None, Some(cut)) => {
                    let new = cut_point(args, &cut);
                    args.entities.get_mut(&id).expect("the entity").shape = rebuilt(new, end);
                    replace_end(args, id, start, new);
                }
                (Some(cut), None) => {
                    let new = cut_point(args, &cut);
                    args.entities.get_mut(&id).expect("the entity").shape = rebuilt(start, new);
                    replace_end(args, id, end, new);
                }
                (Some(lo), Some(hi)) => {
                    // The middle goes: this entity up to the lower cut, a
                    // new one from the upper cut on.
                    let (n1, n2) = (cut_point(args, &lo), cut_point(args, &hi));
                    let mut second = args.entities[&id].clone();
                    second.shape = rebuilt(n2, end);
                    args.entities.get_mut(&id).expect("the entity").shape = rebuilt(start, n1);
                    let other = args.fresh();
                    args.entities.insert(other, second);
                }
            }
        }
        _ => return false,
    }
    true
}

/// Trims every line, arc and circle the pointer passed over on `path`.
/// Returns the trimmed sketch, or `None` if the path touched nothing that
/// can be trimmed.
pub(crate) fn trim(args: &SketchArgs, path: &[Point2], view: &CanvasView) -> Option<SketchArgs> {
    let reach = REACH_PX * view.pixel;
    // The path, sampled closely enough to miss nothing it crosses.
    let mut samples = Vec::new();
    for pair in path.windows(2) {
        let steps = (pair[0].distance(pair[1]) / reach).ceil().max(1.0) as usize;
        samples.extend((0..steps).map(|k| pair[0].lerp(pair[1], k as f64 / steps as f64)));
    }
    samples.extend(path.last());
    let mut new = args.clone();
    let mut changed = false;
    for p in samples {
        let hit = new
            .entities
            .iter()
            .filter(|(id, e)| {
                e.projected.is_none() && e.axis.is_none() && carrier(&new, **id).is_some()
            })
            .map(|(&id, _)| (id, new.distance_to(id, p)))
            .filter(|(_, d)| *d <= reach)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((id, _)) = hit {
            changed |= trim_at(&mut new, id, p);
        }
    }
    if changed {
        new.solve();
    }
    changed.then_some(new)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> CanvasView {
        CanvasView {
            pixel: 0.01,
            right: Point2::X,
            up: Point2::Y,
            middle: Point2::ZERO,
        }
    }

    fn line(args: &mut SketchArgs, a: (f64, f64), b: (f64, f64)) -> Id {
        let (a, b) = (args.add_point(a.into()), args.add_point(b.into()));
        args.add_line(a, b)
    }

    #[test]
    fn a_line_that_touches_a_circle_meets_it_once() {
        // The side of a rounded corner, a rounding off its tangent
        // point: one place, not two a hair apart.
        let (c, r) = (
            Point2::new(-3.74451854585251, -2.400212479775463),
            0.870829569243641,
        );
        let round = Carrier::Round(c, r, None);
        let side = Carrier::Line(
            Point2::new(c.x - 1e-15, c.y - r),
            Point2::new(4.6, c.y - r + 1e-16),
        );
        assert_eq!(crossings(&side, &round, 0.0).len(), 1);
        let through = Carrier::Line(
            Point2::new(c.x, c.y - r * 0.99),
            Point2::new(4.6, c.y - r * 0.99),
        );
        assert_eq!(crossings(&through, &round, 0.0).len(), 2);
    }

    #[test]
    fn ends_and_middles_are_cut_at_the_crossing_curves() {
        // A long line crossed by two uprights at x = 1 and x = 3.
        let mut args = SketchArgs::default();
        let long = line(&mut args, (0.0, 0.0), (4.0, 0.0));
        let left = line(&mut args, (1.0, -1.0), (1.0, 1.0));
        let right = line(&mut args, (3.0, -1.0), (3.0, 1.0));
        // The middle goes: two pieces, ending on the uprights.
        let cut = trim(&args, &[Point2::new(2.0, 0.0)], &view()).expect("a cut");
        assert_eq!(cut.entities.len(), 4);
        let (a, b) = cut.line(long).expect("the first piece");
        assert!(a.abs_diff_eq(Point2::ZERO, 1e-9) && b.abs_diff_eq(Point2::new(1.0, 0.0), 1e-9));
        let on = |entity: Id| {
            cut.constraints
                .values()
                .filter(|c| matches!(c.kind, Kind::PointOn { entity: e, .. } if e == entity))
                .count()
        };
        assert_eq!((on(left), on(right)), (1, 1));
        // An end goes: the line now starts on the left upright.
        let cut = trim(&args, &[Point2::new(0.5, 0.0)], &view()).expect("a cut");
        assert_eq!(cut.entities.len(), 3);
        let (a, _) = cut.line(long).expect("the line");
        assert!(a.abs_diff_eq(Point2::new(1.0, 0.0), 1e-9));
        // Dragging across both uprights' tops trims both.
        let path = [Point2::new(0.5, 0.5), Point2::new(3.5, 0.5)];
        let cut = trim(&args, &path, &view()).expect("cuts");
        for upright in [left, right] {
            let (a, b) = cut.line(upright).expect("the upright");
            assert!(a.y.max(b.y).abs() < 1e-9, "{a} {b}");
        }
        // Nothing crosses a lone line: all of it goes.
        let mut alone = SketchArgs::default();
        line(&mut alone, (0.0, 0.0), (1.0, 0.0));
        let cut = trim(&alone, &[Point2::new(0.5, 0.0)], &view()).expect("a cut");
        assert!(cut.entities.is_empty() && cut.points.is_empty());
    }

    #[test]
    fn circles_become_arcs() {
        let mut args = SketchArgs::default();
        let center = args.add_point(Point2::ZERO);
        let circle = args.add(Shape::Circle {
            center,
            radius: 2.0,
        });
        line(&mut args, (-3.0, 0.0), (3.0, 0.0));
        // Cut the top half away: the bottom half is left.
        let cut = trim(&args, &[Point2::new(0.0, 2.0)], &view()).expect("a cut");
        let Shape::Arc { start, end, .. } = cut.entities[&circle].shape else {
            panic!("expected an arc");
        };
        assert!(cut.at(start).abs_diff_eq(Point2::new(-2.0, 0.0), 1e-9));
        assert!(cut.at(end).abs_diff_eq(Point2::new(2.0, 0.0), 1e-9));
        assert!(cut.diagnose().solved);
    }
}
