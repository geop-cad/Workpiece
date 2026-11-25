//! The cylinder a face lies on, and a thread cut around one.

use std::f64::consts::TAU;

use anyhow::{Context, ensure};
use shapecraft_boolean::{Op, Solid, combine};
use shapecraft_core::math::{Point2, Point3};
use shapecraft_core::world::topo::Curve3;

use crate::swept::{Outline, polygon, steady, swept};

/// The cylinder a face lies on.
pub struct Cylinder {
    /// A point of its axis.
    pub origin: Point3,
    /// The axis's direction.
    pub axis: Point3,
    /// Its radius.
    pub radius: f64,
    /// How far along the axis the face reaches: from and to.
    pub reach: (f64, f64),
    /// Whether the face is a hole's wall.
    pub hole: bool,
}

/// The cylinder face `face` of `solid` lies on.
pub fn cylinder(solid: &Solid, face: usize) -> anyhow::Result<Cylinder> {
    let it = &solid.faces[face];
    let mut samples = Vec::new();
    let mut uvs = Vec::new();
    for use_ in it.loops.iter().flatten() {
        let (a, b) = use_.pcurve.domain();
        for k in 0..=8 {
            let uv: Point2 = use_.pcurve.point_at(a + (b - a) * k as f64 / 8.0);
            uvs.push(uv);
            let p = it.surface.point_at(uv.x, uv.y);
            let n = crate::thick::normal(&it.surface, uv);
            samples.push((p, if it.outward { n } else { -n }));
        }
    }
    let not = "the face is not part of a cylinder";
    let mut axis = Point3::ZERO;
    for (_, n) in &samples {
        let across = samples[0].1.cross(*n);
        if across.length() > axis.length() {
            axis = across;
        }
    }
    ensure!(axis.length() > 0.1, not);
    let axis = axis.normalize();
    let (e1, e2) = axis.any_orthonormal_pair();
    let flat = |p: Point3| Point2::new(p.dot(e1), p.dot(e2));
    let (q0, m0) = (flat(samples[0].0), flat(samples[0].1));
    let other = samples
        .iter()
        .max_by(|a, b| {
            let far = |s: &(Point3, Point3)| (flat(s.1) - m0).length();
            far(a).total_cmp(&far(b))
        })
        .context(not)?;
    let (q1, m1) = (flat(other.0), flat(other.1));
    let radius = (q0 - q1).dot(m0 - m1) / (m0 - m1).length_squared();
    let centre = q0 - m0 * radius;
    let size = radius.abs();
    // (Inside the face too, not only along its rim: a ball whose rim
    // is one circle through its poles is round across that rim.)
    let (lo, hi) = uvs.iter().fold(
        (
            Point2::splat(f64::INFINITY),
            Point2::splat(f64::NEG_INFINITY),
        ),
        |(lo, hi), uv| (lo.min(*uv), hi.max(*uv)),
    );
    let inside = (1..4).flat_map(|i| {
        (1..4).map(move |j| {
            let uv = lo + (hi - lo) * Point2::new(f64::from(i), f64::from(j)) / 4.0;
            let p = it.surface.point_at(uv.x, uv.y);
            let n = crate::thick::normal(&it.surface, uv);
            (p, if it.outward { n } else { -n })
        })
    });
    for (p, n) in inside.collect::<Vec<_>>().iter().chain(&samples) {
        ensure!(n.dot(axis).abs() < 1e-6, not);
        ensure!(
            (flat(*p) - flat(*n) * radius).distance(centre) < size * 1e-6,
            not
        );
    }
    let (mut from, mut to) = (f64::INFINITY, f64::NEG_INFINITY);
    for (p, _) in &samples {
        from = from.min(p.dot(axis));
        to = to.max(p.dot(axis));
    }
    ensure!(to > from && size > 0.0, not);
    let origin = e1 * centre.x + e2 * centre.y;
    Ok(Cylinder {
        origin,
        axis,
        radius: size,
        reach: (from, to),
        hole: radius < 0.0,
    })
}

/// `solid` with a thread cut into its cylindrical face `face`: a V
/// groove `depth` deep that advances `pitch` a turn, left-handed if
/// `left`.
pub fn threaded(
    solid: &Solid,
    face: usize,
    pitch: f64,
    depth: f64,
    left: bool,
) -> anyhow::Result<Solid> {
    ensure!(
        pitch > 0.0 && depth > 0.0,
        "a thread has a pitch and a depth"
    );
    let Cylinder {
        origin,
        axis,
        radius,
        reach: (from, to),
        hole,
    } = cylinder(solid, face)?;
    ensure!(
        hole || depth < radius,
        "the thread is deeper than the rod is thick"
    );
    // The groove: a triangle with its point in the material and its
    // flanks 60° apart, open a little way off the face.
    let off = pitch * 0.1;
    let half = (depth + off) * (TAU / 12.0).tan();
    ensure!(
        half * 2.0 < pitch * 0.98,
        "the thread is too deep for its pitch: its turns would run into each other"
    );
    // A helix a turn longer than the face at either end, in cubics a
    // sixteenth of a turn each.
    let hand = if left { -1.0 } else { 1.0 };
    let e1 = axis.any_orthonormal_vector();
    let e2 = axis.cross(e1);
    let start = from - pitch;
    let turns = (to - from) / pitch + 2.0;
    let pieces = (turns * 16.0).ceil() as usize;
    let step = turns * TAU / pieces as f64;
    let rise = pitch / TAU;
    let at = |a: f64| {
        origin + (e1 * a.cos() + e2 * (a.sin() * hand)) * radius + axis * (start + rise * a)
    };
    let way = |a: f64| (e2 * (a.cos() * hand) - e1 * a.sin()) * radius + axis * rise;
    let handle = 4.0 / 3.0 * (step / 4.0).tan();
    let mut points = vec![at(0.0)];
    let mut knots = vec![0.0; 4];
    for k in 0..pieces {
        let (a, b) = (step * k as f64, step * (k + 1) as f64);
        points.extend([at(a) + way(a) * handle, at(b) - way(b) * handle, at(b)]);
        knots.extend([(k + 1) as f64; 3]);
    }
    knots.push(pieces as f64);
    let path = Curve3::new(3, knots, points, None)?;
    // The section's x is across the path, towards the axis for a
    // right-handed helix: how far out from the face a place is, in it.
    let out = |by: f64| -by * hand;
    let (point, mouth) = if hole { (depth, -off) } else { (-depth, off) };
    let (sides, corners) = polygon(&[
        Point2::new(out(point), 0.0),
        Point2::new(out(mouth), -half),
        Point2::new(out(mouth), half),
    ]);
    let section = steady(&corners);
    let outline = Outline {
        sides,
        at: &section,
    };
    let size = (to - from) + radius * 2.0;
    let groove = swept("thread", &outline, &path, path.domain(), axis, size)?;
    Ok(combine(Op::Cut, solid, &groove)
        .context("cutting the thread")?
        .solid)
}
