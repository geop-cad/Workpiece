//! A sketch's dimensions as annotations of the model: what a drawing
//! made of the part shows of them.

use shapecraft_core::math::{Point2, Point3};
use shapecraft_core::world::{Annotation, DatumPlane, Measured, Note};

use crate::model::{Kind, Shape, SketchArgs, SketchAxis};

/// The dimensions of the sketch `id` with `args` (solved) on `plane`,
/// each as an annotation called `{id}/{name}` where it is named, else
/// `{id}/d{constraint}`.
pub(crate) fn annotations(
    id: &str,
    args: &SketchArgs,
    plane: &DatumPlane,
) -> Vec<(String, Annotation)> {
    let place = |p: Point2| plane.origin() + plane.x_dir() * p.x + plane.y_dir() * p.y;
    let point = |p: &u32| args.points.get(p).map(|p| p.at);
    let line = |e: &u32| match args.entities.get(e)?.shape {
        Shape::Line(a, b) => Some((point(&a)?, point(&b)?)),
        Shape::Axis { through, direction } => {
            let at = point(&through)?;
            Some((at, at + direction))
        }
        _ => None,
    };
    // A circle's or an arc's middle and radius.
    let round = |e: &u32| match args.entities.get(e)?.shape {
        Shape::Circle { center, radius } => Some((point(&center)?, radius)),
        Shape::Arc { center, start, .. } => {
            let centre = point(&center)?;
            Some((centre, centre.distance(point(&start)?)))
        }
        _ => None,
    };
    let geometry = shapecraft_core::world::Opaque::new(args.clone());
    let mut all = Vec::new();
    for (key, constraint) in &args.constraints {
        let Some(dimension) = &constraint.dimension else {
            continue;
        };
        let measured = match &constraint.kind {
            Kind::Distance(a, b) => point(a).zip(point(b)).map(|(a, b)| Measured::Length {
                from: place(a),
                to: place(b),
                along: None,
            }),
            Kind::DistanceAlong { a, b, axis } => {
                point(a).zip(point(b)).map(|(a, b)| Measured::Length {
                    from: place(a),
                    to: place(b),
                    along: Some(match axis {
                        SketchAxis::U => plane.x_dir(),
                        SketchAxis::V => plane.y_dir(),
                    }),
                })
            }
            Kind::PointLine { point: p, line: l } => point(p).zip(line(l)).map(|(p, (a, b))| {
                let way = (b - a).normalize_or_zero();
                let foot = a + way * (p - a).dot(way);
                if dimension.diameter {
                    // About the line as the axis: what the point sweeps.
                    Measured::Diameter {
                        centre: place(foot),
                        axis: plane.x_dir() * way.x + plane.y_dir() * way.y,
                        radius: foot.distance(p),
                    }
                } else {
                    Measured::Length {
                        from: place(foot),
                        to: place(p),
                        along: None,
                    }
                }
            }),
            Kind::PointRound {
                point: p,
                round: e,
                far,
            } => point(p).zip(round(e)).map(|(p, (centre, radius))| {
                let way = (centre - p).try_normalize().unwrap_or(Point2::X);
                let side = centre + way * if *far { radius } else { -radius };
                Measured::Length {
                    from: place(p),
                    to: place(side),
                    along: None,
                }
            }),
            Kind::LineRound {
                line: l,
                round: e,
                far,
            } => line(l).zip(round(e)).map(|((a, b), (centre, radius))| {
                let along = (b - a).normalize_or_zero();
                let foot = a + along * (centre - a).dot(along);
                let way = (centre - foot).try_normalize().unwrap_or(along.perp());
                let side = centre + way * if *far { radius } else { -radius };
                Measured::Length {
                    from: place(foot),
                    to: place(side),
                    along: None,
                }
            }),
            Kind::Gap(a, b) => round(a).zip(round(b)).map(|((ca, ra), (cb, rb))| {
                let d = ca.distance(cb);
                let way = (cb - ca).try_normalize().unwrap_or(Point2::X);
                let (from, to) = if d < (ra - rb).abs() {
                    // One inside the other: across the wall, where it
                    // is thinnest.
                    let out = if ra > rb { way } else { -way };
                    (ca + out * ra, cb + out * rb)
                } else {
                    (ca + way * ra, cb - way * rb)
                };
                Measured::Length {
                    from: place(from),
                    to: place(to),
                    along: None,
                }
            }),
            Kind::Size(e) => round(e).map(|(centre, radius)| {
                let (centre, axis) = (place(centre), plane.normal());
                if dimension.diameter {
                    Measured::Diameter {
                        centre,
                        axis,
                        radius,
                    }
                } else {
                    Measured::Radius {
                        centre,
                        axis,
                        radius,
                    }
                }
            }),
            Kind::Angle(first, second) => line(first).zip(line(second)).map(|((a, b), (c, d))| {
                // Where the lines cross, or the first one's start.
                let (u, v) = (b - a, d - c);
                let cross = u.perp_dot(v);
                let at = if cross.abs() > 1e-12 {
                    a + u * ((c - a).perp_dot(v) / cross)
                } else {
                    a
                };
                // A line measured from an axis of the plane, which goes
                // on without end, is measured at its own end nearer the
                // axis as soon as it does not reach it, the axis taken
                // through there: where an axis is crossed is always on
                // it. Two lines of the sketch are measured where they
                // cross, however far off what is drawn of them that is:
                // both are drawn on to there (ISO 129-1), no copy of
                // either taken through a point of the other.
                let endless = |e: &u32| args.is_endless(*e);
                let off = |(p, q): (Point2, Point2), e: &u32, other: &u32| {
                    let along = (q - p).normalize_or_zero();
                    let s = (at - p).dot(along);
                    let past = (-s).max(s - p.distance(q)).max(0.0);
                    (endless(other) && !endless(e) && past > 0.0).then_some(if s < 0.0 {
                        p
                    } else {
                        q
                    })
                };
                let at = off((a, b), first, second)
                    .or_else(|| off((c, d), second, first))
                    .unwrap_or(at);
                let way = |w: Point2| -> Point3 {
                    let w = w.normalize_or_zero();
                    plane.x_dir() * w.x + plane.y_dir() * w.y
                };
                Measured::Angle {
                    at: place(at),
                    first: way(u),
                    second: way(v),
                }
            }),
            _ => None,
        };
        let Some(measured) = measured else {
            continue;
        };
        // A driven dimension says what it measures, as that is: in
        // brackets, as a reference.
        let value = match (dimension.driven, &measured) {
            (true, Measured::Length { from, to, along }) => match along {
                Some(along) => (*to - *from).dot(*along).abs(),
                None => from.distance(*to),
            },
            (true, Measured::Radius { radius, .. }) => *radius,
            (true, Measured::Diameter { radius, .. }) => radius * 2.0,
            (true, Measured::Angle { first, second, .. }) => {
                first.angle_between(*second).to_degrees()
            }
            _ => dimension.value,
        };
        let tolerance = if dimension.driven {
            shapecraft_core::world::Tolerance::Reference
        } else {
            dimension.tolerance.clone()
        };
        all.push((
            match &dimension.name {
                Some(name) => format!("{id}/{name}"),
                None => format!("{id}/d{key}"),
            },
            Annotation {
                note: Note::Dimension {
                    measured,
                    value,
                    tolerance,
                    elsewhere: Vec::new(),
                },
                at: place(dimension.label),
                sketch: Some(shapecraft_core::world::SketchOf {
                    id: id.to_owned(),
                    constraint: *key,
                    geometry: geometry.clone(),
                    origin: plane.origin(),
                    x: plane.x_dir(),
                    y: plane.y_dir(),
                }),
            },
        ));
    }
    all
}
