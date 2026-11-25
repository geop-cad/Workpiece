//! Cutting a world open: what of it is on one side of a plane.

use std::sync::Arc;

use shapecraft_boolean::Solid;
use shapecraft_core::math::{Point2, Point3};
use shapecraft_core::world::World;
use shapecraft_core::world::topo::{BodyKind, Curve3};

/// The corners of the box from `lo` to `hi`.
pub fn corners(lo: Point3, hi: Point3) -> [Point3; 8] {
    std::array::from_fn(|i| {
        let pick = |bit: usize, a: f64, b: f64| if i >> bit & 1 == 1 { b } else { a };
        Point3::new(
            pick(0, lo.x, hi.x),
            pick(1, lo.y, hi.y),
            pick(2, lo.z, hi.z),
        )
    })
}

/// `world` with what of its solid bodies is beyond the plane (through
/// `origin`, `normal` pointing beyond) cut away, all through what is
/// placed in it.
pub fn cut(world: &World, origin: Point3, normal: Point3, far: f64) -> World {
    let mut out = world.clone();
    let x = normal.any_orthonormal_vector();
    let y = normal.cross(x);
    let corner = origin - (x + y + normal) * far;
    let block = shapecraft_boolean::block(
        "section",
        corner,
        [x, y, normal],
        [far * 2.0, far * 2.0, far],
    );
    let bodies: Vec<_> = out
        .topo
        .bodies()
        .iter()
        .filter(|(_, b)| b.kind == BodyKind::Solid)
        .map(|(id, _)| id)
        .collect();
    for body in bodies {
        let (Ok(solid), Ok(block)) = (shapecraft_boolean::read(&out.topo, body), &block) else {
            continue;
        };
        let Some(name) = out.topo.name_of(body).map(str::to_owned) else {
            continue;
        };
        // All of it on one side: as it is, or gone.
        let (lo, hi) = solid.extent();
        let sides = corners(lo, hi).map(|c| (c - origin).dot(normal));
        if sides.iter().all(|s| *s <= 0.0) {
            continue;
        }
        if sides.iter().all(|s| *s >= 0.0) {
            shapecraft_boolean::remove(&mut out.topo, body);
            continue;
        }
        let Ok(kept) =
            shapecraft_boolean::combine(shapecraft_boolean::Op::Intersect, &solid, block)
        else {
            continue;
        };
        shapecraft_boolean::remove(&mut out.topo, body);
        if !kept.solid.is_empty() {
            // (A body that cannot be written back is not shown cut.)
            let _ = shapecraft_boolean::write(&mut out.topo, &kept.solid, &name);
        }
    }
    for instance in out.instances.values_mut() {
        let back = instance.pose.inverse();
        // (A part a section leaves whole, as it is.)
        if !instance.world.sectioned.cuts(back.transform_vector(normal)) {
            continue;
        }
        let inner = cut(
            &instance.world,
            back.transform_point(origin),
            back.transform_vector(normal),
            far,
        );
        instance.world = Arc::new(inner);
    }
    out
}

/// `world` with what of its solid bodies is to the left of the path
/// `path` cut away, all through what is placed in it: the path lies in
/// the plane through `origin` along `x` and `y` (unit vectors, square
/// to each other), is gone along from its first point to its last, runs
/// on straight past both ends, and cuts square to the plane as far as
/// `far` either way.
pub fn cut_left_of(
    world: &World,
    origin: Point3,
    x: Point3,
    y: Point3,
    path: &[Point2],
    far: f64,
) -> World {
    let mut out = world.clone();
    let Some(prism) = prism(origin, x, y, path, far) else {
        return out;
    };
    let bodies: Vec<_> = out
        .topo
        .bodies()
        .iter()
        .filter(|(_, b)| b.kind == BodyKind::Solid)
        .map(|(id, _)| id)
        .collect();
    for body in bodies {
        let Ok(solid) = shapecraft_boolean::read(&out.topo, body) else {
            continue;
        };
        let Some(name) = out.topo.name_of(body).map(str::to_owned) else {
            continue;
        };
        let Ok(kept) = shapecraft_boolean::combine(shapecraft_boolean::Op::Cut, &solid, &prism)
        else {
            continue;
        };
        shapecraft_boolean::remove(&mut out.topo, body);
        if !kept.solid.is_empty() {
            let _ = shapecraft_boolean::write(&mut out.topo, &kept.solid, &name);
        }
    }
    for instance in out.instances.values_mut() {
        let back = instance.pose.inverse();
        let inner = shapecraft_core::world::Cutting {
            origin: back.transform_point(origin),
            x: back.transform_vector(x),
            y: back.transform_vector(y),
            path: path.to_vec(),
            bounded: false,
        };
        if left_whole(&instance.world, &inner) {
            continue;
        }
        let inner = cut_left_of(&instance.world, inner.origin, inner.x, inner.y, path, far);
        instance.world = Arc::new(inner);
    }
    out
}

