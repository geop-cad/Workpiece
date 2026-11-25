//! Changes to the curves drawn: a corner rounded or cut off, a curve
//! split at a point, a line run on to meet another curve.

use workpiece_core::math::Point2;

use crate::model::{Id, Item, Kind, Shape, SketchArgs};

/// The two lines that end at `corner`, and each one's other end.
fn legs(args: &SketchArgs, corner: Id) -> Option<[(Id, Id); 2]> {
    let lines: Vec<(Id, Id)> = args
        .entities_at(corner)
        .into_iter()
        .filter_map(|e| match args.entities[&e].shape {
            Shape::Line(a, b) if a == corner => Some((e, b)),
            Shape::Line(a, b) if b == corner => Some((e, a)),
            _ => None,
        })
        .collect();
    match lines[..] {
        [a, b] if args.entities_at(corner).len() == 2 => Some([a, b]),
        _ => None,
    }
}

/// Whether two lines end at `corner` and nothing else does: a corner
/// that can be rounded or cut off.
pub(crate) fn is_corner(args: &SketchArgs, corner: Id) -> bool {
    legs(args, corner).is_some()
}

/// Line `line` with its end `from` put to `to`.
fn retie(args: &mut SketchArgs, line: Id, from: Id, to: Id) {
    if let Some(Shape::Line(a, b)) = args.entities.get_mut(&line).map(|e| &mut e.shape) {
        if *a == from {
            *a = to;
        } else if *b == from {
            *b = to;
        }
    }
}

/// How far from the corner along its legs something of `size` may
/// start: no further than most of the shorter leg.
fn back(
    args: &SketchArgs,
    corner: Id,
    legs: [(Id, Id); 2],
    size: f64,
) -> Option<(f64, [Point2; 2])> {
    let c = args.at(corner);
    let ways = [args.at(legs[0].1) - c, args.at(legs[1].1) - c];
    let short = ways[0].length().min(ways[1].length());
    let ways = [ways[0].try_normalize()?, ways[1].try_normalize()?];
    Some((size.min(short * 0.8), ways))
}

/// The corner where two lines meet at `corner`, rounded: an arc of
/// about `radius` tangent to both, the lines ending where it starts.
/// Returns the arc, to be selected (and given its radius).
pub(crate) fn fillet(args: &mut SketchArgs, corner: Id, radius: f64) -> Option<Vec<Item>> {
    let legs = legs(args, corner)?;
    let c = args.at(corner);
    let (_, ways) = back(args, corner, legs, radius)?;
    let half = ways[0].angle_to(ways[1]).abs() / 2.0;
    if half <= 1e-6 || half >= std::f64::consts::FRAC_PI_2 - 1e-6 {
        return None;
    }
    // (No further back along the legs than they are long.)
    let (reach, _) = back(args, corner, legs, radius / half.tan())?;
    let radius = reach * half.tan();
    let middle = (ways[0] + ways[1]).try_normalize()?;
    let centre = c + middle * (radius / half.sin());
    let ends = [c + ways[0] * reach, c + ways[1] * reach];
    let points = ends.map(|at| args.add_point(at));
    let centre = args.add_point(centre);
    // Counter-clockwise from its start.
    let turn = (ends[0] - args.at(centre)).perp_dot(ends[1] - args.at(centre));
    let (start, end) = if turn > 0.0 {
        (points[0], points[1])
    } else {
        (points[1], points[0])
    };
    for (leg, point) in legs.iter().zip(points) {
        retie(args, leg.0, corner, point);
    }
    let arc = args.add(Shape::Arc {
        center: centre,
        start,
        end,
    });
    for leg in legs {
        args.constrain(Kind::Tangent {
            a: leg.0,
            b: arc,
            internal: false,
        });
    }
    args.remove(&[Item::Point(corner)]);
    Some(vec![Item::Entity(arc)])
}

/// The corner where two lines meet at `corner`, cut off: a line
/// between the two, about `distance` back from the corner along each.
pub(crate) fn chamfer(args: &mut SketchArgs, corner: Id, distance: f64) -> Option<Vec<Item>> {
    let legs = legs(args, corner)?;
    let c = args.at(corner);
    let (reach, ways) = back(args, corner, legs, distance)?;
    let points = ways.map(|way| args.add_point(c + way * reach));
    for (leg, point) in legs.iter().zip(points) {
        retie(args, leg.0, corner, point);
    }
    let cut = args.add_line(points[0], points[1]);
    args.remove(&[Item::Point(corner)]);
    Some(vec![Item::Entity(cut)])
}

/// Curve `entity` (a line or an arc) split at `point`: two curves that
/// meet there. A line's halves stay on one line; an arc's keep its
/// centre.
pub(crate) fn split(args: &mut SketchArgs, entity: Id, point: Id) -> Option<Vec<Item>> {
    let shape = args.entities.get(&entity)?.shape.clone();
    let construction = args.entities[&entity].construction;
    let other = match shape {
        Shape::Line(a, b) if a != point && b != point => {
            args.entities.get_mut(&entity)?.shape = Shape::Line(a, point);
            let other = args.add_line(point, b);
            args.constrain(Kind::Collinear(entity, other));
            other
        }
        Shape::Arc { center, start, end } if start != point && end != point && center != point => {
            args.entities.get_mut(&entity)?.shape = Shape::Arc {
                center,
                start,
                end: point,
            };
            args.add(Shape::Arc {
                center,
                start: point,
                end,
            })
        }
        _ => return None,
    };
    args.entities.get_mut(&other)?.construction = construction;
    // (The point was on the curve; it is its halves' end now.)
    args.constraints.retain(|_, c| {
        !matches!(c.kind, Kind::PointOn { point: p, entity: e } if p == point && e == entity)
    });
    Some(vec![Item::Entity(entity), Item::Entity(other)])
}

/// Line `line` run on from its end nearer `target` (a line or a
/// circle) until it meets it, and kept on it.
pub(crate) fn extend(args: &mut SketchArgs, line: Id, target: Id) -> Option<Vec<Item>> {
    let Shape::Line(a, b) = args.entities.get(&line)?.shape else {
        return None;
    };
    let (pa, pb) = (args.at(a), args.at(b));
    let way = (pb - pa).try_normalize()?;
    // Where the line, as far as it goes either way, meets the target:
    // how far along it from its first end.
    let mut meets: Vec<f64> = Vec::new();
    if let Some((q, r)) = args.line(target) {
        let across = (r - q).perp();
        let along = way.dot(across);
        if along.abs() > 1e-12 {
            meets.push((q - pa).dot(across) / along);
        }
    } else if let Some((_, centre, radius)) = args.circle(target) {
        let foot = (centre - pa).dot(way);
        let off = (pa + way * foot).distance(centre);
        if off <= radius {
            let half = (radius * radius - off * off).sqrt();
            meets.extend([foot - half, foot + half]);
        }
    }
    let long = pa.distance(pb);
    // Beyond an end, the nearest: before the first end, or past the
    // second.
    let best = meets
        .into_iter()
        .filter(|t| *t < -1e-9 || *t > long + 1e-9)
        .min_by(|x, y| {
            let beyond = |t: f64| if t < 0.0 { -t } else { t - long };
            beyond(*x).total_cmp(&beyond(*y))
        })?;
    let end = if best < 0.0 { a } else { b };
    args.points.get_mut(&end)?.at = pa + way * best;
    args.constrain(Kind::PointOn {
        point: end,
        entity: target,
    });
    Some(vec![Item::Entity(line)])
}
