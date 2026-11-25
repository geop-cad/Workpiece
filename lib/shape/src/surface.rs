//! Surfaces as sheets: faces copied off a body, holes filled, edges
//! ruled, faces made whole, bounded, trimmed and rounded.

use anyhow::{Context, ensure};
use workpiece_boolean::Solid;
use workpiece_boolean::model::{Face, Use, Walk};

/// The faces `faces` of `solid` as a sheet of their own.
pub fn faces_of(solid: &Solid, faces: &[usize]) -> Solid {
    let mut out = Solid::default();
    let mut vertices = std::collections::BTreeMap::new();
    let mut edges = std::collections::BTreeMap::new();
    for &f in faces {
        let face = &solid.faces[f];
        let mut loops = Vec::new();
        for walk in &face.loops {
            let mut steps = Vec::new();
            for step in walk {
                let mut vertex = |out: &mut Solid, v: usize| {
                    *vertices.entry(v).or_insert_with(|| {
                        out.vertices.push(solid.vertices[v].clone());
                        out.vertices.len() - 1
                    })
                };
                let walk = match step.walk {
                    Walk::Edge { edge, reversed } => {
                        let e = &solid.edges[edge];
                        let ends = [vertex(&mut out, e.ends[0]), vertex(&mut out, e.ends[1])];
                        let edge = *edges.entry(edge).or_insert_with(|| {
                            out.edges
                                .push(workpiece_boolean::model::Edge { ends, ..e.clone() });
                            out.edges.len() - 1
                        });
                        Walk::Edge { edge, reversed }
                    }
                    Walk::Pole { vertex: v, range } => Walk::Pole {
                        vertex: vertex(&mut out, v),
                        range,
                    },
                };
                steps.push(Use {
                    walk,
                    pcurve: step.pcurve.clone(),
                });
            }
            loops.push(steps);
        }
        out.faces.push(Face {
            loops,
            smooth: Vec::new(),
            ..face.clone()
        });
    }
    out
}

/// `sheet` with its flat holes filled: every ring of free edges that
/// lies in a plane gets a flat face. Returns how many were filled.
pub fn filled(sheet: &Solid) -> anyhow::Result<(Solid, usize)> {
    use workpiece_core::math::{Point2, Point3};
    use workpiece_core::world::topo::{Curve2, NurbsSurface};
    let mut walked = vec![0; sheet.edges.len()];
    for step in sheet.faces.iter().flat_map(|f| f.loops.iter().flatten()) {
        if let Walk::Edge { edge, .. } = step.walk {
            walked[edge] += 1;
        }
    }
    let mut free: Vec<usize> = (0..sheet.edges.len()).filter(|e| walked[*e] == 1).collect();
    let (lo, hi) = sheet.extent();
    let within = (hi - lo).length() * 1e-7;
    let mut out = sheet.clone();
    let mut made = 0;
    while let Some(first) = free.pop() {
        // Around from edge to edge, back to where it started.
        let mut ring = vec![(first, false)];
        let (start, mut at) = (sheet.edges[first].ends[0], sheet.edges[first].ends[1]);
        while at != start {
            let next = free.iter().position(|e| sheet.edges[*e].ends.contains(&at));
            let Some(next) = next else {
                break;
            };
            let edge = free.swap_remove(next);
            let reversed = sheet.edges[edge].ends[0] != at;
            at = sheet.edges[edge].ends[usize::from(!reversed)];
            ring.push((edge, reversed));
        }
        if at != start {
            continue;
        }
        // Its plane, if it lies in one.
        let mut places: Vec<Point3> = Vec::new();
        for &(e, reversed) in &ring {
            let edge = &sheet.edges[e];
            let mut along: Vec<Point3> = (0..16)
                .map(|k| {
                    edge.point(edge.range.0 + (edge.range.1 - edge.range.0) * f64::from(k) / 16.0)
                })
                .collect();
            if reversed {
                along.push(edge.point(edge.range.1));
                along.reverse();
                along.pop();
            }
            places.extend(along);
        }
        let origin = places[0];
        let normal: Point3 = (0..places.len())
            .map(|k| (places[k] - origin).cross(places[(k + 1) % places.len()] - origin))
            .sum();
        let Some(normal) = normal.try_normalize() else {
            continue;
        };
        if places
            .iter()
            .any(|p| (*p - origin).dot(normal).abs() > within)
        {
            continue;
        }
        // (Its normal is the way the ring turns: the ring runs with the
        // face on its left.)
        let x = normal.any_orthonormal_vector();
        let y = normal.cross(x);
        let flat = |p: Point3| Point2::new((p - origin).dot(x), (p - origin).dot(y));
        let (mut low, mut high) = (Point2::INFINITY, Point2::NEG_INFINITY);
        for edge in ring.iter().map(|r| &sheet.edges[r.0]) {
            for p in edge.curve.control_points() {
                (low, high) = (low.min(flat(*p)), high.max(flat(*p)));
            }
        }
        let pad = (high - low).length() * 0.1 + within;
        let (low, high) = (low - Point2::splat(pad), high + Point2::splat(pad));
        let place = |p: Point2| origin + x * p.x + y * p.y;
        let surface = NurbsSurface::new(
            1,
            1,
            vec![low.x, low.x, high.x, high.x],
            vec![low.y, low.y, high.y, high.y],
            2,
            2,
            vec![
                place(low),
                place(Point2::new(low.x, high.y)),
                place(Point2::new(high.x, low.y)),
                place(high),
            ],
            None,
        )?;
        let mut steps = Vec::new();
        for &(e, reversed) in &ring {
            let curve = &sheet.edges[e].curve;
            let pcurve = Curve2::new(
                curve.degree(),
                curve.knots().to_vec(),
                curve.control_points().iter().map(|p| flat(*p)).collect(),
                Some(curve.weights().to_vec()),
            )?;
            steps.push(Use {
                walk: Walk::Edge { edge: e, reversed },
                pcurve: std::sync::Arc::new(pcurve),
            });
        }
        out.faces.push(Face {
            surface: std::sync::Arc::new(surface),
            outward: true,
            loops: vec![steps],
            tolerance: within.max(1e-9),
            name: None,
            also: Vec::new(),
            smooth: Vec::new(),
        });
        made += 1;
    }
    Ok((out, made))
}

