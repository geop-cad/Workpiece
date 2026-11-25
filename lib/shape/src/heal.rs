//! Faces taken off a body, and the faces around them closed over where
//! they were — a hole or a pocket filled, a boss removed, a chamfer or a
//! round taken back to the edge it was cut from.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, ensure};
use workpiece_boolean::model::{Edge, Face, Use, Walk};
use workpiece_boolean::{Op, Solid, combine, measure};
use workpiece_core::math::Point2;

/// `solid` without its faces `faces`: where they met the faces that
/// stay, those are closed over — each rim of what is deleted has to lie
/// in one face (a hole's mouth in a plate's top, a boss's foot on it).
/// What the deleted faces and those covers enclose is filled in, if it
/// was a hollow of the body, or taken off, if it stood out of it.
///
/// Where a rim runs on from one face into the next (a chamfer between
/// two sides, a round about a rim, a notch through a side), the faces
/// beside what is deleted are run on until they meet.
pub fn faces_deleted(solid: &Solid, faces: &[usize]) -> anyhow::Result<Solid> {
    ensure!(!faces.is_empty(), "pick the faces to delete");
    let gone: BTreeSet<usize> = faces.iter().copied().collect();
    ensure!(
        gone.len() < solid.faces.len(),
        "a body is more than the faces deleted"
    );
    let covered = covered_over(solid, &gone);
    if covered.is_ok() {
        return covered;
    }
    run_on_to_meet(solid, &gone)
}

