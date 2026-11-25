//! A solid cut in two by a plane.

use std::sync::Arc;

use anyhow::Context;
use workpiece_boolean::model::Walk;
use workpiece_boolean::{Op, Solid, combine};
use workpiece_core::math::Point3;
use workpiece_core::world::topo::Curve3;

/// What of `solid` is in front of the plane through `origin` across
/// `normal`, and what is behind it. Either is empty if the plane misses
/// the solid.
pub fn halves(solid: &Solid, origin: Point3, normal: Point3) -> anyhow::Result<[Solid; 2]> {
    let normal = normal.try_normalize().context("a plane has a normal")?;
    let far = reach(solid, origin);
    let (a, b) = normal.any_orthonormal_pair();
    let side = |out: Point3, name: &str| {
        let block = crate::blend::cuboid(
            name,
            origin - (a + b) * far,
            [out, a, b],
            [far, far * 2.0, far * 2.0],
        )?;
        anyhow::Ok(combine(Op::Cut, solid, &block)?.solid)
    };
    Ok([side(-normal, "behind")?, side(normal, "front")?])
}

/// What of `solid` is in front of the sheet `sheet` (the way its faces
/// look), and what is behind it. The sheet has to go all the way
/// through the solid: its free edges outside of it. Either is empty if
/// the sheet misses the solid.
pub fn halves_by(solid: &Solid, sheet: &Solid) -> anyhow::Result<[Solid; 2]> {
    use anyhow::ensure;
    ensure!(!sheet.faces.is_empty(), "a face to split by");
    // The way the sheet looks, all in all: what is in front of it is
    // what it sweeps going that way.
    let mut front = Point3::ZERO;
    for face in &sheet.faces {
        let ((u0, u1), (v0, v1)) = face.surface.domain();
        for i in 0..5 {
            for j in 0..5 {
                let (s, t) = ((i as f64 + 0.5) / 5.0, (j as f64 + 0.5) / 5.0);
                let (_, du, dv) = face
                    .surface
                    .derivatives_at(u0 + (u1 - u0) * s, v0 + (v1 - v0) * t);
                let normal = du.cross(dv).try_normalize().unwrap_or(Point3::ZERO);
                front += if face.outward { normal } else { -normal };
            }
        }
    }
    let front = front
        .try_normalize()
        .context("the face looks no one way: it cannot split a body")?;
    let mut walked = vec![0; sheet.edges.len()];
    for step in sheet.faces.iter().flat_map(|f| f.loops.iter().flatten()) {
        if let Walk::Edge { edge, .. } = step.walk {
            walked[edge] += 1;
        }
    }
    let free: Vec<usize> = (0..sheet.edges.len()).filter(|e| walked[*e] == 1).collect();
    ensure!(
        !free.is_empty(),
        "the faces close up: split by an open face"
    );
    for &e in &free {
        let edge = &sheet.edges[e];
        for k in 0..=8 {
            let t = edge.range.0 + (edge.range.1 - edge.range.0) * k as f64 / 8.0;
            ensure!(
                workpiece_boolean::holds(solid, edge.point(t)) != Some(true),
                "the face ends inside the body: it has to go all the way through it"
            );
        }
    }
    let (lo, hi) = sheet.extent();
    let far = reach(solid, (lo + hi) / 2.0) + (hi - lo).length();
    let by = front * far;
    let mut parts = vec![sheet.clone(), sheet.moved(&|p| p + by)?];
    for (k, &e) in free.iter().enumerate() {
        let edge = &sheet.edges[e];
        parts.push(crate::surface::ruled(
            &edge.curve,
            edge.range,
            by,
            &format!("split/w{k}"),
        )?);
    }
    let (block, closed) = workpiece_boolean::stitched(&parts, (hi - lo).length() * 1e-7)?;
    ensure!(closed, "the face does not sweep a closed block to split by");
    Ok([
        combine(Op::Intersect, solid, &block)?.solid,
        combine(Op::Cut, solid, &block)?.solid,
    ])
}

/// How far `solid` reaches from `origin`, and a little more: its faces'
/// control points are around all of it.
fn reach(solid: &Solid, origin: Point3) -> f64 {
    let mut far = 0.0_f64;
    for face in &solid.faces {
        for p in face.surface.control_points() {
            far = far.max(p.distance(origin));
        }
    }
    far * 1.5 + 1.0
}

/// A piece of a curve: the curve, and the parameters the piece is
/// between.
pub type Piece = (Arc<Curve3>, (f64, f64));

/// Where the plane through `origin` across `normal` cuts the boundary of
/// `solid`: the curves around the cut, each with the parameters of its
/// piece. They join end to end into closed outlines (an outline of one
/// curve ends where it starts), in no order. Nothing where the plane
/// misses the solid. They are the edges of the face the plane leaves on
/// what of the solid is in front of it.
pub fn section(solid: &Solid, origin: Point3, normal: Point3) -> anyhow::Result<Vec<Piece>> {
    let normal = normal.try_normalize().context("a plane has a normal")?;
    let far = reach(solid, origin);
    let a = normal.any_orthonormal_vector();
    let b = normal.cross(a);
    // A block behind the plane; its face `f0` is in it.
    let behind = workpiece_boolean::block(
        "section",
        origin - (a + b) * far,
        [b, a, -normal],
        [far * 2.0, far * 2.0, far],
    )?;
    let front = combine(Op::Cut, solid, &behind)
        .context("cutting the body at the plane")?
        .solid;
    let in_plane = |face: &workpiece_boolean::model::Face| {
        // (A face left in several pieces: `section/f0#2`, ….)
        let named = |n: &String| n == "section/f0" || n.starts_with("section/f0#");
        face.name.as_ref().is_some_and(named) || face.also.iter().any(named)
    };
    // (An edge between two faces in the plane is no outline of the cut.)
    let mut walked = vec![0; front.edges.len()];
    for face in front.faces.iter().filter(|f| in_plane(f)) {
        for step in face.loops.iter().flatten() {
            if let Walk::Edge { edge, .. } = step.walk {
                walked[edge] += 1;
            }
        }
    }
    let around = (0..front.edges.len()).filter(|&e| walked[e] == 1);
    Ok(around
        .map(|e| (front.edges[e].curve.clone(), front.edges[e].range))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use workpiece_boolean::geometry::chords;

    #[test]
    fn a_ball_is_cut_in_a_circle() -> anyhow::Result<()> {
        let centre = Point3::new(1.0, 2.0, 3.0);
        let ball = crate::blend::ball("ball", centre, 2.0)?;
        let normal = Point3::new(1.0, 1.0, 0.5).normalize();
        let origin = centre + normal * 1.2;
        let cut = section(&ball, origin, normal)?;
        assert!(!cut.is_empty());
        let mut length = 0.0;
        for (curve, range) in &cut {
            let points = chords(curve, *range, 1e-4);
            for pair in points.windows(2) {
                length += pair[0].distance(pair[1]);
            }
            for p in points {
                assert!((p - origin).dot(normal).abs() < 1e-8, "{p:?}");
                assert!((p.distance(centre) - 2.0).abs() < 1e-8, "{p:?}");
            }
        }
        let around = std::f64::consts::TAU * (4.0_f64 - 1.44).sqrt();
        assert!((length - around).abs() < 1e-3 * around, "{length} {around}");
        assert!(section(&ball, centre + normal * 3.0, normal)?.is_empty());
        Ok(())
    }
}