/// The surface the piece of `curve` over `range` sweeps moved by `by`
/// without turning: one face, ruled from the curve to itself that far
/// on.
pub fn ruled(
    curve: &workpiece_core::world::topo::Curve3,
    range: (f64, f64),
    by: workpiece_core::math::Point3,
    id: &str,
) -> anyhow::Result<Solid> {
    use std::sync::Arc;
    use workpiece_boolean::model::{Edge, Vertex};
    use workpiece_core::math::Point2;
    use workpiece_core::world::topo::{Curve2, Curve3, NurbsSurface};
    ensure!(by.length() > 0.0 && by.is_finite(), "swept some way");
    let near = curve.trimmed(range.0, range.1)?;
    let far = Curve3::new(
        near.degree(),
        near.knots().to_vec(),
        near.control_points().iter().map(|p| *p + by).collect(),
        Some(near.weights().to_vec()),
    )?;
    let (mut points, mut weights) = (Vec::new(), Vec::new());
    for k in 0..near.control_points().len() {
        for c in [&near, &far] {
            points.push(c.control_points()[k]);
            weights.push(c.weights()[k]);
        }
    }
    let surface = NurbsSurface::new(
        near.degree(),
        1,
        near.knots().to_vec(),
        vec![0.0, 0.0, 1.0, 1.0],
        near.control_points().len(),
        2,
        points,
        Some(weights),
    )?;
    let (t0, t1) = range;
    let (a, b) = (near.point_at(t0), near.point_at(t1));
    let tolerance = by.length() * 1e-9;
    let closed = a.distance(b) <= tolerance;
    let mut out = Solid::default();
    let vertex = |out: &mut Solid, point| {
        out.vertices.push(Vertex {
            point,
            tolerance: tolerance * 4.0,
            name: None,
        });
        out.vertices.len() - 1
    };
    let (v0, w0) = (vertex(&mut out, a), vertex(&mut out, a + by));
    let (v1, w1) = if closed {
        (v0, w0)
    } else {
        (vertex(&mut out, b), vertex(&mut out, b + by))
    };
    let edge = |out: &mut Solid, curve: Curve3, range, ends, name: &str| {
        out.edges.push(Edge {
            curve: Arc::new(curve),
            range,
            ends,
            tolerance,
            name: Some(format!("{id}/{name}")),
            also: Vec::new(),
        });
        out.edges.len() - 1
    };
    let along = edge(&mut out, near, range, [v0, v1], "near");
    let beyond = edge(&mut out, far, range, [w0, w1], "far");
    let first = edge(
        &mut out,
        Curve3::line(a, a + by)?,
        (0.0, 1.0),
        [v0, w0],
        "start",
    );
    let last = if closed {
        first
    } else {
        edge(
            &mut out,
            Curve3::line(b, b + by)?,
            (0.0, 1.0),
            [v1, w1],
            "end",
        )
    };
    let line =
        |from: (f64, f64), to: (f64, f64), range: (f64, f64)| -> anyhow::Result<Arc<Curve2>> {
            Ok(Arc::new(Curve2::new(
                1,
                vec![range.0, range.0, range.1, range.1],
                vec![Point2::new(from.0, from.1), Point2::new(to.0, to.1)],
                None,
            )?))
        };
    let step = |edge, reversed, pcurve| Use {
        walk: Walk::Edge { edge, reversed },
        pcurve,
    };
    let walk = vec![
        step(along, false, line((t0, 0.0), (t1, 0.0), range)?),
        step(last, false, line((t1, 0.0), (t1, 1.0), (0.0, 1.0))?),
        step(beyond, true, line((t0, 1.0), (t1, 1.0), range)?),
        step(first, true, line((t0, 0.0), (t0, 1.0), (0.0, 1.0))?),
    ];
    out.faces.push(Face {
        surface: Arc::new(surface),
        outward: true,
        loops: vec![walk],
        tolerance,
        name: Some(format!("{id}/face")),
        also: Vec::new(),
        smooth: Vec::new(),
    });
    Ok(out)
}

