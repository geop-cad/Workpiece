//! The curves of a sketch, or of a curve in space, as one path.

use anyhow::Context;
use shapecraft_core::Source;
use shapecraft_core::math::Point3;
use shapecraft_core::world::Profile;
use shapecraft_core::world::profile::EdgeUse;
use shapecraft_core::world::topo::Curve3;

use crate::sweep::Frame;

/// The edges of the curve body `name` as one path, as [`path`] gives a
/// sketch's; `None` if there is no such body. What stands for the
/// normal of its plane is the way it winds about its middle (a helix's
/// axis; the normal of a flat curve's plane).
fn wire_path(
    world: &shapecraft_core::World,
    name: &str,
    near: Point3,
) -> anyhow::Result<Option<(Vec<Curve3>, bool, Point3)>> {
    use shapecraft_core::world::topo::{BodyId, BodyKind};
    let topo = &world.topo;
    let Some(body) = topo.resolve::<BodyId>(name).and_then(|b| topo.body(b)) else {
        return Ok(None);
    };
    if body.kind != BodyKind::Wire {
        return Ok(None);
    }
    let mut edges = Vec::new();
    for &id in &body.free_edges {
        let edge = topo.edge(id).context("an edge of the curve is gone")?;
        let curve = topo.curve(edge.curve).context("an edge without a curve")?;
        edges.push((edge.start, edge.end, curve.trimmed(edge.t0, edge.t1)?));
    }
    let meets =
        |v| edges.iter().filter(|e| e.0 == v).count() + edges.iter().filter(|e| e.1 == v).count();
    let ends: Vec<_> = edges.iter().flat_map(|e| [e.0, e.1]).collect();
    anyhow::ensure!(
        ends.iter().all(|v| meets(*v) <= 2),
        "the path branches: it is one curve after the other"
    );
    let open: Vec<_> = ends.iter().copied().filter(|v| meets(*v) == 1).collect();
    anyhow::ensure!(open.len() <= 2, "the path is in several pieces");
    let closes = open.is_empty();
    let place = |v| topo.vertex(v).map_or(Point3::ZERO, |v| v.point);
    let candidates = if closes { ends.clone() } else { open };
    let mut at = candidates
        .into_iter()
        .min_by(|a, b| {
            place(*a)
                .distance(near)
                .total_cmp(&place(*b).distance(near))
        })
        .context("the path has no ends")?;
    let mut used = vec![false; edges.len()];
    let mut curves = Vec::new();
    while let Some(e) =
        (0..edges.len()).find(|e| !used[*e] && (edges[*e].0 == at || edges[*e].1 == at))
    {
        used[e] = true;
        let reversed = edges[e].0 != at;
        curves.push(if reversed {
            edges[e].2.reversed()
        } else {
            edges[e].2.clone()
        });
        at = if reversed { edges[e].0 } else { edges[e].1 };
    }
    anyhow::ensure!(used.iter().all(|u| *u), "the path is in several pieces");
    // The way it winds about its middle.
    let along: Vec<Point3> = curves
        .iter()
        .flat_map(|c| {
            let (from, to) = c.domain();
            (0..64).map(move |k| c.point_at(from + (to - from) * f64::from(k) / 64.0))
        })
        .collect();
    let middle = along.iter().sum::<Point3>() / along.len() as f64;
    let winds: Point3 = along
        .windows(2)
        .map(|w| (w[0] - middle).cross(w[1] - middle))
        .sum();
    let first = curves[0].derivative_at(curves[0].domain().0).1;
    let up = winds
        .try_normalize()
        .unwrap_or_else(|| first.normalize_or_zero().any_orthonormal_vector());
    Ok(Some((curves, closes, up)))
}

/// The curves of the sketch `source` names as one path, each starting
/// where the one before ends: from the end nearer `near`; whether it
/// closes; and the normal of its plane.
pub fn path(
    world: &shapecraft_core::World,
    source: &Source,
    near: Point3,
) -> anyhow::Result<(Vec<Curve3>, bool, Point3)> {
    let name = source.name();
    // (A curve in space is a path as a sketch's curves are.)
    if !world.profiles.contains_key(name)
        && let Some(found) = wire_path(world, name, near)?
    {
        return Ok(found);
    }
    let profile = world
        .profiles
        .get(name)
        .with_context(|| format!("there is no sketch {name:?}"))?;
    let frame = Frame::of(profile);
    let edges = &profile.edges;
    anyhow::ensure!(
        !edges.is_empty(),
        "the sketch {name:?} has no curve to go along"
    );
    let mut meets = vec![0; profile.vertices.len()];
    for edge in edges {
        meets[edge.start] += 1;
        meets[edge.end] += 1;
    }
    anyhow::ensure!(
        meets.iter().all(|n| *n <= 2),
        "the path branches: it is one curve after the other"
    );
    let open: Vec<usize> = (0..meets.len()).filter(|v| meets[*v] == 1).collect();
    anyhow::ensure!(open.len() <= 2, "the path is in several pieces");
    let closes = open.is_empty();
    // From the end nearer the section; all the way around, from the
    // corner nearest it.
    let far = |v: &usize| frame.place(profile.vertices[*v].point).distance(near);
    let candidates: Vec<usize> = if closes {
        (0..meets.len()).filter(|v| meets[*v] > 0).collect()
    } else {
        open
    };
    let first = candidates
        .into_iter()
        .min_by(|a, b| far(a).total_cmp(&far(b)))
        .context("the path has no ends")?;
    let mut used = vec![false; edges.len()];
    let mut at = first;
    let mut curves = Vec::new();
    while let Some(e) =
        (0..edges.len()).find(|e| !used[*e] && (edges[*e].start == at || edges[*e].end == at))
    {
        used[e] = true;
        let reversed = edges[e].start != at;
        curves.push(curve(profile, &frame, (e, reversed))?);
        at = if reversed {
            edges[e].start
        } else {
            edges[e].end
        };
    }
    anyhow::ensure!(used.iter().all(|u| *u), "the path is in several pieces");
    Ok((curves, closes, frame.normal()))
}

/// Edge `e` of `profile` in space, the way `reversed` says.
pub fn curve(profile: &Profile, frame: &Frame, (e, reversed): EdgeUse) -> anyhow::Result<Curve3> {
    let edge = &profile.edges[e];
    let piece = edge.curve.trimmed(edge.t0, edge.t1)?;
    let points = piece.control_points().iter().map(|&p| frame.place(p));
    let curve = Curve3::new(
        piece.degree(),
        piece.knots().to_vec(),
        points.collect(),
        Some(piece.weights().to_vec()),
    )?;
    Ok(if reversed { curve.reversed() } else { curve })
}