/// `solid` without its faces `gone`, the faces beside them run on until
/// they meet over where those were. What is put there (or taken off) is
/// what is within some distance of the deleted faces — outside the body
/// or inside it — and not past a face that is run on; the distance is
/// the least for which nothing but the faces run on bounds it.
fn run_on_to_meet(solid: &Solid, gone: &BTreeSet<usize>) -> anyhow::Result<Solid> {
    use crate::thick::{opening, run_on, slab};
    // The faces beside what is deleted, each with its edges there.
    let mut beside: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    let mut theirs: BTreeSet<usize> = BTreeSet::new();
    for &f in gone {
        for step in solid.faces[f].loops.iter().flatten() {
            if let Walk::Edge { edge, .. } = step.walk {
                theirs.insert(edge);
            }
        }
    }
    let mut size = 0.0_f64;
    let mut corners: Vec<workpiece_core::math::Point3> = Vec::new();
    for &e in &theirs {
        let edge = &solid.edges[e];
        for k in 0..=8 {
            let t = edge.range.0 + (edge.range.1 - edge.range.0) * f64::from(k) / 8.0;
            corners.push(edge.curve.point_at(t));
        }
    }
    for a in &corners {
        for b in &corners {
            size = size.max(a.distance(*b));
        }
    }
    ensure!(size > 0.0, "what is deleted has no size");
    for (f, face) in solid.faces.iter().enumerate() {
        if gone.contains(&f) {
            continue;
        }
        for step in face.loops.iter().flatten() {
            if let Walk::Edge { edge, .. } = step.walk
                && theirs.contains(&edge)
            {
                let edges = beside.entry(f).or_default();
                if !edges.contains(&edge) {
                    edges.push(edge);
                }
            }
        }
    }
    ensure!(
        !beside.is_empty(),
        "no face is beside what is deleted to close it over"
    );
    // Outside edges about it first say that the body grows over it.
    let rims = beside.values().flatten();
    let (mut outside, mut inside) = (0, 0);
    for &e in rims {
        match opening(solid, e) {
            Some(a) if a > std::f64::consts::PI => inside += 1,
            Some(_) => outside += 1,
            None => {}
        }
    }
    let before = measure(solid).volume;
    let attempt = |grow: bool, reach: f64| -> anyhow::Result<Option<Solid>> {
        let way = if grow { 1.0 } else { -1.0 };
        let mut plug = Solid::default();
        for &f in gone {
            // (As far as it can be moved, if not that far: a round
            // boss's side no further than to its middle.)
            let mut by = reach;
            let part = loop {
                match slab(solid, f, way * by, &format!("heal/gone{f}")) {
                    Ok(part) => break part,
                    Err(e) if by < reach / 16.0 => return Err(e),
                    Err(_) => by /= 2.0,
                }
            };
            plug = combine(Op::Union, &plug, &part)?.solid;
        }
        // Places the deleted faces come to as they are moved off
        // themselves: a face beside them is in the way of that where
        // one of those is past it.
        let mut moved = Vec::new();
        for &f in gone {
            let face = &solid.faces[f];
            let inner = crate::surface::inside(solid, f);
            for step in face.loops.iter().flatten() {
                let (a, b) = solid.span(step);
                for k in 0..=8 {
                    let uv = step.pcurve.point_at(a + (b - a) * f64::from(k) / 8.0);
                    let n = crate::thick::normal(&face.surface, uv);
                    let out = if face.outward { n } else { -n };
                    let from = face.surface.point_at(uv.x, uv.y);
                    for share in [0.5, 0.999] {
                        moved.push(from + out * (way * reach * share));
                        if let Some(inner) = inner {
                            moved.push(inner + (from - inner) * 0.5 + out * (way * reach * share));
                        }
                    }
                }
            }
        }
        let near = solid.tolerance().max(size * 1e-9);
        let mut run: Vec<std::sync::Arc<workpiece_core::world::topo::NurbsSurface>> = Vec::new();
        for (&f, edges) in &beside {
            workpiece_core::cancel::checkpoint();
            for &e in edges {
                let wider = run_on(solid, f, &[(e, reach * 3.0)])?;
                // (As thick as it can be, if not that: a bore no further
                // than to its middle.)
                let mut thick = reach * 1.25;
                let past = loop {
                    match slab(&wider, 0, way * thick, &format!("heal/beside{f}")) {
                        Ok(past) => break past,
                        Err(e) if thick < reach / 16.0 => return Err(e),
                        Err(_) => thick /= 2.0,
                    }
                };
                run.push(wider.faces[0].surface.clone());
                let placer = workpiece_boolean::Placer::new(&past);
                let within = |p: &workpiece_core::math::Point3| {
                    placer.side(*p, near) == Some(workpiece_boolean::Side::In)
                };
                if !moved.iter().any(within) {
                    continue;
                }
                plug = combine(Op::Cut, &plug, &past)?.solid;
            }
            if plug.is_empty() {
                return Ok(None);
            }
        }
        let op = if grow { Op::Union } else { Op::Cut };
        let mut made = combine(op, solid, &plug)?.solid;
        // Nothing of what it was built of may show, but the faces run
        // on (and what lies in a face of the body); nor a face that
        // was to go.
        let of = |n: &str| -> Option<String> {
            let f = n.strip_prefix("heal/beside")?.strip_suffix("/near")?;
            solid.faces[f.parse::<usize>().ok()?].name.clone()
        };
        let own = |n: &String| !n.starts_with("heal/");
        // (A wall of what was put there that lies in a face run on
        // is as good as that.)
        let lies_in = |made: &Solid, f: &Face| {
            run.iter().any(|surface| {
                let ((u0, u1), (v0, v1)) = surface.domain();
                f.loops.iter().flatten().all(|step| {
                    let (a, b) = made.span(step);
                    (0..=4).all(|k| {
                        let uv = step.pcurve.point_at(a + (b - a) * f64::from(k) / 4.0);
                        let p = f.surface.point_at(uv.x, uv.y);
                        (0..9).any(|g| {
                            let guess = Point2::new(
                                u0 + (u1 - u0) * (0.5 + f64::from(g % 3)) / 3.0,
                                v0 + (v1 - v0) * (0.5 + f64::from(g / 3)) / 3.0,
                            );
                            workpiece_boolean::geometry::foot(surface, p, guess).1 <= near * 16.0
                        })
                    })
                })
            })
        };
        let stray = |made: &Solid, f: &Face| {
            let mut names = f.name.iter().chain(&f.also);
            f.name.is_some() && names.all(|n| !own(n) && !n.ends_with("/near")) && !lies_in(made, f)
        };
        let went = |f: &Face| {
            let mut names = f.name.iter().chain(&f.also);
            names.any(|n| {
                gone.iter()
                    .any(|&g| solid.faces[g].name.as_ref() == Some(n))
            })
        };
        if made.is_empty()
            || made.faces.iter().any(|f| stray(&made, f) || went(f))
            || (measure(&made).volume > before) != grow
        {
            return Ok(None);
        }
        for face in &mut made.faces {
            let names: Vec<String> = face.name.iter().chain(&face.also).cloned().collect();
            let mut kept: Vec<String> = names.iter().filter(|n| own(n)).cloned().collect();
            for n in names.iter().filter_map(|n| of(n)) {
                if !kept.contains(&n) {
                    kept.push(n);
                }
            }
            if face.name.is_some() {
                face.also = kept.split_off(1.min(kept.len()));
                face.name = kept.pop();
            }
        }
        Ok(Some(made))
    };
    let mut why = None;
    for grow in if inside > outside {
        [false, true]
    } else {
        [true, false]
    } {
        for k in -6..=1 {
            match attempt(grow, size * 2.0_f64.powi(k)) {
                Ok(Some(made)) => return Ok(made),
                Ok(None) => {}
                Err(e) => why = why.or(Some(e)),
            }
        }
    }
    Err(why
        .unwrap_or_else(|| anyhow::anyhow!("the faces around it do not meet"))
        .context("the faces around what is deleted could not be run on to close it over"))
}