/// All of `surface` as one face: bounded by its four sides. For a
/// surface with four sides of its own: not one that closes on itself
/// or comes to a point along a side.
pub fn whole(
    surface: &std::sync::Arc<workpiece_core::world::topo::NurbsSurface>,
    id: &str,
) -> anyhow::Result<Solid> {
    use std::sync::Arc;
    use workpiece_boolean::model::{Edge, Vertex};
    use workpiece_core::math::Point2;
    use workpiece_core::world::topo::{Curve2, Curve3};
    let (p, q) = surface.degrees();
    let (nu, nv) = surface.counts();
    let (ku, kv) = surface.knots();
    let ((u0, u1), (v0, v1)) = surface.domain();
    let at = |i: usize, j: usize| {
        (
            surface.control_points()[i * nv + j],
            surface.weights()[i * nv + j],
        )
    };
    // Its sides: along u at the first and the last v, along v at the
    // first and the last u.
    let along_u = |j: usize| -> anyhow::Result<Curve3> {
        let (points, weights): (Vec<_>, Vec<_>) = (0..nu).map(|i| at(i, j)).unzip();
        Ok(Curve3::new(p, ku.to_vec(), points, Some(weights))?)
    };
    let along_v = |i: usize| -> anyhow::Result<Curve3> {
        let (points, weights): (Vec<_>, Vec<_>) = (0..nv).map(|j| at(i, j)).unzip();
        Ok(Curve3::new(q, kv.to_vec(), points, Some(weights))?)
    };
    let corners = [
        surface.point_at(u0, v0),
        surface.point_at(u1, v0),
        surface.point_at(u1, v1),
        surface.point_at(u0, v1),
    ];
    let size = corners[0]
        .distance(corners[2])
        .max(corners[1].distance(corners[3]));
    let tolerance = size * 1e-9;
    for k in 0..4 {
        ensure!(
            corners[k].distance(corners[(k + 1) % 4]) > tolerance * 16.0,
            "the surface closes on itself or comes to a point: it has no four sides to go back to"
        );
    }
    let mut out = Solid::default();
    for point in corners {
        out.vertices.push(Vertex {
            point,
            tolerance: tolerance * 4.0,
            name: None,
        });
    }
    let sides = [
        (along_u(0)?, (u0, u1), [0, 1], "south"),
        (along_v(nu - 1)?, (v0, v1), [1, 2], "east"),
        (along_u(nv - 1)?, (u0, u1), [3, 2], "north"),
        (along_v(0)?, (v0, v1), [0, 3], "west"),
    ];
    for (curve, range, ends, name) in sides {
        out.edges.push(Edge {
            curve: Arc::new(curve),
            range,
            ends,
            tolerance,
            name: Some(format!("{id}/{name}")),
            also: Vec::new(),
        });
    }
    let line =
        |from: (f64, f64), to: (f64, f64), range: (f64, f64)| -> anyhow::Result<Arc<Curve2>> {
            Ok(Arc::new(Curve2::new(
                1,
                vec![range.0, range.0, range.1, range.1],
                vec![Point2::new(from.0, from.1), Point2::new(to.0, to.1)],
                None,
            )?))
        };
    let step = |edge, reversed, pcurve| Use {
        walk: Walk::Edge { edge, reversed },
        pcurve,
    };
    let walk = vec![
        step(0, false, line((u0, v0), (u1, v0), (u0, u1))?),
        step(1, false, line((u1, v0), (u1, v1), (v0, v1))?),
        step(2, true, line((u0, v1), (u1, v1), (u0, u1))?),
        step(3, true, line((u0, v0), (u0, v1), (v0, v1))?),
    ];
    out.faces.push(Face {
        surface: surface.clone(),
        outward: true,
        loops: vec![walk],
        tolerance,
        name: Some(format!("{id}/face")),
        also: Vec::new(),
        smooth: Vec::new(),
    });
    Ok(out)
}

