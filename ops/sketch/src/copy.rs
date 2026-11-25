//! Copies that stay tied to their originals: mirrored in a line, or
//! offset to the side.

use std::collections::BTreeMap;

use workpiece_core::math::Point2;

use crate::model::{Id, Item, Kind, Shape, SketchArgs};

/// The selected entities of `selection`.
fn entities(args: &SketchArgs, selection: &[Item]) -> Vec<Id> {
    selection
        .iter()
        .filter_map(|item| match item {
            Item::Entity(e) if args.entities.contains_key(e) => Some(*e),
            _ => None,
        })
        .collect()
}

/// Adds copies of `originals`, every point of them where `to` puts
/// it: free of their originals, but sharing points among themselves as
/// the originals do. `to` moves, turns or scales (it does not flip).
/// Returns the copies.
pub(crate) fn copied(
    args: &mut SketchArgs,
    originals: &[Id],
    to: &dyn Fn(Point2) -> Point2,
    scale: f64,
) -> Vec<Id> {
    let mut image: BTreeMap<Id, Id> = BTreeMap::new();
    let mut copies = Vec::new();
    for &e in originals {
        for p in args.points_of(e) {
            if let std::collections::btree_map::Entry::Vacant(free) = image.entry(p) {
                free.insert(args.add_point(to(args.at(p))));
            }
        }
        let original = args.entities[&e].clone();
        let shape = match &original.shape {
            Shape::Point(p) => Shape::Point(image[p]),
            Shape::Line(p, q) => Shape::Line(image[p], image[q]),
            Shape::Circle { center, radius } => Shape::Circle {
                center: image[center],
                radius: *radius * scale,
            },
            Shape::Axis { through, direction } => {
                let at = args.at(*through);
                let turned = (to(at + *direction) - to(at)).normalize_or(*direction);
                Shape::Axis {
                    through: image[through],
                    direction: turned,
                }
            }
            Shape::Arc { center, start, end } => Shape::Arc {
                center: image[center],
                start: image[start],
                end: image[end],
            },
            Shape::Spline {
                control,
                degree,
                knots,
                weights,
            } => Shape::Spline {
                control: control.iter().map(|p| image[p]).collect(),
                degree: *degree,
                knots: knots.clone(),
                weights: weights.clone(),
            },
            Shape::Text(text) => Shape::Text(crate::TextBox {
                corners: text.corners.map(|p| image[&p]),
                size: text.size * scale,
                ..text.clone()
            }),
        };
        let copy = args.add(shape);
        if let Some(entity) = args.entities.get_mut(&copy) {
            entity.construction = original.construction;
        }
        copies.push(copy);
    }
    copies
}

/// The selected entities that can be copied: those drawn here.
pub(crate) fn copyable(args: &SketchArgs, selection: &[Item]) -> Vec<Id> {
    entities(args, selection)
        .into_iter()
        .filter(|e| !args.is_endless(*e) && !args.is_projected(*e))
        .collect()
}

/// Adds mirror images of the selected entities in line `mirror`, each
/// point symmetric to its original. Returns the copies.
pub(crate) fn mirror(args: &mut SketchArgs, selection: &[Item], mirror: Id) -> Vec<Item> {
    let Some((a, b)) = args.line(mirror) else {
        return Vec::new();
    };
    let axis = (b - a).normalize_or(Point2::X);
    let reflect = |p: Point2| a + axis * 2.0 * (p - a).dot(axis) - (p - a);
    let on_mirror = args.points_of(mirror);
    let mut image: BTreeMap<Id, Id> = BTreeMap::new();
    let mut copies = Vec::new();
    for e in entities(args, selection) {
        if e == mirror || args.is_endless(e) {
            continue;
        }
        for p in args.points_of(e) {
            if image.contains_key(&p) {
                continue;
            }
            // The mirror line's own points are their own images.
            let q = if on_mirror.contains(&p) {
                p
            } else {
                let q = args.add_point(reflect(args.at(p)));
                args.constrain(Kind::Symmetric {
                    a: p,
                    b: q,
                    line: mirror,
                });
                q
            };
            image.insert(p, q);
        }
        let original = args.entities[&e].clone();
        let shape = match &original.shape {
            Shape::Point(p) => Shape::Point(image[p]),
            Shape::Line(p, q) => Shape::Line(image[p], image[q]),
            Shape::Circle { center, radius } => Shape::Circle {
                center: image[center],
                radius: *radius,
            },
            Shape::Axis { through, direction } => Shape::Axis {
                through: image[through],
                direction: axis * 2.0 * direction.dot(axis) - *direction,
            },
            // A mirrored arc runs the other way round.
            Shape::Arc { center, start, end } => Shape::Arc {
                center: image[center],
                start: image[end],
                end: image[start],
            },
            Shape::Spline {
                control,
                degree,
                knots,
                weights,
            } => Shape::Spline {
                control: control.iter().map(|p| image[p]).collect(),
                degree: *degree,
                knots: knots.clone(),
                weights: weights.clone(),
            },
            // (Its letters as the mirror shows them.)
            Shape::Text(text) => Shape::Text(crate::TextBox {
                corners: text.corners.map(|p| image[&p]),
                ..text.clone()
            }),
        };
        let round = matches!(shape, Shape::Circle { .. });
        let copy = args.add(shape);
        if let Some(entity) = args.entities.get_mut(&copy) {
            entity.construction = original.construction;
        }
        if round {
            args.constrain(Kind::Equal(e, copy));
        }
        copies.push(Item::Entity(copy));
    }
    args.solve();
    copies
}