/// As [`faces_deleted`], where each rim of what is deleted lies in one
/// face: closed over in that face, as it is.
fn covered_over(solid: &Solid, gone: &BTreeSet<usize>) -> anyhow::Result<Solid> {
    let gone = gone.clone();
    // Which faces walk each edge.
    let mut beside: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); solid.edges.len()];
    for (f, face) in solid.faces.iter().enumerate() {
        for step in face.loops.iter().flatten() {
            if let Walk::Edge { edge, .. } = step.walk {
                beside[edge].insert(f);
            }
        }
    }
    let rim = |edge: usize| {
        let of = &beside[edge];
        of.iter().any(|f| gone.contains(f)) && of.iter().any(|f| !gone.contains(f))
    };
    // The plug: the deleted faces, as they are.
    let mut plug = Solid::default();
    let mut vertices: BTreeMap<usize, usize> = BTreeMap::new();
    let mut edges: BTreeMap<usize, usize> = BTreeMap::new();
    let mut taken = |plug: &mut Solid, step: &Use| -> Use {
        let mut vertex = |plug: &mut Solid, v: usize| {
            *vertices.entry(v).or_insert_with(|| {
                plug.vertices.push(solid.vertices[v].clone());
                plug.vertices.len() - 1
            })
        };
        let walk = match step.walk {
            Walk::Edge { edge, reversed } => {
                let e = &solid.edges[edge];
                let ends = [vertex(plug, e.ends[0]), vertex(plug, e.ends[1])];
                let edge = *edges.entry(edge).or_insert_with(|| {
                    plug.edges.push(Edge { ends, ..e.clone() });
                    plug.edges.len() - 1
                });
                Walk::Edge { edge, reversed }
            }
            Walk::Pole { vertex: v, range } => Walk::Pole {
                vertex: vertex(plug, v),
                range,
            },
        };
        Use {
            walk,
            pcurve: step.pcurve.clone(),
        }
    };
    for &f in &gone {
        let face = &solid.faces[f];
        let loops = face
            .loops
            .iter()
            .map(|l| l.iter().map(|step| taken(&mut plug, step)).collect())
            .collect();
        plug.faces.push(Face {
            loops,
            smooth: Vec::new(),
            ..face.clone()
        });
    }
    // Its covers: of each face that stays, the loops that are all rim,
    // walked the other way. (A loop that is rim only in part — what is
    // deleted reaches the face's own edge — is not closed over yet.)
    let mut covered = 0;
    let rims = (0..solid.edges.len()).filter(|e| rim(*e)).count();
    for (g, face) in solid.faces.iter().enumerate() {
        if gone.contains(&g) {
            continue;
        }
        let mut around: Vec<(Vec<Use>, f64, Point2)> = Vec::new();
        for walk in &face.loops {
            let on_rim = |step: &Use| matches!(step.walk, Walk::Edge { edge, .. } if rim(edge));
            if !walk.iter().any(on_rim) {
                continue;
            }
            ensure!(
                walk.iter().all(on_rim),
                "what is deleted runs into the edge of {}: that is not closed over yet",
                face.name.as_deref().unwrap_or("a face")
            );
            let mut back = Vec::new();
            for step in walk.iter().rev() {
                let mut step = taken(&mut plug, step);
                if let Walk::Edge { reversed, .. } = &mut step.walk {
                    *reversed = !*reversed;
                }
                back.push(step);
                covered += 1;
            }
            // How it turns in (u, v), walked that way; and a place on it.
            let mut corners = Vec::new();
            for step in &back {
                let (a, b) = plug.span(step);
                for k in 0..16 {
                    corners.push(step.pcurve.point_at(a + (b - a) * f64::from(k) / 16.0));
                }
            }
            let area: f64 = (0..corners.len())
                .map(|k| corners[k].perp_dot(corners[(k + 1) % corners.len()]))
                .sum();
            around.push((back, area, corners[0]));
        }
        // Those that turn the cover's way are each a cover's outline;
        // the others are holes in the cover around them.
        let inside = |outline: &[Use], p: Point2| {
            let mut corners = Vec::new();
            for step in outline {
                let (a, b) = plug.span(step);
                for k in 0..16 {
                    corners.push(step.pcurve.point_at(a + (b - a) * f64::from(k) / 16.0));
                }
            }
            let mut within = false;
            for k in 0..corners.len() {
                let (a, b) = (corners[k], corners[(k + 1) % corners.len()]);
                if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x)
                {
                    within = !within;
                }
            }
            within
        };
        let mut covers: Vec<Vec<Vec<Use>>> = Vec::new();
        for (outline, _, _) in around.iter().filter(|a| a.1 > 0.0) {
            covers.push(vec![outline.clone()]);
        }
        for (hole, _, at) in around.iter().filter(|a| a.1 <= 0.0) {
            let cover = covers.iter_mut().find(|c| inside(&c[0], *at));
            let cover = cover.context(
                "what is deleted leaves an island of a face behind: that is not closed over yet",
            )?;
            cover.push(hole.clone());
        }
        for loops in covers {
            plug.faces.push(Face {
                surface: face.surface.clone(),
                // (Facing the body, if the plug stands on this face.)
                outward: !face.outward,
                loops,
                tolerance: face.tolerance,
                name: None,
                also: Vec::new(),
                smooth: Vec::new(),
            });
        }
    }
    ensure!(
        covered == rims,
        "what is deleted is not closed over by the faces around it"
    );
    // Standing out of the body, it is taken off; a hollow of it — its
    // faces then face into it — is filled in.
    if measure(&plug).volume > 0.0 {
        return Ok(combine(Op::Cut, solid, &plug)
            .context("taking the faces off")?
            .solid);
    }
    for face in &mut plug.faces {
        face.outward = !face.outward;
    }
    Ok(combine(Op::Union, solid, &plug)
        .context("filling in where the faces were")?
        .solid)
}