/// The surface between four curves that meet end to end around it
/// (each given with the piece of it that counts): every point of it
/// blended from the four, so that it runs along each of them. One face.
pub fn between(
    sides: &[(
        std::sync::Arc<workpiece_core::world::topo::Curve3>,
        (f64, f64),
    ); 4],
    id: &str,
) -> anyhow::Result<Solid> {
    use workpiece_core::math::Point3;
    use workpiece_core::world::topo::NurbsSurface;
    let end = |k: usize, last: bool| {
        let (curve, range) = &sides[k];
        curve.point_at(if last { range.1 } else { range.0 })
    };
    let size = (0..4)
        .map(|k| end(k, false).distance(end(k, true)))
        .fold(0.0, f64::max);
    ensure!(size > 0.0, "the edges have some length");
    let near = size * 1e-6;
    // Around: each from where the one before ends (turned around, if it
    // runs the other way).
    let mut around: Vec<(usize, bool)> = vec![(0, false)];
    let mut at = end(0, true);
    let mut left = vec![1, 2, 3];
    while !left.is_empty() {
        let next = left
            .iter()
            .position(|&k| end(k, false).distance(at) <= near || end(k, true).distance(at) <= near);
        let next = next.context("the four edges do not meet end to end around one face")?;
        let k = left.remove(next);
        let flipped = end(k, false).distance(at) > near;
        at = end(k, !flipped);
        around.push((k, flipped));
    }
    ensure!(
        at.distance(end(0, false)) <= near,
        "the four edges do not close around one face"
    );
    // Along each, from 0 to 1 the way it is walked.
    let on = |n: usize, t: f64| {
        let (k, flipped) = around[n];
        let (curve, (a, b)) = &sides[k];
        let t = if flipped { 1.0 - t } else { t };
        curve.point_at(a + (b - a) * t)
    };
    // South and north run along u, west and east along v.
    let south = |u: f64| on(0, u);
    let east = |v: f64| on(1, v);
    let north = |u: f64| on(2, 1.0 - u);
    let west = |v: f64| on(3, 1.0 - v);
    let corners = [south(0.0), south(1.0), north(1.0), north(0.0)];
    let blend = |u: f64, v: f64| -> Point3 {
        let ruled_v = south(u) * (1.0 - v) + north(u) * v;
        let ruled_u = west(v) * (1.0 - u) + east(v) * u;
        let flat = corners[0] * ((1.0 - u) * (1.0 - v))
            + corners[1] * (u * (1.0 - v))
            + corners[2] * (u * v)
            + corners[3] * ((1.0 - u) * v);
        ruled_v + ruled_u - flat
    };
    // As a bicubic surface as close to that as anything here.
    let knots = vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
    let start: Vec<Point3> = (0..4)
        .flat_map(|i| (0..4).map(move |j| (i, j)))
        .map(|(i, j)| blend(i as f64 / 3.0, j as f64 / 3.0))
        .collect();
    let like = NurbsSurface::new(3, 3, knots.clone(), knots, 4, 4, start, None)?;
    let surface = crate::thick::refit(&like, &blend, size * 1e-8)?;
    whole(&std::sync::Arc::new(surface), id)
}