/// The solid of everything to the left of `path` (see [`cut_left_of`]).
fn prism(origin: Point3, x: Point3, y: Point3, path: &[Point2], far: f64) -> Option<Solid> {
    use crate::swept::{Outline, polygon, steady, swept};
    let mut points: Vec<Point2> = Vec::new();
    for p in path {
        if points.last().is_none_or(|q| q.distance(*p) > far * 1e-9) {
            points.push(*p);
        }
    }
    let (first, last) = (
        (points[1..].first()? - points[0]).normalize(),
        (*points.last()? - points[points.len() - 2]).normalize(),
    );
    // On past both ends, and around on the left.
    let start = points[0] - first * far;
    let end = *points.last()? + last * far;
    points[0] = start;
    *points.last_mut()? = end;
    points.push(end + last.perp() * far);
    points.push(start + first.perp() * far);
    // A point just left of the path's first piece is in it, whichever
    // way the outline is taken to lie.
    let probe = {
        let middle = (points[0] + points[1]) / 2.0 + first * (far * 0.5);
        let p = middle + first.perp() * (far * 1e-3);
        origin + x * p.x + y * p.y
    };
    let z = x.cross(y);
    let path3 = Curve3::line(origin - z * far, origin + z * far).ok()?;
    for mirror in [1.0, -1.0] {
        let corners: Vec<Point2> = points
            .iter()
            .map(|p| Point2::new(p.x * mirror, p.y))
            .collect();
        let (sides, weighted) = polygon(&corners);
        let at = steady(&weighted);
        let outline = Outline { sides, at: &at };
        let Ok(solid) = swept("section", &outline, &path3, path3.domain(), y, far * 4.0) else {
            continue;
        };
        if shapecraft_boolean::holds(&solid, probe) == Some(true) {
            return Some(solid);
        }
    }
    None
}

/// Whether a section along `cut` leaves the part `world` (in its own
/// frame) whole: a part not sectioned along it (a shaft, a screw) that
/// the cut runs through or that stays all of it — not one all of which
/// the cut takes away.
pub fn left_whole(world: &World, cut: &shapecraft_core::world::Cutting) -> bool {
    if world.sectioned.cut_by(cut) {
        return false;
    }
    // (Around its faces, not its vertices: a turned part's vertices are
    // all on one side of its axis.)
    let Some((lo, hi)) = world.reach() else {
        return true;
    };
    // (Taken: to the left of the piece of the line it is beside, the
    // first and the last running on.)
    let pieces = cut.path.len().saturating_sub(1);
    let taken = |p: Point3| {
        let d = p - cut.origin;
        let q = Point2::new(d.dot(cut.x), d.dot(cut.y));
        cut.path
            .windows(2)
            .enumerate()
            .map(|(k, w)| {
                let along = w[1] - w[0];
                let s = ((q - w[0]).dot(along) / along.length_squared().max(1e-300)).clamp(
                    if k == 0 { f64::NEG_INFINITY } else { 0.0 },
                    if k + 1 == pieces { f64::INFINITY } else { 1.0 },
                );
                let off = (w[0] + along * s).distance(q);
                (off, along.normalize_or_zero().perp_dot(q - w[0]))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .is_some_and(|(_, left)| left > 0.0)
    };
    !corners(lo, hi).into_iter().all(taken)
}
