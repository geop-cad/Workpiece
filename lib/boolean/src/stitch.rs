//! Sewing sheets together: faces whose edges lie on each other become
//! faces of one sheet that share those edges — and, if no edge is left
//! free, of a solid.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::Refusal;
use crate::fit::reparametrize;
use crate::model::{Solid, Walk};

/// How often each edge of `sheet` is walked.
fn walked(sheet: &Solid) -> Vec<usize> {
    let mut times = vec![0; sheet.edges.len()];
    for step in sheet.faces.iter().flat_map(|f| f.loops.iter().flatten()) {
        if let Walk::Edge { edge, .. } = step.walk {
            times[edge] += 1;
        }
    }
    times
}

/// The faces of `sheets` as one sheet: vertices that lie within `within`
/// of each other become one, and free edges that lie on each other from
/// end to end become one edge of both their faces. The faces are turned
/// to face one way across the edges they share. The second value says
/// whether no edge is left free: the sheet closes, and is a solid (then
/// facing out of what it encloses).
pub fn stitched(sheets: &[Solid], within: f64) -> Result<(Solid, bool), Refusal> {
    let failed = |why: &str| Refusal::Open(why.to_owned());
    let mut all = Solid::default();
    for sheet in sheets {
        let (v0, e0) = (all.vertices.len(), all.edges.len());
        all.vertices.extend(sheet.vertices.iter().cloned());
        for edge in &sheet.edges {
            let mut edge = edge.clone();
            edge.ends = edge.ends.map(|v| v + v0);
            all.edges.push(edge);
        }
        for face in &sheet.faces {
            let mut face = face.clone();
            face.smooth.clear();
            for step in face.loops.iter_mut().flatten() {
                match &mut step.walk {
                    Walk::Edge { edge, .. } => *edge += e0,
                    Walk::Pole { vertex, .. } => *vertex += v0,
                }
            }
            all.faces.push(face);
        }
    }
    // Vertices that are one.
    let n = all.vertices.len();
    let mut one: Vec<usize> = (0..n).collect();
    for b in 0..n {
        for a in 0..b {
            let near = within + all.vertices[a].tolerance + all.vertices[b].tolerance;
            if one[a] == a && all.vertices[a].point.distance(all.vertices[b].point) <= near {
                one[b] = a;
                break;
            }
        }
    }
    for edge in &mut all.edges {
        edge.ends = edge.ends.map(|v| one[v]);
    }
    for step in all
        .faces
        .iter_mut()
        .flat_map(|f| f.loops.iter_mut().flatten())
    {
        if let Walk::Pole { vertex, .. } = &mut step.walk {
            *vertex = one[*vertex];
        }
    }
    // Free edges that lie on each other: the later is the earlier.
    let times = walked(&all);
    let mut same: BTreeMap<usize, (usize, bool)> = BTreeMap::new();
    let mut taken = vec![false; all.edges.len()];
    for b in 0..all.edges.len() {
        if times[b] != 1 {
            continue;
        }
        for a in 0..b {
            if times[a] != 1 || taken[a] || same.contains_key(&a) {
                continue;
            }
            let (ea, eb) = (&all.edges[a], &all.edges[b]);
            let (straight, crossed) = (ea.ends == eb.ends, ea.ends == [eb.ends[1], eb.ends[0]]);
            if !straight && !crossed {
                continue;
            }
            let near = within + ea.tolerance + eb.tolerance;
            // Where on `a` the places along `b` are; whether they are on
            // it at all, and which way they run.
            let on_a = |p: workpiece_core::math::Point3| {
                let (t0, t1) = ea.range;
                let mut best = (f64::INFINITY, t0);
                for k in 0..=64 {
                    let t = t0 + (t1 - t0) * f64::from(k) / 64.0;
                    let d = ea.curve.point_at(t).distance(p);
                    if d < best.0 {
                        best = (d, t);
                    }
                }
                let mut t = best.1;
                for _ in 0..30 {
                    let (q, dq) = ea.curve.derivative_at(t);
                    let step = (p - q).dot(dq) / dq.length_squared().max(f64::MIN_POSITIVE);
                    t = (t + step).clamp(t0, t1);
                }
                (ea.curve.point_at(t).distance(p), t)
            };
            let at_b = |f: f64| {
                eb.curve
                    .point_at(eb.range.0 + (eb.range.1 - eb.range.0) * f)
            };
            let places = [0.2, 0.5, 0.8].map(|f| on_a(at_b(f)));
            if places.iter().any(|p| p.0 > near) {
                continue;
            }
            let flipped = places[2].1 < places[0].1;
            // (Between the same two ends the same way, unless it comes
            // back to where it starts: then either.)
            let closed = ea.ends[0] == ea.ends[1];
            if !closed && flipped != crossed {
                continue;
            }
            same.insert(b, (a, flipped));
            taken[a] = true;
            break;
        }
    }
    let target = within.max(1e-12);
    for f in 0..all.faces.len() {
        for l in 0..all.faces[f].loops.len() {
            for s in 0..all.faces[f].loops[l].len() {
                let Walk::Edge { edge, reversed } = all.faces[f].loops[l][s].walk else {
                    continue;
                };
                let Some(&(to, flipped)) = same.get(&edge) else {
                    continue;
                };
                let (was, now) = (&all.edges[edge], &all.edges[to]);
                let fitted = reparametrize(
                    &all.faces[f].surface,
                    &all.faces[f].loops[l][s].pcurve,
                    &was.curve,
                    was.range,
                    &now.curve,
                    now.range,
                    flipped,
                    target,
                )
                .ok_or_else(|| failed("an edge of two sheets could not be made one"))?;
                let tolerance = now.tolerance.max(fitted.off * 1.5);
                all.edges[to].tolerance = tolerance;
                let step = &mut all.faces[f].loops[l][s];
                step.pcurve = Arc::new(fitted.curve);
                step.walk = Walk::Edge {
                    edge: to,
                    reversed: reversed != flipped,
                };
            }
        }
    }
    // Only what is still walked stays.
    let times = walked(&all);
    let mut edge_at = vec![usize::MAX; all.edges.len()];
    let mut vertex_at = vec![usize::MAX; all.vertices.len()];
    let mut out = Solid::default();
    let mut vertex = |out: &mut Solid, v: usize| {
        if vertex_at[v] == usize::MAX {
            vertex_at[v] = out.vertices.len();
            out.vertices.push(all.vertices[v].clone());
        }
        vertex_at[v]
    };
    for (e, edge) in all.edges.iter().enumerate() {
        if times[e] == 0 {
            continue;
        }
        let mut edge = edge.clone();
        edge.ends = [
            vertex(&mut out, edge.ends[0]),
            vertex(&mut out, edge.ends[1]),
        ];
        edge_at[e] = out.edges.len();
        out.edges.push(edge);
    }
    for face in &all.faces {
        let mut face = face.clone();
        for step in face.loops.iter_mut().flatten() {
            match &mut step.walk {
                Walk::Edge { edge, .. } => *edge = edge_at[*edge],
                Walk::Pole { vertex: v, .. } => *v = vertex(&mut out, *v),
            }
        }
        out.faces.push(face);
    }
    // All facing one way: across a shared edge, two faces walk it
    // opposite ways as seen from their fronts.
    let mut uses: Vec<Vec<(usize, bool)>> = vec![Vec::new(); out.edges.len()];
    for (f, face) in out.faces.iter().enumerate() {
        for step in face.loops.iter().flatten() {
            if let Walk::Edge { edge, reversed } = step.walk {
                uses[edge].push((f, reversed));
            }
        }
    }
    let mut seen = vec![false; out.faces.len()];
    for start in 0..out.faces.len() {
        if seen[start] {
            continue;
        }
        seen[start] = true;
        let mut queue = vec![start];
        while let Some(f) = queue.pop() {
            for step in out.faces[f].loops.clone().iter().flatten() {
                let Walk::Edge { edge, reversed } = step.walk else {
                    continue;
                };
                let [first, second] = uses[edge][..] else {
                    continue;
                };
                let (g, other) = if first == (f, reversed) {
                    second
                } else {
                    first
                };
                if g == f || seen[g] {
                    continue;
                }
                let mine = reversed != out.faces[f].outward;
                let theirs = other != out.faces[g].outward;
                if mine == theirs {
                    out.faces[g].outward = !out.faces[g].outward;
                }
                seen[g] = true;
                queue.push(g);
            }
        }
    }
    let closed = uses.iter().all(|u| u.len() == 2);
    if closed && crate::measure(&out).volume < 0.0 {
        for face in &mut out.faces {
            face.outward = !face.outward;
        }
    }
    Ok((out, closed))
}