/// What of `sheet` is behind the plane through `origin` (`normal`
/// pointing to its front): its faces cut where the plane crosses them.
/// Nothing, for a sheet all in front.
pub fn trimmed(
    sheet: &Solid,
    origin: workpiece_core::math::Point3,
    normal: workpiece_core::math::Point3,
) -> anyhow::Result<Solid> {
    let (lo, hi) = sheet.extent();
    let reach = (hi - lo).length();
    ensure!(reach > 0.0, "the surface has some size");
    // The sheet as a thin solid, cut as solids are; of what is left,
    // the faces in the sheet's own surfaces.
    let all: Vec<usize> = (0..sheet.faces.len()).collect();
    let thin = crate::thick::thicken(sheet, &all, reach * 1e-3, false, true)?;
    let far = reach * 2.0 + lo.distance(origin) + hi.distance(origin);
    let x = normal.any_orthonormal_vector();
    let y = normal.cross(x);
    let corner = origin - (x + y + normal) * far;
    let behind =
        workpiece_boolean::block("trim", corner, [x, y, normal], [far * 2.0, far * 2.0, far])?;
    let cut = workpiece_boolean::combine(workpiece_boolean::Op::Intersect, &thin, &behind)?.solid;
    // (Which of the sheet's faces a face left is a piece of: by the
    // name it came with, which stays with it through the cut.)
    let kept: Vec<usize> = (0..cut.faces.len())
        .filter(|&f| tag_of(&cut.faces[f]).is_some())
        .collect();
    let mut out = faces_of(&cut, &kept);
    for face in &mut out.faces {
        if let Some(was) = tag_of(face).map(|k| &sheet.faces[k]) {
            face.outward = was.outward;
            face.name = was.name.clone();
            face.also = was.also.clone();
        }
    }
    Ok(out)
}

/// Which face of the sheet that was thickened `face` is a piece of, if
/// it is one: told by its name, which says so (`thick/s0/f3/near`, the
/// sheet's fourth face itself) and stays with it through what is done
/// to the solid — cut, rounded, made one with another in its plane.
fn tag_of(face: &Face) -> Option<usize> {
    let names = face.name.iter().chain(&face.also);
    let mut places = names.filter_map(|name| {
        let whole = name.split('#').next()?;
        let of = whole.strip_prefix("thick/s")?.strip_suffix("/near")?;
        of.split_once("/f")?.1.parse().ok()
    });
    places.next()
}

/// A point of face `face` of `solid` that is on none of its edges.
pub fn inside(solid: &Solid, face: usize) -> Option<workpiece_core::math::Point3> {
    let it = &solid.faces[face];
    let ((u0, u1), (v0, v1)) = it.surface.domain();
    let reach = {
        let (lo, hi) = solid.extent();
        (hi - lo).length()
    };
    for step in it.loops.iter().flatten() {
        let (from, to) = step.pcurve.domain();
        let middle = (from + to) / 2.0;
        let at = step.pcurve.point_at(middle);
        let along = step.pcurve.point_at(middle + (to - from) * 1e-3) - at;
        // The face is to the left of its edges as they are walked.
        let walked = match step.walk {
            Walk::Edge { reversed: true, .. } => -along,
            _ => along,
        };
        let Some(left) = workpiece_core::math::Point2::new(-walked.y, walked.x).try_normalize()
        else {
            continue;
        };
        for share in [1e-2, 1e-3] {
            let uv =
                at + left * workpiece_core::math::Point2::new(u1 - u0, v1 - v0).length() * share;
            let p = it.surface.point_at(uv.x.clamp(u0, u1), uv.y.clamp(v0, v1));
            let off_edges = solid.edges.iter().all(|e| {
                let near = (0..=8).map(|k| {
                    let t = e.range.0 + (e.range.1 - e.range.0) * f64::from(k) / 8.0;
                    e.curve.point_at(t).distance(p)
                });
                near.fold(f64::INFINITY, f64::min) > reach * 1e-6
            });
            if off_edges && workpiece_boolean::lies_on(solid, face, p, reach * 1e-9) {
                return Some(p);
            }
        }
    }
    None
}