/// Adds copies of the selected lines, arcs and circles `distance` to
/// their side: to the left of the first as it is drawn (outside, for a
/// circle), and the same side all along a chain. The copies of curves
/// that meet share a point, and one dimension holds the distance. Returns
/// the copies, or `None` if nothing can be offset.
pub(crate) fn offset(
    args: &mut SketchArgs,
    selection: &[Item],
    distance: f64,
) -> Option<Vec<Item>> {
    let chosen: Vec<Id> = entities(args, selection)
        .into_iter()
        .filter(|&e| args.is_line(e) || args.circle(e).is_some())
        .collect();
    let first = *chosen.first()?;
    // The ends of each curve, as it is drawn.
    let ends = |args: &SketchArgs, e: Id| match args.entities[&e].shape {
        Shape::Line(a, b) => Some((a, b)),
        Shape::Arc { start, end, .. } => Some((start, end)),
        _ => None,
    };
    // Walk the chains: `reversed[e]` when a curve runs against the walk.
    let mut reversed: BTreeMap<Id, bool> = BTreeMap::new();
    let mut queue = vec![first];
    reversed.insert(first, false);
    let mut seeds = chosen.iter();
    loop {
        while let Some(e) = queue.pop() {
            let Some((start, end)) = ends(args, e) else {
                continue;
            };
            let against = reversed[&e];
            // The point the walk leaves `e` at, and the one it enters at.
            let (enter, leave) = if against { (end, start) } else { (start, end) };
            for &other in &chosen {
                if reversed.contains_key(&other) {
                    continue;
                }
                let Some((s, t)) = ends(args, other) else {
                    continue;
                };
                // Following on from `e`, or leading into it.
                let flipped = if s == leave || t == enter {
                    Some(false)
                } else if t == leave || s == enter {
                    Some(true)
                } else {
                    None
                };
                if let Some(flipped) = flipped {
                    reversed.insert(other, flipped);
                    queue.push(other);
                }
            }
        }
        match seeds.find(|e| !reversed.contains_key(e)) {
            Some(&seed) => {
                reversed.insert(seed, false);
                queue.push(seed);
            }
            None => break,
        }
    }

    let mut image: BTreeMap<Id, Id> = BTreeMap::new();
    let mut copies = Vec::new();
    let mut group: Option<Id> = None;
    for &e in &chosen {
        // Left of the walk: left of a line drawn along it, inside an arc
        // (they run counter-clockwise) drawn along it.
        let against = reversed[&e];
        let (shape, flip, label) = match args.entities[&e].shape.clone() {
            Shape::Line(a, b) => {
                let (pa, pb) = (args.at(a), args.at(b));
                let left = (pb - pa).normalize_or(Point2::X).perp();
                let side = if against { -left } else { left };
                let mut moved = |args: &mut SketchArgs, p: Id| {
                    *image
                        .entry(p)
                        .or_insert_with(|| args.add_point(args.at(p) + side * distance))
                };
                let (ca, cb) = (moved(args, a), moved(args, b));
                (
                    Shape::Line(ca, cb),
                    against,
                    (pa + pb) / 2.0 + side * distance / 2.0,
                )
            }
            Shape::Arc { center, start, end } => {
                let c = args.at(center);
                let inward = !against;
                let grow = if inward { -distance } else { distance };
                let r = c.distance(args.at(start));
                if r + grow <= 0.0 {
                    continue;
                }
                let mut moved = |args: &mut SketchArgs, p: Id| {
                    *image.entry(p).or_insert_with(|| {
                        let at = args.at(p);
                        args.add_point(c + (at - c).normalize_or(Point2::X) * (r + grow))
                    })
                };
                let (cs, ce) = (moved(args, start), moved(args, end));
                let shape = Shape::Arc {
                    center,
                    start: cs,
                    end: ce,
                };
                (
                    shape,
                    inward,
                    c + (args.at(start) - c).normalize_or(Point2::X) * (r + grow / 2.0),
                )
            }
            Shape::Circle { center, radius } => {
                let shape = Shape::Circle {
                    center,
                    radius: radius + distance,
                };
                let label = args.at(center) + Point2::X * (radius + distance / 2.0);
                (shape, false, label)
            }
            _ => continue,
        };
        let copy = args.add(shape);
        let id = args.dimension(
            Kind::Offset {
                a: e,
                b: copy,
                flip,
                group: 0,
            },
            distance,
            label,
        );
        let group = *group.get_or_insert(id);
        if let Some(Kind::Offset { group: g, .. }) =
            args.constraints.get_mut(&id).map(|c| &mut c.kind)
        {
            *g = group;
        }
        copies.push(Item::Entity(copy));
    }
    args.solve();
    (!copies.is_empty()).then_some(copies)
}