/// `sheet` with its edges `edges` (each between two of its faces)
/// rounded to `radius`: the faces cut back to where the round leaves
/// them, the round between them. Its faces are called `{id}/…`.
pub fn rounded(sheet: &Solid, edges: &[usize], radius: f64, id: &str) -> anyhow::Result<Solid> {
    ensure!(radius > 0.0, "a round has some radius");
    let shape = crate::BlendShape::Fillet {
        by: crate::By::Radius,
        size: radius,
        until: None,
        section: crate::Section::Circular,
        rho: 0.5,
    };
    blended_sheet(sheet, edges, &shape, id)
}

/// `sheet` with its edges `edges` (each between two of its faces)
/// blended as `shape` says — rounded or cut off — as a solid's edges
/// are: the faces cut back to where the blend leaves them, the blend
/// between them. Its faces are called `{id}/…`.
pub fn blended_sheet(
    sheet: &Solid,
    edges: &[usize],
    shape: &crate::BlendShape,
    id: &str,
) -> anyhow::Result<Solid> {
    let size = match *shape {
        crate::BlendShape::Fillet { size, until, .. } => size.max(until.unwrap_or(0.0)),
        crate::BlendShape::Chamfer { first, second } => first.max(second),
    };
    ensure!(size > 0.0, "a blend has some size");
    // The sheet as a solid behind its faces, thick enough to hold the
    // blend; its edges blended as a solid's are; of what is left, the
    // sheet's own faces and the blends.
    let all: Vec<usize> = (0..sheet.faces.len()).collect();
    let thin = crate::thick::thicken(sheet, &all, -size * 2.0, false, true)
        .context("the surface is too tight for a blend this big")?;
    // Which of the sheet's faces a face of a solid is a piece of: by
    // the name the sheet's face was given for this, which stays with
    // it through the thickening and the round (faces in one plane,
    // made one, are known by the names of both).
    let on = |solid: &Solid, f: usize| tag_of(&solid.faces[f]);
    // Which faces walk each edge, of the sheet and of the solid.
    let beside = |solid: &Solid, edge: usize| -> Vec<usize> {
        (0..solid.faces.len())
            .filter(|&f| {
                let steps = solid.faces[f].loops.iter().flatten();
                steps
                    .into_iter()
                    .any(|s| matches!(s.walk, Walk::Edge { edge: e, .. } if e == edge))
            })
            .collect()
    };
    let mut found = Vec::new();
    for &edge in edges {
        let pair = beside(sheet, edge);
        ensure!(
            pair.len() == 2,
            "a blend is between two faces: pick an edge inside the surface"
        );
        let same = (0..thin.edges.len()).find(|&e| {
            let mut faces: Vec<Option<usize>> =
                beside(&thin, e).iter().map(|&f| on(&thin, f)).collect();
            faces.sort();
            faces == [Some(pair[0].min(pair[1])), Some(pair[0].max(pair[1]))]
        });
        found.push(same.context("the edge is lost in the surface's thickness")?);
    }
    let made = crate::blend_solid(&thin, id, &found, shape, false)?;
    // (What the thickening made is called after it; the rounds, after
    // this.)
    let known = |f: &Face| f.name.as_deref().is_some_and(|n| n.starts_with("thick/"));
    let kept: Vec<usize> = (0..made.faces.len())
        .filter(|&f| on(&made, f).is_some() || !known(&made.faces[f]))
        .collect();
    let mut out = faces_of(&made, &kept);
    for f in 0..out.faces.len() {
        if let Some(was) = on(&out, f) {
            out.faces[f].name = sheet.faces[was].name.clone();
            out.faces[f].also = sheet.faces[was].also.clone();
        }
    }
    Ok(out)
}
